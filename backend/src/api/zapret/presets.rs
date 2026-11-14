use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;

use super::diagnostics::BLOCKCHECK_STRATEGIES;
use super::firewall::{
    build_zapret_firewall_script_with_queue, parse_zapret_queue_spec,
    QueueSpec, DEFAULT_ZAPRET_HOSTS, NDM_IFSTATE_SCRIPT, NDM_NETFILTER_SCRIPT,
};
use crate::api::{api_err, api_ok, atomic_write_file};
use crate::{config, log_w, routing, AppState};
pub fn is_nfqws2_available() -> bool {
    (std::path::Path::new("/opt/zapret2/nfqws2").exists()
        || std::path::Path::new("/opt/sbin/nfqws2").exists()
        || std::path::Path::new("/opt/bin/nfqws2").exists()
        || std::path::Path::new("/opt/usr/bin/nfqws2").exists()
        || std::path::Path::new("/opt/zapret/nfq/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/arm64/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/arm/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/mipsel/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/mips/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/x86_64/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-arm64/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-arm/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-mips32r2-lsb/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-mips32r2-msb/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-x86_64/nfqws2").exists())
        && (std::path::Path::new("/opt/zapret2/lua/zapret-lib.lua").exists()
            || std::path::Path::new("/opt/zapret/lua/zapret-lib.lua").exists()
            || std::path::Path::new("/opt/zapret2/files/lua/zapret-lib.lua").exists())
}

pub fn is_nfqws1_available() -> bool {
    std::path::Path::new("/opt/zapret/nfq/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-arm64/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-arm/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-mips32r2-lsb/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-mips32r2-msb/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-x86_64/nfqws").exists()
        || std::path::Path::new("/opt/sbin/nfqws").exists()
        || std::path::Path::new("/opt/bin/nfqws").exists()
        || std::path::Path::new("/opt/usr/bin/nfqws").exists()
        || std::path::Path::new("/opt/zapret/nfqws.bak").exists()
}

pub fn normalize_engine_choice(engine: &str) -> &'static str {
    match engine.trim().to_ascii_lowercase().as_str() {
        "v1" | "legacy" | "1" | "1.x" | "nfqws" => "v1",
        _ => "v2",
    }
}

pub fn should_use_nfqws2(cfg: &crate::config::ZapretConfig) -> bool {
    #[cfg(not(target_os = "linux"))]
    {
        normalize_engine_choice(&cfg.engine) == "v2"
    }
    #[cfg(target_os = "linux")]
    {
        if normalize_engine_choice(&cfg.engine) == "v1" {
            !is_nfqws1_available() && is_nfqws2_available()
        } else {
            is_nfqws2_available()
        }
    }
}

pub async fn detect_installed_zapret1_version() -> String {
    if let Ok(ver) = tokio::fs::read_to_string("/opt/zapret/version.txt").await {
        let v = ver.trim();
        if !v.is_empty() {
            return if v.starts_with('v') { v.to_string() } else { format!("v{v}") };
        }
    }
    "v72.13".to_string()
}

pub async fn detect_installed_zapret2_version() -> String {
    if let Ok(ver) = tokio::fs::read_to_string("/opt/zapret2/version.txt").await {
        let v = ver.trim();
        if !v.is_empty() {
            return if v.starts_with('v') { v.to_string() } else { format!("v{v}") };
        }
    }
    "v1.0.6".to_string()
}

pub fn is_zapret_5am_due(last_checked: Option<&str>, now: chrono::DateTime<chrono::Local>) -> bool {
    let naive_now = now.naive_local();
    use chrono::Datelike;
    if naive_now.year() < 2024 {
        return false;
    }
    let today_5am = match naive_now.date().and_hms_opt(5, 0, 0) {
        Some(t) => t,
        None => return true,
    };
    let most_recent_5am = if naive_now >= today_5am {
        today_5am
    } else {
        today_5am - chrono::Duration::days(1)
    };

    match last_checked {
        None => true,
        Some(s) => {
            if let Ok(last_dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                last_dt < most_recent_5am
            } else {
                true
            }
        }
    }
}

pub async fn check_zapret_update_core(state: &AppState, force: bool) -> Result<serde_json::Value, String> {
    let now = chrono::Local::now();
    let v1_inst = is_nfqws1_available();
    let v2_inst = is_nfqws2_available();
    let (engine, cur_ver, last_checked, update_avail, latest_ver) = {
        let cfg = state.config.read().await;
        let eng = if normalize_engine_choice(&cfg.zapret.engine) == "v1" {
            "v1"
        } else if v2_inst {
            "v2"
        } else if v1_inst {
            "v1"
        } else {
            normalize_engine_choice(&cfg.zapret.engine)
        };
        (
            eng.to_string(),
            if eng == "v2" { detect_installed_zapret2_version().await } else { detect_installed_zapret1_version().await },
            cfg.zapret.last_update_check.clone(),
            cfg.zapret.update_available,
            cfg.zapret.latest_version.clone(),
        )
    };

    let default_notes = if engine == "v2" {
        "• Движок Zapret 2 (nfqws2 + Lua): поддержка адаптивных Lua-стратегий десинхронизации (multisplit, multidisorder, fake TLS ClientHello с рандомизацией Session ID и tcp_ts_up).\n• Полная совместимость с YouTube, Discord и голосовыми UDP-каналами (STUN / IP Discovery).\n• Мгновенное переключение между движками Zapret 1 и Zapret 2 без переустановки."
    } else {
        "• Классический движок Zapret 1 (nfqws v72.13): проверенная десинхронизация fake,split2 / disorder2 с минимальным потреблением RAM.\n• Доступно переключение на движок Zapret 2 (nfqws2 + Lua) в 1 клик."
    };

    let is_due = is_zapret_5am_due(last_checked.as_deref(), now);

    if !force && !is_due && last_checked.is_some() {
        let label = if engine == "v2" {
            format!("Zapret 2 {cur_ver}")
        } else {
            format!("Zapret 1 {cur_ver}")
        };
        let lat = latest_ver.unwrap_or_else(|| cur_ver.clone());
        let effective_update = update_avail && crate::updater::is_newer(&lat, &cur_ver);
        return Ok(json!({
            "current_engine": engine,
            "current_version": cur_ver,
            "label": label,
            "latest_version": lat,
            "update_available": effective_update,
            "upgrade_available": engine == "v1" && !v2_inst,
            "v1_installed": v1_inst,
            "v2_installed": v2_inst,
            "notes": default_notes,
            "last_check": last_checked,
        }));
    }

    let proxy_url = {
        let cfg = state.config.read().await;
        cfg.mihomo_proxy_url()
    };

    let target_repo = if engine == "v2" {
        "bol-van/zapret2"
    } else {
        "bol-van/zapret"
    };

    let mut fetched_latest: Option<String> = None;
    let mut fetched_notes: Option<String> = None;
    let direct_client = &state.http;
    let proxied_client = reqwest::Proxy::all(&proxy_url)
        .ok()
        .and_then(|p| reqwest::Client::builder().proxy(p).build().ok());

    let mut clients = Vec::new();
    if let Some(ref p) = proxied_client {
        clients.push(p);
    }
    clients.push(direct_client);

    let gh_latest_url = format!("https://github.com/{target_repo}/releases/latest");
    for client in &clients {
        if let Ok(res) = client.get(&gh_latest_url).timeout(std::time::Duration::from_secs(6)).send().await {
            let final_url = res.url().to_string();
            if let Some(tag) = final_url.split("/releases/tag/").nth(1) {
                let tag = tag.trim_matches('/').to_string();
                if !tag.is_empty() {
                    fetched_latest = Some(tag);
                    break;
                }
            }
        }
    }

    let gh_api_url = format!("https://api.github.com/repos/{target_repo}/releases/latest");
    for client in &clients {
        if let Ok(res) = client
            .get(&gh_api_url)
            .header("User-Agent", "xkeen-route")
            .header("Accept", "application/vnd.github+json")
            .timeout(std::time::Duration::from_secs(6))
            .send()
            .await
        {
            if res.status().is_success() {
                if let Ok(body) = res.json::<serde_json::Value>().await {
                    if fetched_latest.is_none() {
                        if let Some(tag) = body.get("tag_name").and_then(|v| v.as_str()) {
                            fetched_latest = Some(tag.to_string());
                        }
                    }
                    if let Some(b) = body.get("body").and_then(|v| v.as_str()) {
                        if !b.trim().is_empty() {
                            fetched_notes = Some(b.trim().to_string());
                        }
                    }
                    break;
                }
            }
        }
    }

    let default_latest = if engine == "v2" { "v1.0.6".to_string() } else { "v72.13".to_string() };
    // Z-07: при недоступности GitHub захардкоженный дефолт НЕ считается «последней
    // версией» — иначе в офлайне появлялось фантомное update_available=true,
    // которое ещё и персистилось в конфиг. Сохраняем предыдущее состояние флага.
    let fetched_any = fetched_latest.is_some();
    let final_latest = fetched_latest.unwrap_or_else(|| latest_ver.clone().unwrap_or(default_latest));

    let has_newer_tag = fetched_any && crate::updater::is_newer(&final_latest, &cur_ver);
    let upgrade_available = engine == "v1" && !v2_inst;
    let update_available = if fetched_any { has_newer_tag } else { update_avail };

    let now_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let display_latest = if upgrade_available && !has_newer_tag {
        "v1.0.6".to_string()
    } else {
        final_latest
    };

    {
        let _guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.last_update_check = Some(now_str.clone());
        cfg.zapret.update_available = update_available;
        cfg.zapret.latest_version = Some(display_latest.clone());
        let _ = crate::config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg);
    }

    let label = if engine == "v2" {
        format!("Zapret 2 {cur_ver}")
    } else {
        format!("Zapret 1 {cur_ver}")
    };

    Ok(json!({
        "current_engine": engine,
        "current_version": cur_ver,
        "label": label,
        "latest_version": display_latest,
        "update_available": update_available,
        "upgrade_available": upgrade_available,
        "v1_installed": v1_inst,
        "v2_installed": v2_inst,
        "notes": fetched_notes.unwrap_or_else(|| default_notes.to_string()),
        "last_check": now_str,
    }))
}

pub async fn check_zapret_update_route(
    State(state): State<AppState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let force = query.get("force").map(|v| v == "1" || v == "true").unwrap_or(false);
    match check_zapret_update_core(&state, force).await {
        Ok(data) => api_ok(data),
        Err(e) => api_err(e),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ZapretHardwareInfo {
    pub model: String,
    pub arch_label: String,
    pub target_arch: String,
    pub ram_mb: u64,
    pub recommended_engine: String,
    pub hint_text: String,
}

pub fn normalize_router_model(raw_model: &str, raw_device: &str, raw_hw_id: &str) -> String {
    let mut m = raw_model.trim().to_string();
    if m.is_empty() {
        m = raw_device.trim().to_string();
    }
    if let Some(rest) = m.strip_prefix("Keenetic ") {
        m = rest.trim().to_string();
    } else if let Some(rest) = m.strip_prefix("keenetic ") {
        m = rest.trim().to_string();
    }

    let dev = raw_device
        .trim()
        .strip_prefix("Keenetic ")
        .or_else(|| raw_device.trim().strip_prefix("keenetic "))
        .unwrap_or(raw_device.trim())
        .trim();

    if m.starts_with("KN-") && !dev.is_empty() && !dev.starts_with("KN-") {
        m = format!("{dev} {m}");
    }

    // Replace "(KN-XXXX)" with "KN-XXXX"
    if m.contains("(KN-") {
        m = m.replace("(KN-", "KN-").replace(')', "");
    }

    let hw = raw_hw_id.trim();
    if !hw.is_empty() && !m.contains(hw) {
        if m.is_empty() {
            m = hw.to_string();
        } else {
            m = format!("{m} {hw}");
        }
    }

    let cleaned = m.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        "Keenetic Router".to_string()
    } else {
        cleaned
    }
}

pub fn parse_arch_info(uname_m: &str, is_little_endian: bool) -> (String, String) {
    let mut arch = uname_m.trim().to_ascii_lowercase();
    if arch.is_empty() {
        arch = std::env::consts::ARCH.to_ascii_lowercase();
    }
    if arch == "aarch64" || arch == "arm64" {
        ("ARM64".to_string(), "linux-arm64".to_string())
    } else if arch.starts_with("armv7") || arch.starts_with("armv8") || arch.starts_with("arm") {
        ("ARMv7".to_string(), "linux-arm".to_string())
    } else if arch.starts_with("mips") {
        if arch.contains("el") || is_little_endian {
            ("MIPS32 (mipsel)".to_string(), "linux-mips32r2-lsb".to_string())
        } else {
            ("MIPS32 (mips)".to_string(), "linux-mips32r2-msb".to_string())
        }
    } else if arch == "x86_64" || arch == "amd64" {
        ("x86_64".to_string(), "linux-x86_64".to_string())
    } else if arch == "i686" || arch == "i386" || arch == "x86" {
        ("x86".to_string(), "linux-x86".to_string())
    } else {
        ("ARM64".to_string(), "linux-arm64".to_string())
    }
}

pub fn normalize_ram_mb(mem_total_kb: u64) -> u64 {
    let raw_mb = mem_total_kb / 1024;
    match raw_mb {
        0 => 512,
        1..=40 => 32,
        41..=72 => 64,
        73..=160 => 128,
        161..=320 => 256,
        321..=640 => 512,
        641..=1280 => 1024,
        1281..=2560 => 2048,
        other => other,
    }
}

pub fn build_zapret_hardware_info(
    raw_model: &str,
    raw_device: &str,
    raw_hw_id: &str,
    uname_m: &str,
    is_little_endian: bool,
    mem_total_kb: u64,
) -> ZapretHardwareInfo {
    let model = normalize_router_model(raw_model, raw_device, raw_hw_id);
    let (arch_label, target_arch) = parse_arch_info(uname_m, is_little_endian);
    let ram_mb = normalize_ram_mb(mem_total_kb);
    let recommended_engine = if ram_mb <= 64 || (ram_mb < 128 && target_arch.starts_with("linux-mips")) {
        "v1".to_string()
    } else {
        "v2".to_string()
    };
    let rec_label = if recommended_engine == "v2" {
        "рекомендуется Zapret 2.0"
    } else {
        "рекомендуется Legacy 1.x (экономия RAM)"
    };
    let hint_text = format!(
        "Обнаружен {} ({}, {}MB RAM) — {}",
        model, arch_label, ram_mb, rec_label
    );

    ZapretHardwareInfo {
        model,
        arch_label,
        target_arch,
        ram_mb,
        recommended_engine,
        hint_text,
    }
}

pub async fn detect_zapret_hardware(state: &AppState) -> ZapretHardwareInfo {
    let cfg = state.config.read().await.clone();
    let mut raw_model = String::new();
    let mut raw_device = String::new();
    let mut raw_hw_id = String::new();

    if let Ok(ver) = crate::rci::get_version(&state.http, &cfg).await {
        if let Some(m) = ver.get("model") {
            raw_model = m.clone();
        }
        if let Some(d) = ver.get("device") {
            raw_device = d.clone();
        }
        if let Some(h) = ver.get("hw_id") {
            raw_hw_id = h.clone();
        }
    }

    let probe_out = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("cat /proc/device-tree/model 2>/dev/null || cat /tmp/sysinfo/model 2>/dev/null || awk -F: '/machine/ {print $2; exit}' /proc/cpuinfo 2>/dev/null; echo '---'; uname -m 2>/dev/null; echo '---'; awk '/MemTotal/ {print $2}' /proc/meminfo 2>/dev/null; echo '---'; (echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }' || hexdump -s 5 -n 1 -e '\"%02x\"' /bin/sh 2>/dev/null)")
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let parts: Vec<&str> = probe_out.split("---").collect();
    let probed_model = parts.first().map(|s| s.trim().trim_matches('\0')).unwrap_or("");
    if raw_model.is_empty() && !probed_model.is_empty() {
        raw_model = probed_model.to_string();
    }
    let uname_m = parts.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or(std::env::consts::ARCH);
    let mem_total_kb = parts
        .get(2)
        .and_then(|s| s.trim().lines().next())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(512 * 1024);
    let endian_str = parts.get(3).map(|s| s.trim()).unwrap_or("");
    let is_little_endian = if endian_str.starts_with('1') || endian_str.starts_with("01") {
        true
    } else { endian_str.is_empty() };

    build_zapret_hardware_info(
        &raw_model,
        &raw_device,
        &raw_hw_id,
        uname_m,
        is_little_endian,
        mem_total_kb,
    )
}


pub fn convert_lua_to_legacy_desync(args: &str) -> String {
    if (args.contains("--daemon") || args.contains("--qnum"))
        && !args.contains("lua-desync")
        && !args.contains("payload=")
        && !args.contains("out-range=")
    {
        return args.replace("multisplit", "split2").replace("multidisorder", "disorder2");
    }
    if args.contains("disorder2") || args.contains("multidisorder") {
        if args.contains("midsld") {
            "--dpi-desync=fake,disorder2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=5 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
        } else {
            "--dpi-desync=fake,disorder2 --dpi-desync-split-pos=1 --dpi-desync-repeats=4 --dpi-desync-fooling=md5sig --dpi-desync-cutoff=d4".to_string()
        }
    } else if args.contains("seqovl") {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-split-seqovl=5 --dpi-desync-fooling=badseq --dpi-desync-cutoff=d4".to_string()
    } else if args.contains("multisplit") {
        if args.contains("midsld") {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4".to_string()
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
        }
    } else if args.contains("split2") || args.contains("split") {
        if args.contains("ts_up") || args.contains("repeats=6") {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-cutoff=d4".to_string()
        }
    } else {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
    }
}

pub fn build_nfqws_args_with_desync(cfg: &crate::config::ZapretConfig, custom_desync: Option<&str>) -> (String, bool) {
    let mut profiles: Vec<String> = Vec::new();

    let default_yt_desync = if cfg.aggressive_dpi {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4"
    } else {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
    };
    let yt_desync = custom_desync.unwrap_or(default_yt_desync);

    // YouTube profile (TCP 80/443) - fake,split2 at pos 1 with repeats=6 and ts (TCP timestamp) fooling reliably bypasses TSPU inspection
    if cfg.youtube_turbo || cfg.hybrid_youtube || cfg.smart_tv_mode {
        let yt_domains = if cfg.smart_tv_mode {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,redirector.googlevideo.com,manifest.googlevideo.com,gvt1.com,play.google.com"
        } else {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com"
        };
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist-domains={yt_domains} {yt_desync}"
        ));
    }

    // Discord Web/Chat profile - fake,split2 with ts fooling for TLS 1.3
    if cfg.hybrid_discord {
        let dc_default = if cfg.aggressive_dpi {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4"
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
        };
        let dc_desync = custom_desync.unwrap_or(dc_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist-domains=discord.com,discord.gg,discordapp.com,discordapp.net,discord.media,discord-attachments-uploads-prd.storage.googleapis.com,dis.gd,discord-activities.com {dc_desync}"
        ));
    }

    // Discord Voice UDP profile
    if cfg.discord_voice_udp {
        profiles.push("--filter-udp=50000-65535 --filter-l7=discord,stun --dpi-desync=fake --dpi-desync-repeats=6".to_string());
    }

    // General Web Hostlist profile (включает универсальный хостлист, GitHub, торренты, 18+ и пользовательские сайты)
    let has_active_custom = cfg.custom_entries.iter().any(|e| e.enabled);
    if cfg.general_bypass || cfg.bypass_github || cfg.bypass_torrents || cfg.bypass_adult || cfg.community_hostlist_enabled || has_active_custom {
        let gen_default = if cfg.aggressive_dpi {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4"
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
        };
        let gen_desync = custom_desync.unwrap_or(gen_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist=/opt/etc/zapret/zapret-hosts.txt {gen_desync}"
        ));
    }

    // If no specific profiles enabled, provide safe basic profile
    if profiles.is_empty() {
        let fallback_desync = custom_desync.unwrap_or("--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4");
        profiles.push(format!("--filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com {}", fallback_desync));
    }

    let args = format!("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 {}", profiles.join(" --new "));
    let voice_enabled = cfg.discord_voice_udp;
    (args, voice_enabled)
}

pub fn build_nfqws_args(cfg: &crate::config::ZapretConfig) -> (String, bool) {
    build_nfqws_args_with_desync(cfg, None)
}

/// Генератор аргументов zapret2 (nfqws2 с рантаймом Lua, --payload и --lua-desync)
pub fn build_nfqws2_args_with_desync(cfg: &crate::config::ZapretConfig, custom_desync: Option<&str>) -> (String, bool) {
    let mut profiles: Vec<String> = Vec::new();

    let default_fake_desync = if cfg.aggressive_dpi {
        "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"
    } else if cfg.youtube_turbo || cfg.smart_tv_mode {
        "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1,midsld"
    } else {
        "--lua-desync=fake:blob=fake_default_tls --lua-desync=multisplit:pos=1"
    };
    let fake_desync = custom_desync.unwrap_or(default_fake_desync);

    // YouTube profile (TCP 80/443) - Fake ClientHello + multisplit desync с рандомизацией и ранняя отсечка -d10
    if cfg.youtube_turbo || cfg.hybrid_youtube || cfg.smart_tv_mode {
        let yt_domains = if cfg.smart_tv_mode {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,redirector.googlevideo.com,manifest.googlevideo.com,gvt1.com,play.google.com"
        } else {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com"
        };
        profiles.push(format!(
            "--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains={yt_domains} --out-range=-d10 --payload=tls_client_hello {fake_desync}"
        ));
    }

    // Discord Web/Chat profile (TCP 80/443)
    if cfg.hybrid_discord {
        let dc_default = if cfg.aggressive_dpi {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"
        } else {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1,midsld"
        };
        let dc_desync = custom_desync.unwrap_or(dc_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=discord.com,discord.gg,discordapp.com,discordapp.net,discord.media,discord-attachments-uploads-prd.storage.googleapis.com,dis.gd,discord-activities.com --out-range=-d10 --payload=tls_client_hello {dc_desync}"
        ));
    }

    // Discord Voice UDP profile (перехват голосовых каналов UDP 50000:65535, фейковые пакеты для STUN и IP Discovery)
    if cfg.discord_voice_udp {
        profiles.push("--filter-udp=50000-65535 --filter-l7=discord,stun --payload=discord_ip_discovery,stun --lua-desync=fake:blob=0x00000000:repeats=4".to_string());
    }

    // General Web Hostlist profile (общий файл zapret-hosts.txt)
    let has_active_custom = cfg.custom_entries.iter().any(|e| e.enabled);
    if cfg.general_bypass || cfg.bypass_github || cfg.bypass_torrents || cfg.bypass_adult || cfg.community_hostlist_enabled || has_active_custom {
        let gen_default = if cfg.aggressive_dpi {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"
        } else {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1"
        };
        let desync_args = custom_desync.unwrap_or(gen_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --filter-l7=tls,http --hostlist=/opt/etc/zapret/zapret-hosts.txt --out-range=-d10 --payload=tls_client_hello {desync_args}"
        ));
    }

    // Базовый безопасный профиль по умолчанию, если ничего не выбрано
    if profiles.is_empty() {
        let fallback_desync = custom_desync.unwrap_or("--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1");
        profiles.push(format!("--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com --out-range=-d10 --payload=tls_client_hello {}", fallback_desync));
    }

    let args = format!("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 {}", profiles.join(" --new "));
    let voice_enabled = cfg.discord_voice_udp;
    (args, voice_enabled)
}

pub fn build_nfqws2_args(cfg: &crate::config::ZapretConfig) -> (String, bool) {
    build_nfqws2_args_with_desync(cfg, None)
}

pub const ZAPRET_HOSTS_MANAGED_BEGIN: &str = "# --- START XKEEN ROUTE DYNAMIC HOSTS ---";
pub const ZAPRET_HOSTS_MANAGED_END: &str = "# --- END XKEEN ROUTE DYNAMIC HOSTS ---";
pub const ZAPRET_COMMUNITY_MANAGED_BEGIN: &str = "# --- START COMMUNITY HOSTLIST ---";
pub const ZAPRET_COMMUNITY_MANAGED_END: &str = "# --- END COMMUNITY HOSTLIST ---";

pub fn strip_managed_blocks(text: &str) -> String {
    let mut manual_lines: Vec<String> = Vec::new();
    let mut in_managed_block = false;
    let mut in_community_block = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == ZAPRET_HOSTS_MANAGED_BEGIN {
            in_managed_block = true;
            continue;
        }
        if trimmed == ZAPRET_HOSTS_MANAGED_END {
            in_managed_block = false;
            continue;
        }
        if trimmed == ZAPRET_COMMUNITY_MANAGED_BEGIN {
            in_community_block = true;
            continue;
        }
        if trimmed == ZAPRET_COMMUNITY_MANAGED_END {
            in_community_block = false;
            continue;
        }
        if !in_managed_block && !in_community_block {
            manual_lines.push(line.to_string());
        }
    }
    manual_lines.join("\n")
}

pub fn sync_zapret_hosts_content(existing: &str, cfg: &crate::config::ZapretConfig) -> String {
    // 1. Исключаем существующий блок авто-управления и community-блок, сохраняя ручные строки пользователя
    let stripped = strip_managed_blocks(existing);
    let mut manual_lines: Vec<String> = stripped.lines().map(|l| l.to_string()).collect();

    // 2. Список динамически управляемых доменов
    let mut dynamic_domains: Vec<String> = Vec::new();

    let mut add_dynamic = |d: &str| {
        let clean = crate::config::normalize_domain(d);
        if !clean.is_empty() && !dynamic_domains.iter().any(|existing| existing.eq_ignore_ascii_case(&clean)) {
            dynamic_domains.push(clean);
        }
    };

    if cfg.bypass_github {
        for d in crate::routing::GITHUB_DOMAINS {
            add_dynamic(d);
        }
    }

    if cfg.bypass_torrents {
        for d in crate::routing::TORRENT_DOMAINS {
            add_dynamic(d);
        }
    }

    if cfg.bypass_adult {
        for d in crate::routing::ADULT_DOMAINS {
            add_dynamic(d);
        }
    }

    for entry in &cfg.custom_entries {
        if entry.enabled {
            add_dynamic(&entry.domain);
            for cdn in &entry.cdns {
                add_dynamic(cdn);
            }
        }
    }

    // 3. Удаляем динамические домены из ручной части (предотвращает дубли и зависание отключенных доменов)
    let mut all_managed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for d in crate::routing::GITHUB_DOMAINS {
        all_managed.insert(crate::config::normalize_domain(d));
    }
    for d in crate::routing::TORRENT_DOMAINS {
        all_managed.insert(crate::config::normalize_domain(d));
    }
    for d in crate::routing::ADULT_DOMAINS {
        all_managed.insert(crate::config::normalize_domain(d));
    }
    for entry in &cfg.custom_entries {
        all_managed.insert(crate::config::normalize_domain(&entry.domain));
        for cdn in &entry.cdns {
            all_managed.insert(crate::config::normalize_domain(cdn));
        }
    }

    manual_lines.retain(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return true;
        }
        let norm = crate::config::normalize_domain(trimmed);
        !all_managed.contains(&norm)
    });

    // 4. Сборка итогового файла
    let mut result = manual_lines.join("\n").trim().to_string();
    if !result.is_empty() {
        result.push('\n');
    }

    if !dynamic_domains.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(ZAPRET_HOSTS_MANAGED_BEGIN);
        result.push_str("\n# Автоматически управляемые службы и сайты /boost (Zapret)\n");
        for d in dynamic_domains {
            result.push_str(&d);
            result.push('\n');
        }
        result.push_str(ZAPRET_HOSTS_MANAGED_END);
        result.push('\n');
    }

    result
}

pub fn validate_custom_args(args: &str) -> Result<(), String> {
    for ch in args.chars() {
        if ch == ';' || ch == '&' || ch == '|' || ch == '`' || ch == '$' || ch == '\n' || ch == '\r' || ch == '(' || ch == ')' || ch == '<' || ch == '>' || ch == '!' || ch == '"' || ch == '\'' {
            return Err(format!("Недопустимый спецсимвол в аргументах Zapret: '{}'. Разрешены только флаги nfqws/nfqws2", ch));
        }
    }
    parse_zapret_queue_spec(args).map(|_| ())?;
    Ok(())
}

pub fn validate_zapret_conf(content: &str) -> Result<(), String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = trimmed.split_once('=') {
            let k = key.trim();
            if k == "NFQWS_ARGS" {
                let clean_val = val.trim().trim_matches('"').trim_matches('\'');
                validate_custom_args(clean_val)?;
            } else if k == "ZAPRET_ENGINE" {
                let clean_val = val.trim().trim_matches('"').trim_matches('\'');
                if clean_val != "v1" && clean_val != "v2" && clean_val != "legacy" && clean_val != "modern" {
                    return Err(format!("Недопустимое значение ZAPRET_ENGINE: '{clean_val}'. Разрешены только v1, v2"));
                }
            } else if k == "EXCLUDED_IPS" {
                // Z-04: значения попадают в `iptables -s "$ex_ip"` внутри S51zapret —
                // без валидации command substitution `$(cmd)` выполнялся бы как RCE.
                let clean_val = val.trim().trim_matches('"').trim_matches('\'');
                for ip in clean_val.split_whitespace() {
                    if !crate::config::is_valid_ip_or_cidr(ip) {
                        return Err(format!("Недопустимое значение в EXCLUDED_IPS: '{ip}'. Разрешены только IP-адреса или CIDR"));
                    }
                }
            } else if k == "EXCLUDED_MACS" {
                // Z-04: аналогичная защита для MAC-исключений
                let clean_val = val.trim().trim_matches('"').trim_matches('\'');
                for mac in clean_val.split_whitespace() {
                    if !crate::config::is_valid_mac(mac) {
                        return Err(format!("Недопустимое значение в EXCLUDED_MACS: '{mac}'. Разрешены только MAC-адреса формата 00:11:22:33:44:55"));
                    }
                }
            } else if k != "DISCORD_VOICE_ENABLED" && k != "BLOCK_QUIC" && k != "SMART_TV_MODE" && k != "ADGUARD_ENABLED" {
                return Err(format!("Неизвестный параметр в zapret.conf: '{k}'. Разрешены только ZAPRET_ENGINE, NFQWS_ARGS, DISCORD_VOICE_ENABLED, BLOCK_QUIC, SMART_TV_MODE, EXCLUDED_IPS, EXCLUDED_MACS, ADGUARD_ENABLED"));
            }
        } else {
            return Err("Некорректная строка в zapret.conf: должна быть в формате КЛЮЧ=\"ЗНАЧЕНИЕ\"".to_string());
        }
    }
    Ok(())
}

/// Флаг ADGUARD_ENABLED для zapret.conf (баг 31): AdGuard «активен», только если интеграция включена
/// в конфиге или процесс AdGuardHome реально работает. Одно наличие init-скрипта ничего не значит.
pub fn zapret_conf_adguard_flag(integration_enabled: bool, process_running: bool, _installed: bool) -> bool {
    integration_enabled || process_running
}

pub async fn sync_zapret_files(cfg: &crate::config::ZapretConfig) -> Result<(), String> {
    sync_zapret_files_with_config_path(cfg, std::path::Path::new("/opt/etc/xkeen-route/config.json")).await
}

pub async fn sync_zapret_files_with_config_path(
    cfg: &crate::config::ZapretConfig,
    config_path: &std::path::Path,
) -> Result<(), String> {
    tokio::fs::create_dir_all("/opt/etc/zapret")
        .await
        .map_err(|e| format!("Не удалось создать /opt/etc/zapret: {e}"))?;
    tokio::fs::create_dir_all("/opt/etc/init.d")
        .await
        .map_err(|e| format!("Не удалось создать /opt/etc/init.d: {e}"))?;

    // 1. S51zapret script
    let mihomo_dns_port = std::fs::read_to_string(&crate::config::MihomoConfig::default().config_path)
        .ok()
        .and_then(|y| crate::routing::parse_mihomo_dns_listen_port(&y))
        .unwrap_or(crate::routing::DEFAULT_MIHOMO_DNS_PORT);
    let gaming_ips: Vec<String> = {
        if config_path.exists() {
            let app_cfg = crate::config::load(config_path);
            if app_cfg.gaming.enabled {
                app_cfg
                    .gaming
                    .devices
                    .iter()
                    .filter(|d| d.enabled && !d.ip.trim().is_empty())
                    .map(|d| d.ip.trim().to_string())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    };
    let mut queue_spec = match cfg.custom_args.as_deref() {
        Some(custom) if !custom.trim().is_empty() => {
            validate_custom_args(custom)?;
            parse_zapret_queue_spec(custom)?
        }
        _ => QueueSpec::default(),
    };
    if let Some(custom) = cfg.custom_args.as_deref() {
        let is_full_cmdline = custom.contains("--daemon") || custom.contains("--qnum");
        if !is_full_cmdline && queue_spec.tcp_ports.is_empty() {
            queue_spec.tcp_ports = vec!["80".to_string(), "443".to_string()];
        }
    }
    let s51_script = build_zapret_firewall_script_with_queue(mihomo_dns_port, &gaming_ips, &queue_spec);
    atomic_write_file("/opt/etc/init.d/S51zapret", &s51_script)
        .await
        .map_err(|e| format!("Не удалось записать /opt/etc/init.d/S51zapret: {e}"))?;
    tokio::process::Command::new("chmod")
        .arg("+x")
        .arg("/opt/etc/init.d/S51zapret")
        .output()
        .await
        .map_err(|e| format!("Не удалось сделать S51zapret исполняемым: {e}"))?;

    // 1.1 Keenetic ndm netfilter & ifstatechanged hooks (prevent netfilter drop on DHCP/WAN reload)
    let _ = tokio::fs::create_dir_all("/opt/etc/ndm/netfilter.d").await;
    let _ = tokio::fs::create_dir_all("/opt/etc/ndm/ifstatechanged.d").await;
    let _ = tokio::fs::create_dir_all("/opt/etc/ndm/wan.d").await;
    if let Ok(()) = atomic_write_file("/opt/etc/ndm/netfilter.d/050-zapret.sh", NDM_NETFILTER_SCRIPT).await {
        let _ = tokio::process::Command::new("chmod")
            .arg("+x")
            .arg("/opt/etc/ndm/netfilter.d/050-zapret.sh")
            .output()
            .await;
    }
    if let Ok(()) = atomic_write_file("/opt/etc/ndm/ifstatechanged.d/050-zapret.sh", NDM_IFSTATE_SCRIPT).await {
        let _ = tokio::process::Command::new("chmod")
            .arg("+x")
            .arg("/opt/etc/ndm/ifstatechanged.d/050-zapret.sh")
            .output()
            .await;
    }
    if let Ok(()) = atomic_write_file("/opt/etc/ndm/wan.d/050-zapret.sh", NDM_IFSTATE_SCRIPT).await {
        let _ = tokio::process::Command::new("chmod")
            .arg("+x")
            .arg("/opt/etc/ndm/wan.d/050-zapret.sh")
            .output()
            .await;
    }

    // 2. Default hostlist if missing, and sync active domains into it
    let hosts_path = "/opt/etc/zapret/zapret-hosts.txt";
    let base_hosts = if std::path::Path::new(hosts_path).exists() {
        tokio::fs::read_to_string(hosts_path).await.unwrap_or_else(|_| DEFAULT_ZAPRET_HOSTS.to_string())
    } else {
        DEFAULT_ZAPRET_HOSTS.to_string()
    };
    let mut synced_hosts = sync_zapret_hosts_content(&base_hosts, cfg);

    // Подгрузка Community Hostlist при активации
    if cfg.community_hostlist_enabled {
        if let Ok(comm) = tokio::fs::read_to_string("/opt/etc/zapret/community-hosts.txt").await {
            let comm_domains: Vec<&str> = comm
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect();
            if !comm_domains.is_empty() {
                synced_hosts.push_str("\n");
                synced_hosts.push_str(ZAPRET_COMMUNITY_MANAGED_BEGIN);
                synced_hosts.push('\n');
                for d in comm_domains {
                    synced_hosts.push_str(d);
                    synced_hosts.push('\n');
                }
                synced_hosts.push_str(ZAPRET_COMMUNITY_MANAGED_END);
                synced_hosts.push('\n');
            }
        }
    }
    let _ = atomic_write_file(hosts_path, &synced_hosts).await;

    // 3. zapret.conf через единый сериализатор (баг 35)
    if let Some(custom) = &cfg.custom_args {
        validate_custom_args(custom)?;
    }
    let conf_data = crate::zapret::serialize_zapret_conf(cfg, config_path);
    atomic_write_file("/opt/etc/zapret/zapret.conf", &conf_data)
        .await
        .map_err(|e| format!("Не удалось записать /opt/etc/zapret/zapret.conf: {e}"))?;
    Ok(())
}

pub fn format_nfqws_proc_cmdline(raw: &[u8]) -> String {
    let mut c = String::new();
    for (idx, part) in raw.split(|&b| b == 0).filter(|s| !s.is_empty()).enumerate() {
        let token = String::from_utf8_lossy(part);
        if idx == 0 {
            c.push_str(&token);
        } else if token.starts_with('-') {
            c.push(' ');
            c.push_str(&token);
        } else {
            // Legacy nfqws modifies argv in-place via strtok(optarg, ","), replacing ',' with '\0'
            c.push(',');
            c.push_str(&token);
        }
    }
    c
}


/// GET /api/zapret/status — статус nfqws/nfqws2, iptables и S51zapret
pub async fn get_zapret_status(State(state): State<AppState>) -> Response {
    let init_script = std::path::Path::new("/opt/etc/init.d/S51zapret");
    let installed = init_script.exists() && (is_nfqws2_available() || is_nfqws1_available());

    let mut running = false;
    let mut pid: Option<u32> = None;
    let mut cmdline: Option<String> = None;

    if installed {
        if let Some(p) = crate::zapret::find_our_zapret_pid().await {
            running = true;
            pid = Some(p);
            let proc_cmd = format!("/proc/{p}/cmdline");
            if let Ok(raw) = tokio::fs::read(&proc_cmd).await {
                let c = format_nfqws_proc_cmdline(&raw);
                if !c.is_empty() {
                    cmdline = Some(c);
                }
            }
        }
    }

    let autostart = if installed {
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg("[ -x /opt/etc/init.d/S51zapret ]")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        false
    };

    let mut iptables_active = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("(iptables -w 2 -t mangle -S POSTROUTING 2>/dev/null | grep -q zapret || iptables -w 2 -t mangle -S PREROUTING 2>/dev/null | grep -q zapret) && iptables -w 2 -t mangle -nL zapret 2>/dev/null | grep -q NFQUEUE")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    let config_content = tokio::fs::read_to_string("/opt/etc/zapret/zapret.conf").await.ok();
    let hosts_content = tokio::fs::read_to_string("/opt/etc/zapret/zapret-hosts.txt").await.ok();

    let cfg = state.config.read().await.clone();

    // Z-05: GET-эндпоинт стал строго read-only — убирать процессы здесь нельзя:
    // поллинг каждые 10с и мутации в идемпотентном GET — источник гонок со стартом.
    // Чисткой осиротевшего процесса при выключенной службе занимается watchdog.
    if !cfg.zapret.enabled {
        running = false;
        iptables_active = false;
        pid = None;
        cmdline = None;
    }

    // Определение текущего пресета
    let check_str = cmdline.as_deref().or(config_content.as_deref()).unwrap_or("");
    let preset = if cfg.zapret.custom_args.is_some() {
        "custom"
    } else if cfg.zapret.aggressive_dpi || check_str.contains("ts,md5sig") || check_str.contains("badseq,md5sig") {
        "aggressive"
    } else if cfg.zapret.youtube_turbo && !cfg.zapret.discord_voice_udp && !cfg.zapret.general_bypass && !cfg.zapret.bypass_github && !cfg.zapret.hybrid_discord {
        "youtube"
    } else if !cfg.zapret.youtube_turbo && (cfg.zapret.discord_voice_udp || cfg.zapret.hybrid_discord) && !cfg.zapret.general_bypass && !cfg.zapret.hybrid_youtube {
        "discord"
    } else if cfg.zapret.youtube_turbo && cfg.zapret.discord_voice_udp && cfg.zapret.general_bypass && !cfg.zapret.bypass_adult {
        "gamer"
    } else if cfg.zapret.youtube_turbo && cfg.zapret.discord_voice_udp && cfg.zapret.general_bypass && cfg.zapret.bypass_adult {
        "default"
    } else if cfg.zapret.general_bypass {
        "general"
    } else {
        "custom"
    };

    let v2_installed = is_nfqws2_available();
    let v1_installed = is_nfqws1_available();
    let can_rollback_v1 = v1_installed
        || std::path::Path::new("/opt/etc/zapret/zapret.v1.conf.bak").exists()
        || std::path::Path::new("/opt/zapret/nfqws.bak").exists();
    let active_engine = if let Some(ref cmd) = cmdline {
        if cmd.contains("nfqws2") || cmd.contains("--lua-desync") {
            "v2"
        } else {
            "v1"
        }
    } else if normalize_engine_choice(&cfg.zapret.engine) == "v1" {
        "v1"
    } else if v2_installed {
        "v2"
    } else if v1_installed {
        "v1"
    } else {
        normalize_engine_choice(&cfg.zapret.engine)
    };
    let hardware = detect_zapret_hardware(&state).await;

    let (zapret_ver, zapret_label) = if active_engine == "v2" {
        let v = detect_installed_zapret2_version().await;
        let l = format!("Zapret 2 {v}");
        (v, l)
    } else {
        let v = detect_installed_zapret1_version().await;
        let l = format!("Zapret 1 {v}");
        (v, l)
    };

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

    let current_op = crate::zapret::get_current_operation_state();
    let operation_in_progress = current_op != crate::zapret::ZapretOperationState::Idle;
    let mihomo_dns = crate::routing::mihomo_dns_port(&cfg);
    let dns_resolver_ready = crate::zapret::check_dns_resolver_ready(mihomo_dns).await;
    let dns_redirect_active = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(format!("iptables -w 2 -t nat -S PREROUTING 2>/dev/null | grep -q '{mihomo_dns}'"))
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    api_ok(json!({
        "installed": installed,
        "running": running,
        "pid": pid,
        "autostart": autostart,
        "iptables_active": iptables_active,
        "dns_resolver_ready": dns_resolver_ready,
        "dns_redirect_active": dns_redirect_active,
        "operation_state": current_op,
        "operation_in_progress": operation_in_progress,
        "preset": preset,
        "active_strategy_id": active_strategy_id,
        "cmdline": cmdline,
        "config": config_content,
        "hosts": hosts_content,
        "features": &cfg.zapret,
        "engine": active_engine,
        "version": zapret_ver,
        "version_label": zapret_label,
        "update_available": cfg.zapret.update_available
            && cfg.zapret.latest_version.as_deref().is_some_and(|lat| crate::updater::is_newer(lat, &zapret_ver)),
        "latest_version": cfg.zapret.latest_version,
        "v2_installed": v2_installed,
        "v1_installed": v1_installed,
        "can_rollback_v1": can_rollback_v1,
        "hardware": hardware,
    }))
}

#[derive(Deserialize)]
pub struct ZapretActionReq {
    pub action: String, // "start" | "stop" | "restart" | "toggle" | "install" | "upgrade_zapret2" | "rollback_v1" | "switch_engine" | "set_preset" | "save_config" | "save_hosts" | "test_dpi" | "toggle_feature" | "set_features" | "reset_features" | "add_custom_domain" | "remove_custom_domain" | "toggle_custom_domain" | "boost_custom_domain" | "apply_strategy" | "reset_analytics" | "sync_community_hostlist"
    pub preset: Option<String>,
    pub custom_args: Option<String>,
    pub config_content: Option<String>,
    pub hosts_content: Option<String>,
    pub feature: Option<String>,
    pub enabled: Option<bool>,
    pub features: Option<ZapretFeatures>,
    pub domain: Option<String>,
    pub strategy_id: Option<String>,
    pub url: Option<String>,
    pub engine: Option<String>,
    pub version: Option<String>,
}

/// Поля «функций» Zapret, передаваемые из UI в `set_features` (без перезаписи устройств, движка и статистики).
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ZapretFeatures {
    pub enabled: Option<bool>,
    pub hybrid_youtube: Option<bool>,
    pub hybrid_discord: Option<bool>,
    pub discord_voice_udp: Option<bool>,
    pub youtube_turbo: Option<bool>,
    pub general_bypass: Option<bool>,
    pub aggressive_dpi: Option<bool>,
    pub isolated_proxy: Option<bool>,
    pub bypass_github: Option<bool>,
    pub bypass_torrents: Option<bool>,
    pub bypass_adult: Option<bool>,
    pub custom_entries: Option<Vec<crate::config::ZapretCustomEntry>>,
    pub custom_args: Option<String>,
    pub smart_tv_mode: Option<bool>,
    pub community_hostlist_enabled: Option<bool>,
    pub community_hostlist_url: Option<String>,
    pub community_hostlist_auto_update: Option<bool>,
    pub excluded_devices: Option<Vec<String>>,
    pub engine: Option<String>,
}

/// Накладывает поля функций из запроса на текущий `ZapretConfig`, сохраняя
/// `excluded_devices`, `custom_entries`, `engine`, метаданные community-листа и статистику.
pub fn merge_zapret_features(
    cur: &crate::config::ZapretConfig,
    req: &ZapretFeatures,
) -> crate::config::ZapretConfig {
    let mut merged = cur.clone();
    if let Some(v) = req.enabled {
        merged.enabled = v;
    }
    if let Some(v) = req.hybrid_youtube {
        merged.hybrid_youtube = v;
    }
    if let Some(v) = req.hybrid_discord {
        merged.hybrid_discord = v;
    }
    if let Some(v) = req.discord_voice_udp {
        merged.discord_voice_udp = v;
    }
    if let Some(v) = req.youtube_turbo {
        merged.youtube_turbo = v;
    }
    if let Some(v) = req.general_bypass {
        merged.general_bypass = v;
    }
    if let Some(v) = req.aggressive_dpi {
        merged.aggressive_dpi = v;
    }
    if let Some(v) = req.isolated_proxy {
        merged.isolated_proxy = v;
    }
    if let Some(v) = req.bypass_github {
        merged.bypass_github = v;
    }
    if let Some(v) = req.bypass_torrents {
        merged.bypass_torrents = v;
    }
    if let Some(v) = req.bypass_adult {
        merged.bypass_adult = v;
    }
    if let Some(ref ca) = req.custom_args {
        merged.custom_args = if !should_use_nfqws2(&merged)
            && (ca.contains("--lua-desync") || ca.contains("multisplit"))
        {
            Some(convert_lua_to_legacy_desync(ca))
        } else {
            Some(ca.clone())
        };
    }
    if let Some(v) = req.smart_tv_mode {
        merged.smart_tv_mode = v;
    }
    if let Some(v) = req.community_hostlist_enabled {
        merged.community_hostlist_enabled = v;
    }
    if let Some(ref u) = req.community_hostlist_url {
        if !u.trim().is_empty() {
            merged.community_hostlist_url = u.trim().to_string();
        }
    }
    if let Some(v) = req.community_hostlist_auto_update {
        merged.community_hostlist_auto_update = v;
    }
    merged
}

/// Сбрасывает только поля «функций» Zapret в значения по умолчанию,
/// сохраняя `enabled`, `excluded_devices`, `custom_entries`, `engine`, метаданные и статистику.
pub fn reset_zapret_features(cur: &crate::config::ZapretConfig) -> crate::config::ZapretConfig {
    let mut reset = crate::config::ZapretConfig::default();
    reset.enabled = cur.enabled;
    reset.excluded_devices = cur.excluded_devices.clone();
    reset.custom_entries = cur.custom_entries.clone();
    reset.engine = cur.engine.clone();
    reset.community_hostlist_last_updated = cur.community_hostlist_last_updated.clone();
    reset.community_hostlist_count = cur.community_hostlist_count;
    reset.last_update_check = cur.last_update_check.clone();
    reset.update_available = cur.update_available;
    reset.latest_version = cur.latest_version.clone();
    reset.period_saved_bytes = cur.period_saved_bytes;
    reset
}

/// Точечное применение пресета Zapret без сброса несвязанных флагов пользователя (баг 111).
pub fn apply_zapret_preset(
    zcfg: &mut crate::config::ZapretConfig,
    p: &str,
    custom_args_opt: Option<&str>,
) -> Result<(), String> {
    match p {
        "youtube" => {
            zcfg.custom_args = None;
            zcfg.youtube_turbo = true;
            zcfg.hybrid_youtube = true;
        }
        "discord" => {
            zcfg.custom_args = None;
            zcfg.hybrid_discord = true;
            zcfg.discord_voice_udp = true;
        }
        "gamer" | "media" => {
            zcfg.custom_args = None;
            zcfg.youtube_turbo = true;
            zcfg.hybrid_youtube = true;
            zcfg.hybrid_discord = true;
            zcfg.discord_voice_udp = true;
            zcfg.general_bypass = true;
            zcfg.aggressive_dpi = false;
            zcfg.isolated_proxy = true;
            zcfg.bypass_github = true;
            zcfg.bypass_torrents = true;
        }
        "aggressive" => {
            zcfg.custom_args = None;
            zcfg.youtube_turbo = true;
            zcfg.hybrid_youtube = true;
            zcfg.hybrid_discord = true;
            zcfg.discord_voice_udp = true;
            zcfg.general_bypass = true;
            zcfg.aggressive_dpi = true;
            zcfg.isolated_proxy = true;
            zcfg.bypass_github = true;
            zcfg.bypass_torrents = true;
            zcfg.bypass_adult = true;
        }
        "custom" => {
            if let Some(custom) = custom_args_opt {
                let clean = custom.trim().to_string();
                if !clean.is_empty() {
                    validate_custom_args(&clean)?;
                    let effective = if !should_use_nfqws2(zcfg)
                        && (clean.contains("--lua-desync") || clean.contains("multisplit"))
                    {
                        convert_lua_to_legacy_desync(&clean)
                    } else {
                        clean
                    };
                    zcfg.custom_args = Some(effective);
                }
            }
        }
        _ => {
            // "general" / "all"
            zcfg.custom_args = None;
            zcfg.youtube_turbo = true;
            zcfg.hybrid_youtube = true;
            zcfg.hybrid_discord = true;
            zcfg.discord_voice_udp = true;
            zcfg.general_bypass = true;
            zcfg.aggressive_dpi = false;
            zcfg.isolated_proxy = true;
            zcfg.bypass_github = true;
            zcfg.bypass_torrents = true;
            zcfg.bypass_adult = true;
        }
    }
    Ok(())
}

/// Атомарное обновление настроек Zapret, файлов zapret.conf/S51zapret и правил config.yaml через ConfigTx.
/// При ошибке reload Mihomo выполняется автоматический откат.
pub(super) async fn commit_zapret_config_with_routing<F>(
    state: &AppState,
    mutate: F,
) -> Result<config::AppConfig, String>
where
    F: FnOnce(&mut config::ZapretConfig) -> Result<(), String>,
{
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    mutate(&mut tx.config_mut().zapret)?;

    sync_zapret_files_with_config_path(&tx.config().zapret, &state.config_path)
        .await
        .map_err(|e| format!("Ошибка синхронизации файлов Zapret: {e}"))?;

    if std::path::Path::new(&tx.config().mihomo.config_path).exists() {
        let raw_yaml = tx.read_yaml().await?;
        let (new_yaml, _) = routing::apply_routing(&raw_yaml, tx.config())
            .map_err(|e| format!("Ошибка формирования маршрутов Zapret: {e}"))?;
        tx.set_yaml(new_yaml)?;
        tx.commit_and_reload().await?;
    } else {
        tx.commit_without_reload().await?;
    }

    Ok(tx.config().clone())
}

/// Применение пресета конфигурации Zapret (используется и веб-панелью, и Telegram-ботом)
pub async fn apply_zapret_preset_action(
    state: &AppState,
    p: &str,
    custom_args_opt: Option<&str>,
) -> Result<String, String> {
    let _cfg = commit_zapret_config_with_routing(state, |zapret_cfg| {
        apply_zapret_preset(zapret_cfg, p, custom_args_opt)
    })
    .await?;

    #[cfg(unix)]
    {
        let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
        if is_running && !_cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("stop").output().await;
        } else if _cfg.zapret.enabled {
            let cmd_arg = if is_running { "restart" } else { "start" };
            if let Ok(out) = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg(cmd_arg).output().await {
                if !out.status.success() {
                    let err_msg = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                    return Err(format!("Ошибка перезапуска Zapret при применении пресета '{p}':\n{}", err_msg.trim()));
                }
            }
        }
    }

    Ok(format!("Применен пресет '{p}'"))
}

/// POST /api/zapret/action — запуск, остановка, переключение, пресеты и тест DPI
/// X-04: Ядро переключения службы Zapret (вкл/выкл) для UI и Telegram-бота.
/// В отличие от прямого вызова S51zapret синхронно обновляет cfg.zapret.enabled,
/// файлы zapret.conf, маркер автозапуска (Z-03) и стабилизирует процесс —
/// иначе watchdog откатывает действие бота через ≤12с, а GET /api/zapret/status убивает процесс.
pub async fn zapret_set_enabled_core(state: &AppState, enabled: bool) -> Result<String, String> {
    let init_script = "/opt/etc/init.d/S51zapret";

    if enabled {
        if !std::path::Path::new(init_script).exists() {
            return Err("Служба Zapret (S51zapret) не установлена в /opt/etc/init.d/".to_string());
        }
        {
            // Файлы службы пишутся с целевым флагом, но в config.json он попадает только после успешного старта
            let mut zcfg = state.config.read().await.zapret.clone();
            zcfg.enabled = true;
            sync_zapret_files_with_config_path(&zcfg, &state.config_path).await?;
        }
        crate::zapret::clear_zapret_disabled_marker();
        let out = tokio::process::Command::new(init_script)
            .arg("start")
            .output()
            .await
            .map_err(|e| {
                crate::zapret::set_zapret_disabled_marker();
                format!("Не удалось запустить S51zapret: {e}")
            })?;
        if !out.status.success() {
            // Баг 57: неудачный старт не оставляет enabled=true в конфиге
            crate::zapret::set_zapret_disabled_marker();
            return Err(format!(
                "Ошибка запуска Zapret:\n{}",
                format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim()
            ));
        }
        if let Err(e) = persist_zapret_enabled_strict(state, true).await {
            // Баг 58: при ошибке reload Mihomo откатываем и службу, и конфиг
            crate::zapret::set_zapret_disabled_marker();
            let _ = tokio::process::Command::new(init_script).arg("stop").output().await;
            return Err(e);
        }
        let cur_cfg = state.config.read().await.clone();
        if cur_cfg.vkturn.enabled {
            let turn_ips = state.vkturn.rt.read().await.turn_ips.clone();
            crate::vkturn::apply_exemptions(&cur_cfg.vkturn.peer, &turn_ips).await;
        }
        Ok("🟢 Служба Zapret запущена".to_string())
    } else {
        // Баг 58: сначала транзакционно обновляем конфиг и reload Mihomo; при ошибке ничего не ломаем
        persist_zapret_enabled_strict(state, false).await?;
        crate::zapret::set_zapret_disabled_marker();
        let zcfg = state.config.read().await.zapret.clone();
        let _ = sync_zapret_files_with_config_path(&zcfg, &state.config_path).await;
        let _ = tokio::process::Command::new(init_script).arg("stop").output().await;
        Ok("⚪ Служба Zapret остановлена".to_string())
    }
}

/// Строгая транзакционная фиксация zapret.enabled с reload Mihomo без тихого fallback (баг 58).
pub async fn persist_zapret_enabled_strict(state: &AppState, enabled: bool) -> Result<(), String> {
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    tx.config_mut().zapret.enabled = enabled;
    let raw = tx.read_yaml().await?;
    let (new_yaml, _) = routing::apply_routing(&raw, tx.config())?;
    tx.set_yaml(new_yaml)?;
    tx.commit_and_reload().await
}

/// Сохранение zapret.conf с пересборкой правил Mihomo (apply_routing) внутри ConfigTx (баг 30).
pub async fn apply_zapret_conf_save(
    state: &AppState,
    content: &str,
) -> Result<config::AppConfig, String> {
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    tx.config_mut().zapret = crate::zapret::parse_zapret_conf(content, &tx.config().zapret);
    let new_cfg = tx.config().clone();

    // Если есть config.yaml Mihomo — пересобираем правила маршрутизации
    if let Ok(yaml) = tx.read_yaml().await {
        if let Ok((new_yaml, _)) = crate::routing::apply_routing(&yaml, &new_cfg) {
            tx.set_yaml(new_yaml)?;
        }
    }

    // Сохраняем и перезагружаем ядро Mihomo атомарно
    tx.commit_and_reload().await?;
    Ok(new_cfg)
}

pub async fn persist_zapret_enabled_with_routing(state: &AppState, enabled: bool) -> Result<(), String> {
    async fn attempt(state: &AppState, enabled: bool, reload: bool) -> Result<(), String> {
        let mut tx = crate::transaction::ConfigTx::begin(state).await?;
        tx.config_mut().zapret.enabled = enabled;
        let raw = tx.read_yaml().await?;
        let (new_yaml, _) = routing::apply_routing(&raw, tx.config())?;
        tx.set_yaml(new_yaml)?;
        if reload {
            tx.commit_and_reload().await
        } else {
            tx.commit_without_reload().await
        }
    }
    if let Err(e) = attempt(state, enabled, true).await {
        log_w!("[ZAPRET] Reload Mihomo не удался ({e}) — сохраняю конфигурацию без reload");
        attempt(state, enabled, false).await?;
    }
    Ok(())
}

/// Shell-скрипт параллельной проверки YouTube/Discord напрямую и через прокси Mihomo (кнопка «Проверка DPI»).
pub fn build_dpi_test_script(proxy_q: &str) -> String {
    format!(r#"
            t=$(mktemp -d 2>/dev/null || echo /tmp/xkr-dpi-$$); mkdir -p "$t"
            probe() {{ out=$(curl -4 -k -m 2.5 -s -o /dev/null -w "%{{http_code}}:%{{time_total}}" "$@" 2>/dev/null) || out="000:0.0"; [ -n "$out" ] || out="000:0.0"; echo "$out"; }}
            probe https://www.youtube.com/generate_204 > "$t/d_yt" &
            probe https://discord.com > "$t/d_dc" &
            probe -x '{proxy_q}' https://www.youtube.com/generate_204 > "$t/p_yt" &
            probe -x '{proxy_q}' https://discord.com > "$t/p_dc" &
            wait
            echo "$(cat "$t/d_yt")#$(cat "$t/d_dc")#$(cat "$t/p_yt")#$(cat "$t/p_dc")"
            rm -rf "$t"
        "#)
}

