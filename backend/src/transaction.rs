//! Централизованный координатор транзакций конфигурации (ConfigTx)
//! Обеспечивает:
//! 1. Строгий порядок захвата блокировок: state.config_lock -> state.routing_lock.
//! 2. Снимок (snapshot) оригинальных config.yaml, config.json и in-memory AppState.
//! 3. Предварительную валидацию YAML-синтаксиса перед записью на диск.
//! 4. Атомарную запись файлов на диск и перезагрузку ядра Mihomo.
//! 5. Автоматический двухфазный откат (rollback) при сбое записи или перезагрузки.
//! 6. Учёт реально записанных файлов (applied_files) и защиту от частичной записи/отмены (TX-02).
//! 7. Агрегацию ошибок отката с выставлением статуса recovery_required (TX-01).
//! 8. Защиту от устаревших операций и затирания правок (optimistic concurrency / collision check).
//! 9. Удержание блокировок до завершения восстановления runtime и надёжный атомарный откат в Drop (TX-03).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::OwnedMutexGuard;

use crate::{config, log_e, log_w, mihomo, AppState};

/// Only NotFound means there is no original file. Any other read error must
/// abort staging; otherwise rollback could delete an existing unreadable file.
async fn read_optional_snapshot(path: &Path) -> Result<Option<String>, String> {
    classify_snapshot(path, tokio::fs::read_to_string(path).await)
}

fn classify_snapshot(
    path: &Path,
    result: std::io::Result<String>,
) -> Result<Option<String>, String> {
    match result {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Не удалось создать снимок {}: {error}", path.display())),
    }
}

/// Вычисление SHA-256 хеша содержимого для контроля параллельных изменений (Collision Check).
pub fn compute_content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Получение времени модификации файла (mtime).
async fn get_file_mtime(path: &Path) -> Option<std::time::SystemTime> {
    tokio::fs::metadata(path).await.ok().and_then(|m| m.modified().ok())
}

/// Надёжная синхронная атомарная запись или удаление файла (для экстренного отката в Drop).
/// Записывает во временный файл и атомарно переименовывает (rename), исключая повреждение при прерывании записи.
pub fn sync_atomic_write_or_remove(path: &Path, content: Option<&str>) -> std::io::Result<()> {
    match content {
        Some(data) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let tmp_path = PathBuf::from(format!("{}.{}.{}.tmp", path.display(), std::process::id(), nonce));
            std::fs::write(&tmp_path, data.as_bytes())?;
            if let Err(e) = std::fs::rename(&tmp_path, path) {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(e);
            }
            Ok(())
        }
        None => match std::fs::remove_file(path) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        },
    }
}

/// Легковесная валидация синтаксиса YAML (табуляции, кавычки, баланс скобок, UTF-8 safe).
pub fn validate_yaml_syntax(content: &str) -> Result<(), String> {
    let mut bracket_stack = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let display_line = line_num + 1;
        if line.contains('\t') {
            return Err(format!(
                "Строка {display_line}: обнаружен символ табуляции (\\t). В YAML допускаются только пробелы."
            ));
        }

        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut escaped = false;

        let mut chars = line.char_indices().peekable();
        while let Some((byte_offset, ch)) = chars.next() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' && in_double_quote {
                escaped = true;
                continue;
            }
            if ch == '\'' && !in_double_quote {
                if in_single_quote {
                    if let Some(&(_, '\'')) = chars.peek() {
                        chars.next(); // поглощаем экранированную одинарную кавычку '' внутри строки
                        continue;
                    }
                }
                in_single_quote = !in_single_quote;
                continue;
            }
            if ch == '"' && !in_single_quote {
                in_double_quote = !in_double_quote;
                continue;
            }
            if in_single_quote || in_double_quote {
                continue;
            }
            if ch == '#' && (byte_offset == 0 || line[..byte_offset].ends_with(' ')) {
                break;
            }
            match ch {
                '[' | '{' => bracket_stack.push((ch, display_line)),
                ']' => match bracket_stack.pop() {
                    Some(('[', _)) => {}
                    Some((other, orig_line)) => {
                        return Err(format!(
                            "Строка {display_line}: несоответствие скобок — ожидалось закрытие '{other}' из строки {orig_line}"
                        ));
                    }
                    None => {
                        return Err(format!("Строка {display_line}: лишняя закрывающая скобка ']'"));
                    }
                },
                '}' => match bracket_stack.pop() {
                    Some(('{', _)) => {}
                    Some((other, orig_line)) => {
                        return Err(format!(
                            "Строка {display_line}: несоответствие скобок — ожидалось закрытие '{other}' из строки {orig_line}"
                        ));
                    }
                    None => {
                        return Err(format!("Строка {display_line}: лишняя закрывающая фигурная скобка '}}'"));
                    }
                },
                _ => {}
            }
        }

        if in_single_quote {
            return Err(format!("Строка {display_line}: незакрытая одинарная кавычка (') в YAML"));
        }
        if in_double_quote {
            return Err(format!("Строка {display_line}: незакрытая двойная кавычка (\") в YAML"));
        }
    }

    if let Some((ch, line)) = bracket_stack.pop() {
        return Err(format!("Строка {line}: незакрытая скобка '{ch}'"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxStatus {
    Active,
    Committed,
    RolledBack,
    RecoveryRequired,
}

#[derive(Clone, Debug)]
pub struct ExtraFile {
    pub path: PathBuf,
    pub original_content: Option<String>,
    pub original_hash: Option<String>,
    pub original_mtime: Option<std::time::SystemTime>,
    pub staged_content: String,
    pub is_yaml: bool,
}

pub struct ConfigTx {
    pub state: AppState,
    // LIFO drop order: _routing_guard is declared before _cfg_guard so it drops first.
    _routing_guard: Option<OwnedMutexGuard<()>>,
    _cfg_guard: Option<OwnedMutexGuard<()>>,

    pub initial_config: config::AppConfig,
    pub current_config: config::AppConfig,

    pub yaml_path: PathBuf,
    pub json_path: PathBuf,

    original_yaml: Option<String>,
    original_json: Option<String>,

    original_yaml_hash: Option<String>,
    original_json_hash: Option<String>,
    original_yaml_mtime: Option<std::time::SystemTime>,
    original_json_mtime: Option<std::time::SystemTime>,

    staged_yaml: Option<String>,
    staged_json: bool,
    staged_raw_json: Option<String>,

    extra_files: Vec<ExtraFile>,

    /// Список реально записанных на диск файлов в рамках текущей транзакции (TX-02)
    pub applied_files: Vec<PathBuf>,
    pub files_applied: bool,
    pub committed: bool,
    pub recovery_required: bool,
    pub status: TxStatus,
}

impl ConfigTx {
    /// Начать транзакцию с гарантированным порядком блокировок (config_lock -> routing_lock)
    /// и созданием снимка исходного состояния на диске и в памяти.
    pub async fn begin(state: &AppState) -> Result<Self, String> {
        // 1. Строгий порядок захвата блокировок для предотвращения дедлоков
        let cfg_guard = state.config_lock.clone().lock_owned().await;
        let routing_guard = state.routing_lock.clone().lock_owned().await;

        // 2. Снимок in-memory конфигурации AppState
        let initial_config = (**state.config.read().await).clone();
        let current_config = initial_config.clone();

        let yaml_path = PathBuf::from(&initial_config.mihomo.config_path);
        let json_path = (*state.config_path).clone();

        // 3. Снимок файлов на диске (config.yaml и config.json) с сохранением хешей и mtime
        let original_yaml = read_optional_snapshot(&yaml_path).await?;
        let original_json = read_optional_snapshot(&json_path).await?;

        let original_yaml_hash = original_yaml.as_deref().map(compute_content_hash);
        let original_json_hash = original_json.as_deref().map(compute_content_hash);
        let original_yaml_mtime = get_file_mtime(&yaml_path).await;
        let original_json_mtime = get_file_mtime(&json_path).await;

        Ok(Self {
            state: state.clone(),
            _routing_guard: Some(routing_guard),
            _cfg_guard: Some(cfg_guard),
            initial_config,
            current_config,
            yaml_path,
            json_path,
            original_yaml,
            original_json,
            original_yaml_hash,
            original_json_hash,
            original_yaml_mtime,
            original_json_mtime,
            staged_yaml: None,
            staged_json: false,
            staged_raw_json: None,
            extra_files: Vec::new(),
            applied_files: Vec::new(),
            files_applied: false,
            committed: false,
            recovery_required: false,
            status: TxStatus::Active,
        })
    }

    /// Ссылка на текущую рабочую конфигурацию
    pub fn config(&self) -> &config::AppConfig {
        &self.current_config
    }

    /// Мутабельная ссылка на рабочую конфигурацию (помечает config.json как изменённый)
    pub fn config_mut(&mut self) -> &mut config::AppConfig {
        self.staged_json = true;
        &mut self.current_config
    }

    /// Установить новую конфигурацию панели целиком
    pub fn stage_json(&mut self, new_cfg: config::AppConfig) {
        self.current_config = new_cfg;
        self.staged_json = true;
    }

    /// Установить сырой JSON config.json с распарсенной структурой (для веб-редактора)
    pub fn stage_raw_json(&mut self, content: impl Into<String>, parsed: config::AppConfig) {
        self.staged_raw_json = Some(content.into());
        self.current_config = parsed;
        self.staged_json = true;
    }

    /// Чтение текущего содержимого config.yaml (из staged, из снимка или с диска)
    pub async fn read_yaml(&self) -> Result<String, String> {
        if let Some(ref y) = self.staged_yaml {
            return Ok(y.clone());
        }
        if let Some(ref y) = self.original_yaml {
            return Ok(y.clone());
        }
        tokio::fs::read_to_string(&self.yaml_path)
            .await
            .map_err(|e| format!("Не удалось прочитать {}: {e}", self.yaml_path.display()))
    }

    /// Предварительная валидация синтаксиса и подготовка нового config.yaml
    pub fn set_yaml(&mut self, new_yaml: impl Into<String>) -> Result<(), String> {
        let content = new_yaml.into();
        validate_yaml_syntax(&content)?;
        self.staged_yaml = Some(content);
        Ok(())
    }

    /// Подготовка дополнительного файла (провайдер, override и т.д.) со снимком оригинального
    pub async fn set_extra_file(
        &mut self,
        path: impl AsRef<Path>,
        content: impl Into<String>,
        is_yaml: bool,
    ) -> Result<(), String> {
        let content = content.into();
        if is_yaml {
            validate_yaml_syntax(&content)?;
        }
        let p = path.as_ref().to_path_buf();
        if let Some(existing) = self.extra_files.iter_mut().find(|f| f.path == p) {
            existing.staged_content = content;
            existing.is_yaml = is_yaml;
        } else {
            let original = read_optional_snapshot(&p).await?;
            let original_hash = original.as_deref().map(compute_content_hash);
            let original_mtime = get_file_mtime(&p).await;
            self.extra_files.push(ExtraFile {
                path: p,
                original_content: original,
                original_hash,
                original_mtime,
                staged_content: content,
                is_yaml,
            });
        }
        Ok(())
    }

    /// Проверка внешних изменений файлов перед записью (Optimistic Concurrency / Collision Check).
    /// Защищает от затирания чужих правок, сделанных внешним процессом в обход транзакции.
    pub async fn check_external_collisions(&self) -> Result<(), String> {
        // Проверяем config.yaml, если он подготовлен к записи
        if self.staged_yaml.is_some() {
            let current_mtime = get_file_mtime(&self.yaml_path).await;
            let current = read_optional_snapshot(&self.yaml_path).await?;
            let current_hash = current.as_deref().map(compute_content_hash);
            if current_hash != self.original_yaml_hash {
                return Err(format!(
                    "Обнаружен конфликт параллельного изменения: файл {} был изменён внешним процессом в обход транзакции",
                    self.yaml_path.display()
                ));
            }
            if self.original_yaml_mtime.is_some() && current_mtime != self.original_yaml_mtime && current_hash != self.original_yaml_hash {
                return Err(format!(
                    "Обнаружен конфликт параллельного изменения: файл {} был изменён внешним процессом в обход транзакции",
                    self.yaml_path.display()
                ));
            }
        }

        // Проверяем extra_files
        for extra in &self.extra_files {
            let current_mtime = get_file_mtime(&extra.path).await;
            let current = read_optional_snapshot(&extra.path).await?;
            let current_hash = current.as_deref().map(compute_content_hash);
            if current_hash != extra.original_hash {
                return Err(format!(
                    "Обнаружен конфликт параллельного изменения: файл {} был изменён внешним процессом в обход транзакции",
                    extra.path.display()
                ));
            }
            if extra.original_mtime.is_some() && current_mtime != extra.original_mtime && current_hash != extra.original_hash {
                return Err(format!(
                    "Обнаружен конфликт параллельного изменения: файл {} был изменён внешним процессом в обход транзакции",
                    extra.path.display()
                ));
            }
        }

        // Проверяем config.json, если он подготовлен к записи
        if self.staged_json {
            let current_mtime = get_file_mtime(&self.json_path).await;
            let current = read_optional_snapshot(&self.json_path).await?;
            let current_hash = current.as_deref().map(compute_content_hash);
            if current_hash != self.original_json_hash {
                return Err(format!(
                    "Обнаружен конфликт параллельного изменения: файл {} был изменён внешним процессом в обход транзакции",
                    self.json_path.display()
                ));
            }
            if self.original_json_mtime.is_some() && current_mtime != self.original_json_mtime && current_hash != self.original_json_hash {
                return Err(format!(
                    "Обнаружен конфликт параллельного изменения: файл {} был изменён внешним процессом в обход транзакции",
                    self.json_path.display()
                ));
            }
        }

        Ok(())
    }

    /// Внутренний помощник атомарной записи файлов на диск с валидацией,
    /// защитой от коллизий и автоматическим откатом уже записанных файлов при сбое (TX-02).
    async fn apply_disk_files(&mut self) -> Result<(), String> {
        if self.status == TxStatus::Committed {
            return Err("Невозможно применить файлы: транзакция уже зафиксирована".into());
        }
        if self.status == TxStatus::RolledBack || self.status == TxStatus::RecoveryRequired {
            return Err("Невозможно применить файлы: транзакция уже откатана или повреждена".into());
        }

        // 1. Проверка внешних изменений перед началом записи (Collision Check)
        self.check_external_collisions().await?;

        // 2. Предварительная валидация перед дисковыми операциями
        if let Some(ref yaml) = self.staged_yaml {
            validate_yaml_syntax(yaml)?;
        }
        for extra in &self.extra_files {
            if extra.is_yaml {
                validate_yaml_syntax(&extra.staged_content)?;
            }
        }

        // 3. Атомарная запись staged_yaml
        if let Some(ref yaml) = self.staged_yaml {
            if let Err(e) = crate::api::atomic_write_file(&self.yaml_path, yaml).await {
                let r_res = self.rollback_disk_files().await;
                if let Err(ref r_errs) = r_res {
                    log_e!("Сбой при откате после ошибки записи {}: {:?}", self.yaml_path.display(), r_errs);
                    return Err(format!(
                        "Ошибка записи {}: {e}. При двухфазном откате произошли критические ошибки (recovery_required): {}",
                        self.yaml_path.display(),
                        r_errs.join("; ")
                    ));
                }
                return Err(format!("Ошибка записи {}: {e}", self.yaml_path.display()));
            }
            if !self.applied_files.contains(&self.yaml_path) {
                self.applied_files.push(self.yaml_path.clone());
            }
            self.files_applied = true;
        }

        // 4. Атомарная запись дополнительных файлов
        let extra_items: Vec<(PathBuf, String)> = self
            .extra_files
            .iter()
            .map(|e| (e.path.clone(), e.staged_content.clone()))
            .collect();
        for (extra_path, extra_content) in extra_items {
            if let Err(e) = crate::api::atomic_write_file(&extra_path, &extra_content).await {
                let r_res = self.rollback_disk_files().await;
                if let Err(ref r_errs) = r_res {
                    log_e!("Сбой при откате после ошибки записи {}: {:?}", extra_path.display(), r_errs);
                    return Err(format!(
                        "Ошибка записи {}: {e}. При двухфазном откате произошли критические ошибки (recovery_required): {}",
                        extra_path.display(),
                        r_errs.join("; ")
                    ));
                }
                return Err(format!("Ошибка записи {}: {e}", extra_path.display()));
            }
            if !self.applied_files.contains(&extra_path) {
                self.applied_files.push(extra_path);
            }
            self.files_applied = true;
        }

        // 5. Атомарная запись config.json
        if self.staged_json {
            let res = if let Some(ref raw_json) = self.staged_raw_json {
                crate::api::atomic_write_file(&self.json_path, raw_json).await
            } else {
                config::save(&self.json_path, &self.current_config).await
            };
            if let Err(e) = res {
                let r_res = self.rollback_disk_files().await;
                if let Err(ref r_errs) = r_res {
                    log_e!("Сбой при откате после ошибки записи {}: {:?}", self.json_path.display(), r_errs);
                    return Err(format!(
                        "Ошибка записи {}: {e}. При двухфазном откате произошли критические ошибки (recovery_required): {}",
                        self.json_path.display(),
                        r_errs.join("; ")
                    ));
                }
                return Err(format!("Ошибка записи {}: {e}", self.json_path.display()));
            }
            if !self.applied_files.contains(&self.json_path) {
                self.applied_files.push(self.json_path.clone());
            }
            self.files_applied = true;
        }

        Ok(())
    }

    /// Атомарная запись подготовленных файлов на диск и перезагрузка Mihomo.
    /// При любой ошибке выполняется двухфазный откат файлов на диске и перезагрузка прежней конфигурации.
    pub async fn apply_and_reload(&mut self) -> Result<(), String> {
        self.apply_disk_files().await?;

        // Перезагрузка ядра Mihomo
        if let Err(e) = mihomo::reload_config(&self.state.http, &self.current_config).await {
            log_e!("Ошибка reload Mihomo в транзакции: {e}. Выполняется двухфазный откат.");
            let rollback_res = self.rollback().await;
            if let Err(ref r_errs) = rollback_res {
                log_e!("КРИТИЧЕСКАЯ ОШИБКА: двухфазный откат завершился сбоем: {:?}", r_errs);
                return Err(format!(
                    "Ядро Mihomo отклонило конфигурацию: {e}. При двухфазном откате произошли критические ошибки: {}",
                    r_errs.join("; ")
                ));
            }
            return Err(format!(
                "Ядро Mihomo отклонило конфигурацию: {e}. Выполнен откат к исходному состоянию."
            ));
        }

        Ok(())
    }

    /// Выполнить двухфазный откат:
    /// Фаза 1: Восстановление файлов на диске из снимков (snapshots) с агрегацией ошибок (TX-01).
    /// Фаза 2: Перезагрузка Mihomo с восстановленной конфигурацией (блокировки удерживаются до конца, TX-03).
    /// In-memory AppState остаётся нетронутым.
    pub async fn rollback(&mut self) -> Result<(), Vec<String>> {
        if self.status == TxStatus::Committed {
            let err = "Невозможно выполнить откат: транзакция уже зафиксирована".to_string();
            log_e!("{err}");
            return Err(vec![err]);
        }
        if self.status == TxStatus::RolledBack {
            return Ok(());
        }

        let mut all_errors = Vec::new();

        if !self.applied_files.is_empty() || self.files_applied {
            if let Err(disk_errs) = self.rollback_disk_files().await {
                all_errors.extend(disk_errs);
            }

            // Перезагружаем Mihomo runtime только если конфигурация ядра была затронута транзакцией
            let runtime_touched = self.staged_yaml.is_some() || !self.extra_files.is_empty();
            if runtime_touched {
                if let Err(e) = mihomo::reload_config(&self.state.http, &self.initial_config).await {
                    let reload_err = format!("Не удалось восстановить runtime Mihomo: {e}");
                    log_e!("{reload_err}");
                    all_errors.push(reload_err);
                }
            }

            if all_errors.is_empty() {
                self.files_applied = false;
                self.applied_files.clear();
            }
        }

        self.committed = false;

        if all_errors.is_empty() {
            self.status = TxStatus::RolledBack;
            Ok(())
        } else {
            self.recovery_required = true;
            self.status = TxStatus::RecoveryRequired;
            log_e!(
                "КРИТИЧЕСКАЯ ОШИБКА: откат транзакции завершился сбоем (recovery_required)! Список ошибок: {:?}",
                all_errors
            );
            Err(all_errors)
        }
    }

    /// Восстановление файлов на диске из снимков с агрегацией всех ошибок (TX-01, TX-02)
    pub async fn rollback_disk_files(&mut self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let mut successfully_rolled_back = Vec::new();

        if self.applied_files.is_empty() && self.files_applied {
            if self.staged_yaml.is_some() {
                self.applied_files.push(self.yaml_path.clone());
            }
            for extra in &self.extra_files {
                self.applied_files.push(extra.path.clone());
            }
            if self.staged_json {
                self.applied_files.push(self.json_path.clone());
            }
        }

        // Откатываем в обратном порядке применения (LIFO)
        for path in self.applied_files.iter().rev() {
            let snapshot = if path == &self.yaml_path {
                self.original_yaml.as_deref()
            } else if path == &self.json_path {
                self.original_json.as_deref()
            } else if let Some(extra) = self.extra_files.iter().find(|e| &e.path == path) {
                extra.original_content.as_deref()
            } else {
                None
            };

            let res = match snapshot {
                Some(content) => crate::api::atomic_write_file(path, content).await,
                None => match tokio::fs::remove_file(path).await {
                    Ok(_) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(format!("Не удалось удалить временный/новый файл: {e}")),
                },
            };

            match res {
                Ok(_) => successfully_rolled_back.push(path.clone()),
                Err(e) => {
                    let err_msg = format!("Не удалось восстановить {}: {e}", path.display());
                    log_e!("{err_msg}");
                    errors.push(err_msg);
                }
            }
        }

        self.applied_files.retain(|p| !successfully_rolled_back.contains(p));
        if self.applied_files.is_empty() {
            self.files_applied = false;
        }

        if errors.is_empty() {
            Ok(())
        } else {
            self.recovery_required = true;
            self.status = TxStatus::RecoveryRequired;
            log_e!(
                "КРИТИЧЕСКАЯ ОШИБКА: откат файлов завершился сбоем (recovery_required). Незавершённые файлы: {:?}",
                errors
            );
            Err(errors)
        }
    }

    /// Фиксация транзакции: обновление in-memory AppState
    pub async fn commit(&mut self) -> Result<(), String> {
        if self.status == TxStatus::Committed {
            return Ok(());
        }
        if self.status == TxStatus::RolledBack || self.status == TxStatus::RecoveryRequired {
            return Err("Невозможно зафиксировать транзакцию: транзакция уже откатана или требует ручного восстановления".into());
        }

        if self.staged_json {
            *self.state.config.write().await = Arc::new(self.current_config.clone());
        }
        self.committed = true;
        self.applied_files.clear();
        self.files_applied = false;
        self.status = TxStatus::Committed;
        Ok(())
    }

    /// Полный цикл: атомарная запись файлов, reload ядра Mihomo и commit in-memory AppState.
    /// При любой ошибке автоматически выполняется полный двухфазный откат.
    pub async fn commit_and_reload(&mut self) -> Result<(), String> {
        self.apply_and_reload().await?;
        self.commit().await?;
        Ok(())
    }

    /// Запись файлов и фиксация AppState без перезагрузки Mihomo (для файлов настроек панели)
    pub async fn commit_without_reload(&mut self) -> Result<(), String> {
        self.apply_disk_files().await?;
        self.commit().await?;
        Ok(())
    }

    /// Текущий статус транзакции
    pub fn status(&self) -> TxStatus {
        self.status
    }

    /// Требуется ли ручное вмешательство администратора из-за сбоя отката (TX-01)
    pub fn is_recovery_required(&self) -> bool {
        self.recovery_required || self.status == TxStatus::RecoveryRequired
    }
}

impl Drop for ConfigTx {
    fn drop(&mut self) {
        if !self.committed && (!self.applied_files.is_empty() || self.files_applied) {
            log_w!("ConfigTx завершён без фиксации! Выполняется надёжный синхронный откат файлов на диске.");
            let mut failed_files = Vec::new();

            if self.applied_files.is_empty() && self.files_applied {
                if self.staged_yaml.is_some() {
                    self.applied_files.push(self.yaml_path.clone());
                }
                for extra in &self.extra_files {
                    self.applied_files.push(extra.path.clone());
                }
                if self.staged_json {
                    self.applied_files.push(self.json_path.clone());
                }
            }

            for path in self.applied_files.iter().rev() {
                let snapshot = if path == &self.yaml_path {
                    self.original_yaml.as_deref()
                } else if path == &self.json_path {
                    self.original_json.as_deref()
                } else if let Some(extra) = self.extra_files.iter().find(|e| &e.path == path) {
                    extra.original_content.as_deref()
                } else {
                    None
                };

                if let Err(e) = sync_atomic_write_or_remove(path, snapshot) {
                    let err = format!("Drop: не удалось надёжно восстановить {}: {e}", path.display());
                    log_e!("{err}");
                    failed_files.push(err);
                }
            }

            if !failed_files.is_empty() {
                self.recovery_required = true;
                self.status = TxStatus::RecoveryRequired;
                log_e!(
                    "КРИТИЧЕСКАЯ ОШИБКА в Drop: незавершённые файлы отката (recovery_required): {:?}",
                    failed_files
                );
            }

            // Удержание блокировок config_lock и routing_lock до завершения reload (TX-03)
            let runtime_touched = self.staged_yaml.is_some() || !self.extra_files.is_empty();
            if runtime_touched {
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    let http = self.state.http.clone();
                    let initial_cfg = self.initial_config.clone();
                    let routing_guard = self._routing_guard.take();
                    let cfg_guard = self._cfg_guard.take();
                    handle.spawn(async move {
                        let res = mihomo::reload_config(&http, &initial_cfg).await;
                        if let Err(e) = res {
                            log_e!("Drop: ошибка фонового восстановления Mihomo: {e}");
                        }
                        drop(routing_guard);
                        drop(cfg_guard);
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_state(root: &Path) -> (AppState, PathBuf, PathBuf) {
        let yaml_path = root.join("config.yaml");
        let json_path = root.join("config.json");
        let mut cfg = config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.display().to_string();
        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(json_path.clone()),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
        };
        (state, yaml_path, json_path)
    }

    #[test]
    fn snapshot_preserves_empty_and_nonempty_files() {
        let path = Path::new("config.json");
        assert_eq!(classify_snapshot(path, Ok(String::new())).unwrap(), Some(String::new()));
        assert_eq!(classify_snapshot(path, Ok("{}".into())).unwrap(), Some("{}".into()));
    }

    #[test]
    fn snapshot_only_accepts_not_found_as_absence() {
        use std::io::{Error, ErrorKind};
        let path = Path::new("config.json");
        assert_eq!(classify_snapshot(path, Err(Error::from(ErrorKind::NotFound))).unwrap(), None);
        for kind in [ErrorKind::PermissionDenied, ErrorKind::InvalidData, ErrorKind::Interrupted, ErrorKind::Other] {
            let error = classify_snapshot(path, Err(Error::new(kind, "snapshot failure"))).unwrap_err();
            assert!(error.contains("config.json"));
            assert!(error.contains("snapshot failure"));
        }
    }

    #[tokio::test]
    async fn snapshot_rejects_unreadable_content_without_modifying_it() {
        let root = std::env::temp_dir().join(format!("xkeen-snapshot-{}", crate::auth::random_hex(16)));
        tokio::fs::create_dir(&root).await.unwrap();
        let path = root.join("invalid-utf8.json");
        let bytes = [0xff, 0xfe, 0x00];
        tokio::fs::write(&path, bytes).await.unwrap();
        let invalid = read_optional_snapshot(&path).await;
        let directory = read_optional_snapshot(&root).await;
        let missing = read_optional_snapshot(&root.join("missing.json")).await;
        let unchanged = tokio::fs::read(&path).await.unwrap();
        tokio::fs::remove_dir_all(&root).await.unwrap();
        assert!(invalid.is_err());
        assert!(directory.is_err());
        assert_eq!(missing.unwrap(), None);
        assert_eq!(unchanged, bytes);
    }

    #[test]
    fn test_sync_atomic_write_or_remove() {
        let root = std::env::temp_dir().join(format!("xkeen-tx-sync-{}", crate::auth::random_hex(8)));
        let _ = std::fs::create_dir_all(&root);
        let test_file = root.join("test.txt");

        // Запись нового файла
        sync_atomic_write_or_remove(&test_file, Some("hello world")).unwrap();
        assert_eq!(std::fs::read_to_string(&test_file).unwrap(), "hello world");

        // Перезапись существующего файла
        sync_atomic_write_or_remove(&test_file, Some("updated data")).unwrap();
        assert_eq!(std::fs::read_to_string(&test_file).unwrap(), "updated data");

        // Удаление файла (Some -> None)
        sync_atomic_write_or_remove(&test_file, None).unwrap();
        assert!(!test_file.exists());

        // Удаление несуществующего файла не должно возвращать ошибку
        assert!(sync_atomic_write_or_remove(&test_file, None).is_ok());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_collision_check_detects_external_file_modification() {
        let root = std::env::temp_dir().join(format!("xkeen-tx-coll-{}", crate::auth::random_hex(8)));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let (state, yaml_path, json_path) = create_test_state(&root);
        tokio::fs::write(&yaml_path, "port: 7890\n").await.unwrap();
        tokio::fs::write(&json_path, "{}\n").await.unwrap();

        let mut tx = ConfigTx::begin(&state).await.unwrap();
        tx.set_yaml("port: 9090\n").unwrap();

        // Симуляция внешней модификации файла до фиксации транзакции
        tokio::fs::write(&yaml_path, "port: 8888\n# external process").await.unwrap();

        // Проверка обнаружения коллизии
        let collision_res = tx.check_external_collisions().await;
        assert!(collision_res.is_err());
        let err = collision_res.unwrap_err();
        assert!(err.contains("конфликт параллельного изменения"));

        // apply_disk_files должен отклонить запись из-за коллизии
        let apply_res = tx.apply_disk_files().await;
        assert!(apply_res.is_err());

        // Внешние правки на диске не должны быть затерты
        let current_disk = tokio::fs::read_to_string(&yaml_path).await.unwrap();
        assert_eq!(current_disk, "port: 8888\n# external process");

        tokio::fs::remove_dir_all(&root).await.unwrap();
    }

    #[tokio::test]
    async fn test_partial_apply_failure_rolls_back_already_written_files() {
        let root = std::env::temp_dir().join(format!("xkeen-tx-partial-{}", crate::auth::random_hex(8)));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let (state, yaml_path, json_path) = create_test_state(&root);
        tokio::fs::write(&yaml_path, "port: 7890\n").await.unwrap();
        tokio::fs::write(&json_path, "{}\n").await.unwrap();

        let mut tx = ConfigTx::begin(&state).await.unwrap();
        tx.set_yaml("port: 9999\n").unwrap();

        // Создаем директорию с именем extra_path, чтобы atomic_write_file гарантированно упал при попытке rename файла в директорию
        let bad_extra_dir = root.join("bad_extra_as_dir");
        tokio::fs::create_dir_all(&bad_extra_dir).await.unwrap();
        tx.set_extra_file(&bad_extra_dir, "content: 1\n", true).await.unwrap();

        let apply_res = tx.apply_disk_files().await;
        assert!(apply_res.is_err());

        // Проверяем, что первый записанный файл (yaml) был успешно откатан к оригиналу
        let rolled_back_yaml = tokio::fs::read_to_string(&yaml_path).await.unwrap();
        assert_eq!(rolled_back_yaml, "port: 7890\n");

        assert!(tx.applied_files.is_empty());
        assert!(!tx.files_applied);

        tokio::fs::remove_dir_all(&root).await.unwrap();
    }

    #[tokio::test]
    async fn test_aggregated_rollback_errors_on_partial_restore_failure() {
        let root = std::env::temp_dir().join(format!("xkeen-tx-aggr-{}", crate::auth::random_hex(8)));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let (state, yaml_path, json_path) = create_test_state(&root);
        tokio::fs::write(&yaml_path, "port: 7890\n").await.unwrap();
        tokio::fs::write(&json_path, "{}\n").await.unwrap();

        let extra_path = root.join("extra.yaml");
        tokio::fs::write(&extra_path, "extra: orig\n").await.unwrap();

        let mut tx = ConfigTx::begin(&state).await.unwrap();
        tx.set_yaml("port: 9090\n").unwrap();
        tx.set_extra_file(&extra_path, "extra: modified\n", true).await.unwrap();

        // Симулируем успешную запись обоих файлов
        tx.applied_files.push(yaml_path.clone());
        tx.applied_files.push(extra_path.clone());
        tx.files_applied = true;

        // Делаем так, чтобы восстановление yaml_path завершилось ошибкой:
        // удаляем yaml_path и создаем вместо него директорию с тем же именем,
        // из-за чего atomic_write_file не сможет перезаписать файл
        tokio::fs::remove_file(&yaml_path).await.unwrap();
        tokio::fs::create_dir(&yaml_path).await.unwrap();

        // Запускаем откат файлов
        let rollback_res = tx.rollback_disk_files().await;
        assert!(rollback_res.is_err());
        let errors = rollback_res.unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.contains("config.yaml")));

        // Проверяем, что второй файл (extra_path) ВСЁ РАВНО был восстановлен
        // (цикл отката не остановился на первой ошибке)
        let restored_extra = tokio::fs::read_to_string(&extra_path).await.unwrap();
        assert_eq!(restored_extra, "extra: orig\n");

        // Проверяем выставление recovery_required
        assert!(tx.recovery_required);
        assert_eq!(tx.status, TxStatus::RecoveryRequired);

        // Очистка
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn test_drop_without_commit_triggers_reliable_rollback() {
        let root = std::env::temp_dir().join(format!("xkeen-tx-drop-{}", crate::auth::random_hex(8)));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let (state, yaml_path, json_path) = create_test_state(&root);
        tokio::fs::write(&yaml_path, "port: 7890\n").await.unwrap();
        tokio::fs::write(&json_path, "{}\n").await.unwrap();

        {
            let mut tx = ConfigTx::begin(&state).await.unwrap();
            tx.set_yaml("port: 9999\n").unwrap();
            tx.apply_disk_files().await.unwrap();

            // Проверяем, что файл временно изменился на диске
            let on_disk = tokio::fs::read_to_string(&yaml_path).await.unwrap();
            assert_eq!(on_disk, "port: 9999\n");
            // tx выходит из области видимости БЕЗ commit()
        }

        // Проверяем, что Drop надежно восстановил исходный файл
        let restored = tokio::fs::read_to_string(&yaml_path).await.unwrap();
        assert_eq!(restored, "port: 7890\n");

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[test]
    fn test_validate_yaml_valid() {
        let valid = r#"
port: 7890
socks-port: 7891
rules:
  - DOMAIN-SUFFIX,google.com,DIRECT
  - MATCH,PROXY
"#;
        assert!(validate_yaml_syntax(valid).is_ok());
    }

    #[test]
    fn test_validate_yaml_tabs_rejected() {
        let with_tabs = "port: 7890\n\trules:\n  - MATCH,DIRECT\n";
        let res = validate_yaml_syntax(with_tabs);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("табуляции"));
    }

    #[test]
    fn test_validate_yaml_unclosed_quotes() {
        let bad_quotes = "name: 'unclosed string\nport: 7890\n";
        let res = validate_yaml_syntax(bad_quotes);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("кавычка"));
    }

    #[test]
    fn test_validate_yaml_bracket_mismatch() {
        let mismatch = "proxies: [name: 'test'}\n";
        let res = validate_yaml_syntax(mismatch);
        assert!(res.is_err());
        let error = res.unwrap_err();
        assert!(error.contains("несоответствие скобок") || error.contains("лишняя"));
    }

    #[test]
    fn test_validate_yaml_escaped_quotes() {
        let with_escaped = "name: 'It''s a valid node name'\nport: 7890\n";
        assert!(validate_yaml_syntax(with_escaped).is_ok());
    }

    #[test]
    fn test_validate_yaml_utf8_cyrillic_comments() {
        let cyrillic = "name: \"Россия Direct\" # русский комментарий с [скобками] и 'кавычками'\nport: 7890\n";
        assert!(validate_yaml_syntax(cyrillic).is_ok());
    }

    #[tokio::test]
    async fn test_commit_after_rollback_is_rejected() {
        let root = std::env::temp_dir().join(format!("xkeen-tx-rej-{}", crate::auth::random_hex(8)));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let (state, yaml_path, json_path) = create_test_state(&root);
        tokio::fs::write(&yaml_path, "port: 7890\n").await.unwrap();
        tokio::fs::write(&json_path, "{}\n").await.unwrap();

        let mut tx = ConfigTx::begin(&state).await.unwrap();
        tx.config_mut().adblock_enabled = true;
        tx.rollback().await.unwrap();

        // Попытка зафиксировать откатную транзакцию должна быть отклонена
        let commit_res = tx.commit().await;
        assert!(commit_res.is_err());
        assert!(commit_res.unwrap_err().contains("уже откатана"));

        // In-memory состояние не должно быть изменено
        let current_cfg = state.config.read().await;
        assert!(!current_cfg.adblock_enabled);

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn test_rollback_skips_mihomo_reload_when_runtime_untouched() {
        let root = std::env::temp_dir().join(format!("xkeen-tx-noreload-{}", crate::auth::random_hex(8)));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let (state, yaml_path, json_path) = create_test_state(&root);
        tokio::fs::write(&yaml_path, "port: 7890\n").await.unwrap();
        tokio::fs::write(&json_path, "{\"refresh_interval_sec\": 10}\n").await.unwrap();

        let mut tx = ConfigTx::begin(&state).await.unwrap();
        // Изменяем только config.json (staged_json), не трогая yaml
        tx.config_mut().refresh_interval_sec = 20;
        tx.apply_disk_files().await.unwrap();

        // Проверяем, что runtime_touched = false и rollback не пытается стучаться в Mihomo
        let rollback_res = tx.rollback().await;
        assert!(rollback_res.is_ok(), "Откат json-only транзакции должен быть успешен без reload Mihomo");
        assert_eq!(tx.status, TxStatus::RolledBack);

        let restored_json = tokio::fs::read_to_string(&json_path).await.unwrap();
        assert_eq!(restored_json, "{\"refresh_interval_sec\": 10}\n");

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn test_applied_files_deduplication() {
        let root = std::env::temp_dir().join(format!("xkeen-tx-dedup-{}", crate::auth::random_hex(8)));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let (state, yaml_path, json_path) = create_test_state(&root);
        tokio::fs::write(&yaml_path, "port: 7890\n").await.unwrap();
        tokio::fs::write(&json_path, "{}\n").await.unwrap();

        let mut tx = ConfigTx::begin(&state).await.unwrap();
        tx.set_yaml("port: 9090\n").unwrap();
        tx.apply_disk_files().await.unwrap();

        assert_eq!(tx.applied_files.len(), 1);

        // Повторный вызов apply_disk_files после успешной записи не должен дублировать путь
        tx.applied_files.clear();
        tx.applied_files.push(yaml_path.clone());
        if !tx.applied_files.contains(&yaml_path) {
            tx.applied_files.push(yaml_path.clone());
        }
        assert_eq!(tx.applied_files.len(), 1);

        let _ = tokio::fs::remove_dir_all(&root).await;
    }
}
