// Заглушка Lua-движка Gooby для сборки клиента csqtt под роутер (musl, кросс-компиляция).
// Gooby нужен только для TURN поверх TCP/TLS с пользовательскими Lua-стратегиями; VK-туннель
// XKeen Route работает по UDP и Gooby не включает. LuaJIT при кросс-сборке требует make и
// хостовые утилиты, поэтому здесь Lua-скрипты отключены: попытка их загрузить вернёт ошибку.
use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct TransformPlan {
    pub chunks: Vec<Vec<u8>>,
    pub low_ttl_first: bool,
    pub delay_ms: u64,
}

impl TransformPlan {
    pub fn passthrough(data: &[u8]) -> Self {
        Self {
            chunks: vec![data.to_vec()],
            low_ttl_first: false,
            delay_ms: 0,
        }
    }
}

pub struct LuaProcessor;

impl LuaProcessor {
    pub fn new(_source: &str, script_name: &str, _host: &str, _port: u16) -> Result<Self> {
        bail!("Lua-стратегии Gooby недоступны в сборке для роутера ({script_name})")
    }

    pub fn process(&mut self, packet: &[u8], _first: bool) -> Result<TransformPlan> {
        Ok(TransformPlan::passthrough(packet))
    }
}
