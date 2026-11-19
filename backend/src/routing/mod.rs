//! Раздельная маршрутизация per-device: AUTO-DEVICE-GROUPS / AUTO-DEVICE-RULES блоки
//! в /opt/etc/mihomo/config.yaml. Формат блоков 1:1 с десктопным KeeneticPolicyManager
//! (общее хранилище правил ПК/Android/веб-версий).

pub use crate::whitelist_catalog::*;

pub mod validator;
pub mod dns_redirect;
pub mod ipset;
pub mod policy_routing;
pub mod policy_isolation;
pub mod iptables;

pub use validator::*;
pub use dns_redirect::*;
pub use ipset::*;
pub use policy_routing::*;
pub use policy_isolation::*;
pub use iptables::*;


#[cfg(test)]
mod tests;
