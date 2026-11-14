//! Интеллектуальное обнаружение CDN и медиа-доменов для проксируемых сайтов.
//! Сочетает базу известных бандлов, опрос типовых CDN-поддоменов и быстрый HTML-сканер.

use std::collections::BTreeSet;
use std::time::Duration;

/// Каталог известных бандлов: основной домен -> список связанных CDN и медиа-серверов.
pub const KNOWN_BUNDLES: &[(&str, &[&str])] = &[
    (
        "mysku.club",
        &[
            "mysku-st.ru",
            "mysku-st.net",
            "img.mysku-st.ru",
            "art.mysku-st.net",
            "ext.mysku-st.net",
        ],
    ),
    (
        "mysku.ru",
        &[
            "mysku-st.ru",
            "mysku-st.net",
            "img.mysku-st.ru",
            "art.mysku-st.net",
            "ext.mysku-st.net",
        ],
    ),
    ("habr.com", &["habrastorage.org", "web.habrastorage.org", "hsto.org", "assets.habr.com"]),
    (
        "rutracker.org",
        &["rutracker.cc", "rutracker.net", "static.rutracker.cc"],
    ),
    ("4pda.to", &["s.4pda.to", "cs4pda.to", "i.4pda.to"]),
    ("4pda.ru", &["s.4pda.to", "cs4pda.to", "i.4pda.to"]),
    (
        "kinozal.tv",
        &["kinozal-tv.org", "s1.kinozal.tv", "s2.kinozal.tv"],
    ),
    ("rutor.info", &["rutor.is", "d.rutor.info"]),
    ("flibusta.is", &["flibusta.site", "flibusta.me"]),
    ("twitter.com", &["twimg.com", "t.co", "x.com"]),
    ("x.com", &["twimg.com", "t.co", "twitter.com"]),
    ("instagram.com", &["cdninstagram.com", "ig.me"]),
    ("ntc.party", &["ntc-party.discourse-cdn.com"]),
    (
        "avito.ru",
        &[
            "avito.st",
            "static.avito.st",
            "avitocdn.net",
            "oavito.com",
            "oavito.ru",
        ],
    ),
    (
        "github.com",
        &[
            "githubassets.com",
            "githubusercontent.com",
            "raw.githubusercontent.com",
            "assets-cdn.github.com",
            "objects.githubusercontent.com",
            "codeload.github.com",
            "avatars.githubusercontent.com",
            "api.github.com",
            "gist.github.com",
        ],
    ),
    (
        "steampowered.com",
        &[
            "steamcommunity.com",
            "steamstatic.com",
            "steamcontent.com",
            "steamcdn-a.akamaihd.net",
        ],
    ),
    (
        "steamcommunity.com",
        &["steampowered.com", "steamstatic.com", "steamcontent.com"],
    ),
    (
        "twitch.tv",
        &["ttvnw.net", "jtvnw.net", "live-video.net"],
    ),
    (
        "speedtest.net",
        &["ooklaserver.net", "speedtestcustom.com"],
    ),
    (
        "hdrezka.ag",
        &["hdrezka.me", "rezka.ag", "stream.voidboost.cc"],
    ),
    (
        "lostfilm.tv",
        &["lostfilm.run", "lostfilm.online", "static.lostfilm.top"],
    ),
    (
        "flow.google.com",
        &[
            "labs.google",
            "aisandbox-pa.googleapis.com",
            "aisandbox-pa.google",
            "alkalimakersuite-pa.googleapis.com",
            "deepmind.google",
            "genai-media.googleusercontent.com",
            "video-downloads.googleusercontent.com",
        ],
    ),
    (
        "labs.google",
        &[
            "flow.google.com",
            "aisandbox-pa.googleapis.com",
            "genai-media.googleusercontent.com",
            "video-downloads.googleusercontent.com",
        ],
    ),
];

/// Типовые префиксы CDN-поддоменов.
const COMMON_CDN_PREFIXES: &[&str] = &[
    "img", "art", "ext", "cdn", "static", "assets", "media", "images", "pic", "files",
    "web", "s", "i", "st", "cache", "thumb",
];

/// Раскрывает список доменов по известным бандлам.
pub fn expand_bundles(domains: &[String]) -> BTreeSet<String> {
    let mut extra = BTreeSet::new();
    for d in domains {
        let clean = d.trim().to_lowercase();
        let base = clean.trim_start_matches("www.");
        for &(key, cdns) in KNOWN_BUNDLES {
            if base == key || base.ends_with(&format!(".{key}")) {
                for &cdn in cdns {
                    if cdn != base {
                        extra.insert(cdn.to_string());
                    }
                }
            }
        }
    }
    extra
}

/// Проверяет, что IP является публичным адресом, а не fake-ip (198.18.0.0/15) или частным/локальным адресом (баг 87).
fn is_public_cdn_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            let is_fake_ip = o[0] == 198 && (o[1] & 0xfe) == 18; // 198.18.0.0/15
            let is_cgnat = o[0] == 100 && (o[1] & 0xc0) == 64; // 100.64.0.0/10
            !v4.is_private()
                && !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_broadcast()
                && !v4.is_unspecified()
                && !v4.is_multicast()
                && !is_fake_ip
                && !is_cgnat
        }
        std::net::IpAddr::V6(v6) => {
            let s0 = v6.segments()[0];
            let is_ula = (s0 & 0xfe00) == 0xfc00;
            let is_ll = (s0 & 0xffc0) == 0xfe80;
            !v6.is_loopback() && !v6.is_unspecified() && !v6.is_multicast() && !is_ula && !is_ll
        }
    }
}

/// Чистый фильтр поддоменов CDN (баг 87):
/// отбрасывает fake-ip/частные адреса и результаты, полностью совпадающие с wildcard-DNS.
pub fn accept_subdomains(
    wildcard_ips: Option<&std::collections::HashSet<std::net::IpAddr>>,
    results: &[(String, std::collections::HashSet<std::net::IpAddr>)],
) -> Vec<String> {
    let mut accepted = Vec::new();
    for (sub, ips) in results {
        let public_ips: std::collections::HashSet<std::net::IpAddr> =
            ips.iter().copied().filter(|ip| is_public_cdn_ip(*ip)).collect();
        if public_ips.is_empty() {
            continue;
        }
        if let Some(wc) = wildcard_ips.filter(|s| !s.is_empty()) {
            // Принимаем поддомен только если есть публичный IP, отличный от wildcard-набора
            if !public_ips.iter().any(|ip| !wc.contains(ip)) {
                continue;
            }
        }
        accepted.push(sub.clone());
    }
    accepted
}

/// Асинхронно проверяет типичные CDN-поддомены (img., cdn., static. и т.д.) параллельно
pub async fn probe_subdomains(domain: &str) -> Vec<String> {
    let clean = domain.trim().trim_start_matches("www.").to_lowercase();
    if clean.is_empty() || clean.contains('/') {
        return Vec::new();
    }

    // Проверяем wildcard-DNS через случайную несуществующую метку
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(1);
    let wc_host = format!("xkr-{nonce:x}.{clean}:443");
    let mut wildcard_ips = std::collections::HashSet::new();
    if let Ok(Ok(iter)) = tokio::time::timeout(Duration::from_millis(1200), tokio::net::lookup_host(wc_host)).await {
        for sa in iter {
            wildcard_ips.insert(sa.ip());
        }
    }

    let mut set = tokio::task::JoinSet::new();
    for &prefix in COMMON_CDN_PREFIXES {
        let sub = format!("{prefix}.{clean}");
        set.spawn(async move {
            let addr = format!("{sub}:443");
            let res = tokio::time::timeout(Duration::from_millis(1200), tokio::net::lookup_host(addr)).await;
            if let Ok(Ok(iter)) = res {
                let ips: std::collections::HashSet<std::net::IpAddr> = iter.map(|sa| sa.ip()).collect();
                if !ips.is_empty() {
                    return Some((sub, ips));
                }
            }
            None
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(Some(item)) = res {
            results.push(item);
        }
    }
    let wc_opt = if wildcard_ips.is_empty() { None } else { Some(&wildcard_ips) };
    accept_subdomains(wc_opt, &results)
}

static PROXIED_CLIENT: std::sync::RwLock<Option<(String, reqwest::Client)>> = std::sync::RwLock::new(None);

fn proxied_client(proxy_url: &str) -> reqwest::Client {
    if let Ok(guard) = PROXIED_CLIENT.read() {
        if let Some((ref cached_url, ref client)) = *guard {
            if cached_url == proxy_url {
                return client.clone();
            }
        }
    }
    let new_client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(proxy_url).unwrap_or_else(|_| reqwest::Proxy::custom(|_| None::<reqwest::Url>)))
        .timeout(Duration::from_millis(2500))
        
        .build()
        .unwrap_or_default();
    if let Ok(mut guard) = PROXIED_CLIENT.write() {
        *guard = Some((proxy_url.to_string(), new_client.clone()));
    }
    new_client
}

/// Быстрый парсер HTML через локальный прокси роутера для выявления доменов статики.
pub async fn scan_html_cdns(domain: &str, proxy_url: &str) -> Vec<String> {
    let mut found = BTreeSet::new();
    let clean = domain.trim().trim_start_matches("www.").to_lowercase();
    if clean.is_empty() || !crate::config::is_valid_domain(&clean) {
        return Vec::new();
    }

    let client = proxied_client(proxy_url);

    let url = format!("https://{clean}/");
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => {
            // Пробуем HTTP fallback
            let url_http = format!("http://{clean}/");
            match client.get(&url_http).send().await {
                Ok(r) => r,
                Err(_) => return Vec::new(),
            }
        }
    };

    let html = match resp.text().await {
        Ok(t) => {
            if t.len() > 150_000 {
                let mut end = 150_000;
                while end > 0 && !t.is_char_boundary(end) {
                    end -= 1;
                }
                t[..end].to_string()
            } else {
                t
            }
        }
        Err(_) => return Vec::new(),
    };

    // Извлекаем URL из src="..." и href="..."
    let root_keyword = clean.split('.').next().unwrap_or(&clean);
    static URL_PATTERN: std::sync::LazyLock<Option<regex_lite::Regex>> =
        std::sync::LazyLock::new(|| regex_lite::Regex::new(r#"(?i)(?:src|href)=["']https?://([^/"':\s]+)"#).ok());

    if let Some(re) = URL_PATTERN.as_ref() {
        for cap in re.captures_iter(&html) {
            if let Some(m) = cap.get(1) {
                let d = m.as_str().to_lowercase();
                if d == clean || d == format!("www.{clean}") {
                    continue;
                }
                // Проверяем отношение к сайту:
                // 1) Содержит корень сайта (например, mysku в mysku-st.ru)
                // 2) Является поддоменом (заканчивается на .clean)
                // 3) Является CDN-паттерном с корнем сайта
                let is_subdomain = d.ends_with(&format!(".{clean}"));
                let has_keyword = root_keyword.len() >= 4 && d.contains(root_keyword);
                let is_cdn_pattern = d.contains("cdn") || d.contains("-st.") || d.contains("static");

                if is_subdomain || (has_keyword && is_cdn_pattern) {
                    found.insert(d);
                }
            }
        }
    }

    found.into_iter().collect()
}

/// Полный цикл обнаружения CDN: бандлы + опрос поддоменов + фоновый HTML-сканер.
pub async fn discover_all_cdns(domains: &[String], proxy_url: &str) -> BTreeSet<String> {
    let mut discovered = BTreeSet::new();

    // 1. Быстрые статические бандлы
    let from_bundles = expand_bundles(domains);
    discovered.extend(from_bundles);

    // 2. Для каждого исходного домена опрашиваем поддомены и сканируем HTML
    for d in domains {
        let subdomains = probe_subdomains(d).await;
        discovered.extend(subdomains);

        let html_cdns = scan_html_cdns(d, proxy_url).await;
        discovered.extend(html_cdns);
    }

    // Исключаем из найденных те, что уже есть в исходном списке
    for d in domains {
        let clean = d.trim().to_lowercase();
        discovered.remove(&clean);
        discovered.remove(&clean.trim_start_matches("www.").to_string());
    }

    discovered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_bundles() {
        let domains = vec!["mysku.club".to_string(), "habr.com".to_string()];
        let expanded = expand_bundles(&domains);

        assert!(expanded.contains("img.mysku-st.ru"));
        assert!(expanded.contains("art.mysku-st.net"));
        assert!(expanded.contains("ext.mysku-st.net"));
        assert!(expanded.contains("habrastorage.org"));
        assert!(expanded.contains("hsto.org"));
        assert!(!expanded.contains("mysku.club"));
    }

    #[test]
    fn test_expand_unknown() {
        let domains = vec!["unknown-site.org".to_string()];
        let expanded = expand_bundles(&domains);
        assert!(expanded.is_empty());
    }

    #[test]
    fn test_expand_github_bundle() {
        let domains = vec!["github.com".to_string()];
        let expanded = expand_bundles(&domains);
        assert!(expanded.contains("githubassets.com"));
        assert!(expanded.contains("raw.githubusercontent.com"));
        assert!(expanded.contains("objects.githubusercontent.com"));
        assert!(!expanded.contains("github.com"));
    }

    #[test]
    fn test_expand_avito_bundle() {
        let domains = vec!["avito.ru".to_string()];
        let expanded = expand_bundles(&domains);
        assert!(expanded.contains("avito.st"));
        assert!(expanded.contains("static.avito.st"));
        assert!(expanded.contains("avitocdn.net"));
        assert!(expanded.contains("oavito.com"));
        assert!(expanded.contains("oavito.ru"));
        assert!(!expanded.contains("avito.ru"));
    }

    #[test]
    fn test_expand_bundles_instantaneous_with_many_domains() {
        // Анти-регрессия: обработка даже 100+ доменов (включая routerai.ru и 38 PROXY доменов)
        // должна происходить строго мгновенно в памяти (< 20 мс) без блокирующих сетевых запросов.
        let mut domains = Vec::new();
        for i in 0..100 {
            domains.push(format!("custom-site-{i}.org"));
        }
        domains.push("github.com".to_string());
        domains.push("routerai.ru".to_string());
        domains.push("tiktok.com".to_string());
        domains.push("habr.com".to_string());

        let start = std::time::Instant::now();
        let expanded = expand_bundles(&domains);
        let elapsed = start.elapsed();

        assert!(elapsed < std::time::Duration::from_millis(50), "expand_bundles must be instantaneous (< 50ms), took {:?}", elapsed);
        assert!(expanded.contains("githubassets.com"));
        assert!(expanded.contains("habrastorage.org"));
        assert!(!expanded.contains("routerai.ru"));
    }

    #[test]
    fn regression_bug_87b_wildcard_dns_is_not_cdn() {
        use std::collections::HashSet;
        use std::net::IpAddr;

        let wc_ip: IpAddr = "93.184.216.34".parse().unwrap();
        let other_pub: IpAddr = "104.16.132.229".parse().unwrap();
        let fake_ip: IpAddr = "198.18.0.15".parse().unwrap();
        let fake_ip2: IpAddr = "198.19.255.10".parse().unwrap();
        let priv_ip: IpAddr = "192.168.1.1".parse().unwrap();
        let loopback_ip: IpAddr = "127.0.0.1".parse().unwrap();

        let wc_set: HashSet<IpAddr> = [wc_ip].into_iter().collect();

        // 1. Все префиксы вернули тот же IP, что и случайная wildcard-метка -> пусто
        let results_wc = vec![
            ("img.example.com".to_string(), [wc_ip].into_iter().collect()),
            ("cdn.example.com".to_string(), [wc_ip].into_iter().collect()),
        ];
        assert!(accept_subdomains(Some(&wc_set), &results_wc).is_empty());

        // 2. Один префикс вернул иной публичный IP -> принят только он
        let results_mixed = vec![
            ("img.example.com".to_string(), [wc_ip].into_iter().collect()),
            ("cdn.example.com".to_string(), [other_pub].into_iter().collect()),
        ];
        assert_eq!(accept_subdomains(Some(&wc_set), &results_mixed), vec!["cdn.example.com".to_string()]);

        // 3. fake-ip (198.18.0.0/15) и частные/локальные адреса отклоняются
        let results_fake_priv = vec![
            ("fake1.example.com".to_string(), [fake_ip].into_iter().collect()),
            ("fake2.example.com".to_string(), [fake_ip2].into_iter().collect()),
            ("priv.example.com".to_string(), [priv_ip].into_iter().collect()),
            ("loop.example.com".to_string(), [loopback_ip].into_iter().collect()),
            ("real.example.com".to_string(), [other_pub].into_iter().collect()),
        ];
        assert_eq!(accept_subdomains(None, &results_fake_priv), vec!["real.example.com".to_string()]);
    }
}

