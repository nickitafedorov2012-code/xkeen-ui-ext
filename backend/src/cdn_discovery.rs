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

/// Асинхронно проверяет типичные CDN-поддомены (img., cdn., static. и т.д.) параллельно
pub async fn probe_subdomains(domain: &str) -> Vec<String> {
    let clean = domain.trim().trim_start_matches("www.").to_lowercase();
    if clean.is_empty() || clean.contains('/') {
        return Vec::new();
    }

    let mut set = tokio::task::JoinSet::new();
    for &prefix in COMMON_CDN_PREFIXES {
        let sub = format!("{prefix}.{clean}");
        set.spawn(async move {
            let addr = format!("{sub}:443");
            let res = tokio::time::timeout(Duration::from_millis(1200), tokio::net::lookup_host(addr)).await;
            if let Ok(Ok(mut iter)) = res {
                if iter.next().is_some() {
                    return Some(sub);
                }
            }
            None
        });
    }

    let mut found = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(Some(sub)) = res {
            found.push(sub);
        }
    }
    found
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
    if clean.is_empty() {
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
}
