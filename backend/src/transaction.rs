//! Централизованный координатор транзакций конфигурации (ConfigTx)
//! Обеспечивает:
//! 1. Строгий порядок захвата блокировок: state.config_lock -> state.routing_lock.
//! 2. Снимок (snapshot) оригинальных config.yaml, config.json и in-memory AppState.
//! 3. Предварительную валидацию YAML-синтаксиса перед записью на диск.
//! 4. Атомарную запись файлов на диск и перезагрузку ядра Mihomo.
//! 5. Автоматический двухфазный откат (rollback) при сбое записи или перезагрузки.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::OwnedMutexGuard;

use crate::{config, log_e, log_i, log_w, mihomo, AppState};

/// Легковесная валидация синтаксиса YAML (табуляции, кавычки, баланс скобок).
pub fn validate_yaml_syntax(content: &str) -> Result<(), String> {
    let mut bracket_stack = Vec::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;

    for (line_num, line) in content.lines().enumerate() {
        if line.contains('\t') {
            return Err(format!(
                "Строка {}: обнаружен символ табуляции (\\t). В YAML допускаются только пробелы.",
                line_num + 1
            ));
        }

        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }

        for (col, ch) in line.chars().enumerate() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' && in_double_quote {
                escaped = true;
                continue;
            }
            if ch == '\'' && !in_double_quote {
                if in_single_quote && col + 1 < line.len() && line[col + 1..].starts_with('\'') {
                    // Экранированная одинарная кавычка '' внутри строки
                    continue;
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
            if ch == '#' && (col == 0 || line[..col].ends_with(' ')) {
                break;
            }
            match ch {
                '[' | '{' => bracket_stack.push((ch, line_num + 1)),
                ']' => match bracket_stack.pop() {
                    Some(('[', _)) => {}
                    Some((other, orig_line)) => {
                        return Err(format!(
                            "Строка {}: несоответствие скобок — ожидалось закрытие '{}' из строки {}",
                            line_num + 1,
                            other,
                            orig_line
                        ));
                    }
                    None => {
                        return Err(format!("Строка {}: лишняя закрывающая скобка ']'", line_num + 1));
                    }
                },
                '}' => match bracket_stack.pop() {
                    Some(('{', _)) => {}
                    Some((other, orig_line)) => {
                        return Err(format!(
                            "Строка {}: несоответствие скобок — ожидалось закрытие '{}' из строки {}",
                            line_num + 1,
                            other,
                            orig_line
                        ));
                    }
                    None => {
                        return Err(format!(
                            "Строка {}: лишняя закрывающая фигурная скобка '}}'",
                            line_num + 1
                        ));
                    }
                },
                _ => {}
            }
        }
    }

    if let Some((ch, line)) = bracket_stack.pop() {
        return Err(format!("Строка {}: незакрытая скобка '{}'", line, ch));
    }
    if in_single_quote || in_double_quote {
        return Err("Обнаружена незакрытая кавычка в файле YAML".into());
    }
    Ok(())
}

pub struct ConfigTx {
    pub state: AppState,
    // LIFO drop order: _routing_guard is declared before _cfg_guard so it drops first.
    _routing_guard: OwnedMutexGuard<()>,
    _cfg_guard: OwnedMutexGuard<()>,

    pub initial_config: config::AppConfig,
    pub current_config: config::AppConfig,

    pub yaml_path: PathBuf,
    pub json_path: PathBuf,

    original_yaml: Option<String>,
    original_json: Option<String>,

    staged_yaml: Option<String>,
    staged_json: bool,

    extra_file: Option<(PathBuf, Option<String>)>,
    staged_extra: Option<String>,
    extra_is_yaml: bool,

    committed: bool,
    files_applied: bool,
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

        // 3. Снимок файлов на диске (config.yaml и config.json)
        let original_yaml = tokio::fs::read_to_string(&yaml_path).await.ok();
        let original_json = tokio::fs::read_to_string(&json_path).await.ok();

        Ok(Self {
            state: state.clone(),
            _routing_guard: routing_guard,
            _cfg_guard: cfg_guard,
            initial_config,
            current_config,
            yaml_path,
            json_path,
            original_yaml,
            original_json,
            staged_yaml: None,
            staged_json: false,
            extra_file: None,
            staged_extra: None,
            extra_is_yaml: false,
            committed: false,
            files_applied: false,
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
        let original = tokio::fs::read_to_string(&p).await.ok();
        self.extra_file = Some((p, original));
        self.staged_extra = Some(content);
        self.extra_is_yaml = is_yaml;
        Ok(())
    }

    /// Атомарная запись подготовленных файлов на диск и перезагрузка Mihomo.
    /// При любой ошибке выполняется двухфазный откат файлов на диске и перезагрузка прежней конфигурации.
    pub async fn apply_and_reload(&mut self) -> Result<(), String> {
        // Предварительная валидация перед дисковыми операциями
        if let Some(ref yaml) = self.staged_yaml {
            validate_yaml_syntax(yaml)?;
        }
        if let Some(ref extra) = self.staged_extra {
            if self.extra_is_yaml {
                validate_yaml_syntax(extra)?;
            }
        }

        // Атомарная запись файлов
        if let Some(ref yaml) = self.staged_yaml {
            if let Err(e) = crate::api::atomic_write_file(&self.yaml_path, yaml).await {
                self.rollback_disk_files().await;
                return Err(format!("Ошибка записи {}: {e}", self.yaml_path.display()));
            }
        }

        if let Some(ref extra) = self.staged_extra {
            if let Some((ref extra_path, _)) = self.extra_file {
                if let Err(e) = crate::api::atomic_write_file(extra_path, extra).await {
                    self.rollback_disk_files().await;
                    return Err(format!("Ошибка записи {}: {e}", extra_path.display()));
                }
            }
        }

        if self.staged_json {
            if let Err(e) = config::save(&self.json_path, &self.current_config).await {
                self.rollback_disk_files().await;
                return Err(format!("Ошибка записи {}: {e}", self.json_path.display()));
            }
        }

        self.files_applied = true;

        // Перезагрузка ядра Mihomo
        if let Err(e) = mihomo::reload_config(&self.state.http, &self.current_config).await {
            log_e!("Ошибка reload Mihomo в транзакции: {e}. Выполняется двухфазный откат.");
            self.rollback().await;
            return Err(format!(
                "Ядро Mihomo отклонило конфигурацию: {e}. Выполнен откат к исходному состоянию."
            ));
        }

        Ok(())
    }

    /// Выполнить двухфазный откат:
    /// Фаза 1: Восстановление файлов на диске из снимков (snapshots).
    /// Фаза 2: Перезагрузка Mihomo с восстановленной конфигурацией.
    /// In-memory AppState остаётся нетронутым.
    pub async fn rollback(&mut self) {
        if self.files_applied {
            self.rollback_disk_files().await;
            let _ = mihomo::reload_config(&self.state.http, &self.initial_config).await;
            self.files_applied = false;
        }
        self.committed = false;
    }

    /// Восстановление файлов на диске из снимков
    async fn rollback_disk_files(&self) {
        if self.staged_yaml.is_some() {
            if let Some(ref orig) = self.original_yaml {
                let _ = crate::api::atomic_write_file(&self.yaml_path, orig).await;
            } else {
                let _ = tokio::fs::remove_file(&self.yaml_path).await;
            }
        }

        if self.staged_extra.is_some() {
            if let Some((ref extra_path, ref orig_opt)) = self.extra_file {
                if let Some(ref orig) = orig_opt {
                    let _ = crate::api::atomic_write_file(extra_path, orig).await;
                } else {
                    let _ = tokio::fs::remove_file(extra_path).await;
                }
            }
        }

        if self.staged_json {
            if let Some(ref orig) = self.original_json {
                let _ = crate::api::atomic_write_file(&self.json_path, orig).await;
            }
        }
    }

    /// Фиксация транзакции: обновление in-memory AppState
    pub async fn commit(&mut self) -> Result<(), String> {
        if self.staged_json {
            *self.state.config.write().await = Arc::new(self.current_config.clone());
        }
        self.committed = true;
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
        if let Some(ref yaml) = self.staged_yaml {
            validate_yaml_syntax(yaml)?;
            if let Err(e) = crate::api::atomic_write_file(&self.yaml_path, yaml).await {
                self.rollback_disk_files().await;
                return Err(format!("Ошибка записи {}: {e}", self.yaml_path.display()));
            }
        }

        if let Some(ref extra) = self.staged_extra {
            if self.extra_is_yaml {
                validate_yaml_syntax(extra)?;
            }
            if let Some((ref extra_path, _)) = self.extra_file {
                if let Err(e) = crate::api::atomic_write_file(extra_path, extra).await {
                    self.rollback_disk_files().await;
                    return Err(format!("Ошибка записи {}: {e}", extra_path.display()));
                }
            }
        }

        if self.staged_json {
            if let Err(e) = config::save(&self.json_path, &self.current_config).await {
                self.rollback_disk_files().await;
                return Err(format!("Ошибка записи {}: {e}", self.json_path.display()));
            }
            *self.state.config.write().await = Arc::new(self.current_config.clone());
        }

        self.committed = true;
        Ok(())
    }
}

impl Drop for ConfigTx {
    fn drop(&mut self) {
        if self.files_applied && !self.committed {
            log_w!("ConfigTx завершён без фиксации! Выполняется синхронный откат файлов на диске.");
            if self.staged_yaml.is_some() {
                if let Some(ref orig) = self.original_yaml {
                    let _ = std::fs::write(&self.yaml_path, orig);
                } else {
                    let _ = std::fs::remove_file(&self.yaml_path);
                }
            }

            if self.staged_extra.is_some() {
                if let Some((ref extra_path, ref orig_opt)) = self.extra_file {
                    if let Some(ref orig) = orig_opt {
                        let _ = std::fs::write(extra_path, orig);
                    } else {
                        let _ = std::fs::remove_file(extra_path);
                    }
                }
            }

            if self.staged_json {
                if let Some(ref orig) = self.original_json {
                    let _ = std::fs::write(&self.json_path, orig);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(res.unwrap_err().contains("несоответствие скобок") || res.unwrap_err().contains("лишняя"));
    }
}
