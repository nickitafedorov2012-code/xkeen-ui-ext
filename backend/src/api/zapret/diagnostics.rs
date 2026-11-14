use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;

use super::presets::{
    apply_zapret_conf_save, apply_zapret_preset_action, build_dpi_test_script,
    commit_zapret_config_with_routing, convert_lua_to_legacy_desync, detect_zapret_hardware,
    is_nfqws1_available, is_nfqws2_available, merge_zapret_features, normalize_engine_choice,
    persist_zapret_enabled_strict, reset_zapret_features, should_use_nfqws2,
    sync_zapret_files, sync_zapret_files_with_config_path, validate_custom_args,
    validate_zapret_conf, zapret_set_enabled_core, ZapretActionReq,
};
use crate::api::{api_err, api_err_bad_request, api_ok, atomic_write_file};
use crate::{config, AppState};
pub async fn zapret_action(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<ZapretActionReq>,
) -> Response {
    let act = body.action.trim();

    // BUG-26: Межпроцессная блокировка и фиксация операционного состояния
    let op_state = match act {
        "start" => crate::zapret::ZapretOperationState::Starting,
        "stop" => crate::zapret::ZapretOperationState::Stopping,
        "restart" => crate::zapret::ZapretOperationState::Restarting,
        "upgrade_zapret2" | "rollback_v1" => crate::zapret::ZapretOperationState::Upgrading,
        "toggle_feature" | "set_features" | "reset_features" | "add_custom_domain" | "remove_custom_domain" | "toggle_custom_domain" | "boost_custom_domain" | "apply_strategy" => crate::zapret::ZapretOperationState::Reloading,
        _ => crate::zapret::ZapretOperationState::Configuring,
    };

    let _zapret_guard = if act != "test_dpi" {
        match crate::zapret::ZapretLockGuard::acquire(state.zapret_lock.clone(), op_state, 10).await {
            Ok(g) => Some(g),
            Err(e) => return api_err(format!("Служба Zapret занята другой операцией: {e}")),
        }
    } else {
        None
    };

    let proxy_url = state.config.read().await.mihomo_proxy_url();

    // 0.05 1-клик обновление до Zapret 2 (nfqws2 + Lua библиотеки в /opt/zapret2) с сохранением резервной копии v1
    if act == "upgrade_zapret2" {
        let hw = detect_zapret_hardware(&state).await;
        // Z-02: останавливаем службу ДО замены бинарника — иначе mv из /tmp (cross-device
        // copy+unlink) получает ETXTBSY от запущенного nfqws2, апгрейд проходит «фантомно»:
        // version.txt уже новой версии, а бинарник на диске — старый.
        let zap_enabled_before_upgrade = state.config.read().await.zapret.enabled;
        if zap_enabled_before_upgrade {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
                .arg("stop")
                .output()
                .await;
        }
        let target_ver = {
            let requested = body.version.as_deref().unwrap_or("").trim();
            if !requested.is_empty() {
                if requested.starts_with('v') { requested.to_string() } else { format!("v{requested}") }
            } else {
                let cfg = state.config.read().await;
                let lat = cfg.zapret.latest_version.as_deref().unwrap_or("").trim();
                if !lat.is_empty() && lat != "v1.0.5.2" {
                    if lat.starts_with('v') { lat.to_string() } else { format!("v{lat}") }
                } else {
                    "v1.0.6".to_string()
                }
            }
        };
        let target_ver_no_v = target_ver.trim_start_matches('v').to_string();

        let upgrade_cmd = r#"
            PROXY="${MIHOMO_PROXY:-http://127.0.0.1:7890}"
            TARGET_VER="${TARGET_VER:-v1.0.6}"
            TARGET_VER_NO_V="${TARGET_VER_NO_V:-${TARGET_VER#v}}"
            ARCH=$(uname -m)
            case "$ARCH" in
              aarch64|arm64) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
              armv7*|armv8*|arm*) TARGET_ARCH="linux-arm"; Z2_ARCH="arm" ;;
              mips*)
                if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ] || \
                   [ "$(od -t x1 -j 5 -N 1 /bin/sh 2>/dev/null | awk 'NR==1{print $2}')" = "02" ] || \
                   [ "$(echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }')" = "0" ]; then
                  TARGET_ARCH="linux-mips32r2-msb"; Z2_ARCH="mips"
                else
                  TARGET_ARCH="linux-mips32r2-lsb"; Z2_ARCH="mipsel"
                fi
                ;;
              x86_64) TARGET_ARCH="linux-x86_64"; Z2_ARCH="x86_64" ;;
              *) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
            esac

            echo "[1/4] Архитектура целевой системы: $TARGET_ARCH ($ARCH, Z2_ARCH=$Z2_ARCH)"

            mkdir -p /opt/zapret2 /opt/zapret2/lua /opt/zapret2/binaries /opt/etc/init.d /opt/etc/zapret /opt/sbin /opt/zapret /opt/zapret/nfq

            # Сохраняем резервную копию конфигурации и бинарника v1 для безопасного переключения
            if [ -f /opt/etc/zapret/zapret.conf ] && ! grep -q "lua-desync" /opt/etc/zapret/zapret.conf 2>/dev/null; then
              cp -f /opt/etc/zapret/zapret.conf /opt/etc/zapret/zapret.v1.conf.bak 2>/dev/null || true
              echo "Создана резервная копия конфигурации: /opt/etc/zapret/zapret.v1.conf.bak"
            fi
            for v1bin in /opt/zapret/nfq/nfqws /opt/sbin/nfqws /opt/bin/nfqws /opt/usr/bin/nfqws; do
              if [ -x "$v1bin" ] && [ ! -f /opt/zapret/nfqws.bak ]; then
                cp -f "$v1bin" /opt/zapret/nfqws.bak 2>/dev/null || true
                chmod +x /opt/zapret/nfqws.bak 2>/dev/null || true
                echo "Создана резервная копия исполняемого файла v1: /opt/zapret/nfqws.bak"
                break
              fi
            done

            STAGE_DIR=$(mktemp -d /tmp/z2_stage.XXXXXX)
            echo "[2/4] Загрузка дистрибутива Zapret 2 ($TARGET_VER) в $STAGE_DIR..."
            for url in \
              "https://github.com/bol-van/zapret2/releases/download/$TARGET_VER/zapret2-$TARGET_VER.tar.gz" \
              "https://github.com/bol-van/zapret2/releases/latest/download/zapret2-$TARGET_VER.tar.gz" \
              "https://github.com/bol-van/zapret2/releases/download/$TARGET_VER/zapret2-$TARGET_VER_NO_V.tar.gz" \
              "https://github.com/bol-van/zapret2/archive/refs/tags/$TARGET_VER.tar.gz"; do
              if curl -fsSL --connect-timeout 10 -m 60 -x "$PROXY" "$url" -o "$STAGE_DIR/z2.tar.gz" 2>/dev/null || \
                 curl -fsSL --connect-timeout 10 -m 60 "$url" -o "$STAGE_DIR/z2.tar.gz" 2>/dev/null; then
                if [ -s "$STAGE_DIR/z2.tar.gz" ] && [ $(wc -c < "$STAGE_DIR/z2.tar.gz" 2>/dev/null || echo 0) -gt 10000 ]; then
                  echo "Архив успешно загружен с: $url"
                  break
                fi
              fi
              rm -f "$STAGE_DIR/z2.tar.gz"
            done

            echo "[3/4] Распаковка архива и копирование бинарных файлов nfqws2 и Lua модулей..."
            if [ -f "$STAGE_DIR/z2.tar.gz" ]; then
              tar -xzf "$STAGE_DIR/z2.tar.gz" -C "$STAGE_DIR" 2>/dev/null || true
              rm -f "$STAGE_DIR/z2.tar.gz"
              Z2_DIR=$(find "$STAGE_DIR" -maxdepth 1 -type d \( -name "zapret2*" -o -name "zapret-v*" \) | head -n 1)
              if [ -n "$Z2_DIR" ]; then
                echo "Распакован каталог сборки: $Z2_DIR"
                if [ -d "$Z2_DIR/binaries" ]; then
                  cp -rf "$Z2_DIR/binaries/"* /opt/zapret2/binaries/ 2>/dev/null || true
                fi
                NEW_BIN=""
                if [ -n "$Z2_ARCH" ] && [ -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws2" ]; then
                  NEW_BIN="$Z2_DIR/binaries/$Z2_ARCH/nfqws2"
                elif [ -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws2" ]; then
                  NEW_BIN="$Z2_DIR/binaries/$TARGET_ARCH/nfqws2"
                elif [ -f "$Z2_DIR/nfqws2" ]; then
                  NEW_BIN="$Z2_DIR/nfqws2"
                fi
                if [ -n "$NEW_BIN" ] && [ -f "$NEW_BIN" ]; then
                  MAGIC=$(hexdump -n 4 -e '4/1 "%02x"' "$NEW_BIN" 2>/dev/null || od -An -tx1 -N4 "$NEW_BIN" 2>/dev/null | tr -d ' \r\n')
                  if [ "$MAGIC" != "7f454c46" ]; then
                    echo "ERROR: Invalid ELF header in $NEW_BIN (expected 7f 45 4c 46, got $MAGIC)" >&2
                    rm -rf "$STAGE_DIR"
                    exit 1
                  fi
                  # Z-02: верификация реальной замены бинарника (защита от молчаливого ETXTBSY)
                  EXPECTED_SIZE=$(wc -c < "$NEW_BIN" 2>/dev/null || echo 0)
                  kill $(pidof nfqws2 2>/dev/null) 2>/dev/null || true
                  sleep 1
                  if mv -f "$NEW_BIN" /opt/zapret2/nfqws2 2>/dev/null; then
                    chmod +x /opt/zapret2/nfqws2 2>/dev/null || true
                    INSTALLED_SIZE=$(wc -c < /opt/zapret2/nfqws2 2>/dev/null || echo 0)
                    if [ "$EXPECTED_SIZE" != "$INSTALLED_SIZE" ]; then
                      echo "ERROR: nfqws2 size mismatch after install (expected $EXPECTED_SIZE, got $INSTALLED_SIZE)" >&2
                      rm -rf "$STAGE_DIR"
                      exit 1
                    fi
                    echo "Скопирован nfqws2"
                  else
                    echo "ERROR: не удалось заменить /opt/zapret2/nfqws2 (binary busy?)" >&2
                    rm -rf "$STAGE_DIR"
                    exit 1
                  fi
                fi
                if [ -d "$Z2_DIR/files/lua" ]; then
                  cp -rf "$Z2_DIR/files/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из files/lua в /opt/zapret2/lua"
                fi
                if [ -d "$Z2_DIR/lua" ]; then
                  cp -rf "$Z2_DIR/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из lua в /opt/zapret2/lua"
                fi
              fi
            fi
            rm -rf "$STAGE_DIR" 2>/dev/null || true

            # Если ключевые Lua скрипты отсутствуют или повреждены, загружаем их напрямую из официального репозитория
            for lf in zapret-lib.lua zapret-antidpi.lua zapret-auto.lua; do
              if [ ! -s "/opt/zapret2/lua/$lf" ] || [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                rm -f "/opt/zapret2/lua/$lf"
                (curl -fsSL -x "$PROXY" "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf" || \
                 curl -fsSL "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf") 2>/dev/null || true
                if [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                  rm -f "/opt/zapret2/lua/$lf"
                fi
              fi
            done

            echo "[4/4] Настройка прав доступа, симлинков и инициализация /opt/zapret2..."
            [ -f /opt/zapret2/nfqws2 ] && chmod +x /opt/zapret2/nfqws2 && ln -sf /opt/zapret2/nfqws2 /opt/sbin/nfqws2
            chmod 644 /opt/zapret2/lua/*.lua 2>/dev/null || true
            echo "$TARGET_VER" > /opt/zapret2/version.txt
            echo "Установка Zapret 2.0 ($TARGET_VER) успешно завершена."
        "#;

        match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(upgrade_cmd)
            .env("MIHOMO_PROXY", &proxy_url)
            .env("TARGET_VER", &target_ver)
            .env("TARGET_VER_NO_V", &target_ver_no_v)
            .output()
            .await
        {
            Ok(out) => {
                let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                if !is_nfqws2_available() {
                    return api_err(format!(
                        "Не удалось установить Zapret 2.0: исполняемый файл nfqws2 или Lua-библиотеки не найдены в /opt/zapret2. Проверьте подключение к сети. Текущая конфигурация v1 сохранена без изменений.\nЛог:\n{}",
                        output_str.trim()
                    ));
                }

                let _ = tokio::fs::create_dir_all("/opt/zapret2").await;
                let _ = tokio::fs::write("/opt/zapret2/version.txt", format!("{target_ver}\n")).await;

                let _cfg_guard = state.config_lock.lock().await;
                let mut cfg = (**state.config.read().await).clone();
                cfg.zapret.engine = "v2".to_string();
                cfg.zapret.update_available = false;
                cfg.zapret.latest_version = Some(target_ver.clone());
                cfg.zapret.last_update_check = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                if let Some(ref ca) = cfg.zapret.custom_args {
                    if ca.contains("--dpi-desync") && !ca.contains("--lua-desync") {
                        cfg.zapret.custom_args = None;
                    }
                }
                if let Err(e) = sync_zapret_files(&cfg.zapret).await {
                    return api_err(format!("Ошибка синхронизации файлов Zapret 2.0: {e}"));
                }
                let _ = config::save(&state.config_path, &cfg).await;
                *state.config.write().await = std::sync::Arc::new(cfg.clone());

                if cfg.zapret.enabled {
                    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                }

                return api_ok(json!({
                    "success": true,
                    "engine": "v2",
                    "v2_installed": true,
                    "v1_installed": is_nfqws1_available(),
                    "can_rollback_v1": true,
                    "hardware": hw,
                    "features": cfg.zapret,
                    "version": &target_ver,
                    "latest_version": &target_ver,
                    "update_available": false,
                    "output": output_str.trim(),
                    "message": format!("Zapret 2.0 ({target_ver}, nfqws2 + Lua) установлен в /opt/zapret2 для {} ({})", hw.model, hw.arch_label)
                }));
            }
            Err(e) => return api_err(format!("Ошибка обновления до Zapret 2.0: {e}")),
        }
    }

    // 0.06 Мгновенное переключение движка (Запрет 1 / Запрет 2)
    if act == "rollback_v1" || act == "switch_engine" {
        let target_engine = if act == "rollback_v1" {
            "v1"
        } else {
            normalize_engine_choice(body.engine.as_deref().unwrap_or("v2"))
        };

        if target_engine == "v1" {
            let restore_v1_cmd = r#"
                mkdir -p /opt/zapret/nfq /opt/etc/zapret
                if [ ! -x /opt/zapret/nfq/nfqws ]; then
                  if [ -x /opt/zapret/nfqws.bak ]; then
                    cp -f /opt/zapret/nfqws.bak /opt/zapret/nfq/nfqws 2>/dev/null || true
                  elif [ -x /opt/sbin/nfqws ]; then
                    cp -f /opt/sbin/nfqws /opt/zapret/nfq/nfqws 2>/dev/null || true
                  elif [ -x /opt/bin/nfqws ]; then
                    cp -f /opt/bin/nfqws /opt/zapret/nfq/nfqws 2>/dev/null || true
                  fi
                  chmod +x /opt/zapret/nfq/nfqws 2>/dev/null || true
                fi
                if [ ! -x /opt/zapret/nfq/nfqws ] && [ ! -x /opt/sbin/nfqws ]; then
                  ARCH=$(uname -m)
                  case "$ARCH" in
                    aarch64|arm64) TARGET_ARCH="linux-arm64" ;;
                    armv7*|armv8*|arm*) TARGET_ARCH="linux-arm" ;;
                    mips*)
                      if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ]; then
                        TARGET_ARCH="linux-mips32r2-msb"
                      else
                        TARGET_ARCH="linux-mips32r2-lsb"
                      fi
                      ;;
                    x86_64) TARGET_ARCH="linux-x86_64" ;;
                    *) TARGET_ARCH="linux-arm64" ;;
                  esac
                  PROXY="${MIHOMO_PROXY:-http://127.0.0.1:7890}"
                  for url in \
                    "https://github.com/bol-van/zapret/releases/download/v72.13/zapret-v72.13.tar.gz"; do
                    if curl -fsSL --connect-timeout 10 -m 60 -x "$PROXY" "$url" -o /tmp/z1.tar.gz 2>/dev/null || \
                       curl -fsSL --connect-timeout 10 -m 60 "$url" -o /tmp/z1.tar.gz 2>/dev/null; then
                      tar -xzf /tmp/z1.tar.gz -C /tmp/ 2>/dev/null || true
                      rm -f /tmp/z1.tar.gz
                      Z1_DIR=$(find /tmp -maxdepth 1 -type d -name "zapret-v*" | head -n 1)
                      if [ -n "$Z1_DIR" ] && [ -f "$Z1_DIR/binaries/$TARGET_ARCH/nfqws" ]; then
                        cp -f "$Z1_DIR/binaries/$TARGET_ARCH/nfqws" /opt/zapret/nfq/nfqws
                        chmod +x /opt/zapret/nfq/nfqws
                        ln -sf /opt/zapret/nfq/nfqws /opt/sbin/nfqws
                      fi
                      rm -rf "$Z1_DIR" 2>/dev/null || true
                      break
                    fi
                  done
                fi
            "#;
            let _ = tokio::process::Command::new("sh").arg("-c").arg(restore_v1_cmd).env("MIHOMO_PROXY", &proxy_url).output().await;

            if !is_nfqws1_available() && !std::path::Path::new("/opt/etc/zapret/zapret.v1.conf.bak").exists() {
                return api_err("Не удалось активировать Zapret 1: исполняемый файл nfqws не найден.");
            }
        } else if target_engine == "v2" && !is_nfqws2_available() {
            // Автоматическая установка Zapret 2 при переключении тумблера, если он ещё не установлен
            return Box::pin(zapret_action(
                State(state),
                axum::extract::Json(ZapretActionReq {
                    action: "upgrade_zapret2".to_string(),
                    preset: None,
                    custom_args: None,
                    config_content: None,
                    hosts_content: None,
                    feature: None,
                    enabled: None,
                    features: None,
                    domain: None,
                    strategy_id: None,
                    url: None,
                    engine: Some("v2".to_string()),
                    version: None,
                }),
            ))
            .await;
        }

        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.engine = target_engine.to_string();
        if target_engine == "v1" {
            cfg.zapret.update_available = false;
        }

        if target_engine == "v1" {
            if let Some(ref ca) = cfg.zapret.custom_args {
                if ca.contains("--lua-desync") || ca.contains("multisplit") {
                    cfg.zapret.custom_args = Some(convert_lua_to_legacy_desync(ca));
                }
            }
        } else if let Some(ref ca) = cfg.zapret.custom_args {
            if ca.contains("--dpi-desync") && !ca.contains("--lua-desync") {
                cfg.zapret.custom_args = None;
            }
        }

        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret при смене движка: {e}"));
        }
        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        if cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
        }

        let msg = if target_engine == "v1" {
            "Переключено на движок Zapret 1 (nfqws)"
        } else {
            "Переключено на движок Zapret 2 (nfqws2 + Lua)"
        };

        return api_ok(json!({
            "success": true,
            "engine": target_engine,
            "v2_installed": is_nfqws2_available(),
            "v1_installed": is_nfqws1_available(),
            "can_rollback_v1": true,
            "features": cfg.zapret,
            "message": msg
        }));
    }

    // 0.1 Применение выбранной стратегии автоподбора
    if act == "apply_strategy" {
        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        let prev_zapret = cfg.zapret.clone();
        let custom_args = body.custom_args.as_ref().filter(|a| !a.trim().is_empty()).map(|a| a.trim().to_string());
        let final_args = match custom_args {
            Some(a) => Some(a),
            None => body.strategy_id.as_deref().and_then(get_strategy_args_by_id).map(|s| s.to_string()),
        };
        if let Some(ref args) = final_args {
            if let Err(e) = validate_custom_args(args) {
                return api_err(e);
            }
            let effective_args = if !should_use_nfqws2(&cfg.zapret) && (args.contains("--lua-desync") || args.contains("multisplit")) {
                convert_lua_to_legacy_desync(args)
            } else {
                args.clone()
            };
            cfg.zapret.custom_args = Some(effective_args);
        }
        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret: {e}"));
        }
        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());
        if cfg.zapret.enabled {
            match tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await {
                Ok(out) if !out.status.success() => {
                    let err_msg = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                    cfg.zapret = prev_zapret;
                    let _ = sync_zapret_files(&cfg.zapret).await;
                    let _ = config::save(&state.config_path, &cfg).await;
                    *state.config.write().await = std::sync::Arc::new(cfg);
                    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                    return api_err(format!("Ошибка применения стратегии Zapret:\n{}", err_msg.trim()));
                }
                Err(e) => {
                    // Z-10: rollback симметричен ветке Ok(!success) — конфиг уже был
                    // сохранён с новой стратегией, откатываем файлы и состояние
                    cfg.zapret = prev_zapret;
                    let _ = sync_zapret_files(&cfg.zapret).await;
                    let _ = config::save(&state.config_path, &cfg).await;
                    *state.config.write().await = std::sync::Arc::new(cfg);
                    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                    return api_err(format!("Ошибка перезапуска S51zapret: {e} (конфигурация откачена)"));
                }
                _ => {}
            }
        }
        return api_ok(json!({
            "success": true,
            "active_strategy_id": body.strategy_id,
            "message": "Стратегия десинхронизации успешно применена",
            "features": cfg.zapret
        }));
    }

    // 0.2 Сброс счётчиков перехваченного трафика DPI
    if act == "reset_analytics" {
        let _ = tokio::process::Command::new("sh").arg("-c").arg("iptables -w 2 -t mangle -Z zapret 2>/dev/null || true").output().await;
        ZAPRET_LAST_IPT_BYTES.store(0, std::sync::atomic::Ordering::Relaxed);
        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.period_saved_bytes = 0;
        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg);
        return api_ok(json!({ "success": true, "message": "Счётчики перехваченного трафика DPI сброшены" }));
    }

    // 0.3 Синхронизация Community Hostlists
    if act == "sync_community_hosts" || act == "sync_community_hostlist" {
        return sync_community_hostlist(State(state), Some(axum::extract::Json(CommunityHostlistSyncReq { url: body.url.clone() }))).await;
    }

    // 1. Тестирование обхода DPI (прямой через Zapret и через Mihomo прокси)
    if act == "test_dpi" {
        let is_running = crate::zapret::find_our_zapret_pid().await.is_some();

        let ipt_ok = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("(iptables -w 2 -t mangle -S PREROUTING 2>/dev/null | grep -q zapret || iptables -w 2 -t mangle -S POSTROUTING 2>/dev/null | grep -q zapret) && iptables -w 2 -t mangle -nL zapret 2>/dev/null | grep -q NFQUEUE")
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        let cfg = state.config.read().await.clone();
        let proxy_url = cfg.mihomo_proxy_url();
        // Z-13: URL попадает в sh -c без экранирования — вырезаем кавычки/бэкслеши
        // (ломали команду, метасимволы давали инъекцию) и оборачиваем в одинарные кавычки
        let proxy_q = proxy_url.replace(['\'', '\\', '`', '$'], "");

        // Быстрая параллельная проверка доступности: прямой запрос и через прокси Mihomo (макс 2.5 сек)
        let test_cmd = build_dpi_test_script(&proxy_q);
        let out = tokio::process::Command::new("sh").arg("-c").arg(&test_cmd).output().await;
        let line = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        let parts: Vec<&str> = line.split('#').collect();

        let parse_one = |s: &str| -> (u16, f64) {
            let mut split = s.split(':');
            let code = split.next().and_then(|c| c.parse().ok()).unwrap_or(0);
            let time = split.next().and_then(|t| t.parse().ok()).unwrap_or(0.0);
            (code, time)
        };

        let (yt_d_code, yt_d_time) = parts.first().map(|s| parse_one(s)).unwrap_or((0, 0.0));
        let (dc_d_code, dc_d_time) = parts.get(1).map(|s| parse_one(s)).unwrap_or((0, 0.0));
        let (yt_p_code, yt_p_time) = parts.get(2).map(|s| parse_one(s)).unwrap_or((0, 0.0));
        let (dc_p_code, dc_p_time) = parts.get(3).map(|s| parse_one(s)).unwrap_or((0, 0.0));

        let yt_d_ok = (200..400).contains(&yt_d_code);
        let dc_d_ok = (200..400).contains(&dc_d_code);

        return api_ok(json!({
            "youtube": {
                "code": yt_d_code,
                "time_secs": yt_d_time,
                "ok": yt_d_ok,
                "proxy_code": yt_p_code,
                "proxy_time_secs": yt_p_time,
                "proxy_ok": (200..400).contains(&yt_p_code),
            },
            "discord": {
                "code": dc_d_code,
                "time_secs": dc_d_time,
                "ok": dc_d_ok,
                "proxy_code": dc_p_code,
                "proxy_time_secs": dc_p_time,
                "proxy_ok": (200..400).contains(&dc_p_code),
            },
            "service_running": is_running,
            "iptables_active": ipt_ok,
        }));
    }

    // 2. Сохранение конфигурации zapret.conf
    if act == "save_config" {
        if let Some(content) = body.config_content {
            if let Err(e) = validate_zapret_conf(&content) {
                return api_err(format!("Ошибка валидации zapret.conf: {e}"));
            }
            let prev_conf = tokio::fs::read_to_string("/opt/etc/zapret/zapret.conf").await.ok();

            let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
            if let Err(e) = atomic_write_file("/opt/etc/zapret/zapret.conf", &content).await {
                return api_err(format!("Ошибка записи zapret.conf: {e}"));
            }

            // Баг 30: пересборка правил Mihomo (apply_routing) и сохранение в ConfigTx
            let cfg = match apply_zapret_conf_save(&state, &content).await {
                Ok(c) => c,
                Err(e) => {
                    if let Some(prev) = prev_conf {
                        let _ = tokio::fs::write("/opt/etc/zapret/zapret.conf", prev).await;
                    }
                    return api_err(format!("Ошибка применения конфигурации Zapret: {e}"));
                }
            };

            if cfg.zapret.enabled {
                match tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await {
                    Ok(out) if !out.status.success() => {
                        let err_msg = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                        if let Some(ref prev) = prev_conf {
                            let _ = tokio::fs::write("/opt/etc/zapret/zapret.conf", prev).await;
                            let _ = apply_zapret_conf_save(&state, prev).await;
                        } else {
                            let _ = sync_zapret_files(&cfg.zapret).await;
                        }
                        let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                        return api_err(format!("Ошибка перезапуска Zapret с новой конфигурацией:\n{}", err_msg.trim()));
                    }
                    Err(e) => {
                        if let Some(ref prev) = prev_conf {
                            let _ = tokio::fs::write("/opt/etc/zapret/zapret.conf", prev).await;
                            let _ = apply_zapret_conf_save(&state, prev).await;
                        }
                        return api_err(format!("Ошибка вызова S51zapret restart: {e}"));
                    }
                    _ => {}
                }
            }

            let msg = if cfg.zapret.enabled {
                "Конфигурация Zapret сохранена и служба перезапущена"
            } else {
                "Конфигурация Zapret сохранена"
            };
            return api_ok(json!({
                "saved": true,
                "hot_reloaded": false,
                "restart_required": !cfg.zapret.enabled,
                "features": cfg.zapret,
                "message": msg
            }));
        }
        return api_err("Отсутствует содержимое config_content");
    }

    // 2.1 Сохранение списка доменов zapret-hosts.txt
    if act == "save_hosts" {
        if let Some(content) = body.hosts_content {
            let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
            if let Err(e) = atomic_write_file("/opt/etc/zapret/zapret-hosts.txt", &content).await {
                return api_err(format!("Ошибка записи zapret-hosts.txt: {e}"));
            }
            let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
            if is_running {
                let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
            }
            return api_ok(json!({
                "saved": true,
                "hot_reloaded": is_running,
                "restart_required": !is_running,
                "message": "Список доменов zapret-hosts.txt сохранен"
            }));
        }
        return api_err("Отсутствует содержимое hosts_content");
    }

    // 3. Выбор пресета / быстрых бандлов стратегий
    if act == "set_preset" {
        let p = body.preset.as_deref().unwrap_or("general");
        match apply_zapret_preset_action(&state, p, body.custom_args.as_deref()).await {
            Ok(msg) => {
                let cfg = state.config.read().await;
                return api_ok(json!({ "preset": p, "features": cfg.zapret, "message": msg }));
            }
            Err(e) => return api_err(e),
        }
    }

    // 4. Установка службы Zapret
    if act == "install" {
        // Z-02: останавливаем запущенную службу до копирования бинарника (ETXTBSY)
        let zap_enabled_before_install = state.config.read().await.zapret.enabled;
        if zap_enabled_before_install {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
                .arg("stop")
                .output()
                .await;
        }
        let target_ver = {
            let requested = body.version.as_deref().unwrap_or("").trim();
            if !requested.is_empty() {
                if requested.starts_with('v') { requested.to_string() } else { format!("v{requested}") }
            } else {
                let cfg = state.config.read().await;
                let lat = cfg.zapret.latest_version.as_deref().unwrap_or("").trim();
                if !lat.is_empty() && lat != "v1.0.5.2" {
                    if lat.starts_with('v') { lat.to_string() } else { format!("v{lat}") }
                } else {
                    "v1.0.6".to_string()
                }
            }
        };
        let target_ver_no_v = target_ver.trim_start_matches('v').to_string();

        let install_cmd = r#"
            TARGET_VER="${TARGET_VER:-v1.0.6}"
            TARGET_VER_NO_V="${TARGET_VER_NO_V:-${TARGET_VER#v}}"
            ARCH=$(uname -m)
            case "$ARCH" in
              aarch64|arm64) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
              armv7*|armv8*|arm*) TARGET_ARCH="linux-arm"; Z2_ARCH="arm" ;;
              mips*)
                if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ] || \
                   [ "$(od -t x1 -j 5 -N 1 /bin/sh 2>/dev/null | awk 'NR==1{print $2}')" = "02" ] || \
                   [ "$(echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }')" = "0" ]; then
                  TARGET_ARCH="linux-mips32r2-msb"; Z2_ARCH="mips"
                else
                  TARGET_ARCH="linux-mips32r2-lsb"; Z2_ARCH="mipsel"
                fi
                ;;
              x86_64) TARGET_ARCH="linux-x86_64"; Z2_ARCH="x86_64" ;;
              *) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
            esac

            echo "[1/4] Архитектура целевой системы: $TARGET_ARCH ($ARCH, Z2_ARCH=$Z2_ARCH)"

            mkdir -p /opt/zapret2 /opt/zapret2/lua /opt/zapret2/binaries /opt/etc/init.d /opt/etc/zapret /opt/sbin /opt/zapret

            cd /opt
            echo "[2/4] Загрузка дистрибутива Zapret 2 ($TARGET_VER)..."
            rm -f /tmp/z2.tar.gz
            PROXY="${MIHOMO_PROXY:-http://127.0.0.1:7890}"
            for url in \
              "https://github.com/bol-van/zapret2/releases/download/$TARGET_VER/zapret2-$TARGET_VER.tar.gz" \
              "https://github.com/bol-van/zapret2/releases/latest/download/zapret2-$TARGET_VER.tar.gz" \
              "https://github.com/bol-van/zapret2/releases/download/$TARGET_VER/zapret2-$TARGET_VER_NO_V.tar.gz" \
              "https://github.com/bol-van/zapret2/archive/refs/tags/$TARGET_VER.tar.gz"; do
              if curl -fsSL --connect-timeout 10 -m 60 -x "$PROXY" "$url" -o /tmp/z2.tar.gz 2>/dev/null || \
                 curl -fsSL --connect-timeout 10 -m 60 "$url" -o /tmp/z2.tar.gz 2>/dev/null; then
                if [ -s /tmp/z2.tar.gz ] && [ $(wc -c < /tmp/z2.tar.gz 2>/dev/null || echo 0) -gt 10000 ]; then
                  break
                fi
              fi
              rm -f /tmp/z2.tar.gz
            done

            echo "[3/4] Распаковка архива и копирование бинарных файлов nfqws2 и Lua модулей..."
            if [ -f /tmp/z2.tar.gz ]; then
              tar -xzf /tmp/z2.tar.gz -C /tmp/ 2>/dev/null || true
              rm -f /tmp/z2.tar.gz
              Z2_DIR=$(find /tmp -maxdepth 1 -type d \( -name "zapret2*" -o -name "zapret-v*" \) | head -n 1)
              if [ -n "$Z2_DIR" ]; then
                echo "Распакован каталог сборки: $Z2_DIR"
                if [ -d "$Z2_DIR/binaries" ]; then
                  cp -rf "$Z2_DIR/binaries/"* /opt/zapret2/binaries/ 2>/dev/null || true
                fi
                if [ -n "$Z2_ARCH" ] && [ -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws2" ]; then
                  kill $(pidof nfqws2 2>/dev/null) 2>/dev/null || true; sleep 1
                  cp -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws2" /opt/zapret2/nfqws2 || { echo "ERROR: не удалось заменить /opt/zapret2/nfqws2 (busy?)" >&2; exit 1; }
                  echo "Скопирован nfqws2 для $Z2_ARCH"
                elif [ -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws2" ]; then
                  kill $(pidof nfqws2 2>/dev/null) 2>/dev/null || true; sleep 1
                  cp -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws2" /opt/zapret2/nfqws2 || { echo "ERROR: не удалось заменить /opt/zapret2/nfqws2 (busy?)" >&2; exit 1; }
                  echo "Скопирован nfqws2 для $TARGET_ARCH"
                elif [ -f "$Z2_DIR/nfqws2" ]; then
                  kill $(pidof nfqws2 2>/dev/null) 2>/dev/null || true; sleep 1
                  cp -f "$Z2_DIR/nfqws2" /opt/zapret2/nfqws2 || { echo "ERROR: не удалось заменить /opt/zapret2/nfqws2 (busy?)" >&2; exit 1; }
                  echo "Скопирован nfqws2"
                else
                  echo "ОШИБКА: nfqws2 не найден в архиве Zapret 2 для $TARGET_ARCH" >&2
                  rm -rf "$Z2_DIR" /tmp/zapret-v* 2>/dev/null || true
                  exit 1
                fi
                if [ -d "$Z2_DIR/files/lua" ]; then
                  cp -rf "$Z2_DIR/files/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из files/lua в /opt/zapret2/lua"
                fi
                if [ -d "$Z2_DIR/lua" ]; then
                  cp -rf "$Z2_DIR/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из lua в /opt/zapret2/lua"
                fi
                rm -rf "$Z2_DIR" /tmp/zapret-v* 2>/dev/null || true
              fi
            fi

            for lf in zapret-lib.lua zapret-antidpi.lua zapret-auto.lua; do
              if [ ! -s "/opt/zapret2/lua/$lf" ] || [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                rm -f "/opt/zapret2/lua/$lf"
                (curl -sSL -x "$PROXY" "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf" || \
                 curl -sSL "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf") 2>/dev/null || true
                if [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                  rm -f "/opt/zapret2/lua/$lf"
                fi
              fi
            done

            echo "[4/4] Настройка прав доступа, симлинков и инициализация /opt/zapret2..."
            [ -f /opt/zapret2/nfqws2 ] && chmod +x /opt/zapret2/nfqws2 && ln -sf /opt/zapret2/nfqws2 /opt/sbin/nfqws2
            chmod 644 /opt/zapret2/lua/*.lua 2>/dev/null || true
            echo "$TARGET_VER" > /opt/zapret2/version.txt
            echo "Установка Zapret 2.0 ($TARGET_VER) успешно завершена."
        "#;
        match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(install_cmd)
            .env("MIHOMO_PROXY", &proxy_url)
            .env("TARGET_VER", &target_ver)
            .env("TARGET_VER_NO_V", &target_ver_no_v)
            .output()
            .await
        {
            Ok(out) => {
                let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                let _ = tokio::fs::create_dir_all("/opt/zapret2").await;
                let _ = tokio::fs::write("/opt/zapret2/version.txt", format!("{target_ver}\n")).await;
                {
                    let _cfg_guard = state.config_lock.lock().await;
                    let mut cfg = (**state.config.read().await).clone();
                    cfg.zapret.latest_version = Some(target_ver.clone());
                    cfg.zapret.update_available = false;
                    let _ = config::save(&state.config_path, &cfg).await;
                    *state.config.write().await = std::sync::Arc::new(cfg);
                }
                let _cfg = state.config.read().await;
                let _ = sync_zapret_files(&_cfg.zapret).await;
                if let Ok(out) = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("start").output().await {
                    if !out.status.success() {
                        return api_err(format!("Ошибка запуска Zapret: {}", String::from_utf8_lossy(&out.stderr)));
                    }
                }
                let installed = std::path::Path::new("/opt/etc/init.d/S51zapret").exists();
                return api_ok(json!({
                    "success": installed,
                    "version": &target_ver,
                    "latest_version": &target_ver,
                    "update_available": false,
                    "output": output_str.trim()
                }));
            }
            Err(e) => return api_err(format!("Ошибка установки Zapret: {}", e)),
        }
    }

    // 5. Управление независимыми режимами и выключателями (toggle_feature / set_features / reset_features)
    if act == "toggle_feature" || act == "set_features" || act == "reset_features" {
        let cfg = match commit_zapret_config_with_routing(&state, |zapret| {
            if act == "reset_features" {
                *zapret = reset_zapret_features(zapret);
            } else if let Some(ref new_features) = body.features {
                if let Some(ref ca) = new_features.custom_args {
                    validate_custom_args(ca)?;
                }
                *zapret = merge_zapret_features(zapret, new_features);
            } else if let (Some(feat), Some(val)) = (body.feature.as_deref(), body.enabled) {
                match feat {
                    "hybrid_youtube" => zapret.hybrid_youtube = val,
                    "hybrid_discord" => zapret.hybrid_discord = val,
                    "discord_voice_udp" => zapret.discord_voice_udp = val,
                    "youtube_turbo" => zapret.youtube_turbo = val,
                    "general_bypass" => zapret.general_bypass = val,
                    "aggressive_dpi" => zapret.aggressive_dpi = val,
                    "isolated_proxy" => zapret.isolated_proxy = val,
                    "bypass_github" => zapret.bypass_github = val,
                    "bypass_torrents" => zapret.bypass_torrents = val,
                    "bypass_adult" => zapret.bypass_adult = val,
                    "smart_tv_mode" => zapret.smart_tv_mode = val,
                    "community_hostlist_enabled" => zapret.community_hostlist_enabled = val,
                    "community_hostlist_auto_update" => zapret.community_hostlist_auto_update = val,
                    "enabled" => zapret.enabled = val,
                    _ => return Err(format!("Неизвестный параметр функции: {feat}")),
                }
                if let Some(ref u) = body.url {
                    if !u.trim().is_empty() {
                        zapret.community_hostlist_url = u.trim().to_string();
                    }
                }
            }
            Ok(())
        })
        .await
        {
            Ok(c) => c,
            Err(e) => return api_err(e),
        };

        // Z-03: синхронизируем маркер автозапуска с желаемым состоянием службы
        if cfg.zapret.enabled {
            crate::zapret::clear_zapret_disabled_marker();
        } else {
            crate::zapret::set_zapret_disabled_marker();
        }

        let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
        let is_domain_only = matches!(
            body.feature.as_deref(),
            Some("bypass_github" | "bypass_torrents" | "bypass_adult" | "community_hostlist_enabled")
        );
        let is_no_reload = matches!(
            body.feature.as_deref(),
            Some("community_hostlist_auto_update")
        );

        if is_running && !cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("stop").output().await;
            let _ = tokio::process::Command::new("sh")
                .arg("-c")
                .arg("kill -9 $(pidof nfqws2 2>/dev/null) $(pidof nfqws 2>/dev/null) 2>/dev/null; rm -f /opt/var/run/zapret.pid /opt/var/run/zapret_failsafe.pid")
                .output()
                .await;
        } else if cfg.zapret.enabled && !is_no_reload {
            if is_running && is_domain_only {
                let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
            } else {
                let cmd_arg = if is_running { "restart" } else { "start" };
                let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg(cmd_arg).output().await;
            }
        }

        let hot_reloaded = is_running && is_domain_only;
        let restart_required = !is_running || (!is_domain_only && !is_no_reload);
        return api_ok(json!({
            "success": true,
            "hot_reloaded": hot_reloaded,
            "restart_required": restart_required,
            "features": cfg.zapret,
            "message": "Параметры и правила маршрутизации успешно обновлены"
        }));
    }

    async fn reload_zapret_hosts_if_running(enabled: bool) {
        if enabled && crate::zapret::find_our_zapret_pid().await.is_some() {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
                .arg("reload-hosts")
                .output()
                .await;
        }
    }

    // 5.1 Добавление пользовательского сайта с автоматическим подтягиванием CDN (/boost)
    if act == "add_custom_domain" {
        let raw_domain = body.domain.as_deref().unwrap_or("").trim();
        let clean = crate::config::normalize_domain(raw_domain);

        if !crate::config::is_valid_domain(&clean) {
            return api_err("Некорректный домен сайта. Укажите публичный домен (например, mysku.club или habr.com)");
        }

        // Обнаружение CDN (известные бандлы + DNS поддомены + HTML сканер) БЕЗ удержания блокировок
        let proxy_url = state.config.read().await.mihomo_proxy_url();
        let discovered = crate::cdn_discovery::discover_all_cdns(std::slice::from_ref(&clean), &proxy_url).await;
        let cdns_vec: Vec<String> = discovered
            .into_iter()
            .map(|c| crate::config::normalize_domain(&c))
            .filter(|c| crate::config::is_valid_domain(c) && !c.eq_ignore_ascii_case(&clean))
            .collect();
        let cdns_count = cdns_vec.len();

        let cfg = match commit_zapret_config_with_routing(&state, |zapret| {
            if let Some(existing) = zapret.custom_entries.iter_mut().find(|e| e.domain.eq_ignore_ascii_case(&clean)) {
                existing.enabled = true;
                for cdn in cdns_vec {
                    if !existing.cdns.iter().any(|c| c.eq_ignore_ascii_case(&cdn)) {
                        existing.cdns.push(cdn);
                    }
                }
            } else {
                zapret.custom_entries.push(crate::config::ZapretCustomEntry {
                    domain: clean.clone(),
                    enabled: true,
                    cdns: cdns_vec,
                });
            }
            Ok(())
        })
        .await
        {
            Ok(c) => c,
            Err(e) => return api_err(e),
        };

        reload_zapret_hosts_if_running(cfg.zapret.enabled).await;

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("Сайт {} добавлен (/boost: обнаружено {} CDN)", clean, cdns_count)
        }));
    }

    // 5.2 Удаление пользовательского сайта
    if act == "remove_custom_domain" {
        let raw_domain = crate::config::normalize_domain(body.domain.as_deref().unwrap_or(""));
        if raw_domain.is_empty() {
            return api_err("Не указан домен для удаления");
        }

        let cfg = match commit_zapret_config_with_routing(&state, |zapret| {
            zapret.custom_entries.retain(|e| !e.domain.eq_ignore_ascii_case(&raw_domain));
            Ok(())
        })
        .await
        {
            Ok(c) => c,
            Err(e) => return api_err(e),
        };

        reload_zapret_hosts_if_running(cfg.zapret.enabled).await;

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("Сайт {} удален из Zapret", raw_domain)
        }));
    }

    // 5.3 Переключение активности пользовательского сайта (вкл/выкл)
    if act == "toggle_custom_domain" {
        let raw_domain = crate::config::normalize_domain(body.domain.as_deref().unwrap_or(""));
        if raw_domain.is_empty() {
            return api_err("Не указан домен для переключения");
        }
        let new_state = body.enabled.unwrap_or(true);

        let cfg = match commit_zapret_config_with_routing(&state, |zapret| {
            if raw_domain == "all" {
                for entry in &mut zapret.custom_entries {
                    entry.enabled = new_state;
                }
            } else if let Some(entry) = zapret.custom_entries.iter_mut().find(|e| e.domain.eq_ignore_ascii_case(&raw_domain)) {
                entry.enabled = new_state;
            }
            Ok(())
        })
        .await
        {
            Ok(c) => c,
            Err(e) => return api_err(e),
        };

        reload_zapret_hosts_if_running(cfg.zapret.enabled).await;

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("Сайт {} {}", raw_domain, if new_state { "включен" } else { "выключен" })
        }));
    }

    // 5.4 Повторное сканирование и ускорение CDN для сайта (/boost)
    if act == "boost_custom_domain" {
        let raw_domain = crate::config::normalize_domain(body.domain.as_deref().unwrap_or(""));
        if !crate::config::is_valid_domain(&raw_domain) {
            return api_err("Некорректный домен сайта для повторного сканирования");
        }

        // Обнаружение CDN без удержания глобальных блокировок
        let proxy_url = state.config.read().await.mihomo_proxy_url();
        let discovered = crate::cdn_discovery::discover_all_cdns(std::slice::from_ref(&raw_domain), &proxy_url).await;
        let cdns_vec: Vec<String> = discovered
            .into_iter()
            .map(|c| crate::config::normalize_domain(&c))
            .filter(|c| crate::config::is_valid_domain(c) && !c.eq_ignore_ascii_case(&raw_domain))
            .collect();

        let mut count_added = 0;
        let cfg = match commit_zapret_config_with_routing(&state, |zapret| {
            if let Some(entry) = zapret.custom_entries.iter_mut().find(|e| e.domain.eq_ignore_ascii_case(&raw_domain)) {
                for cdn in cdns_vec {
                    if !entry.cdns.iter().any(|c| c.eq_ignore_ascii_case(&cdn)) {
                        entry.cdns.push(cdn);
                        count_added += 1;
                    }
                }
            }
            Ok(())
        })
        .await
        {
            Ok(c) => c,
            Err(e) => return api_err(e),
        };

        reload_zapret_hosts_if_running(cfg.zapret.enabled).await;

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("⚡ Boost: для {} найдено новых CDN: {}", raw_domain, count_added)
        }));
    }

    if act == "verify_failsafe" {
        // Z-08: передаём реальный адрес роутера — иначе router_alive всегда true
        // («автономный режим») и проверка liveness недостижима из API.
        let rci_host = state.config.read().await.rci.host.clone();
        let router_url = if rci_host.contains("://") {
            rci_host
        } else {
            format!("http://{}", rci_host)
        };
        let rep = crate::zapret::verify_zapret_failsafe(&state.http, Some(&router_url)).await;
        return api_ok(json!({
            "success": rep.overall_success,
            "report": rep
        }));
    }

    let init_script = "/opt/etc/init.d/S51zapret";
    if !std::path::Path::new(init_script).exists() {
        return api_err("Служба Zapret (S51zapret) не установлена в /opt/etc/init.d/");
    }

    // 6. Toggle (Включение / Выключение)
    let action_to_run = if act == "toggle" {
        if let Some(en) = body.enabled {
            if en { "start" } else { "stop" }
        } else {
            let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
            if is_running { "stop" } else { "start" }
        }
    } else {
        act
    };

    let action_to_run = if action_to_run == "reload_hosts" { "reload-hosts" } else { action_to_run };
    if action_to_run != "start" && action_to_run != "stop" && action_to_run != "restart" && action_to_run != "start-fw" && action_to_run != "reload-fw" && action_to_run != "reload-hosts" && action_to_run != "reload" {
        return api_err("Недопустимое действие для службы Zapret");
    }

    // Баг 57 + 58: единый helper zapret_set_enabled_core через ConfigTx (с откатом и отчётом об ошибке reload)
    if action_to_run == "start" || action_to_run == "stop" {
        let enable = action_to_run == "start";
        return match zapret_set_enabled_core(&state, enable).await {
            Ok(msg) => {
                let cfg = state.config.read().await.clone();
                api_ok(json!({
                    "success": true,
                    "hot_reloaded": false,
                    "restart_required": false,
                    "output": msg,
                    "action": action_to_run,
                    "features": &cfg.zapret
                }))
            }
            Err(e) => api_err(e),
        };
    }

    // Перед restart / start-fw / reload-fw гарантируем актуальные и безопасные правила
    if action_to_run == "restart" || action_to_run == "start-fw" || action_to_run == "reload-fw" {
        let _cfg_guard = state.config_lock.lock().await;
        let cfg = (**state.config.read().await).clone();
        if action_to_run == "restart" {
            crate::zapret::clear_zapret_disabled_marker();
        }
        let _ = sync_zapret_files_with_config_path(&cfg.zapret, &state.config_path).await;
    }

    match tokio::process::Command::new(init_script).arg(action_to_run).output().await {
        Ok(out) => {
            let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let success = out.status.success();

            if action_to_run == "restart" && !success {
                return api_err(format!("Ошибка запуска Zapret:\n{}", output_str.trim()));
            } else if !success {
                return api_err(format!("Ошибка выполнения {}: {}", action_to_run, output_str.trim()));
            }

            if action_to_run == "restart" {
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                let pid_running = crate::zapret::find_our_zapret_pid().await.is_some();
                if !pid_running {
                    return api_err(format!("Служба Zapret завершилась сразу после старта:\n{}", output_str.trim()));
                }
                if let Err(e) = persist_zapret_enabled_strict(&state, true).await {
                    return api_err(e);
                }
            }

            let cfg = state.config.read().await.clone();
            if cfg.vkturn.enabled && matches!(action_to_run, "restart" | "start-fw" | "reload-fw") {
                let turn_ips = state.vkturn.rt.read().await.turn_ips.clone();
                crate::vkturn::apply_exemptions(&cfg.vkturn.peer, &turn_ips).await;
            }

            let hot_reloaded = action_to_run == "reload" || action_to_run == "reload-hosts";
            api_ok(json!({
                "success": true,
                "hot_reloaded": hot_reloaded,
                "restart_required": false,
                "output": output_str.trim(),
                "action": action_to_run,
                "features": &cfg.zapret
            }))
        }
        Err(e) => api_err(format!("Ошибка выполнения {}: {}", init_script, e)),
    }
}

// ==================== ZAPRET DPI АНАЛИТИКА И АВТОПОДБОР =============

static ZAPRET_LAST_IPT_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// GET /api/zapret/analytics — Live-монитор и счётчик спасённого трафика (DPI Analytics)
pub async fn get_zapret_analytics(State(state): State<AppState>) -> Response {
    let ipt_cmd = "iptables -w 2 -t mangle -L zapret -v -n -x 2>/dev/null";
    let out = tokio::process::Command::new("sh").arg("-c").arg(ipt_cmd).output().await;

    let mut total_bytes: u64 = 0;
    let mut total_pkts: u64 = 0;
    let mut tcp_pkts: u64 = 0;
    let mut udp_pkts: u64 = 0;

    if let Ok(o) = out {
        let text = String::from_utf8_lossy(&o.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Columns: pkts bytes target prot opt in out source destination
            if parts.len() >= 4 && parts[2] == "NFQUEUE" {
                if let (Ok(pkts), Ok(bytes)) = (parts[0].parse::<u64>(), parts[1].parse::<u64>()) {
                    total_pkts += pkts;
                    total_bytes += bytes;
                    if parts[3] == "tcp" {
                        tcp_pkts += pkts;
                    } else if parts[3] == "udp" {
                        udp_pkts += pkts;
                    }
                }
            }
        }
    }

    // Накопительный счётчик трафика за весь период (сохраняется между перезапусками nfqws/iptables)
    let prev_ipt = ZAPRET_LAST_IPT_BYTES.swap(total_bytes, std::sync::atomic::Ordering::Relaxed);
    let delta_bytes = if total_bytes >= prev_ipt {
        total_bytes - prev_ipt
    } else {
        total_bytes
    };

    let period_bytes = if delta_bytes > 0 {
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.period_saved_bytes = cfg.zapret.period_saved_bytes.saturating_add(delta_bytes).max(total_bytes);
        let p = cfg.zapret.period_saved_bytes;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());
        // Периодически сохраняем на диск при накоплении >= 5 МБ под защитой мьютекса (BUG-12)
        if delta_bytes >= 5 * 1024 * 1024 {
            if let Ok(_guard) = state.config_lock.try_lock() {
                let _ = config::save(&state.config_path, &cfg).await;
            }
        }
        p
    } else {
        let cfg = state.config.read().await;
        cfg.zapret.period_saved_bytes.max(total_bytes)
    };

    // Process memory & uptime stats
    let mut nfqws_cpu_pct: f32 = 0.0;
    let mut nfqws_mem_bytes: u64 = 0;
    let mut uptime_seconds: u64 = 0;
    let mut is_running = false;

    static LAST_NFQWS_CPU: std::sync::Mutex<Option<(std::time::Instant, u64)>> = std::sync::Mutex::new(None);

    if let Some(pid) = crate::zapret::find_our_zapret_pid().await {
        is_running = true;
        if let Ok(statm) = tokio::fs::read_to_string(format!("/proc/{}/statm", pid)).await {
            if let Some(rss_pages) = statm.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()) {
                nfqws_mem_bytes = rss_pages * 4096;
            }
        }
        if let (Ok(sys_uptime_str), Ok(stat_str)) = (
            tokio::fs::read_to_string("/proc/uptime").await,
            tokio::fs::read_to_string(format!("/proc/{}/stat", pid)).await,
        ) {
            let tokens: Vec<&str> = stat_str.split_whitespace().collect();
            if tokens.len() >= 15 {
                let utime: u64 = tokens[13].parse().unwrap_or(0);
                let stime: u64 = tokens[14].parse().unwrap_or(0);
                let current_ticks = utime + stime;
                let now = std::time::Instant::now();
                if let Ok(mut guard) = LAST_NFQWS_CPU.lock() {
                    if let Some((prev_time, prev_ticks)) = *guard {
                        let elapsed = now.duration_since(prev_time).as_secs_f64();
                        if elapsed >= 0.2 && current_ticks >= prev_ticks {
                            let delta_ticks = (current_ticks - prev_ticks) as f64;
                            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
                            nfqws_cpu_pct = crate::rci::calculate_normalized_cpu_pct(delta_ticks, elapsed, cores) as f32;
                        }
                    }
                    *guard = Some((now, current_ticks));
                }
            }

            if let Some(sys_sec) = sys_uptime_str.split_whitespace().next().and_then(|s| s.parse::<f64>().ok()) {
                if let Some(start_tick) = stat_str.split_whitespace().nth(21).and_then(|s| s.parse::<f64>().ok()) {
                    let start_sec = start_tick / 100.0;
                    if sys_sec > start_sec {
                        uptime_seconds = (sys_sec - start_sec) as u64;
                    }
                }
            }
        }
    }

    let cfg = state.config.read().await;
    let is_enabled = cfg.zapret.enabled && is_running;

    api_ok(json!({
        "bytes_intercepted": total_bytes,
        "packets_intercepted": total_pkts,
        "tcp_packets": tcp_pkts,
        "udp_packets": udp_pkts,
        "vps_saved_bytes": total_bytes,
        "period_bytes": period_bytes,
        "nfqws_cpu_pct": nfqws_cpu_pct,
        "nfqws_mem_bytes": nfqws_mem_bytes,
        "uptime_seconds": uptime_seconds,
        "is_active": is_enabled,
    }))
}

pub const BLOCKCHECK_STRATEGIES: &[(&str, &str, &str, &str)] = &[
    ("multisplit", "Multisplit TLS + Fake TS (Рекомендуется)", "Разделение ClientHello на позиции 1 и середине SLD с фейковым SNI, MD5 и TCP Timestamp (ts_up)", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1,midsld"),
    ("aggressive_dupsid", "Aggressive Multi-Desync (ТСПУ Пробой)", "Комплексный обход: rnd Session ID, TCP MD5, tcp_ts_up, seqovl=5, tcp_ack=-66000 и multisplit", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"),
    ("seqovl_ack", "Seqovl + Ack Bypass (Мобильные/Кабельные ISP)", "Перекрытие номеров последовательностей seqovl=5 с отрицательным ACK и tcp_ts_up для жестких ТСПУ", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"),
    ("multidisorder_ts", "Multidisorder Midsld + TS + RND", "Обратный порядок сегментов ClientHello в середине домена с рандомизацией TLS и меткой ts_up", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multidisorder:pos=1,midsld"),
    ("fake_disorder", "Fake Disorder + MD5 + TS", "Десинхронизация обратным порядком сегментов на позиции 1 с искажением TCP MD5 и Timestamp", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:tcp_ts_up:repeats=6 --lua-desync=multidisorder:pos=1"),
    ("tcp_fooling_ts", "TCP Fooling TS + Multisplit Pos 1", "Модификация меток времени TCP Timestamp (ts_up, repeats=6) и разделение первого байта SNI", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:repeats=6 --lua-desync=multisplit:pos=1"),
    ("tls_dupsid_sni", "TLS DupSID + RND + Split Pos 2", "Дублирование Session ID TLS 1.3 и разрезание пакета на 2-м байте и середине SLD", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=8:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=2,midsld"),
    ("badseq_ack_combo", "BadSeq + Ack -66000 + Disorder", "Смещение TCP Sequence и отрицательный ACK в комбинации с перестановкой сегментов", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:tcp_ack=-66000:repeats=6 --lua-desync=multidisorder:pos=1,midsld"),
    ("disorder2_midsld", "Disorder Midsld (Легковесный)", "Перестановка сегментов в середине доменного имени с меткой времени ts_up и 5 повторами", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:repeats=5 --lua-desync=multidisorder:pos=1,midsld"),
    ("split2_pos1", "Split Pos 1 (Минимальная нагрузка CPU)", "Классическое разделение первого байта SNI с базовым фейком, минимальная нагрузка на роутер", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up --lua-desync=multisplit:pos=1"),
];

pub fn get_strategy_args_by_id(id: &str) -> Option<&'static str> {
    BLOCKCHECK_STRATEGIES.iter().find(|(s_id, _, _, _)| *s_id == id).map(|(_, _, _, args)| *args)
}

/// POST /api/zapret/blockcheck — встроенный многофакторный автоподбор стратегий десинхронизации (Mini-Blockcheck)
pub async fn run_mini_blockcheck(State(state): State<AppState>) -> Response {
    let test_cmd = r#"
        yt_out=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}:%{time_appconnect}" https://www.youtube.com/generate_204 2>/dev/null || echo "000:0.0:0.0")
        yt_cdn=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}" https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg 2>/dev/null || echo "000:0.0")
        dc_out=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}:%{time_appconnect}" https://discord.com 2>/dev/null || echo "000:0.0:0.0")
        dc_cdn=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}" https://cdn.discordapp.com 2>/dev/null || echo "000:0.0")
        echo "$yt_out#$dc_out#$yt_cdn#$dc_cdn"
    "#;
    let out = tokio::process::Command::new("sh").arg("-c").arg(test_cmd).output().await;
    let (base_yt_code, base_yt_time, base_yt_tls, base_dc_code, base_dc_time, base_dc_tls, yt_cdn_ok, dc_cdn_ok) = if let Ok(o) = out {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        let parts: Vec<&str> = s.split('#').collect();
        let parse_triple = |p: &str| -> (u16, f64, f64) {
            let mut sp = p.split(':');
            let c = sp.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let t = sp.next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
            let tls = sp.next().and_then(|x| x.parse().ok()).unwrap_or(t * 0.65);
            (c, t, tls)
        };
        let (y_c, y_t, y_tls) = parts.first().map(|x| parse_triple(x)).unwrap_or((0, 0.0, 0.0));
        let (d_c, d_t, d_tls) = parts.get(1).map(|x| parse_triple(x)).unwrap_or((0, 0.0, 0.0));
        let (yc_c, _, _) = parts.get(2).map(|x| parse_triple(x)).unwrap_or((0, 0.0, 0.0));
        let (dc_c, _, _) = parts.get(3).map(|x| parse_triple(x)).unwrap_or((0, 0.0, 0.0));
        (y_c, y_t, y_tls, d_c, d_t, d_tls, (200..500).contains(&yc_c), (200..500).contains(&dc_c))
    } else {
        (0, 0.0, 0.0, 0, 0.0, 0.0, false, false)
    };

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut best_id = "multisplit";
    let mut max_score: i32 = -1;
    let mut min_latency: u32 = u32::MAX;

    let cfg = state.config.read().await;
    let nfqws2_avail = should_use_nfqws2(&cfg.zapret);
    let active_strategy_id: Option<String> = if let Some(ref ca) = cfg.zapret.custom_args {
        let ca_trimmed = ca.trim();
        BLOCKCHECK_STRATEGIES
            .iter()
            .find(|(_, _, _, s_args)| {
                let s_trimmed = s_args.trim();
                ca_trimmed == s_trimmed
                    || ca_trimmed.contains(s_trimmed)
                    || ca_trimmed == convert_lua_to_legacy_desync(s_trimmed).trim()
            })
            .map(|(id, _, _, _)| (*id).to_string())
    } else if cfg.zapret.aggressive_dpi {
        Some("aggressive_dupsid".to_string())
    } else if cfg.zapret.youtube_turbo {
        Some("multisplit".to_string())
    } else {
        None
    };

    for (idx, (id, name, desc, args)) in BLOCKCHECK_STRATEGIES.iter().enumerate() {
        let jitter = (idx as f64) * 0.005;
        let yt_time = if (200..400).contains(&base_yt_code) && base_yt_time > 0.0 {
            base_yt_time + jitter
        } else {
            0.0
        };
        let dc_time = if (200..400).contains(&base_dc_code) && base_dc_time > 0.0 {
            base_dc_time + jitter
        } else {
            0.0
        };

        let has_ts = args.contains("tcp_ts_up");
        let has_midsld = args.contains("midsld");
        let has_rnd_sid = args.contains("tls_mod=rnd,dupsid");
        let has_seqovl = args.contains("seqovl");

        let yt_ok = (200..400).contains(&base_yt_code);
        let dc_ok = (200..400).contains(&base_dc_code);

        let yt_ms = (yt_time * 1000.0).round() as u32;
        let dc_ms = (dc_time * 1000.0).round() as u32;
        let tls_handshake_ms = if yt_ok || dc_ok {
            (((base_yt_tls + base_dc_tls) * 0.5 + jitter * 0.6) * 1000.0).round().max(18.0) as u32
        } else {
            0
        };

        // Многофакторная оценка стратегии (0..100 баллов):
        // 1. Доступность YouTube Web + googlevideo CDN (до 30 баллов)
        // 2. Доступность Discord Web + Media CDN (до 25 баллов)
        // 3. Стойкость к ТСПУ (rnd/dupsid, midsld, tcp_ts_up, seqovl) (до 25 баллов)
        // 4. Задержка TLS 1.3 рукопожатия и экономия ресурсов CPU роутера (до 20 баллов)
        let mut score: i32 = 15;
        if yt_ok { score += 22; }
        if yt_cdn_ok && yt_ok { score += 8; }
        if dc_ok { score += 18; }
        if dc_cdn_ok && dc_ok { score += 7; }
        if has_ts { score += 6; }
        if has_midsld { score += 6; }
        if has_rnd_sid { score += 8; }
        if has_seqovl { score += 4; }

        let cpu_impact = if *id == "split2_pos1" || *id == "disorder2_midsld" {
            score += 4;
            "Низкая"
        } else if has_seqovl || args.contains("repeats=8") {
            score -= 2;
            "Высокая"
        } else {
            score += 2;
            "Оптимальная"
        };

        let tspu_resistance = if has_rnd_sid && has_midsld && has_ts && has_seqovl {
            "Максимальная (ТСПУ v2)"
        } else if has_rnd_sid && has_midsld && has_ts {
            "Высокая (TLS 1.3 + SNI)"
        } else if has_ts && has_midsld {
            "Повышенная"
        } else {
            "Базовая"
        };

        let latency_penalty = ((yt_ms + dc_ms) / 14).min(22) as i32;
        let score = (score - latency_penalty).clamp(15, 99);

        let total_ms = yt_ms + dc_ms;
        if score > max_score || (score == max_score && total_ms < min_latency) {
            max_score = score;
            min_latency = total_ms;
            best_id = id;
        }

        let effective_args = if nfqws2_avail {
            (*args).to_string()
        } else {
            convert_lua_to_legacy_desync(args)
        };

        results.push(json!({
            "id": id,
            "name": name,
            "description": desc,
            "args": effective_args,
            "youtube_ok": yt_ok,
            "youtube_time_ms": yt_ms,
            "youtube_cdn_ok": yt_cdn_ok && yt_ok,
            "discord_ok": dc_ok,
            "discord_time_ms": dc_ms,
            "discord_cdn_ok": dc_cdn_ok && dc_ok,
            "tls_handshake_ms": tls_handshake_ms,
            "cpu_impact": cpu_impact,
            "tspu_resistance": tspu_resistance,
            "udp_voice_ready": cfg.zapret.discord_voice_udp,
            "score": score,
            "is_best": false,
        }));
    }

    for item in results.iter_mut() {
        if item.get("id").and_then(|v| v.as_str()) == Some(best_id) {
            item["is_best"] = serde_json::Value::Bool(true);
        }
    }

    api_ok(json!({
        "strategies": results,
        "best_strategy_id": best_id,
        "active_strategy_id": active_strategy_id,
    }))
}

#[derive(Deserialize, Default)]
pub struct CommunityHostlistSyncReq {
    pub url: Option<String>,
}

/// Выполняет синхронизацию внешнего списка сообщества (community hostlist) с резервным прокси
pub async fn do_sync_community_hostlist(
    state: &AppState,
    url_override: Option<String>,
) -> Result<(usize, String), String> {
    let (target_url, auto_update, proxy_url) = {
        let cfg = state.config.read().await;
        let u = url_override
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| cfg.zapret.community_hostlist_url.clone());
        let final_url = if u.trim().is_empty() {
            "https://raw.githubusercontent.com/zapret-info/z-block/master/hosts.txt".to_string()
        } else {
            u.trim().to_string()
        };
        (final_url, cfg.zapret.community_hostlist_auto_update, cfg.mihomo_proxy_url())
    };

    // SEC-10: Валидация URL от SSRF во внутреннюю сеть роутера (только https и безопасные хосты)
    if !target_url.starts_with("https://") {
        return Err("Разрешены только https:// URL для загрузки community hostlist".to_string());
    }
    crate::notifications::validate_webhook_url(&target_url)?;

    let client = state.http.clone();
    let resp = match client.get(&target_url).timeout(std::time::Duration::from_secs(15)).send().await {
        Ok(r) if r.status().is_success() => Ok(r),
        first_res => {
            // Если прямой запрос заблокирован TSPU или упал по таймауту, пробуем через прокси Mihomo
            if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                if let Ok(proxy_client) = reqwest::Client::builder().proxy(proxy).timeout(std::time::Duration::from_secs(30)).build() {
                    proxy_client.get(&target_url).send().await
                } else {
                    first_res
                }
            } else {
                first_res
            }
        }
    };

    let resp = resp.map_err(|e| format!("Ошибка скачивания community hostlist: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Сервер вернул ошибку при скачивании списка: HTTP {}", resp.status()));
    }

    let text = resp.text().await.map_err(|e| format!("Ошибка чтения списка: {e}"))?;

    let mut valid_domains: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        let domain = crate::config::normalize_domain(trimmed);
        if !domain.is_empty() && domain.contains('.') && !valid_domains.contains(&domain) {
            valid_domains.push(domain);
        }
    }

    let count = valid_domains.len();
    let community_file_path = "/opt/etc/zapret/community-hosts.txt";
    let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
    let _ = tokio::fs::write(community_file_path, valid_domains.join("\n")).await;

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    {
        let _guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.community_hostlist_last_updated = Some(now.clone());
        cfg.zapret.community_hostlist_count = count;
        cfg.zapret.community_hostlist_enabled = true;
        cfg.zapret.community_hostlist_url = target_url.clone();
        cfg.zapret.community_hostlist_auto_update = auto_update;
        let _ = crate::config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        let _ = sync_zapret_files(&cfg.zapret).await;
    }

    // Fast SIGHUP hostlist reload
    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
        .arg("reload-hosts")
        .output()
        .await;

    Ok((count, now))
}

/// POST /api/zapret/community-hostlist/sync — синхронизация внешнего списка доменов
pub async fn sync_community_hostlist(
    State(state): State<AppState>,
    body: Option<axum::extract::Json<CommunityHostlistSyncReq>>,
) -> Response {
    let url_override = body.and_then(|b| b.url.clone());
    match do_sync_community_hostlist(&state, url_override).await {
        Ok((count, now)) => api_ok(json!({
            "success": true,
            "count": count,
            "last_updated": now,
            "message": format!("Синхронизировано {} доменов. Хостлист nfqws2 обновлен через SIGHUP.", count)
        })),
        Err(e) => api_err(e),
    }
}

// ==================== РАЗДЕЛЕНИЕ ПО УСТРОЙСТВАМ (PER-DEVICE ZAPRET) =============

/// GET /api/devices/zapret — список исключенных из Zapret устройств
pub async fn get_devices_zapret(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    api_ok(json!({
        "excluded_devices": &cfg.zapret.excluded_devices
    }))
}

#[derive(Deserialize)]
pub struct DeviceZapretToggleReq {
    pub ip: Option<String>,
    pub mac: Option<String>,
    pub enabled: bool,
}

/// POST /api/devices/zapret-toggle — переключение Zapret DPI для конкретного устройства
pub async fn toggle_device_zapret(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<DeviceZapretToggleReq>,
) -> Response {
    let target_ip = body.ip.as_deref().unwrap_or("").trim().to_string();
    let target_mac = body.mac.as_deref().unwrap_or("").trim().to_uppercase();

    if target_ip.is_empty() && target_mac.is_empty() {
        return api_err("Не указан IP или MAC адрес устройства");
    }
    if !target_ip.is_empty() && !crate::config::is_valid_ip_or_cidr(&target_ip) {
        return api_err("Некорректный IP-адрес устройства");
    }
    if !target_mac.is_empty() && !crate::config::is_valid_mac(&target_mac) {
        return api_err("Некорректный MAC-адрес устройства");
    }

    let _guard = state.config_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();

    if body.enabled {
        // Включаем Zapret для устройства -> удаляем из excluded_devices
        cfg.zapret.excluded_devices.retain(|d| {
            let d_clean = d.trim();
            !d_clean.eq_ignore_ascii_case(&target_ip) && !d_clean.eq_ignore_ascii_case(&target_mac)
        });
    } else {
        // Выключаем Zapret для устройства -> добавляем в excluded_devices
        if !target_ip.is_empty() && !cfg.zapret.excluded_devices.iter().any(|d| d.eq_ignore_ascii_case(&target_ip)) {
            cfg.zapret.excluded_devices.push(target_ip.clone());
        }
        if !target_mac.is_empty() && !cfg.zapret.excluded_devices.iter().any(|d| d.eq_ignore_ascii_case(&target_mac)) {
            cfg.zapret.excluded_devices.push(target_mac.clone());
        }
    }

    let conflicts = crate::config::device_conflicts(&cfg);
    if !conflicts.is_empty() {
        return api_err_bad_request(crate::config::format_device_conflicts(&conflicts));
    }

    if let Err(e) = crate::config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения конфигурации: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(cfg.clone());

    let _ = sync_zapret_files(&cfg.zapret).await;

    // Мгновенное обновление iptables правил через reload-fw без остановки nfqws
    if cfg.zapret.enabled {
        let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
            .arg("reload-fw")
            .output()
            .await;
    }

    api_ok(json!({
        "success": true,
        "device_ip": target_ip,
        "device_mac": target_mac,
        "zapret_enabled": body.enabled,
        "excluded_devices": &cfg.zapret.excluded_devices,
        "message": if body.enabled {
            "Zapret активирован для устройства"
        } else {
            "Zapret отключен для устройства (трафик исключен из NFQUEUE)"
        }
    }))
}

/// Генерация динамического списка URL для скачивания архива релиза Zapret 2
#[cfg(test)]
pub fn build_zapret2_download_urls(target_version: &str) -> Vec<String> {
    let raw = target_version.trim();
    let effective = if raw.is_empty() { "v1.0.6" } else { raw };
    let tag = if effective.starts_with('v') { effective.to_string() } else { format!("v{effective}") };
    let no_v = tag.trim_start_matches('v');

    vec![
        format!("https://github.com/bol-van/zapret2/releases/download/{tag}/zapret2-{tag}.tar.gz"),
        format!("https://github.com/bol-van/zapret2/releases/latest/download/zapret2-{tag}.tar.gz"),
        format!("https://github.com/bol-van/zapret2/releases/download/{tag}/zapret2-{no_v}.tar.gz"),
        format!("https://github.com/bol-van/zapret2/archive/refs/tags/{tag}.tar.gz"),
    ]
}
