use axum::extract::State;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;

use super::{api_err, api_ok};
use crate::{config, log_e, log_i, AppState};

/// Каталог бэкапов панели: {backup_dir}/xkeen-route.
fn backup_root(cfg: &config::AppConfig) -> std::path::PathBuf {
    std::path::Path::new(&cfg.system.backup_dir).join("xkeen-route")
}

/// Валидация имени бэкапа (защита от path traversal).
fn valid_backup_name(name: &str) -> bool {
    name.len() > 3
        && name.starts_with("xr-")
        && !name.contains("..")
        && !name.contains('/')
        && !name.contains('\\')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// GET /api/backups — список бэкапов.
pub async fn list_backups(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let root = backup_root(&cfg);
    let mut items: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&root).await {
        let mut names: Vec<String> = Vec::new();
        while let Ok(Some(e)) = rd.next_entry().await {
            if e.path().is_dir() {
                names.push(e.file_name().to_string_lossy().to_string());
            }
        }
        names.sort();
        names.reverse();
        for n in names {
            items.push(json!({ "name": n }));
        }
    }
    api_ok(json!({ "backups": items, "dir": root.display().to_string() }))
}

/// POST /api/backups — создать бэкап (config.yaml Mihomo + config.json панели + опционально zapret.conf и zapret-hosts.txt).
pub async fn create_backup(State(state): State<AppState>) -> Response {
    let _cfg_guard = state.config_lock.lock().await;
    let _routing_guard = state.routing_lock.lock().await;

    let cfg = state.config.read().await.clone();
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let name = format!("xr-{ts}");
    let dir = backup_root(&cfg).join(&name);
    let tmp_dir = backup_root(&cfg).join(format!("xr-{ts}.tmp"));

    if let Err(e) = tokio::fs::create_dir_all(&tmp_dir).await {
        return api_err(format!("Не удалось создать временный каталог бэкапа {}: {e}", tmp_dir.display()));
    }

    // 1. config.yaml Mihomo
    if let Err(e) = tokio::fs::copy(&cfg.mihomo.config_path, tmp_dir.join("config.yaml")).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Не удалось скопировать {}: {e}", cfg.mihomo.config_path));
    }
    // 2. config.json панели
    if let Err(e) = tokio::fs::copy(state.config_path.as_path(), tmp_dir.join("config.json")).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Не удалось скопировать {}: {e}", state.config_path.display()));
    }
    // 3. zapret.conf (если существует)
    let zapret_candidates = [
        "/opt/zapret/config/zapret.conf",
        "/opt/etc/zapret/zapret.conf",
    ];
    for zp in zapret_candidates {
        let p = std::path::Path::new(zp);
        if p.is_file() {
            let _ = tokio::fs::copy(p, tmp_dir.join("zapret.conf")).await;
            break;
        }
    }
    // 4. zapret-hosts.txt (если существует)
    let hosts_candidates = [
        "/opt/etc/zapret/zapret-hosts.txt",
        "/opt/zapret/config/zapret-hosts.txt",
    ];
    for hp in hosts_candidates {
        let p = std::path::Path::new(hp);
        if p.is_file() {
            let _ = tokio::fs::copy(p, tmp_dir.join("zapret-hosts.txt")).await;
            break;
        }
    }
    // 5. ru_exclude_override.lst (если существует)
    let override_path = std::path::Path::new(crate::override_sync::OVERRIDE_FILE);
    if override_path.is_file() {
        let _ = tokio::fs::copy(override_path, tmp_dir.join("ru_exclude_override.lst")).await;
    }
    // 6. AdGuardHome.yaml (если существует)
    let agh_candidates = [
        cfg.adguard.config_path.as_str(),
        "/opt/etc/AdGuardHome.yaml",
        "/opt/etc/AdGuardHome/AdGuardHome.yaml",
    ];
    for ap in agh_candidates {
        let p = std::path::Path::new(ap);
        if p.is_file() {
            let _ = tokio::fs::copy(p, tmp_dir.join("AdGuardHome.yaml")).await;
            break;
        }
    }

    // Атомарно активируем каталог бэкапа только после успешной записи полного комплекта
    if let Err(e) = tokio::fs::rename(&tmp_dir, &dir).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Ошибка фиксации бэкапа: {e}"));
    }

    log_i!("Бэкап создан: {} ({})", name, dir.display());
    api_ok(json!({ "name": name, "dir": dir.display().to_string() }))
}

#[derive(Deserialize)]
pub struct BackupReq {
    pub name: String,
}

/// POST /api/backups/restore — восстановить конфиги из бэкапа через ConfigTx, reload Mihomo.
pub async fn restore_backup(State(state): State<AppState>, Json(req): Json<BackupReq>) -> Response {
    if !valid_backup_name(&req.name) {
        return api_err("Некорректное имя бэкапа");
    }
    let cfg = state.config.read().await.clone();
    let dir = backup_root(&cfg).join(&req.name);
    if !dir.is_dir() {
        return api_err(format!("Бэкап {} не найден", req.name));
    }
    let src_yaml = dir.join("config.yaml");
    let src_json = dir.join("config.json");
    if !src_yaml.exists() || !src_json.exists() {
        return api_err(format!("В бэкапе {} отсутствуют необходимые файлы (config.yaml / config.json)", req.name));
    }

    // 1. Предварительное чтение и валидация файлов бэкапа ДО каких-либо изменений на диске
    let new_yaml_content = match tokio::fs::read_to_string(&src_yaml).await {
        Ok(s) if !s.trim().is_empty() => s,
        Ok(_) => return api_err("Файл config.yaml в бэкапе пуст"),
        Err(e) => return api_err(format!("Не удалось прочитать config.yaml из бэкапа: {e}")),
    };
    if let Err(e) = crate::transaction::validate_yaml_syntax(&new_yaml_content) {
        return api_err(format!("Синтаксическая ошибка в config.yaml бэкапа: {e}"));
    }

    let new_json_content = match tokio::fs::read_to_string(&src_json).await {
        Ok(s) => s,
        Err(e) => return api_err(format!("Не удалось прочитать config.json из бэкапа: {e}")),
    };
    let mut new_cfg: config::AppConfig = match serde_json::from_str(&new_json_content) {
        Ok(c) => c,
        Err(e) => return api_err(format!("Файл config.json в бэкапе поврежден: {e}")),
    };

    // BK-04: Сохранение локальных путей хоста при переносе бэкапа
    new_cfg.system = cfg.system.clone();
    new_cfg.mihomo.config_path = cfg.mihomo.config_path.clone();

    // 2. Старт транзакции через ConfigTx с блокировками и снапшотами (BK-04, BK-05)
    let mut tx = match crate::transaction::ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(format!("Ошибка начала транзакции восстановления: {e}")),
    };

    if let Err(e) = tx.set_yaml(new_yaml_content) {
        return api_err(format!("Ошибка подготовки config.yaml: {e}"));
    }
    tx.stage_json(new_cfg);

    // Дополнительные файлы (zapret.conf, zapret-hosts.txt и override), если присутствуют в бэкапе
    let src_zapret = dir.join("zapret.conf");
    if src_zapret.is_file() {
        if let Ok(z_content) = tokio::fs::read_to_string(&src_zapret).await {
            let z_candidates = [
                std::path::PathBuf::from("/opt/zapret/config/zapret.conf"),
                std::path::PathBuf::from("/opt/etc/zapret/zapret.conf"),
            ];
            for z_path in &z_candidates {
                if z_path.parent().map(|p| p.exists()).unwrap_or(false) {
                    let _ = tx.set_extra_file(z_path.as_path(), z_content.clone(), false).await;
                    break;
                }
            }
        }
    }

    let src_hosts = dir.join("zapret-hosts.txt");
    if src_hosts.is_file() {
        if let Ok(h_content) = tokio::fs::read_to_string(&src_hosts).await {
            let h_candidates = [
                std::path::PathBuf::from("/opt/etc/zapret/zapret-hosts.txt"),
                std::path::PathBuf::from("/opt/zapret/config/zapret-hosts.txt"),
            ];
            for h_path in &h_candidates {
                if h_path.parent().map(|p| p.exists()).unwrap_or(false) {
                    let _ = tx.set_extra_file(h_path.as_path(), h_content.clone(), false).await;
                    break;
                }
            }
        }
    }

    let src_override = dir.join("ru_exclude_override.lst");
    if src_override.is_file() {
        if let Ok(ov_content) = tokio::fs::read_to_string(&src_override).await {
            let ov_path = std::path::Path::new(crate::override_sync::OVERRIDE_FILE);
            if ov_path.parent().map(|p| p.exists()).unwrap_or(false) {
                let _ = tx.set_extra_file(ov_path, ov_content, false).await;
            }
        }
    }

    let src_agh = dir.join("AdGuardHome.yaml");
    if src_agh.is_file() {
        if let Ok(agh_content) = tokio::fs::read_to_string(&src_agh).await {
            let agh_candidates = [
                std::path::PathBuf::from(&cfg.adguard.config_path),
                std::path::PathBuf::from("/opt/etc/AdGuardHome.yaml"),
                std::path::PathBuf::from("/opt/etc/AdGuardHome/AdGuardHome.yaml"),
            ];
            for a_path in &agh_candidates {
                if a_path.parent().map(|p| p.exists()).unwrap_or(false) {
                    let _ = tx.set_extra_file(a_path.as_path(), agh_content.clone(), true).await;
                    break;
                }
            }
        }
    }

    // 3. Атомарное применение и reload с автоматическим двухфазным откатом при сбое (BK-05)
    match tx.commit_and_reload().await {
        Ok(_) => {
            crate::vkturn::sync_vkturn_after_restore(&state).await;
            log_i!("Конфиги атомарно восстановлены из бэкапа {}", req.name);
            api_ok(json!({ "restored": req.name }))
        }
        Err(e) => {
            let recovery_note = if tx.is_recovery_required() {
                " (ВНИМАНИЕ: требуется ручное восстановление файлов)"
            } else {
                ""
            };
            log_e!("Ошибка применения бэкапа {}: {e}{recovery_note}", req.name);
            api_err(format!("Ошибка применения бэкапа: {e}{recovery_note}"))
        }
    }
}

/// POST /api/backups/delete — удалить бэкап.
pub async fn delete_backup(State(state): State<AppState>, Json(req): Json<BackupReq>) -> Response {
    if !valid_backup_name(&req.name) {
        return api_err("Некорректное имя бэкапа");
    }
    let cfg = state.config.read().await.clone();
    let dir = backup_root(&cfg).join(&req.name);
    match tokio::fs::remove_dir_all(&dir).await {
        Ok(_) => {
            log_i!("Бэкап {} удалён", req.name);
            api_ok(json!({ "deleted": req.name }))
        }
        Err(e) => api_err(format!("Не удалось удалить {}: {e}", req.name)),
    }
}

// ==================== ЭКСПОРТ И ИМПОРТ БЭКАПОВ В БРАУЗЕРЕ =============

/// GET /api/backups/export/:name — экспорт снимка конфигурации в формате JSON
pub async fn export_backup(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Response {
    if !valid_backup_name(&name) {
        return api_err("Некорректное имя бэкапа");
    }
    let cfg = state.config.read().await.clone();
    let dir = backup_root(&cfg).join(&name);
    if !dir.is_dir() {
        return api_err("Бэкап не найден");
    }

    let mihomo_yaml = match tokio::fs::read_to_string(dir.join("config.yaml")).await {
        Ok(s) => s,
        Err(e) => return api_err(format!("Ошибка чтения config.yaml из бэкапа: {e}")),
    };
    let route_json = match tokio::fs::read_to_string(dir.join("config.json")).await {
        Ok(s) => s,
        Err(e) => return api_err(format!("Ошибка чтения config.json из бэкапа: {e}")),
    };

    let mut files_map = serde_json::Map::new();
    files_map.insert("config.yaml".to_string(), json!(mihomo_yaml));
    files_map.insert("config.json".to_string(), json!(route_json));

    if let Ok(z) = tokio::fs::read_to_string(dir.join("zapret.conf")).await {
        files_map.insert("zapret.conf".to_string(), json!(z));
    }
    if let Ok(h) = tokio::fs::read_to_string(dir.join("zapret-hosts.txt")).await {
        files_map.insert("zapret-hosts.txt".to_string(), json!(h));
    }
    if let Ok(ov) = tokio::fs::read_to_string(dir.join("ru_exclude_override.lst")).await {
        files_map.insert("ru_exclude_override.lst".to_string(), json!(ov));
    }

    let backup_bundle = json!({
        "version": "1.0",
        "name": name,
        "exported_at": chrono::Local::now().to_rfc3339(),
        "files": files_map
    });

    let filename = format!("{}.xkbak", name);
    let payload = match serde_json::to_string_pretty(&backup_bundle) {
        Ok(p) => p,
        Err(e) => return api_err(format!("Ошибка сериализации бэкапа: {e}")),
    };

    (
        [
            ("Content-Type", "application/json; charset=utf-8"),
            ("Content-Disposition", &format!("attachment; filename=\"{}\"", filename)),
            ("Cache-Control", "no-cache, no-store, must-revalidate"),
        ],
        payload,
    )
        .into_response()
}

/// POST /api/backups/import — импорт снимка с ПК в хранилище бэкапов
pub async fn import_backup(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let files = match body.get("files").and_then(|f| f.as_object()) {
        Some(f) => f,
        None => return api_err("Некорректный формат архива бэкапа: отсутствует секция files"),
    };

    // 0. Строгая проверка допустимости имен файлов и защита от path traversal (BK-02)
    for key in files.keys() {
        if key.contains("..") || key.contains('/') || key.contains('\\') {
            return api_err(format!("Обнаружена попытка path traversal в имени файла бэкапа: '{key}'"));
        }
        match key.as_str() {
            "config.yaml" | "config.json" | "zapret.conf" | "zapret-hosts.txt" | "ru_exclude_override.lst" => {},
            _ => return api_err(format!("Недопустимый файл в архиве бэкапа: '{key}'")),
        }
    }

    // 1. Проверка наличия обязательных файлов
    let yaml_str = match files.get("config.yaml").and_then(|y| y.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return api_err("В импортируемом бэкапе отсутствует или пуст обязательный файл config.yaml"),
    };
    let json_str = match files.get("config.json").and_then(|j| j.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return api_err("В импортируемом бэкапе отсутствует или пуст обязательный файл config.json"),
    };

    // 2. Валидация синтаксиса ДО записи на диск
    if let Err(e) = crate::transaction::validate_yaml_syntax(yaml_str) {
        return api_err(format!("Синтаксическая ошибка в config.yaml импортируемого бэкапа: {e}"));
    }
    if let Err(e) = serde_json::from_str::<config::AppConfig>(json_str) {
        return api_err(format!("Поврежденный JSON в config.json импортируемого бэкапа: {e}"));
    }

    // 3. Формирование и валидация имени (BK-02, BK-03: всегда префикс xr-, защита от path traversal)
    let raw_name = body.get("name")
        .and_then(|n| n.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let safe_name = match raw_name {
        Some(name) => {
            let filtered: String = name
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            let base = filtered.trim_start_matches("xr-").trim_matches(|c| c == '-' || c == '_');
            if base.is_empty() {
                format!("xr-imported-{}", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"))
            } else if filtered.starts_with("xr-") {
                filtered
            } else {
                format!("xr-{filtered}")
            }
        }
        None => format!("xr-imported-{}", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")),
    };

    if !valid_backup_name(&safe_name) {
        return api_err("Некорректное имя импортируемого бэкапа");
    }

    let cfg = state.config.read().await.clone();
    let dir = backup_root(&cfg).join(&safe_name);

    // Collision check: не перезаписываем существующий бэкап вслепую
    if dir.exists() {
        return api_err(format!("Бэкап с именем '{}' уже существует", safe_name));
    }

    let tmp_dir = backup_root(&cfg).join(format!("{safe_name}.tmp"));
    if let Err(e) = tokio::fs::create_dir_all(&tmp_dir).await {
        return api_err(format!("Ошибка создания временного каталога бэкапа: {e}"));
    }

    // Запись файлов в staging каталог
    if let Err(e) = tokio::fs::write(tmp_dir.join("config.yaml"), yaml_str).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Ошибка записи config.yaml: {e}"));
    }
    if let Err(e) = tokio::fs::write(tmp_dir.join("config.json"), json_str).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Ошибка записи config.json: {e}"));
    }

    // Опциональные файлы
    if let Some(zapret_str) = files.get("zapret.conf").and_then(|z| z.as_str()) {
        let _ = tokio::fs::write(tmp_dir.join("zapret.conf"), zapret_str).await;
    }
    if let Some(hosts_str) = files.get("zapret-hosts.txt").and_then(|h| h.as_str()) {
        let _ = tokio::fs::write(tmp_dir.join("zapret-hosts.txt"), hosts_str).await;
    }
    if let Some(ov_str) = files.get("ru_exclude_override.lst").and_then(|o| o.as_str()) {
        let _ = tokio::fs::write(tmp_dir.join("ru_exclude_override.lst"), ov_str).await;
    }

    // Атомарное переименование staging -> final
    if let Err(e) = tokio::fs::rename(&tmp_dir, &dir).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Ошибка фиксации каталога импорта: {e}"));
    }

    log_i!("Импортирован бэкап '{}' в {}", safe_name, dir.display());
    api_ok(json!({ "imported": safe_name }))
}

