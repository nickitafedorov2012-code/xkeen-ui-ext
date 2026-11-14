//! Раздельная маршрутизация per-device: AUTO-DEVICE-GROUPS / AUTO-DEVICE-RULES блоки
//! в /opt/etc/mihomo/config.yaml. Формат блоков 1:1 с десктопным KeeneticPolicyManager
//! (общее хранилище правил ПК/Android/веб-версий).

pub use crate::whitelist_catalog::*;

pub mod validator;
pub mod dns_redirect;
pub mod ipset;
pub mod policy_routing;
pub mod iptables;

pub use validator::*;
pub use dns_redirect::*;
pub use ipset::*;
pub use policy_routing::*;
pub use iptables::*;

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_YAML: &str = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";

    #[test]
    fn add_assignment_creates_blocks() {
        let out = apply_assignments(
            BASE_YAML,
            &[Assignment { ip: "192.168.2.118".into(), name: "Big PC".into(), server: Some("рџ‡©рџ‡Є DE".into()) }],
            &["geodema".to_string(), "geodema2".to_string()],
            &[]
        )
        .unwrap();
        assert!(out.contains(GROUPS_BEGIN));
        assert!(out.contains("- name: 'Big PC 192_168_2_118'"));
        assert!(out.contains("SRC-IP-CIDR,192.168.2.118/32,Big PC 192_168_2_118"));
        assert!(out.contains("- geodema2"), "use: провайдеры из параметра");
        // блоки вставлены сразу после секций
        let gpos = out.find("proxy-groups:").unwrap();
        let bpos = out.find(GROUPS_BEGIN).unwrap();
        assert!(bpos > gpos && bpos - gpos < 20);
        let rpos = out.find("rules:").unwrap();
        let rbpos = out.find(RULES_BEGIN).unwrap();
        assert!(rbpos > rpos && rbpos - rpos < 20);
        // исходные правила не тронуты
        assert!(out.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn merge_preserves_other_devices() {
        let with_one = apply_assignments(
            BASE_YAML,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: Some("X".into()) }],
            &[],
            &[],
        )
        .unwrap();
        // добавляем второе устройство — первое должно сохраниться
        let with_two = apply_assignments(
            &with_one,
            &[Assignment { ip: "10.0.0.6".into(), name: "TV".into(), server: Some("Y".into()) }],
            &[],
            &[],
        )
        .unwrap();
        assert!(with_two.contains("Phone 10_0_0_5"));
        assert!(with_two.contains("TV 10_0_0_6"));
        assert_eq!(parse_groups(&with_two).len(), 2);
    }

    #[test]
    fn remove_assignment_deletes_rule() {
        let with_one = apply_assignments(
            BASE_YAML,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: Some("X".into()) }],
            &[],
            &[],
        )
        .unwrap();
        let removed = apply_assignments(
            &with_one,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: None }],
            &[],
            &[],
        )
        .unwrap();
        assert!(parse_groups(&removed).is_empty());
        assert!(!removed.contains(GROUPS_BEGIN));
        assert!(removed.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn default_means_remove() {
        let with_one = apply_assignments(
            BASE_YAML,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: Some("X".into()) }],
            &[],
            &[],
        )
        .unwrap();
        let removed = apply_assignments(
            &with_one,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: Some("default".into()) }],
            &[],
            &[],
        )
        .unwrap();
        assert!(parse_groups(&removed).is_empty());
    }

    #[test]
    fn group_yaml_without_providers_has_no_use() {
        let out = group_yaml("DEV_1_2_3_4", &[]);
        assert!(!out.contains("use:"));
        assert!(out.contains("- Fastest"));
        let out2 = group_yaml("X 1_2_3_4", &["prov1".to_string(), "prov2".to_string()]);
        assert!(out2.contains("use:"));
        assert!(out2.contains("- prov1") && out2.contains("- prov2"));
    }

    #[test]
    fn parse_provider_names_finds_top_level() {
        let yaml = "port: 7890\nproxy-providers:\n  alpha:\n    type: http\n    url: x\n  beta:\n    type: file\nrules:\n  - MATCH,PROXY\n";
        assert_eq!(parse_provider_names(yaml), vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn regression_bug_120_sanitize_domains_rejects_rule_breaking_chars() {
        // Баг 120: домен с `,` или `(` от невалидирующего писателя (бот, импорт, ручная правка config.json)
        // попадал в правило DOMAIN-SUFFIX и ломал всю секцию rules.
        let out = sanitize_domains(&[
            "evil.com,DIRECT".to_string(),
            "a(b).example".to_string(),
            "x.example)".to_string(),
            "ok.example".to_string(),
            "nas.lan".to_string(),
            "_srv.example".to_string(),
        ]);
        assert_eq!(out, vec!["_srv.example".to_string(), "nas.lan".to_string(), "ok.example".to_string()]);
    }

    #[test]
    fn regression_bug_119_device_domain_rules_are_safe_and_targets_exist() {
        // Баг 119: цели доменных правил устройств не сверялись с группами (удалённая группа или имя ноды
        // провайдера → Mihomo отклоняет каждую следующую сборку), `)` в домене или цели ломал AND-правило.
        let yaml = "proxies: []\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,PROXY\n";
        let mut dd = std::collections::BTreeMap::new();
        dd.insert("192.168.1.10".to_string(), vec![
            crate::config::DeviceDomainRule { domain: "ok.example".into(), target: "DeletedGroup".into() },
            crate::config::DeviceDomainRule { domain: "bad.example)".into(), target: "DIRECT".into() },
            crate::config::DeviceDomainRule { domain: "t.example".into(), target: "PROXY),(DST-PORT,443".into() },
        ]);
        let out = apply_domain_rules(yaml, &[], &[], &dd).unwrap();
        let (out, _) = sanitize_rule_targets(&out);
        let rules: Vec<&str> = out.lines().filter(|l| l.contains("SRC-IP-CIDR")).collect();
        assert_eq!(rules, vec!["  - AND,((SRC-IP-CIDR,192.168.1.10/32),(DOMAIN-SUFFIX,ok.example)),PROXY,no-resolve"],
            "несуществующая цель заменена на основную группу, правила со скобками в домене/цели не созданы:\n{out}");
    }

    #[test]
    fn sanitize_domains_normalizes() {
        let out = sanitize_domains(&[" https://WWW.Example.com/path ".to_string(), "example.com".to_string(), "notadomain".to_string()]);
        assert_eq!(out, vec!["example.com".to_string()]);
    }

    #[test]
    fn domain_rules_inserted_and_removed() {
        let empty_map = std::collections::BTreeMap::new();
        let out = apply_domain_rules(BASE_YAML, &["example.com".to_string()], &["forced.org".to_string()], &empty_map).unwrap();
        assert!(out.contains(DIRECT_BEGIN));
        assert!(out.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
        assert!(out.contains("DOMAIN-SUFFIX,forced.org,PROXY"));
        // доменные правила — сразу после rules: (приоритет над остальными)
        let rpos = out.find("rules:").unwrap();
        let dpos = out.find("DOMAIN-SUFFIX,example.com").unwrap();
        assert!(dpos > rpos && dpos - rpos < 80);
        // повторное применение — без дублей
        let out2 = apply_domain_rules(&out, &["example.com".to_string()], &["forced.org".to_string()], &empty_map).unwrap();
        assert_eq!(out2.matches("DOMAIN-SUFFIX,example.com").count(), 1);
        // очистка — блоки удалены
        let cleared = apply_domain_rules(&out2, &[], &[], &empty_map).unwrap();
        assert!(!cleared.contains("DOMAIN-SUFFIX,example.com"));
        assert!(cleared.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn domain_rules_work_with_crlf() {
        let empty_map = std::collections::BTreeMap::new();
        let crlf_yaml = BASE_YAML.replace('\n', "\r\n");
        let out = apply_domain_rules(&crlf_yaml, &["example.com".to_string()], &["forced.org".to_string()], &empty_map).unwrap();
        assert!(out.contains(DIRECT_BEGIN));
        assert!(out.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
        assert!(out.contains("DOMAIN-SUFFIX,forced.org,PROXY"));
        let rpos = out.find("rules:").unwrap();
        let dpos = out.find("DOMAIN-SUFFIX,example.com").unwrap();
        assert!(dpos > rpos && dpos - rpos < 80);
    }

    #[test]
    fn device_domain_rules_and_syntax() {
        let mut dev_map = std::collections::BTreeMap::new();
        dev_map.insert("192.168.2.118".to_string(), vec![
            crate::config::DeviceDomainRule { domain: "youtube.com".into(), target: "Netherlands".into() }
        ]);
        let out = apply_domain_rules(BASE_YAML, &[], &[], &dev_map).unwrap();
        assert!(out.contains(DEV_DOMAINS_BEGIN));
        assert!(out.contains("AND,((SRC-IP-CIDR,192.168.2.118/32),(DOMAIN-SUFFIX,youtube.com)),Netherlands,no-resolve"));
    }

    #[test]
    fn sanitize_name_in_group() {
        assert_eq!(group_name_for("1.2.3.4", "Po'ket,#PC\n"), "PoketPC 1_2_3_4");
        assert_eq!(group_name_for("1.2.3.4", ""), "DEV_1_2_3_4");
    }

    
    const GROUPS_YAML: &str = "proxies:\n  - name: '🇫🇮 Финляндия [⚡ Стабильный ]'\n    type: vless\n  - name: '🇩🇪 Германия'\n    type: vless\nproxy-groups:\n  - name: Fallback\n    type: fallback\n    include-all: true\n    use:\n      - geodema\n\n  - name: Fastest\n    type: url-test\n    include-all: true\n    use:\n      - geodema\n\n  - name: PROXY\n    type: select\n";

    #[test]
    fn ignore_replaces_include_all_with_explicit_list() {
        let out = apply_ignore_to_groups(GROUPS_YAML, &["Финляндия".to_string()]).unwrap();
        assert_eq!(out.matches("    proxies:").count(), 2, "OUT={out}");
        assert!(!out.contains("include-all"));
        assert!(out.contains("- '🇩🇪 Германия'"));
        assert!(!out.contains("- '🇫🇮 Финляндия"));
        assert_eq!(out.matches("exclude-filter: 'Финляндия'").count(), 2);
        let p = out.find("- name: PROXY").unwrap();
        assert!(!out[p..].contains("proxies:"));
    }

    #[test]
    fn ignore_replaces_previous_explicit_list() {
        let with_old = apply_ignore_to_groups(GROUPS_YAML, &["Германия".to_string()]).unwrap();
        let out = apply_ignore_to_groups(&with_old, &["Финляндия".to_string()]).unwrap();
        assert!(out.contains("- '🇩🇪 Германия'"), "OUT={out}");
        assert!(!out.contains("- '🇫🇮 Финляндия"));
    }

    #[test]
    fn ignore_empty_restores_include_all() {
        let with_old = apply_ignore_to_groups(GROUPS_YAML, &["Финляндия".to_string()]).unwrap();
        let out = apply_ignore_to_groups(&with_old, &[]).unwrap();
        assert_eq!(out.matches("include-all: true").count(), 2, "OUT={out}");
        assert!(!out.contains("exclude-filter"), "OUT={out}");
        let g = &out[out.find("proxy-groups:").unwrap()..];
        assert!(!g.contains("    proxies:"), "OUT={out}");
    }

    #[test]
    fn ignore_all_static_drops_proxies_list()
    {
        let out = apply_ignore_to_groups(GROUPS_YAML, &["Финляндия".to_string(), "Германия".to_string()]).unwrap();
        assert!(!out.contains("    proxies:"), "OUT={out}");
        assert!(!out.contains("include-all"));
        assert_eq!(out.matches("exclude-filter").count(), 2);
    }

    #[test]
    fn test_provider_filters_are_completely_stripped_and_never_resurrected()
    {
        let pyaml = "proxy-providers:\n  geodema:\n    type: http\n    exclude-filter: \"(?i)DIRECT|Russia|RU\"\n    interval: 43200\n  geodema2:\n    type: http\nproxy-groups:\n  - name: PROXY\n    type: select\n";
        let mut saved = std::collections::BTreeMap::new();
        saved.insert("geodema".to_string(), "(?i)DIRECT|Russia|RU".to_string());
        let out = apply_ignore_to_providers(pyaml, &["Германия".to_string()], &mut saved);
        assert!(!out.contains("exclude-filter"), "proxy-providers must NEVER contain exclude-filter: OUT={out}");
        assert!(saved.is_empty(), "saved provider_filters map must be cleared to prevent resurrection");
    }

    #[test]
    fn regex_escape_specials() {
        assert_eq!(regex_escape("a.b[c](d)"), "a\\.b\\[c\\]\\(d\\)");
        assert_eq!(regex_escape("plain name"), "plain name");
    }

    #[test]
    fn parse_static_names_skips_groups() {
        let names = parse_static_proxy_names(GROUPS_YAML);
        assert_eq!(names.len(), 2);
        assert!(names[0].starts_with("🇫🇮"));
    }

    #[test]
    fn ip_key_from_group_rejects_non_device_names() {
        assert_eq!(ip_key_from_group("Big PC 192_168_2_118").as_deref(), Some("192.168.2.118"));
        assert_eq!(ip_key_from_group("DEV_aa_bb_cc_dd_ee_ff").as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(ip_key_from_group("Fallback"), None);
        assert_eq!(ip_key_from_group("NoIpHere"), None);
    }

    #[test]
    fn auto_blocks_inserted_with_newline_when_missing() {
        // YAML без перевода строки после "proxy-groups:" и "rules:" —
        // вставка не должна склеивать маркер с ключом секции.
        let yaml = "proxies:\n  - name: 'srv'\n    type: vless\nproxy-groups:\n  - name: PROXY\n    type: select\nrules:\n  - MATCH,PROXY";
        let a = Assignment {
            ip: "192.168.2.50".into(),
            name: "Тест".into(),
            server: Some("srv".into()),
        };
        let out = apply_assignments(yaml, &[a], &[], &[]).unwrap();
        assert!(out.contains("\nproxy-groups:# --- AUTO-DEVICE-GROUPS-BEGIN ---") == false);
        assert!(out.contains("proxy-groups:\n# --- AUTO-DEVICE-GROUPS-BEGIN ---"));
        assert!(out.contains("rules:\n# --- AUTO-DEVICE-RULES-BEGIN ---"));
        // YAML остаётся валидным по структуре: маркеры на отдельных строках
        for line in out.lines() {
            assert!(!line.starts_with("proxy-groups:#") && !line.starts_with("rules:#"));
        }
    }

    #[test]
    fn test_add_provider_to_yaml_custom_health_check() {
        let yaml = "proxy-providers:\n  old_sub:\n    type: http\n    url: \"https://old.sub/sub\"\n";
        let out = add_provider_to_yaml(
            yaml,
            "new_sub",
            "https://my.provider/sub",
            Some("http://cp.cloudflare.com/generate_204"),
            Some(600),
        ).unwrap();
        assert!(out.contains("new_sub:"));
        assert!(out.contains("url: \"http://cp.cloudflare.com/generate_204\""));
        assert!(out.contains("interval: 600"));
    }

    #[test]
    fn test_add_provider_to_yaml_with_hwid() {
        let yaml = "proxy-providers:\n  old_sub:\n    type: http\n    url: \"https://old.sub/sub\"\n";
        let out = add_provider_to_yaml_full(
            yaml,
            "geodema_test",
            "https://my.provider/sub?hwid=test-123",
            None,
            None,
            Some("test-123"),
            None,
        ).unwrap();
        assert!(out.contains("geodema_test:"));
        assert!(out.contains("x-hwid: [\"test-123\"]"));
        assert!(out.contains("User-Agent: [\"ClashMeta/1.19.24; mihomo/1.19.24\"]"));

        let hwids = parse_provider_hwids(&out);
        assert_eq!(hwids.get("geodema_test").map(String::as_str), Some("test-123"));
    }

    #[test]
    fn test_update_provider_hwid_in_yaml() {
        let yaml = r#"proxy-providers:
  geodema:
    type: http
    url: "https://my.provider/sub?token=abc"
    interval: 86400
    path: ./providers/geodema.yaml
    health-check:
      enable: true
"#;

        // 1. Добавление HWID к провайдеру без HWID
        let with_hwid = update_provider_hwid_in_yaml(yaml, "geodema", Some("my-hwid-999")).unwrap();
        assert!(with_hwid.contains("x-hwid: [\"my-hwid-999\"]"));
        let parsed_hwids = parse_provider_hwids(&with_hwid);
        assert_eq!(parsed_hwids.get("geodema").map(String::as_str), Some("my-hwid-999"));

        // 2. Обновление существующего HWID на новый
        let updated_hwid = update_provider_hwid_in_yaml(&with_hwid, "geodema", Some("new-hwid-777")).unwrap();
        assert!(updated_hwid.contains("x-hwid: [\"new-hwid-777\"]"));
        assert!(!updated_hwid.contains("my-hwid-999"));
        let parsed_hwids2 = parse_provider_hwids(&updated_hwid);
        assert_eq!(parsed_hwids2.get("geodema").map(String::as_str), Some("new-hwid-777"));

        // 3. Удаление HWID (передаём None)
        let removed_hwid = update_provider_hwid_in_yaml(&updated_hwid, "geodema", None).unwrap();
        assert!(!removed_hwid.contains("x-hwid"));
        assert!(!removed_hwid.contains("new-hwid-777"));
        let parsed_hwids3 = parse_provider_hwids(&removed_hwid);
        assert_eq!(parsed_hwids3.get("geodema"), None);

        // 4. Ошибка при несуществующем провайдере
        let err = update_provider_hwid_in_yaml(yaml, "nonexistent", Some("hwid-123"));
        assert!(err.is_err());
    }

    #[test]
    fn apply_ignore_to_groups_4_space_indentation() {
        let yaml = "proxy-groups:\n    - name: Fastest\n      type: url-test\n      proxies:\n        - Server1\n        - Server2\n";
        let ignore = vec!["Server1".to_string()];
        let out = apply_ignore_to_groups(yaml, &ignore).unwrap();
        assert!(out.contains("exclude-filter: 'Server1'"));
    }

    #[test]
    fn apply_routing_empty_config_yaml() {
        let cfg = crate::config::AppConfig::default();
        assert!(apply_routing("", &cfg).is_err());
    }

    #[test]
    fn test_apply_adblock_rules_lifecycle() {
        let yaml = "port: 7890\nrules:\n  - DOMAIN-SUFFIX,google.com,DIRECT\n  - MATCH,PROXY\n";
        // 1. Включение
        let with_adblock = apply_adblock_rules(yaml, true).unwrap();
        assert!(with_adblock.contains(ADBLOCK_BEGIN));
        assert!(with_adblock.contains(ADBLOCK_RULE));
        assert!(with_adblock.contains(ADBLOCK_END));
        assert!(with_adblock.contains("DOMAIN-SUFFIX,google.com,DIRECT"));

        // 2. Идемпотентность (повторное включение не плодит блоки)
        let dup = apply_adblock_rules(&with_adblock, true).unwrap();
        assert_eq!(dup.matches(ADBLOCK_BEGIN).count(), 1);

        // 3. Выключение
        let disabled = apply_adblock_rules(&dup, false).unwrap();
        assert!(!disabled.contains(ADBLOCK_BEGIN));
        assert!(!disabled.contains("GEOSITE,category-ads-all,REJECT"));
        assert!(disabled.contains("DOMAIN-SUFFIX,google.com,DIRECT"));
    }

    #[test]
    fn test_apply_zapret_hybrid_rules_lifecycle() {
        let yaml = "port: 7890\nrules:\n  - DOMAIN-SUFFIX,example.com,DIRECT\n  - MATCH,PROXY\n";
        let mut zapret_cfg = crate::config::ZapretConfig {
            enabled: true,
            hybrid_youtube: true,
            hybrid_discord: true,
            discord_voice_udp: true,
            youtube_turbo: false,
            general_bypass: false,
            aggressive_dpi: false,
            isolated_proxy: true,
            bypass_github: true,
            bypass_torrents: true,
            bypass_adult: true,
            custom_entries: vec![crate::config::ZapretCustomEntry {
                domain: "mysku.club".to_string(),
                enabled: true,
                cdns: vec!["img.mysku-st.ru".to_string(), "art.mysku-st.net".to_string()],
            }],
            custom_args: None,
            ..Default::default()
        };

        // 1. Включение всех гибридных правил
        let with_zapret = apply_zapret_hybrid_rules(yaml, &zapret_cfg).unwrap();
        assert!(with_zapret.contains(ZAPRET_HYBRID_BEGIN));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,googlevideo.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,discord.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,raw.githubusercontent.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,rutracker.org,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,pornhub.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,xvideos.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,mysku.club,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,img.mysku-st.ru,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,openai.com,PROXY"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
        // DOMAIN-KEYWORD правила НЕ должны присутствовать (слишком широкие)
        assert!(!with_zapret.contains("DOMAIN-KEYWORD,youtube"));
        assert!(!with_zapret.contains("DOMAIN-KEYWORD,discord"));

        // 2. Идемпотентность
        let dup = apply_zapret_hybrid_rules(&with_zapret, &zapret_cfg).unwrap();
        assert_eq!(dup.matches(ZAPRET_HYBRID_BEGIN).count(), 1);

        // 3. Выключение YouTube (возврат в PROXY) — Discord, GitHub, Torrents, Adult, Custom и isolated_proxy остаются
        zapret_cfg.hybrid_youtube = false;
        let no_yt = apply_zapret_hybrid_rules(&dup, &zapret_cfg).unwrap();
        assert!(!no_yt.contains("googlevideo.com,DIRECT"));
        assert!(no_yt.contains("discord.com,DIRECT"));
        assert!(no_yt.contains("raw.githubusercontent.com,DIRECT"));
        assert!(no_yt.contains("pornhub.com,DIRECT"));
        assert!(no_yt.contains("mysku.club,DIRECT"));
        assert!(no_yt.contains("openai.com,PROXY"));

        // 3.1 Выключение custom entry
        zapret_cfg.custom_entries[0].enabled = false;
        let no_custom = apply_zapret_hybrid_rules(&no_yt, &zapret_cfg).unwrap();
        assert!(!no_custom.contains("mysku.club,DIRECT"));
        assert!(!no_custom.contains("img.mysku-st.ru,DIRECT"));
        assert!(no_custom.contains("raw.githubusercontent.com,DIRECT"));
        assert!(no_custom.contains("pornhub.com,DIRECT"));

        // 4. Полное отключение службы Zapret
        zapret_cfg.enabled = false;
        let disabled = apply_zapret_hybrid_rules(&no_custom, &zapret_cfg).unwrap();
        assert!(!disabled.contains(ZAPRET_HYBRID_BEGIN));
        assert!(!disabled.contains("discord.com,DIRECT"));
        assert!(!disabled.contains("raw.githubusercontent.com"));
        assert!(!disabled.contains("pornhub.com"));
        assert!(!disabled.contains("openai.com"));
        assert!(disabled.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
    }

    #[test]
    fn test_zapret_hybrid_precedence_over_force_domains() {
        let yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\nrules:\n  - MATCH,PROXY\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.force_domains = vec![
            "github.com".to_string(),
            "data.shirogames.com".to_string(),
            "pornhub.com".to_string(),
        ];
        app_cfg.zapret.enabled = true;
        app_cfg.zapret.bypass_github = true;
        app_cfg.zapret.bypass_adult = true;

        let (applied, _) = apply_routing(yaml, &app_cfg).expect("apply_routing should succeed");

        // 1. All user-forced domains must be strictly preserved in FORCE_BEGIN block
        let force_pos = applied.find(FORCE_BEGIN).expect("FORCE_BEGIN must be present");
        let force_end_pos = applied.find(FORCE_END).expect("FORCE_END must be present");
        let force_block = &applied[force_pos..force_end_pos];

        assert!(force_block.contains("DOMAIN-SUFFIX,github.com,PROXY"), "github.com must remain in FORCE block");
        assert!(force_block.contains("DOMAIN-SUFFIX,data.shirogames.com,PROXY"), "data.shirogames.com must remain in FORCE block");
        assert!(force_block.contains("DOMAIN-SUFFIX,pornhub.com,PROXY"), "pornhub.com must remain in FORCE block");

        // 2. Zapret DIRECT rules must NOT duplicate or override domains that user explicitly forced
        assert!(!applied.contains("DOMAIN-SUFFIX,github.com,DIRECT"), "github.com must not be in Zapret DIRECT when user forced it to PROXY");
        assert!(!applied.contains("DOMAIN-SUFFIX,pornhub.com,DIRECT"), "pornhub.com must not be in Zapret DIRECT when user forced it to PROXY");

        // 3. Other Zapret bypasses not forced by the user must still be active
        assert!(applied.contains(ZAPRET_HYBRID_BEGIN));
        assert!(applied.contains("DOMAIN-SUFFIX,xvideos.com,DIRECT"), "Unforced adult domains must remain in Zapret DIRECT");
    }

    #[test]
    fn test_user_force_domains_preserved_with_zapret_enabled() {
        let yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\nrules:\n  - MATCH,PROXY\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.force_domains = vec!["youtube.com".to_string(), "discord.com".to_string()];
        app_cfg.zapret.enabled = true;
        app_cfg.zapret.hybrid_youtube = true;
        app_cfg.zapret.hybrid_discord = true;

        let (applied, _) = apply_routing(yaml, &app_cfg).expect("apply_routing should succeed");

        // User forced youtube.com and discord.com -> they must be PROXY in FORCE block
        assert!(applied.contains("DOMAIN-SUFFIX,youtube.com,PROXY"));
        assert!(applied.contains("DOMAIN-SUFFIX,discord.com,PROXY"));

        // They must NOT be in Zapret DIRECT
        assert!(!applied.contains("DOMAIN-SUFFIX,youtube.com,DIRECT"));
        assert!(!applied.contains("DOMAIN-SUFFIX,discord.com,DIRECT"));

        // But other subdomains/related domains not in force_domains must still be accelerated via Zapret DIRECT
        assert!(applied.contains("DOMAIN-SUFFIX,googlevideo.com,DIRECT"));
        assert!(applied.contains("DOMAIN-SUFFIX,discord.gg,DIRECT"));
    }

    #[test]
    fn test_is_zapret_direct_domain_exact_matching() {
        let mut zapret_cfg = crate::config::ZapretConfig::default();
        zapret_cfg.enabled = true;
        zapret_cfg.hybrid_youtube = true;
        zapret_cfg.hybrid_discord = true;
        zapret_cfg.bypass_github = true;
        zapret_cfg.bypass_torrents = true;
        zapret_cfg.bypass_adult = true;

        // Exact matches and subdomains
        assert!(is_zapret_direct_domain("github.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("api.github.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("raw.githubusercontent.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("pornhub.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("rt.pornhub.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("rutracker.org", &zapret_cfg));
        assert!(is_zapret_direct_domain("googlevideo.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("rr1---sn-4g5ednss.googlevideo.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("discord.gg", &zapret_cfg));

        // Unrelated domains containing substring should NOT match
        assert!(!is_zapret_direct_domain("notyoutube.com", &zapret_cfg));
        assert!(!is_zapret_direct_domain("fake-discord.org", &zapret_cfg));
        assert!(!is_zapret_direct_domain("fakegithub.com", &zapret_cfg));

        // Protected Flow domains must never be treated as Zapret direct
        assert!(!is_zapret_direct_domain("googleapis.com", &zapret_cfg));
        assert!(!is_zapret_direct_domain("gemini.google.com", &zapret_cfg));
    }

    #[test]
    fn test_remove_adblock_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - GEOSITE,category-ads-all,REJECT\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n",
            ADBLOCK_BEGIN
        );
        let result = remove_adblock_rules(&yaml_with_missing_end);
        assert!(!result.contains(ADBLOCK_BEGIN));
        assert!(result.contains("GEOSITE,category-ads-all,REJECT"));
        assert!(result.contains("GEOIP,RU,DIRECT"));
        assert!(result.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_remove_flow_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - DOMAIN-SUFFIX,google.com,FLOW\n  - GEOIP,RU,DIRECT\n",
            GOOGLE_AI_BEGIN
        );
        let result = remove_flow_rules(&yaml_with_missing_end);
        assert!(!result.contains(GOOGLE_AI_BEGIN));
        assert!(result.contains("DOMAIN-SUFFIX,google.com,FLOW"));
        assert!(result.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn test_remove_zapret_hybrid_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - DOMAIN-SUFFIX,youtube.com,DIRECT\n  - MATCH,PROXY\n",
            ZAPRET_HYBRID_BEGIN
        );
        let result = remove_zapret_hybrid_rules(&yaml_with_missing_end);
        assert!(!result.contains(ZAPRET_HYBRID_BEGIN));
        assert!(result.contains("DOMAIN-SUFFIX,youtube.com,DIRECT"));
        assert!(result.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_remove_domain_blocks_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - DOMAIN,blocked.com,REJECT\n  - MATCH,PROXY\n",
            DIRECT_BEGIN
        );
        let result = remove_domain_blocks(&yaml_with_missing_end);
        assert!(!result.contains(DIRECT_BEGIN));
        assert!(result.contains("DOMAIN,blocked.com,REJECT"));
        assert!(result.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_delete_provider_from_yaml_full_cleanup() {
        let yaml = r#"
proxy-groups:
  - name: PROXY
    type: select
    use:
      - geodema
      - subscription_1
      - geodema2
    proxies: [DIRECT, Fallback]
  - name: Discord
    type: select
    use:
      - subscription_1
    proxies: [PROXY]
  - name: Steam
    type: select
    use: [geodema, subscription_1, other]

proxy-providers:
  geodema:
    type: http
    url: "https://example.com/geo"
  subscription_1:
    type: http
    url: "https://example.com/sub1"
  geodema2:
    type: http
    url: "https://example.com/geo2"
"#;

        let cleaned = delete_provider_from_yaml(yaml, "subscription_1").unwrap();
        // Проверяем удаление из proxy-providers
        assert!(!cleaned.contains("subscription_1:"));
        assert!(cleaned.contains("geodema:"));
        assert!(cleaned.contains("geodema2:"));

        // Проверяем удаление из use: списков proxy-groups
        assert!(!cleaned.contains("- subscription_1"));
        assert!(cleaned.contains("- geodema"));
        assert!(cleaned.contains("- geodema2"));

        // Проверяем inline use: [geodema, subscription_1, other] -> [geodema, other]
        assert!(cleaned.contains("use: [geodema, other]"));
    }

    #[test]
    fn test_apply_and_remove_gaming_rules() {
        let yaml = r#"port: 7890
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - GEOIP,RU,DIRECT
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.target_server = "LowPingNode".into();
        cfg.platforms.discord = true;
        cfg.platforms.steam = true;
        cfg.platforms.xbox = true;
        cfg.platforms.supercell = true;
        cfg.custom_domains = vec!["customgame.com".into()];

        let providers = vec!["sub1".to_string()];
        let applied = apply_gaming_rules(yaml, &cfg, &providers).expect("Must apply gaming rules");

        assert!(applied.contains(GAMING_GROUP_BEGIN));
        assert!(applied.contains(GAMING_GROUP_END));
        assert!(applied.contains("- name: '🎮 Gaming'"));
        assert!(applied.contains("- LowPingNode"));
        assert!(applied.contains("- sub1"));

        assert!(applied.contains(GAMING_BEGIN));
        assert!(applied.contains(GAMING_END));
        assert!(applied.contains("DOMAIN-SUFFIX,discord.com,🎮 Gaming"));
        assert!(applied.contains("DOMAIN-SUFFIX,steamcommunity.com,🎮 Gaming"));
        assert!(applied.contains("DOMAIN-SUFFIX,user.auth.xboxlive.com,🎮 Gaming"));
        assert!(applied.contains("DOMAIN-SUFFIX,brawlstars.com,🎮 Gaming"));
        assert!(applied.contains("DOMAIN-SUFFIX,customgame.com,🎮 Gaming"));

        // Удаление при выключении
        cfg.enabled = false;
        let disabled = apply_gaming_rules(&applied, &cfg, &providers).expect("Must disable gaming rules");
        assert!(!disabled.contains(GAMING_GROUP_BEGIN));
        assert!(!disabled.contains(GAMING_BEGIN));
        assert!(!disabled.contains("🎮 Gaming"));
        assert!(disabled.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_apply_gaming_compatibility_mode_device() {
        let yaml = r#"port: 7890
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - GEOIP,RU,DIRECT
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.target_server = "GameVPS".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "00:11:22:33:44:55".into(),
            ip: "192.168.2.115".into(),
            ipv6: vec!["2001:db8::10".into()],
            name: "Gaming-Rig".into(),
            enabled: true,
            server: None,
        });

        let providers = vec![];
        let applied = apply_gaming_rules(yaml, &cfg, &providers).expect("Must apply compatibility mode");

        assert!(applied.contains(GAMING_BEGIN));
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.115/32,🎮 Gaming"));
        assert!(applied.contains("SRC-IP-CIDR,2001:db8::10/128,🎮 Gaming"));
        // Не должно содержать category-games в режиме полной совместимости устройства
        assert!(!applied.contains("GEOSITE,category-games"));
    }

    #[test]
    fn test_apply_gaming_known_services_mode() {
        let yaml = r#"port: 7890
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - GEOIP,RU,DIRECT
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::KnownServices;
        cfg.target_server = "Fastest".into();
        cfg.platforms.category_games = true;
        cfg.platforms.steam = true;

        let providers = vec![];
        let applied = apply_gaming_rules(yaml, &cfg, &providers).expect("Must apply known services mode");

        assert!(applied.contains(GAMING_BEGIN));
        assert!(applied.contains("GEOSITE,category-games,🎮 Gaming"));
        assert!(applied.contains("DOMAIN-SUFFIX,steamcommunity.com,🎮 Gaming"));
        assert!(!applied.contains("SRC-IP-CIDR"));
    }

    #[test]
    fn test_apply_gaming_filters_link_local_ipv6() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:22:33:44:55:66".into(),
            ip: "192.168.2.50".into(),
            ipv6: vec![
                "fe80::1ff:fe00:1".into(),
                "::1".into(),
                "2a02:1234:5678::1".into(),
            ],
            name: "Rig".into(),
            enabled: true,
            server: None,
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.50/32,🎮 Gaming"));
        assert!(applied.contains("SRC-IP-CIDR,2a02:1234:5678::1/128,🎮 Gaming"));
        assert!(!applied.contains("fe80::1ff:fe00:1"), "fe80: must be excluded");
        assert!(!applied.contains("SRC-IP-CIDR,::1/128"), "loopback must be excluded");
    }

    #[test]
    fn test_apply_routing_gaming_priority_over_device_assignment() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.device_routing.insert(
            "192.168.2.115".into(),
            crate::config::DeviceRouting {
                servers: vec!["OldProxy".into()],
                ..Default::default()
            },
        );
        app_cfg.gaming.enabled = true;
        app_cfg.gaming.mode = crate::config::GamingMode::Compatibility;
        app_cfg.gaming.target_server = "GamingNode".into();
        app_cfg.gaming.devices.push(crate::config::GamingDevice {
            mac: "aa:bb:cc:dd:ee:ff".into(),
            ip: "192.168.2.115".into(),
            ipv6: vec![],
            name: "Gaming-Console".into(),
            enabled: true,
            server: None,
        });

        let (applied, _) = apply_routing(yaml, &app_cfg).expect("applied");
        let gaming_pos = applied.find(GAMING_BEGIN).expect("GAMING_BEGIN exists");
        let device_pos = applied.find(RULES_BEGIN).expect("RULES_BEGIN exists");
        assert!(
            gaming_pos < device_pos,
            "Gaming priority route must appear before general device assignment in rules"
        );
    }

    #[test]
    fn regression_bug_14_gaming_rules_order_and_compatibility_scope() {
        // Баг 14:
        // 1. Игровые правила не должны стоять выше AdBlock, DIRECT и персональных доменных правил (DEV_DOMAINS).
        // 2. В режиме Compatibility custom_domains не должны применяться ко всем устройствам сети без привязки к игровым IP.
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,DIRECT\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.adblock_enabled = true;
        cfg.direct_domains = vec!["direct-test.com".into()];
        cfg.device_domain_rules.insert(
            "192.168.2.100".into(),
            vec![crate::config::DeviceDomainRule {
                domain: "console-override.com".into(),
                target: "DIRECT".into(),
            }],
        );
        cfg.gaming.enabled = true;
        cfg.gaming.mode = crate::config::GamingMode::Compatibility;
        cfg.gaming.custom_domains = vec!["custom-game.com".into()];
        cfg.gaming.devices = vec![crate::config::GamingDevice {
            mac: "AA:BB:CC:DD:EE:FF".into(),
            ip: "192.168.2.100".into(),
            ipv6: vec![],
            name: "PlayStation".into(),
            enabled: true,
            server: None,
        }];

        let (applied, _) = apply_routing(yaml, &cfg).expect("apply_routing should succeed");

        let pos_adblock = applied.find(ADBLOCK_BEGIN).expect("ADBLOCK_BEGIN present");
        let pos_dev_domains = applied.find(DEV_DOMAINS_BEGIN).expect("DEV_DOMAINS_BEGIN present");
        let pos_direct = applied.find(DIRECT_BEGIN).expect("DIRECT_BEGIN present");
        let pos_gaming = applied.find(GAMING_BEGIN).expect("GAMING_BEGIN present");

        assert!(pos_adblock < pos_gaming, "AdBlock ({pos_adblock}) must be above Gaming ({pos_gaming})");
        assert!(pos_dev_domains < pos_gaming, "DEV_DOMAINS ({pos_dev_domains}) must be above Gaming ({pos_gaming})");
        assert!(pos_direct < pos_gaming, "DIRECT ({pos_direct}) must be above Gaming ({pos_gaming})");

        // custom_domains in compatibility mode must not be global rule without SRC-IP-CIDR
        assert!(
            !applied.contains("  - DOMAIN-SUFFIX,custom-game.com,🎮 Gaming"),
            "custom_domains in compatibility mode must not apply globally to all devices in the network"
        );
        assert!(
            applied.contains("192.168.2.100"),
            "gaming device IP must be present in rules"
        );
    }

    #[test]
    fn regression_bug_15_discord_zapret_hybrid_vs_gaming_priority() {
        // Баг 15: Discord нужен двум функциям сразу. Игровой режим вел его в 🎮 Gaming и добавлял
        // в geo_override, а Zapret hybrid_discord — в DIRECT. Молча побеждал игровой режим.
        // Когда включен Zapret hybrid_discord (обход DPI на роутере напрямую), Discord должен идти в DIRECT,
        // а не перехватываться игровым прокси-туннелем и не попадать в geo_override.
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.zapret.enabled = true;
        cfg.zapret.hybrid_discord = true;

        cfg.gaming.enabled = true;
        cfg.gaming.mode = crate::config::GamingMode::KnownServices;
        cfg.gaming.platforms.discord = true;

        let (applied, _) = apply_routing(yaml, &cfg).expect("routing should succeed");

        // Discord must be routed to DIRECT by Zapret hybrid rules
        assert!(
            applied.contains("  - DOMAIN-SUFFIX,discord.com,DIRECT"),
            "Discord must have DIRECT rule from Zapret hybrid"
        );
        // Discord must NOT be routed to 🎮 Gaming when Zapret hybrid_discord is active
        assert!(
            !applied.contains("  - DOMAIN-SUFFIX,discord.com,🎮 Gaming"),
            "Discord must not be captured by 🎮 Gaming proxy when Zapret hybrid_discord is handling it"
        );

        // Также в geo_override discord не должен затягиваться, если zapret hybrid_discord активен
        let override_domains = crate::override_sync::collect_override_domains(&cfg, &[]);
        assert!(
            !override_domains.iter().any(|d| d.contains("discord")),
            "Discord domains must not be put into geo_override when Zapret hybrid_discord is handling it"
        );
    }

    #[test]
    fn regression_bug_17_single_disabled_gaming_device_must_stay_inactive() {
        // Баг 17: Единственное выключенное игровое устройство считалось активным из-за
        // запасного правила `len == 1`.
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:22:33:44:55:66".into(),
            ip: "192.168.2.50".into(),
            ipv6: vec![],
            name: "PC".into(),
            enabled: false, // явно выключено!
            server: None,
        });

        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");

        // Выключенное устройство НЕ должно генерировать правило маршрутизации в Gaming
        assert!(
            !applied.contains("192.168.2.50"),
            "Disabled gaming device must not be routed to Gaming even if it is the only device"
        );
    }

    #[test]
    fn test_apply_gaming_target_server_order_in_proxies() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:22:33:44:55:66".into(),
            ip: "192.168.2.50".into(),
            ipv6: vec![],
            name: "PC".into(),
            enabled: true,
            server: None,
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        // Fastest must be the first proxy listed under proxies: in the 🎮 Gaming group
        let group_block = extract_block(&applied, GAMING_GROUP_BEGIN, GAMING_GROUP_END).expect("gaming group block");
        let proxies_pos = group_block.find("proxies:").expect("proxies section");
        let after_proxies = &group_block[proxies_pos..];
        let first_proxy = after_proxies.lines().nth(1).expect("first proxy line").trim();
        assert_eq!(first_proxy, "- Fastest", "Fastest must be the first proxy in the select group, not DIRECT");
    }

    #[test]
    fn test_apply_gaming_excludes_disabled_devices_when_multiple() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:11:11:11:11:11".into(),
            ip: "192.168.2.10".into(),
            ipv6: vec![],
            name: "Active-Console".into(),
            enabled: true,
            server: None,
        });
        cfg.devices.push(crate::config::GamingDevice {
            mac: "22:22:22:22:22:22".into(),
            ip: "192.168.2.20".into(),
            ipv6: vec![],
            name: "Inactive-PC".into(),
            enabled: false,
            server: None,
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.10/32,🎮 Gaming"));
        assert!(!applied.contains("SRC-IP-CIDR,192.168.2.20/32,🎮 Gaming"), "Disabled device must not be routed");
    }

    #[test]
    fn test_format_src_ip_cidr_no_duplicate_suffix() {
        assert_eq!(format_src_ip_cidr("192.168.1.1"), "192.168.1.1/32");
        assert_eq!(format_src_ip_cidr("192.168.1.1/32"), "192.168.1.1/32");
        assert_eq!(format_src_ip_cidr("10.0.0.0/24"), "10.0.0.0/24");
        assert_eq!(format_src_ip_cidr("2001:db8::1"), "2001:db8::1/128");
        assert_eq!(format_src_ip_cidr("2001:db8::1/128"), "2001:db8::1/128");
        assert_eq!(rule_line("192.168.1.1/32", "DEV_GROUP"), "  - SRC-IP-CIDR,192.168.1.1/32,DEV_GROUP");
    }

    #[test]
    fn test_apply_domain_rules_deduplicates_direct_and_force() {
        let yaml = "rules:\n  - MATCH,PROXY\n";
        let direct = vec!["conflict.com".to_string(), "only-direct.com".to_string()];
        let force = vec!["conflict.com".to_string(), "only-force.com".to_string()];
        let empty_dev = std::collections::BTreeMap::new();
        let res = apply_domain_rules(yaml, &direct, &force, &empty_dev).expect("success");
        assert!(res.contains("DOMAIN-SUFFIX,conflict.com,DIRECT"));
        assert!(!res.contains("DOMAIN-SUFFIX,conflict.com,PROXY"), "Conflicting domain must not be added to PROXY when present in DIRECT");
        assert!(res.contains("DOMAIN-SUFFIX,only-force.com,PROXY"));
    }

    #[test]
    fn test_apply_gaming_multiple_devices_with_custom_servers() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:11:11:11:11:11".into(),
            ip: "192.168.2.10".into(),
            ipv6: vec![],
            name: "PC-Gamer".into(),
            enabled: true,
            server: Some("Finland-Node".into()),
        });
        cfg.devices.push(crate::config::GamingDevice {
            mac: "22:22:22:22:22:22".into(),
            ip: "192.168.2.15".into(),
            ipv6: vec![],
            name: "PlayStation-5".into(),
            enabled: true,
            server: None, // Uses default gaming group
        });
        cfg.devices.push(crate::config::GamingDevice {
            mac: "33:33:33:33:33:33".into(),
            ip: "192.168.2.25".into(),
            ipv6: vec![],
            name: "Nintendo-Switch".into(),
            enabled: true,
            server: Some("Poland-Node".into()),
        });

        let applied = apply_gaming_rules(yaml, &cfg, &["sub1".to_string()]).expect("applied");
        let pc_group = gaming_device_group_name("192.168.2.10", "PC-Gamer");
        let switch_group = gaming_device_group_name("192.168.2.25", "Nintendo-Switch");
        assert!(applied.contains(&format!("SRC-IP-CIDR,192.168.2.10/32,{pc_group}")), "PC must route to its own gaming group");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.15/32,🎮 Gaming"), "PS5 must route to default Gaming group");
        assert!(applied.contains(&format!("SRC-IP-CIDR,192.168.2.25/32,{switch_group}")), "Switch must route to its own gaming group");
    }

    #[test]
    fn test_apply_gaming_bypass_ru_preset() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::BypassRu;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:11:11:11:11:11".into(),
            ip: "192.168.2.10".into(),
            ipv6: vec![],
            name: "PC-Gamer".into(),
            enabled: true,
            server: Some("Finland-Node".into()),
        });

        let applied = apply_gaming_rules(yaml, &cfg, &["sub1".to_string()]).expect("applied");
        let pc_group = gaming_device_group_name("192.168.2.10", "PC-Gamer");
        assert!(applied.contains("AND,((SRC-IP-CIDR,192.168.2.10/32),(GEOIP,RU)),DIRECT"), "RU GeoIP must be direct in BypassRu");
        assert!(applied.contains("AND,((SRC-IP-CIDR,192.168.2.10/32),(GEOSITE,category-ru)),DIRECT"), "RU Geosite must be category-ru");
        assert!(!applied.contains("(GEOSITE,ru))"), "Must never generate invalid (GEOSITE,ru)");
        assert!(applied.contains(&format!("SRC-IP-CIDR,192.168.2.10/32,{pc_group}")), "Remaining traffic must route to PC per-device group");
    }

    #[test]
    fn test_apply_gaming_bypass_ru_with_ruleset() {
        let yaml = "port: 7890\nrule-providers:\n  category-ru@domain: {}\n  category-ru@ipcidr: {}\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::BypassRu;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:11:11:11:11:11".into(),
            ip: "192.168.2.10".into(),
            ipv6: vec![],
            name: "PC-Gamer".into(),
            enabled: true,
            server: Some("Finland-Node".into()),
        });

        let applied = apply_gaming_rules(yaml, &cfg, &["sub1".to_string()]).expect("applied");
        let pc_group = gaming_device_group_name("192.168.2.10", "PC-Gamer");
        assert!(applied.contains("AND,((SRC-IP-CIDR,192.168.2.10/32),(GEOIP,RU)),DIRECT"));
        assert!(applied.contains("AND,((SRC-IP-CIDR,192.168.2.10/32),(RULE-SET,category-ru@domain)),DIRECT"));
        assert!(applied.contains("AND,((SRC-IP-CIDR,192.168.2.10/32),(RULE-SET,category-ru@ipcidr)),DIRECT"));
        assert!(!applied.contains("GEOSITE,ru"), "Must not use GEOSITE when RULE-SET is available");
        assert!(applied.contains(&format!("SRC-IP-CIDR,192.168.2.10/32,{pc_group}")));
    }

    #[test]
    fn test_parse_redir_port() {
        let yaml1 = "redir-port: 5000\nport: 7890\n";
        assert_eq!(parse_redir_port(yaml1), Some(5000));

        let yaml2 = "redir-port: '5200'\n";
        assert_eq!(parse_redir_port(yaml2), Some(5200));

        let yaml3 = "tproxy-port: 5000\n";
        assert_eq!(parse_redir_port(yaml3), None);
    }

    #[test]
    fn test_parse_tproxy_port() {
        let yaml1 = "tproxy-port: 5001\nport: 7890\n";
        assert_eq!(parse_tproxy_port(yaml1), Some(5001));

        let yaml2 = "tproxy-port: '5005'\n";
        assert_eq!(parse_tproxy_port(yaml2), Some(5005));

        let yaml3 = "redir-port: 5000\n";
        assert_eq!(parse_tproxy_port(yaml3), None);
    }

    #[test]
    fn regression_bug_46b_redir_ports_single_source_with_warning() {
        let missing = redir_ports("port: 7890\n");
        assert_eq!(missing.redir, 5000);
        assert_eq!(missing.tproxy, 5001);
        assert_eq!(missing.source, PortSource::Default);

        let configured = redir_ports("redir-port: 5555\ntproxy-port: 5556\n");
        assert_eq!(configured.redir, 5555);
        assert_eq!(configured.tproxy, 5556);
        assert_eq!(configured.source, PortSource::FromConfig);
    }

    #[test]
    fn test_provider_and_flow_char_validation() {
        // SEC-12: Reject injection chars in add_provider_to_yaml_full and apply_flow_rules
        let yaml = "proxy-providers:\n  sub1:\n    type: http\n    url: \"https://foo.bar/sub\"\nrules:\n  - MATCH,DIRECT";
        assert!(add_provider_to_yaml_full(yaml, "bad\nid", "https://foo.bar/sub", None, None, None, None).is_err());
        assert!(add_provider_to_yaml_full(yaml, "sub2", "https://foo.bar/sub\"evil", None, None, None, None).is_err());
        assert!(add_provider_to_yaml_full(yaml, "sub2", "https://foo.bar/sub\nevil", None, None, None, None).is_err());
        assert!(add_provider_to_yaml_full(yaml, "sub2", "https://foo.bar/sub", None, None, Some("hwid\"inject"), None).is_err());
        assert!(add_provider_to_yaml_full(yaml, "sub2", "https://foo.bar/sub", None, None, None, Some("ua\ninject")).is_err());

        assert!(apply_flow_rules(yaml, "evil.com\"injected", Some("PROXY")).is_err());
        assert!(apply_flow_rules(yaml, "evil.com\ninjected", Some("PROXY")).is_err());
        assert!(apply_flow_rules(yaml, "evil.com", Some("PROXY\"injected")).is_err());
    }

    #[test]
    fn test_apply_domain_rules_dynamic_group() {
        // BUG-17: Dynamic group resolution in apply_domain_rules
        let yaml = "proxy-groups:\n  - name: MY_CUSTOM_PROXY\n    type: select\nrules:\n  - MATCH,DIRECT";
        let empty_device_domains = std::collections::BTreeMap::new();
        let out = apply_domain_rules(yaml, &[], &["customforce.com".to_string()], &empty_device_domains).unwrap();
        assert!(out.contains("DOMAIN-SUFFIX,customforce.com,MY_CUSTOM_PROXY"));
        assert!(!out.contains("DOMAIN-SUFFIX,customforce.com,PROXY"));
    }

    #[test]
    fn test_bug18_rule_hierarchy_adblock_and_direct_precede_device_assignment() {
        // BUG-18: Rules hierarchy must be FIFO: AdBlock, Direct, Zapret hybrid must precede device rules
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.adblock_enabled = true;
        app_cfg.direct_domains = vec!["mysite.direct.ru".to_string()];
        app_cfg.zapret.enabled = true;
        app_cfg.zapret.hybrid_youtube = true;
        app_cfg.device_routing.insert(
            "192.168.1.55".into(),
            crate::config::DeviceRouting {
                servers: vec!["AssignedNode".into()],
                ..Default::default()
            },
        );

        let (applied, _) = apply_routing(yaml, &app_cfg).expect("apply_routing should succeed");
        let adblock_pos = applied.find(ADBLOCK_BEGIN).expect("ADBLOCK_BEGIN must exist");
        let direct_pos = applied.find(DIRECT_BEGIN).expect("DIRECT_BEGIN must exist");
        let zapret_pos = applied.find(ZAPRET_HYBRID_BEGIN).expect("ZAPRET_HYBRID_BEGIN must exist");
        let device_pos = applied.find(RULES_BEGIN).expect("RULES_BEGIN must exist");

        assert!(adblock_pos < device_pos, "AdBlock must be placed before device assignments so ads are blocked");
        assert!(direct_pos < device_pos, "Direct domains must be placed before device assignments so direct traffic is not proxied");
        assert!(zapret_pos < device_pos, "Zapret hybrid rules must be placed before device assignments");
    }

    #[test]
    fn test_bug20_parse_and_preserve_nonstandard_cidr_masks() {
        // BUG-20: Correct parsing, group mapping and formatting for non-/32 and non-/128 CIDRs
        let yaml = "rules:\n# --- AUTO-DEVICE-RULES-BEGIN ---\n  - SRC-IP-CIDR,192.168.10.0/24,OfficeSubnet\n  - SRC-IP-CIDR,10.0.0.0/8,VpnSubnet\n  - SRC-IP-CIDR,2001:db8::/64,Ipv6Subnet\n# --- AUTO-DEVICE-RULES-END ---\n";
        let parsed = parse_rules(yaml);
        assert!(parsed.contains_key("192.168.10.0/24"), "Must parse /24 mask");
        assert!(parsed.contains_key("10.0.0.0/8"), "Must parse /8 mask");
        assert!(parsed.contains_key("2001:db8::/64"), "Must parse /64 mask");

        // Group name encoding & decoding roundtrip
        let gname = group_name_for("192.168.10.0/24", "Office Network");
        assert_eq!(gname, "Office Network 192_168_10_0_24");
        let decoded = ip_key_from_group(&gname);
        assert_eq!(decoded, Some("192.168.10.0/24".to_string()));

        // Formatting
        assert_eq!(format_src_ip_cidr("192.168.10.0/24"), "192.168.10.0/24");
        assert_eq!(format_src_ip_cidr("10.0.0.0/8"), "10.0.0.0/8");
        assert_eq!(format_src_ip_cidr("2001:db8::/64"), "2001:db8::/64");
    }

    #[test]
    fn test_bug07_provider_yaml_formatting_and_crlf_lf_preservation() {
        // BUG-07: Consistent 2-space indentation and CRLF/LF preservation
        let lf_yaml = "proxy-providers:\n  sub1:\n    type: http\n    url: \"https://a.b/c\"\nrules:\n  - MATCH,DIRECT\n";
        let out_lf = add_provider_to_yaml(lf_yaml, "sub2", "https://x.y/z", None, None).expect("added");
        assert!(!out_lf.contains("\r\n"), "LF input must preserve LF");
        assert!(out_lf.contains("  sub2:\n    type: http\n    url: \"https://x.y/z\""), "Must use 2-space indentation hierarchy");

        let crlf_yaml = "proxy-providers:\r\n  sub1:\r\n    type: http\r\n    url: \"https://a.b/c\"\r\nrules:\r\n  - MATCH,DIRECT\r\n";
        let out_crlf = add_provider_to_yaml(crlf_yaml, "sub2", "https://x.y/z", None, None).expect("added");
        assert!(out_crlf.contains("\r\n"), "CRLF input must preserve CRLF");
        assert!(out_crlf.contains("  sub2:\r\n    type: http\r\n    url: \"https://x.y/z\""));

        let deleted = delete_provider_from_yaml(&out_crlf, "sub2").expect("deleted");
        assert!(deleted.contains("\r\n"), "CRLF deletion must preserve CRLF");
        assert!(!deleted.contains("sub2:"));
    }

    #[test]
    fn test_anti_regression_exclude_filter_completely_removed_from_providers_in_apply_routing() {
        // Anti-regression: Exclude-filter in proxy-providers must be stripped to preserve all 13 servers
        let yaml = "proxy-providers:\n  myprov:\n    type: http\n    url: \"https://a.b/c\"\n    exclude-filter: '(?i)DIRECT|Russia|RU'\nproxy-groups:\n  - name: Fastest\n    type: url-test\n    use:\n      - myprov\nrules:\n  - MATCH,DIRECT\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.ignore_servers = vec!["BadNode".to_string()];
        let mut filters = std::collections::BTreeMap::new();
        filters.insert("myprov".to_string(), "(?i)DIRECT|Russia|RU".to_string());
        let (out, _) = apply_routing_with_filters(yaml, &app_cfg, &mut filters).expect("applied");
        assert!(!out.contains("exclude-filter: '(?i)DIRECT|Russia|RU'"), "Provider exclude-filter must be completely stripped");
        assert!(filters.is_empty(), "Filters map must be cleared to prevent resurrection");
    }

    #[test]
    fn test_parse_provider_urls_does_not_get_overwritten_by_health_check() {
        let yaml = r#"
proxy-providers:
  geodema2:
    type: http
    url: "https://my.subscription.org/sub/token123"
    interval: 43200
    health-check:
      enable: true
      url: "https://www.gstatic.com/generate_204"
      interval: 300
"#;
        let map = parse_provider_urls(yaml);
        assert_eq!(map.get("geodema2"), Some(&"https://my.subscription.org/sub/token123".to_string()));
        assert_ne!(map.get("geodema2"), Some(&"https://www.gstatic.com/generate_204".to_string()));
    }

    #[test]
    fn test_convert_http_provider_to_file_in_yaml() {
        let yaml = r#"
proxy-providers:
  geodema2:
    type: http
    url: "https://my.subscription.org/sub/token123"
    interval: 43200
    exclude-filter: "(?i)DIRECT|Russia|RU"
    health-check:
      enable: true
      url: "https://www.gstatic.com/generate_204"
      interval: 300
"#;
        let converted = convert_http_provider_to_file_in_yaml(yaml, "geodema2", "./providers/geodema2.yaml").expect("converted");
        assert!(converted.contains("    type: file"), "Must be type: file");
        assert!(converted.contains("    path: \"./providers/geodema2.yaml\""), "Must contain path");
        assert!(!converted.contains("    url: \"https://my.subscription.org"), "Provider-level url must be removed");
        assert!(!converted.contains("    interval: 43200"), "Provider-level interval must be removed");
        assert!(!converted.contains("exclude-filter"), "exclude-filter must be stripped");
        assert!(converted.contains("health-check:"), "health-check must be preserved");
        assert!(converted.contains("url: \"https://www.gstatic.com/generate_204\""), "health-check url must be preserved");
    }

    #[test]
    fn test_apply_adguard_dns_upstream_lifecycle() {
        let sample_yaml = r#"
dns:
  enable: true
  listen: 0.0.0.0:1053
  nameserver:
    - 77.88.8.8
    - 1.1.1.1
  direct-nameserver:
    - 77.88.8.8
rules:
  - MATCH,DIRECT
"#;
        // 1. Включение AdGuard Home (порт 5353)
        let applied = apply_adguard_dns_upstream(sample_yaml, 5353, true).expect("must succeed");
        assert!(applied.contains(AGH_DNS_BEGIN));
        assert!(applied.contains("- 127.0.0.1:5353"));
        assert!(applied.contains(AGH_DNS_END));
        assert!(applied.contains(AGH_DIRECT_DNS_BEGIN));
        assert!(applied.contains(AGH_DIRECT_DNS_END));
        assert!(applied.contains("- 77.88.8.8"), "Existing nameservers must be preserved as fallbacks");

        // 2. Идемпотентность (повторный вызов не дублирует блоки)
        let re_applied = apply_adguard_dns_upstream(&applied, 5353, true).expect("must succeed");
        assert_eq!(re_applied.matches(AGH_DNS_BEGIN).count(), 1, "Must contain exactly one AGH_DNS_BEGIN");
        assert_eq!(re_applied.matches("127.0.0.1:5353").count(), 2, "Must contain 127.0.0.1:5353 once for nameserver and once for direct-nameserver");

        // 3. Отключение AdGuard Home — чистое удаление
        let disabled = apply_adguard_dns_upstream(&re_applied, 5353, false).expect("must succeed");
        assert!(!disabled.contains(AGH_DNS_BEGIN));
        assert!(!disabled.contains("127.0.0.1:5353"));
        assert!(!disabled.contains(AGH_DNS_END));
        assert!(disabled.contains("- 77.88.8.8"), "Original resolvers must remain intact");
    }

    #[test]
    fn regression_bug_117b_nameserver_flow_style_and_parse_error() {
        // 1. Flow-стиль nameserver: [1.1.1.1, 8.8.8.8] должен распознаваться и включать 127.0.0.1:5353
        let flow_yaml = "dns:\n  enable: true\n  nameserver: [77.88.8.8, 1.1.1.1]\nrules:\n  - MATCH,DIRECT\n";
        let applied = apply_adguard_dns_upstream(flow_yaml, 5353, true)
            .expect("flow-style nameserver must be parsed and integrated");
        assert!(applied.contains("127.0.0.1:5353"), "AGH must be inserted into flow-style nameserver:\n{applied}");
        assert!(applied.contains("77.88.8.8") && applied.contains("1.1.1.1"), "existing nameservers must be kept:\n{applied}");

        let disabled = apply_adguard_dns_upstream(&applied, 5353, false)
            .expect("disabling after flow-style integration must succeed");
        assert!(!disabled.contains("127.0.0.1:5353"));
        assert!(disabled.contains("77.88.8.8"));

        // 2. Нераспознанный формат nameserver или отсутствие секции nameserver -> Err (не тихий Ok)
        let malformed_yaml = "dns:\n  enable: true\n  nameserver: not_a_list\nrules:\n  - MATCH,DIRECT\n";
        assert!(
            apply_adguard_dns_upstream(malformed_yaml, 5353, true).is_err(),
            "malformed nameserver must return Err instead of silently claiming success"
        );
        let missing_ns_yaml = "dns:\n  enable: true\nrules:\n  - MATCH,DIRECT\n";
        assert!(
            apply_adguard_dns_upstream(missing_ns_yaml, 5353, true).is_err(),
            "missing nameserver section must return Err instead of silently claiming success"
        );
    }

    #[test]
    fn test_domestic_catalog_invariants() {
        assert_eq!(DOMESTIC_WHITELIST_DOMAINS.len(), 402, "Must contain exactly 402 base domains");
        assert_eq!(DOMESTIC_CATALOG.len(), 402, "Catalog entries must equal 402");

        let mut seen = std::collections::HashSet::new();
        for &dom in DOMESTIC_WHITELIST_DOMAINS {
            assert!(seen.insert(dom), "Duplicate domain found in catalog: {}", dom);
            assert!(crate::config::is_valid_domain(dom), "Domain failed validity check: {}", dom);
            assert!(!dom.contains("://"), "Domain must not contain protocol: {}", dom);
            assert!(!dom.contains('/'), "Domain must not contain path: {}", dom);
            assert!(!dom.starts_with("www."), "Domain must not contain www prefix: {}", dom);
        }
    }

    #[test]
    fn test_apply_whitelist_rules_lifecycle() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.custom_domains.push("user-added.example".into());
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.105".into(),
            enabled: true,
            name: Some("Work Laptop".into()),
            server: None
        });

        // 1. Применение правил
        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(applied.contains(WHITELIST_BEGIN));
        assert!(applied.contains(WHITELIST_END));
        assert!(applied.contains(WHITELIST_GROUP_BEGIN));
        assert!(applied.contains(WHITELIST_GROUP_END));
        assert!(applied.contains(&format!("- name: '{WHITELIST_GROUP_NAME}'")));
        assert!(!applied.contains("DOMAIN-SUFFIX,gosuslugi.ru,DIRECT"));
        assert!(!applied.contains("DOMAIN-SUFFIX,sberbank.ru,DIRECT"));
        assert!(!applied.contains("DOMAIN-SUFFIX,vk.com,DIRECT"));
        assert!(applied.contains("DOMAIN-SUFFIX,user-added.example,DIRECT"));
        assert!(applied.contains(&format!("SRC-IP-CIDR,192.168.1.105/32,{WHITELIST_GROUP_NAME}")));
        assert!(validate_marker_pair(&applied, WHITELIST_BEGIN, WHITELIST_END).is_ok());
        assert!(validate_marker_pair(&applied, WHITELIST_GROUP_BEGIN, WHITELIST_GROUP_END).is_ok());

        // 2. Отключение — чистое удаление
        cfg.whitelist.enabled = false;
        let disabled = apply_whitelist_rules(&applied, &cfg, None);
        assert!(!disabled.contains(WHITELIST_BEGIN));
        assert!(!disabled.contains(WHITELIST_END));
        assert!(!disabled.contains(WHITELIST_GROUP_BEGIN));
        assert!(!disabled.contains(WHITELIST_GROUP_END));
        assert!(!disabled.contains("SRC-IP-CIDR,192.168.1.105/32"));
        assert!(disabled.contains("GEOIP,RU,DIRECT"), "Original fallback rules must be preserved");
    }

    #[test]
    fn regression_strict_whitelist_tunnels_russian_catalog_and_category_matches() {
        let base_yaml = r#"port: 7890
rule-providers:
  category-ru@domain: { type: http, url: "https://example.com/ru.mrs" }
  category-ru@ipcidr: { type: http, url: "https://example.com/ru_ip.mrs" }
  category-ip-geo-detect@domain: { type: http, url: "https://example.com/geo.mrs" }
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - DOMAIN-SUFFIX,source-explicit.example,DIRECT
  - RULE-SET,category-ip-geo-detect@domain,DIRECT
  - OR,((RULE-SET,category-ru@domain),(RULE-SET,category-ru@ipcidr)),DIRECT
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.whitelist.custom_domains = vec!["user-explicit-direct.example".into()];

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        let start = applied.find(WHITELIST_BEGIN).expect("whitelist rule block must exist");
        let end = applied.find(WHITELIST_END).expect("whitelist rule block must end");
        let block = &applied[start..end];
        let catchall = applied.find("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist").expect("IPv4 LAN catch-all must exist");

        assert!(
            !block.contains("DOMAIN-SUFFIX,sberbank.ru,DIRECT"),
            "the built-in Russian catalog must not add automatic DIRECT rules in StrictXKeen"
        );
        assert!(
            !block.contains("RULE-SET,category-ip-geo-detect@domain,DIRECT")
                && !block.contains("category-ru@domain),(RULE-SET,category-ru@ipcidr)),DIRECT"),
            "country and geo-detected IP rules must not outrank the Whitelist tunnel"
        );
        assert!(
            block.contains("DOMAIN-SUFFIX,user-explicit-direct.example,DIRECT"),
            "a domain explicitly added by the user must keep its DIRECT exception"
        );
        assert!(
            applied.find("DOMAIN-SUFFIX,source-explicit.example,DIRECT").unwrap() > catchall,
            "source-configured rules remain present below the LAN default route"
        );
        assert!(
            applied.find("RULE-SET,category-ip-geo-detect@domain,DIRECT").unwrap() > catchall,
            "source rule-providers and rules must remain intact after the LAN catch-all"
        );

        let disabled = apply_whitelist_rules(&applied, &crate::config::AppConfig::default(), None);
        assert_eq!(disabled, base_yaml.trim_end(), "disabling StrictXKeen must restore the source rules unchanged");
    }

    #[test]
    fn regression_strict_whitelist_keeps_user_domain_lists_and_device_routes_above_default() {
        let base_yaml = r#"port: 7890
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - GEOIP,RU,DIRECT
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.whitelist.excluded_devices = vec!["192.168.1.99".into()];
        cfg.whitelist.custom_domains = vec!["custom-direct.example".into()];
        cfg.direct_domains = vec!["direct-list.example".into()];
        cfg.force_domains = vec!["proxy-list.example".into()];
        cfg.adblock_enabled = true;
        cfg.device_domain_rules.insert(
            "192.168.1.24".into(),
            vec![crate::config::DeviceDomainRule {
                domain: "device-domain.example".into(),
                target: "PROXY".into(),
            }],
        );
        cfg.device_routing.insert(
            "192.168.1.23".into(),
            crate::config::DeviceRouting {
                servers: vec!["PROXY".into()],
                ..Default::default()
            },
        );

        let (applied, _) = apply_routing(base_yaml, &cfg).expect("strict routing generation must succeed");
        let direct_pos = applied.find("DOMAIN-SUFFIX,direct-list.example,DIRECT").expect("direct list entry must remain DIRECT");
        let force_pos = applied
            .find(&format!("DOMAIN-SUFFIX,proxy-list.example,{}", WHITELIST_GROUP_NAME))
            .expect("forced-proxy list entry in StrictXKeen must target Whitelist group (bug 3.1)");
        let default_pos = applied.find("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist").expect("LAN default must target Whitelist");
        let source_country_pos = applied.find("GEOIP,RU,DIRECT").expect("source country rule must be preserved");
        let adblock_pos = applied.find(ADBLOCK_BEGIN).expect("enabled AdBlock block must remain present");
        let device_rule = "SRC-IP-CIDR,192.168.1.23/32,";
        let device_pos = applied.find(device_rule).expect("explicit device route must remain present");

        assert!(direct_pos < default_pos && force_pos < default_pos, "both settings lists must precede the Whitelist default");
        assert!(default_pos < source_country_pos, "source country routes must remain below the StrictXKeen default");
        assert!(direct_pos < adblock_pos && force_pos < adblock_pos, "both settings lists must outrank generated routing blocks");
        let device_domain_pos = applied.find("DOMAIN-SUFFIX,device-domain.example").expect("per-device domain rule must remain present");
        assert!(device_pos < default_pos, "an explicitly routed device must keep its own target ahead of the catch-all");
        assert!(direct_pos < device_domain_pos && force_pos < device_domain_pos, "global domain settings must outrank per-device domain rules");
        assert!(direct_pos < device_pos && force_pos < device_pos, "global domain settings must outrank per-device routes");
        assert!(
            applied.find("SRC-IP-CIDR,192.168.1.99/32,DIRECT").unwrap() < default_pos,
            "an excluded device must keep its explicit DIRECT exception"
        );
        assert!(
            applied.find("DOMAIN-SUFFIX,custom-direct.example,DIRECT").unwrap() < default_pos,
            "a user-added whitelist domain must keep its explicit DIRECT exception"
        );

        let (reapplied, _) = apply_routing(&applied, &cfg).expect("rebuilding on panel/core restart must succeed");
        let reapplied_default = reapplied.find("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist").expect("reapplied LAN default must target Whitelist");
        assert!(reapplied.find("DOMAIN-SUFFIX,direct-list.example,DIRECT").unwrap() < reapplied_default);
        let force_rule_str = format!("DOMAIN-SUFFIX,proxy-list.example,{}", WHITELIST_GROUP_NAME);
        assert!(reapplied.find(&force_rule_str).unwrap() < reapplied_default);
        assert!(reapplied.find("DOMAIN-SUFFIX,direct-list.example,DIRECT").unwrap() < reapplied.find("DOMAIN-SUFFIX,device-domain.example").unwrap());
        assert!(reapplied.find(&force_rule_str).unwrap() < reapplied.find("DOMAIN-SUFFIX,device-domain.example").unwrap());
        assert!(reapplied.find(device_rule).unwrap() < reapplied_default);
        assert!(reapplied.find("GEOIP,RU,DIRECT").unwrap() > reapplied_default);
        assert!(reapplied.contains(WHITELIST_GROUP_BEGIN), "restart must regenerate the selected Whitelist group");
    }

    #[test]
    fn regression_strict_domain_settings_updates_keep_global_lists_first() {
        let yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.direct_domains = vec!["global-direct.example".into()];
        cfg.device_domain_rules.insert(
            "192.168.1.24".into(),
            vec![crate::config::DeviceDomainRule {
                domain: "device-specific.example".into(),
                target: "PROXY".into(),
            }],
        );
        let force = vec!["global-force.example".into()];

        let applied = apply_domain_rules_for_config(yaml, &cfg, &force).expect("domain settings update must succeed");
        let direct_pos = applied.find("DOMAIN-SUFFIX,global-direct.example,DIRECT").unwrap();
        let force_pos = applied.find("DOMAIN-SUFFIX,global-force.example,PROXY").unwrap();
        let device_pos = applied.find("DOMAIN-SUFFIX,device-specific.example").unwrap();
        assert!(direct_pos < device_pos && force_pos < device_pos);

        let mut zapret = crate::config::ZapretConfig::default();
        zapret.enabled = true;
        zapret.hybrid_youtube = true;
        let with_auto_rules = apply_zapret_hybrid_rules_with_force(&applied, &zapret, &force)
            .expect("automatic Zapret rules must be generated");
        assert!(
            with_auto_rules.find(ZAPRET_HYBRID_BEGIN).unwrap() < with_auto_rules.find(FORCE_BEGIN).unwrap(),
            "Zapret may be inserted before force rules before the settings are reapplied"
        );
        let reprioritized = apply_domain_rules_for_config(&with_auto_rules, &cfg, &force)
            .expect("reapplying domain settings must succeed");
        let zapret_pos = reprioritized.find(ZAPRET_HYBRID_BEGIN).unwrap();
        assert!(reprioritized.find("DOMAIN-SUFFIX,global-direct.example,DIRECT").unwrap() < zapret_pos);
        assert!(reprioritized.find("DOMAIN-SUFFIX,global-force.example,PROXY").unwrap() < zapret_pos);

        cfg.whitelist.enabled = false;
        let disabled = apply_domain_rules_for_config(yaml, &cfg, &force).expect("disabled mode update must succeed");
        assert!(disabled.find("DOMAIN-SUFFIX,device-specific.example").unwrap() < disabled.find("DOMAIN-SUFFIX,global-direct.example,DIRECT").unwrap());
    }

    #[test]
    fn regression_strict_whitelist_keeps_gaming_country_bypasses_below_tunnel() {
        let yaml = r#"port: 7890
rule-providers:
  category-ru@domain: { type: http, url: "https://example.com/ru.mrs" }
  category-ru@ipcidr: { type: http, url: "https://example.com/ru_ip.mrs" }
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.gaming.enabled = true;
        cfg.gaming.mode = crate::config::GamingMode::BypassRu;
        cfg.gaming.devices.push(crate::config::GamingDevice {
            mac: "aa:bb:cc:dd:ee:12".into(),
            ip: "192.168.1.45".into(),
            ipv6: vec![],
            name: "Gaming client".into(),
            enabled: true,
            server: Some("PROXY".into()),
        });

        let (applied, _) = apply_routing(yaml, &cfg).expect("strict routing with a saved gaming profile must succeed");
        let tunnel_pos = applied.find("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist").expect("strict default must be present");
        for line in applied.lines().filter(|line| {
            line.contains("(GEOIP,RU)")
                || line.contains("(RULE-SET,category-ru@domain)")
                || line.contains("(RULE-SET,category-ru@ipcidr)")
                || line.contains("(GEOSITE,category-ru)")
        }) {
            let pos = applied.find(line).unwrap();
            assert!(pos > tunnel_pos, "automatic gaming country bypass must not precede StrictXKeen default: {line}");
        }

        let (reapplied, _) = apply_routing(&applied, &cfg).expect("panel/core restart rebuild must preserve the order");
        let tunnel_pos = reapplied.find("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist").unwrap();
        for line in reapplied.lines().filter(|line| {
            line.contains("(GEOIP,RU)")
                || line.contains("(RULE-SET,category-ru@domain)")
                || line.contains("(RULE-SET,category-ru@ipcidr)")
                || line.contains("(GEOSITE,category-ru)")
        }) {
            assert!(reapplied.find(line).unwrap() > tunnel_pos, "reapplied automatic country bypass must remain below StrictXKeen default: {line}");
        }
    }

    #[test]
    fn regression_disabled_whitelist_preserves_legacy_domain_and_country_routing() {
        let base_yaml = r#"port: 7890
rule-providers:
  category-ru@domain: { type: http, url: "https://example.com/ru.mrs" }
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - RULE-SET,category-ru@domain,DIRECT
  - GEOIP,RU,DIRECT
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = false;
        // A retained strategy setting must have no effect while the mode is disabled.
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.direct_domains = vec!["user-direct.example".into()];
        cfg.force_domains = vec!["user-proxy.example".into()];

        let (applied, _) = apply_routing(base_yaml, &cfg).expect("routing with whitelist disabled must succeed");
        assert!(!applied.contains(WHITELIST_BEGIN), "disabled mode must not add whitelist routing rules");
        assert!(!applied.contains(WHITELIST_GROUP_BEGIN), "disabled mode must not add the Whitelist group");
        assert!(!applied.contains("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist"));
        assert!(applied.contains("RULE-SET,category-ru@domain,DIRECT"), "source country rules must remain intact");
        assert!(applied.contains("GEOIP,RU,DIRECT"), "source GEOIP rules must remain intact");

        let direct_pos = applied.find("DOMAIN-SUFFIX,user-direct.example,DIRECT").expect("direct setting must remain active");
        let force_pos = applied.find("DOMAIN-SUFFIX,user-proxy.example,PROXY").expect("force setting must remain active");
        let source_country_pos = applied.find("RULE-SET,category-ru@domain,DIRECT").unwrap();
        assert!(direct_pos < source_country_pos && force_pos < source_country_pos);
    }

    /// Анти-регрессия BS-PIN (сценарий бага v1.9.41): снятие узла устройства на
    /// странице «Устройства» (assignment server=None) обязано сохранять группу
    /// закреплённого БС-устройства — иначе whitelist-правило ссылается в пустоту
    /// и ядро Mihomo отклоняет весь конфиг («proxy not found», откат настроек).
    #[test]
    fn test_apply_assignments_keeps_pinned_whitelist_group_on_remove() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        // Шаг 1: создаём группу телефона пином (как save_whitelist)
        let create = apply_assignments(
            base_yaml,
            &[Assignment {
                ip: "192.168.2.63".into(),
                name: "Xiaomi 11T Pro".into(),
                server: Some("Fastest".into()),
            }],
            &["geodema2".to_string()],
            &["192.168.2.63".to_string()],
        )
        .unwrap();
        assert!(create.contains("- name: 'Xiaomi 11T Pro 192_168_2_63'"));

        // Шаг 2: пользователь на «Устройствах» выбирает «общий активный узел» (server=None)
        let remove = apply_assignments(
            &create,
            &[Assignment {
                ip: "192.168.2.63".into(),
                name: String::new(),
                server: None,
            }],
            &["geodema2".to_string()],
            &["192.168.2.63".to_string()],
        )
        .unwrap();
        // Группа закрепления СОХРАНЯЕТСЯ — на неё ссылается whitelist-правило
        assert!(
            remove.contains("- name: 'Xiaomi 11T Pro 192_168_2_63'"),
            "BS-PIN: группа закреплённого устройства не должна удаляться при снятии узла"
        );

        // Шаг 3: без закрепления — прежнее поведение: группа удаляется
        let remove_unpinned = apply_assignments(
            &create,
            &[Assignment {
                ip: "192.168.2.63".into(),
                name: String::new(),
                server: None,
            }],
            &["geodema2".to_string()],
            &[],
        )
        .unwrap();
        assert!(
            !remove_unpinned.contains("Xiaomi 11T Pro 192_168_2_63"),
            "без закрепления группа устройства удаляется как раньше"
        );
    }

    #[test]
    fn test_pinned_whitelist_ips_extracts_only_pinned() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:02".into(),
            ip: "192.168.2.63".into(),
            enabled: true,
            name: None,
            server: Some("🇳🇱 Нидерланды ADS".into()),
        });
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:03".into(),
            ip: "192.168.2.118".into(),
            enabled: true,
            name: None,
            server: Some("   ".into()), // пробельное имя = не пин
        });
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:04".into(),
            ip: "192.168.2.45".into(),
            enabled: false, // выключенное устройство = не пин
            name: None,
            server: Some("Fastest".into()),
        });
        let pinned = pinned_whitelist_ips(&cfg);
        assert_eq!(pinned, vec!["192.168.2.63".to_string()]);
    }

    #[test]
    fn test_whitelist_strict_direct_no_geoip_ru_wildcard() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.105".into(),
            enabled: true,
            name: Some("Work Laptop".into()),
            server: None,
        });

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        let start = applied.find(WHITELIST_BEGIN).expect("must find begin");
        let end = applied.find(WHITELIST_END).expect("must find end");
        let wl_block = &applied[start..end];

        // Проверяем, что внутри блока белых списков НЕТ правила GEOIP,RU
        assert!(
            !wl_block.contains("GEOIP,RU"),
            "CRITICAL REGRESSION: Whitelist block must NEVER contain broad GEOIP,RU wildcard!"
        );
        assert!(
            !wl_block.contains("GEOIP"),
            "Whitelist block must strictly use explicit DOMAIN-SUFFIX and SRC-IP-CIDR rules!"
        );
    }

    /// Анти-регрессия BS-PIN: устройство с закреплённой нодой получает СВОЁ правило
    /// SRC-IP-CIDR с этой нодой, и оно стоит ВЫШЕ подсетевого catch-all 2.4 →
    /// Failover-переключения группы Whitelist не влияют на трафик устройства.
    #[test]
    fn test_whitelist_device_pinned_server_rule_precedes_catchall() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:02".into(),
            ip: "192.168.2.63".into(),
            enabled: true,
            name: Some("Xiaomi 11T Pro".into()),
            server: Some("🇳🇱 Нидерланды ADS".into()),
        });
        // Второе устройство без закрепления — обычный dev_target
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:03".into(),
            ip: "192.168.2.118".into(),
            enabled: true,
            name: Some("Big PC".into()),
            server: None,
        });

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        let start = applied.find(WHITELIST_BEGIN).expect("must find begin");
        let end = applied.find(WHITELIST_END).expect("must find end");
        let wl_block = &applied[start..end];

        // BS-PIN: правило указывает на per-device группу (селектор с use: providers),
        // потому что провайдерные ноды недоступны правилам напрямую
        let pinned_rule = "SRC-IP-CIDR,192.168.2.63/32,Xiaomi 11T Pro 192_168_2_63";
        let plain_rule = "SRC-IP-CIDR,192.168.2.118/32";
        let catchall = "SRC-IP-CIDR,192.168.0.0/16";

        assert!(wl_block.contains(pinned_rule), "правило закрепления должно ссылаться на per-device группу, блок:\n{wl_block}");
        assert!(wl_block.contains(plain_rule), "устройство без закрепления идёт на dev_target");
        assert!(wl_block.contains(catchall), "catch-all подсети обязан остаться");

        // Закреплённое правило и правило обычного устройства стоят ВЫШЕ catch-all
        let pos_pinned = wl_block.find(pinned_rule).unwrap();
        let pos_plain = wl_block.find(plain_rule).unwrap();
        let pos_catchall = wl_block.find(catchall).unwrap();
        assert!(pos_pinned < pos_catchall, "BS-PIN правило должно стоять выше catch-all");
        assert!(pos_plain < pos_catchall, "обычное правило устройства должно стоять выше catch-all");
    }

    /// BS-PIN: пустое имя ноды трактуется как «не закреплено» (dev_target), а не как битое правило.
    #[test]
    fn test_whitelist_device_empty_pinned_server_falls_back_to_target() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:04".into(),
            ip: "192.168.2.63".into(),
            enabled: true,
            name: None,
            server: Some("   ".into()),
        });

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(
            applied.contains("SRC-IP-CIDR,192.168.2.63/32,🛡️ Whitelist"),
            "пустое закрепление должно дать правило с группой Whitelist, а не пустым таргетом"
        );
        assert!(
            !applied.contains("SRC-IP-CIDR,192.168.2.63/32, \n"),
            "пробельное имя ноды не должно попасть в правило"
        );
    }

    #[test]
    fn test_whitelist_device_assignments_precede_strict_default_route() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.device_routing.insert("192.168.1.105".into(), crate::config::DeviceRouting {
            servers: vec!["Fastest".into()],
            ..Default::default()
        });
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.105".into(),
            enabled: true,
            name: Some("Work Laptop".into()),
            server: None
        });

        let (applied, _) = apply_routing(base_yaml, &cfg).expect("apply_routing must succeed");
        let wl_pos = applied.find(WHITELIST_BEGIN).expect("WHITELIST_BEGIN must be present");
        let dev_pos = applied.find(RULES_BEGIN).expect("RULES_BEGIN must be present");

        // Индивидуальная настройка устройства обязана сохраниться перед catch-all туннелем.
        assert!(
            dev_pos < wl_pos,
            "explicit device rules ({}) must precede the StrictXKeen default ({})",
            dev_pos, wl_pos
        );
    }

    #[test]
    fn test_whitelist_direct_push_rules_precede_google_ai() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.direct_domains = vec!["fcm.googleapis.com".into()];
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.105".into(),
            enabled: true,
            name: Some("Work Phone".into()),
            server: None,
        });

        let (applied, _) = apply_routing(base_yaml, &cfg).expect("apply_routing must succeed");
        let wl_pos = applied.find(WHITELIST_BEGIN).expect("WHITELIST_BEGIN must be present");
        let ai_pos = applied.find(GOOGLE_AI_BEGIN).expect("GOOGLE_AI_BEGIN must be present");

        // Пользовательский DIRECT список имеет приоритет над специализированными правилами Google AI.
        assert!(
            applied.find("DOMAIN-SUFFIX,fcm.googleapis.com,DIRECT").unwrap() < ai_pos,
            "user direct-domain setting must precede Google AI routing"
        );
        assert!(wl_pos > 0, "StrictXKeen default block must remain present");
    }

    #[test]
    fn regression_bug_32_zapret_excluded_devices_bypass_mihomo_direct_and_resolve_mac() {
        // Баг 32: Исключение устройства из Zapret работало только для LAN-трафика напрямую.
        // Hybrid-трафик через Mihomo DIRECT выходил из роутера через OUTPUT с адресом роутера и попадал в Zapret.
        // Исключение по MAC переводилось в IP через ARP только при старте.
        let mut zapret_cfg = crate::config::ZapretConfig::default();
        zapret_cfg.enabled = true;
        zapret_cfg.hybrid_youtube = true;
        // Устройство исключено по IP и по MAC:
        zapret_cfg.excluded_devices = vec!["192.168.1.50".into(), "00:11:22:33:44:55".into()];

        let mock_arp = "IP address       HW type     Flags       HW address            Mask     Device\n\
                        192.168.1.60     0x1         0x2         00:11:22:33:44:55     *        br0\n";

        let excluded_ips = resolve_excluded_zapret_ips_internal(&zapret_cfg, Some(mock_arp));
        assert!(excluded_ips.contains(&"192.168.1.50".to_string()), "IP must be resolved");
        assert!(excluded_ips.contains(&"192.168.1.60".to_string()), "MAC must be dynamically resolved from ARP");

        let yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,DIRECT\n";
        let out = apply_zapret_hybrid_rules_with_force_internal(yaml, &zapret_cfg, &[], Some(mock_arp)).expect("applied");

        // Для исключённых устройств hybrid-домены НЕ должны уходить в DIRECT (где Zapret в OUTPUT их перехватывает)
        // Они должны направляться в PROXY для этого устройства:
        assert!(
            out.contains("SRC-IP-CIDR,192.168.1.50/32"),
            "Excluded IP must have explicit rule diverting away from Zapret DIRECT"
        );
        assert!(
            out.contains("SRC-IP-CIDR,192.168.1.60/32"),
            "Excluded MAC resolved IP must have explicit rule diverting away from Zapret DIRECT"
        );
    }

    #[test]
    fn regression_bug_24_whitelist_dns_applies_in_zero_touch_mode_without_explicit_devices() {
        // Баг 24: DNS-политика БС (российские домены -> DNS роутера 127.0.0.1:53) применялась
        // только при явно заданных устройствах. В режиме Zero-Touch (когда список устройств пуст,
        // но whitelist включен на всю сеть) DNS-политика отсутствовала.
        let yaml = "port: 7890\ndns:\n  enable: true\nproxy-groups:\nrules:\n  - MATCH,DIRECT\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.clear(); // Режим Zero-Touch без явных устройств!

        let applied = apply_whitelist_dns(yaml, &cfg, "127.0.0.1:53");

        assert!(
            applied.contains("nameserver-policy:"),
            "nameserver-policy must be generated in Zero-Touch whitelist mode"
        );
        assert!(
            applied.contains(WHITELIST_DNS_BEGIN),
            "WHITELIST_DNS_BEGIN must be present"
        );
        assert!(
            applied.contains("127.0.0.1:53"),
            "DNS policy must point to router DNS 127.0.0.1:53"
        );
    }

    #[test]
    fn test_whitelist_nameserver_policy_injection() {
        let base_yaml = "port: 7890\ndns:\n  enable: true\n  nameserver:\n    - 77.88.8.8\nrules:\n  - MATCH,DIRECT\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.105".into(),
            enabled: true,
            name: Some("Phone".into()),
            server: None
        });

        let applied = apply_whitelist_dns(base_yaml, &cfg, "127.0.0.1:53");
        assert!(applied.contains(WHITELIST_DNS_BEGIN));
        assert!(applied.contains(WHITELIST_DNS_END));
        assert!(applied.contains("nameserver-policy:"));
        assert!(applied.contains("'+.gosuslugi.ru': '127.0.0.1:53'"));
        assert!(applied.contains("'+.sberbank.ru': '127.0.0.1:53'"));
        assert!(applied.contains("- 77.88.8.8"), "Existing nameservers must be preserved");

        // Отключение очищает nameserver-policy
        cfg.whitelist.enabled = false;
        let disabled = apply_whitelist_dns(&applied, &cfg, "127.0.0.1:53");
        assert!(!disabled.contains(WHITELIST_DNS_BEGIN));
        assert!(!disabled.contains(WHITELIST_DNS_END));
        assert!(!disabled.contains("'+.gosuslugi.ru'"));
        assert!(disabled.contains("- 77.88.8.8"));
    }

    #[test]
    fn test_whitelist_marker_pair_validation() {
        let valid_yaml = format!("rules:\n{}\n  - DOMAIN-SUFFIX,gosuslugi.ru,DIRECT\n{}\n", WHITELIST_BEGIN, WHITELIST_END);
        assert!(validate_marker_pair(&valid_yaml, WHITELIST_BEGIN, WHITELIST_END).is_ok());

        let missing_end = format!("rules:\n{}\n  - DOMAIN-SUFFIX,gosuslugi.ru,DIRECT\n", WHITELIST_BEGIN);
        assert!(validate_marker_pair(&missing_end, WHITELIST_BEGIN, WHITELIST_END).is_err());

        let duplicate_begin = format!("rules:\n{}\n{}\n{}\n", WHITELIST_BEGIN, WHITELIST_BEGIN, WHITELIST_END);
        assert!(validate_marker_pair(&duplicate_begin, WHITELIST_BEGIN, WHITELIST_END).is_err());
    }

    #[test]
    fn test_whitelist_custom_domains() {
        let mut wl_cfg = crate::config::WhitelistConfig::default();
        wl_cfg.custom_domains = vec![
            "https://my-internal-portal.ru/index.html".into(),
            "www.special-bank.ru".into(),
            "invalid domain name with spaces".into(),
            "192.168.1.1".into(),
        ];

        let all = get_all_whitelist_domains(&wl_cfg);
        assert_eq!(all.len(), DOMESTIC_WHITELIST_DOMAINS.len() + 2, "Must add exactly 2 valid custom domains");
        assert!(all.contains(&"my-internal-portal.ru".to_string()));
        assert!(all.contains(&"special-bank.ru".to_string()));
        assert!(!all.contains(&"invalid domain name with spaces".to_string()));
        assert!(!all.contains(&"192.168.1.1".to_string()));
    }

    #[test]
    fn test_zero_touch_whitelist_subnet_rules_and_exclusions() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.whitelist.excluded_devices = vec!["192.168.1.99".into()];

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(applied.contains(WHITELIST_BEGIN));
        assert!(applied.contains(WHITELIST_END));

        // 1. The built-in catalog does not force domestic domains through DIRECT.
        assert!(!applied.contains("DOMAIN-SUFFIX,gosuslugi.ru,DIRECT"));
        assert!(!applied.contains("DOMAIN-SUFFIX,sberbank.ru,DIRECT"));

        // 2. Excluded device -> DIRECT
        assert!(applied.contains("SRC-IP-CIDR,192.168.1.99/32,DIRECT"));

        // 3. Zero-touch subnet coverage
        assert!(applied.contains("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist"));
        assert!(applied.contains("SRC-IP-CIDR,10.0.0.0/8,🛡️ Whitelist"));

        // 4. Explicit device exclusions must be ABOVE subnet tunnel.
        let ex_pos = applied.find("SRC-IP-CIDR,192.168.1.99/32,DIRECT").unwrap();
        let sub_pos = applied.find("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist").unwrap();
        assert!(ex_pos < sub_pos, "Excluded devices must precede subnet redirection");
    }

    #[test]
    fn test_standard_whitelist_strategy_no_subnet_redirection() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::Standard;

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(applied.contains(WHITELIST_BEGIN));
        assert!(applied.contains("DOMAIN-SUFFIX,gosuslugi.ru,DIRECT"));
        // В стандартном режиме blanket subnet перехват не должен добавляться
        assert!(!applied.contains("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist"));
    }

    #[test]
    fn test_resolve_excluded_whitelist_ips_for_disabled_devices_with_dynamic_arp() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "ea:0c:ab:1a:9f:06".into(),
            ip: "192.168.2.48".into(),
            name: Some("Xiaomi 11T Pro".into()),
            enabled: false,
            server: None,
        });

        let arp_sample = "IP address       HW type     Flags       HW address            Mask     Device\n192.168.2.63     0x1         0x2         ea:0c:ab:1a:9f:06     *        br0\n192.168.2.1      0x1         0x2         00:04:9f:ef:01:02     *        br0\n";

        let resolved = resolve_excluded_whitelist_ips_internal(&cfg, Some(arp_sample));
        assert!(resolved.contains(&"192.168.2.63".to_string()));
        assert!(!resolved.contains(&"192.168.2.48".to_string()));
    }

    #[test]
    fn test_resolve_excluded_whitelist_ips_with_mac_lookup() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.150".into(),
            name: Some("Smart TV".into()),
            enabled: true,
            server: None
        });
        cfg.whitelist.excluded_devices = vec![
            "AA:BB:CC:DD:EE:01".into(), // by MAC
            "192.168.1.151".into(),     // by IP
        ];

        let resolved = resolve_excluded_whitelist_ips(&cfg);
        assert_eq!(resolved, vec!["192.168.1.150".to_string(), "192.168.1.151".to_string()]);
    }

    #[test]
    fn regression_bug_2_resolve_excluded_whitelist_ips_for_regular_devices() {
        // Баг 2: Исключение устройства из БС не работает для обычных устройств.
        // UI отправляет MAC обычного устройства сети, которого нет в gaming.devices или whitelist.devices.
        let mut cfg = crate::config::AppConfig::default();
        let regular_mac = "22:33:44:55:66:77";
        cfg.whitelist.excluded_devices = vec![regular_mac.to_string()];

        let arp_sample = "\
IP address       HW type     Flags       HW address            Mask     Device\n\
192.168.2.145    0x1         0x2         22:33:44:55:66:77     *        br0\n\
192.168.2.1      0x1         0x2         00:04:9f:ef:01:02     *        br0\n";

        // Проверяем извлечение IP по MAC из ARP таблицы
        assert_eq!(
            find_ip_for_mac_in_arp(regular_mac, arp_sample),
            Some("192.168.2.145".to_string()),
            "find_ip_for_mac_in_arp обязан находить IP по MAC в таблице ARP"
        );

        // Проверяем, что resolve_excluded_whitelist_ips_internal находит IP обычного устройства
        let resolved = resolve_excluded_whitelist_ips_internal(&cfg, Some(arp_sample));
        assert!(
            resolved.contains(&"192.168.2.145".to_string()),
            "resolve_excluded_whitelist_ips обязан находить IP обычного устройства по его MAC из ARP"
        );
    }

    #[test]
    fn regression_bug_2b_excluded_device_uses_last_known_ip() {
        // 1. Старый JSON без excluded_last_ips читается без ошибок
        let legacy_json = r#"{"enabled":true,"excluded_devices":["22:33:44:55:66:77"]}"#;
        let legacy_wl: crate::config::WhitelistConfig =
            serde_json::from_str(legacy_json).expect("legacy WhitelistConfig without excluded_last_ips must parse");
        assert!(legacy_wl.excluded_last_ips.is_empty());

        // 2. MAC есть в исключениях, ARP пуст, но в excluded_last_ips есть сохранённый IP -> возвращается он
        let mut cfg = crate::config::AppConfig::default();
        let mac = "22:33:44:55:66:77";
        cfg.whitelist.excluded_devices = vec![mac.to_string()];
        cfg.whitelist.excluded_last_ips.insert(mac.to_string(), "192.168.1.77".to_string());

        let resolved_cached = resolve_excluded_whitelist_ips_internal(&cfg, Some(""));
        assert_eq!(resolved_cached, vec!["192.168.1.77".to_string()]);

        // 3. MAC появился в ARP с новым IP -> resolve берёт свежий IP из ARP, а refresh обновляет excluded_last_ips
        let arp_new = "IP address       HW type     Flags       HW address            Mask     Device\n\
                       192.168.1.88    0x1         0x2         22:33:44:55:66:77     *        br0\n";
        let resolved_arp = resolve_excluded_whitelist_ips_internal(&cfg, Some(arp_new));
        assert_eq!(resolved_arp, vec!["192.168.1.88".to_string()]);

        let updated = refresh_excluded_whitelist_last_ips(&mut cfg, arp_new);
        assert!(updated);
        assert_eq!(
            cfg.whitelist.excluded_last_ips.get(mac).map(String::as_str),
            Some("192.168.1.88")
        );
    }

    #[test]
    fn regression_bug_3_whitelist_strict_mode_rule_hierarchy() {
        // Баг 3: В режиме БС (StrictXKeen) правила, стоящие выше catch-all блока БС, обходят его
        // (доменные правила force_domains, zapret-hybrid, gaming направляют трафик в PROXY/VPS в обход БС).
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.force_domains = vec!["force-site.com".into()];
        cfg.direct_domains = vec!["direct-site.com".into()];

        let (applied, _) = apply_routing(base_yaml, &cfg).expect("apply_routing must succeed");
        let wl_pos = applied.find(WHITELIST_BEGIN).expect("WHITELIST_BEGIN must be present");
        let force_pos = applied.find(FORCE_BEGIN).expect("FORCE_BEGIN must be present");

        // Настроенные доменные списки — явные исключения с наивысшим приоритетом.
        assert!(
            force_pos < wl_pos,
            "force_domains (pos {}) must precede the StrictXKeen default route ({})",
            force_pos, wl_pos
        );
        assert!(
            applied.contains(&format!("DOMAIN-SUFFIX,force-site.com,{}", WHITELIST_GROUP_NAME)),
            "force_domains in StrictXKeen must route through Whitelist group instead of PROXY (INV-1, bug 3.1)"
        );
        assert!(
            applied.find("DOMAIN-SUFFIX,direct-site.com,DIRECT").unwrap() < wl_pos,
            "direct_domains must also precede the StrictXKeen default route"
        );
    }

    #[test]
    fn regression_bug_3c_bs_catch_all_not_bypassed_by_proxy_rules() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";

        let combos = [
            ("BS=on,Zapret=on,Gaming=on", true, true, true),
            ("BS=on,Zapret=off,Gaming=off", true, false, false),
            ("BS=off,Zapret=on,Gaming=on", false, true, true),
            ("BS=off,Zapret=off,Gaming=on", false, false, true),
        ];

        for (label, bs_on, zapret_on, gaming_on) in combos {
            let mut cfg = crate::config::AppConfig::default();
            cfg.whitelist.enabled = bs_on;
            cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
            cfg.zapret.enabled = zapret_on;
            cfg.zapret.isolated_proxy = zapret_on;
            cfg.gaming.enabled = gaming_on;
            if gaming_on {
                cfg.gaming.devices.push(crate::config::GamingDevice {
                    name: "PS5".into(),
                    ip: "192.168.1.50".into(),
                    ipv6: vec![],
                    mac: "aa:bb:cc:dd:ee:01".into(),
                    enabled: true,
                    server: None,
                });
            }
            cfg.force_domains = vec!["force-site.com".into()];
            cfg.direct_domains = vec!["direct-site.com".into()];
            cfg.device_domain_rules.insert(
                "192.168.1.20".into(),
                vec![crate::config::DeviceDomainRule {
                    domain: "dev-proxy.com".into(),
                    target: "PROXY".into(),
                }],
            );

            let mut filters = std::collections::BTreeMap::new();
            let (applied, _) = apply_routing_with_filters(base_yaml, &cfg, &mut filters)
                .unwrap_or_else(|e| panic!("[{label}] apply_routing_with_filters failed: {e}"));

            if bs_on {
                let catch_all_idx = applied
                    .lines()
                    .position(|l| l.contains("SRC-IP-CIDR,192.168.0.0/16,"))
                    .unwrap_or_else(|| panic!("[{label}] catch-all BS line missing:\n{applied}"));
                let rules_idx = applied
                    .lines()
                    .position(|l| l.trim_end() == "rules:")
                    .unwrap();

                for (idx, line) in applied.lines().enumerate() {
                    if idx > rules_idx && idx < catch_all_idx {
                        if let Some(target) = rule_target_of(line.trim()) {
                            assert!(
                                target == "DIRECT"
                                    || target == "REJECT"
                                    || target == "REJECT-DROP"
                                    || target == WHITELIST_GROUP_NAME,
                                "[{label}] Rule above BS catch-all must not lead to VPS/PROXY group '{target}': {line}"
                            );
                        }
                    }
                }
            } else {
                assert!(
                    !applied.contains(WHITELIST_BEGIN),
                    "[{label}] BS disabled must not emit WHITELIST block"
                );
                assert!(
                    applied.contains("DOMAIN-SUFFIX,force-site.com,PROXY"),
                    "[{label}] force_domains must use PROXY when BS is off"
                );
            }
        }
    }

    #[test]
    fn test_whitelist_strict_does_not_generate_country_direct_rules() {
        let base_yaml = r#"port: 7890
rule-providers:
  category-ru@domain: { type: http, url: "https://example.com/ru.mrs" }
  category-ru@ipcidr: { type: http, url: "https://example.com/ru_ip.mrs" }
  category-ip-geo-detect@domain: { type: http, url: "https://example.com/geo.mrs" }
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(applied.contains(WHITELIST_BEGIN));
        assert!(applied.contains(WHITELIST_END));

        // Встроенные категории остаются доступны в YAML, но генератор StrictXKeen
        // больше не добавляет им прямые маршруты.
        let wl_start = applied.find(WHITELIST_BEGIN).unwrap();
        let wl_end = applied.find(WHITELIST_END).unwrap();
        let wl_block = &applied[wl_start..wl_end];
        assert!(!wl_block.contains("RULE-SET,category-ip-geo-detect@domain,DIRECT"));
        assert!(!wl_block.contains("category-ru@domain),(RULE-SET,category-ru@ipcidr)),DIRECT"));

        // Общий IPv4 перехват стоит перед исходным fallback.
        assert!(applied.contains("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist"));
        let subnet_pos = applied.find("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist").expect("subnet rule must be present");
        let match_pos = applied.find("- MATCH,PROXY").expect("source fallback must remain present");
        assert!(subnet_pos < match_pos, "LAN default must precede fallback rules");
    }

    #[test]
    fn test_ozon_and_domestic_cdns_in_whitelist_catalog() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);

        // Встроенный каталог сохраняет домены для UI/DNS, но StrictXKeen не выпускает его напрямую.
        let critical_domains = [
            "ozon.ru",
            "ozonusercontent.com",
            "ozone.ru",
            "ozonru.me",
            "o3.ru",
            "o3team.ru",
            "ozon.tech",
            "ozon.travel",
            "ozon.app",
            "ozonfresh.ru",
            "ngenix.net",
            "qrator.net",
            "wbstatic.net",
            "yastatic.net",
        ];

        for &cd in &critical_domains {
            assert!(
                !applied.contains(&format!("DOMAIN-SUFFIX,{cd},DIRECT")),
                "StrictXKeen must route catalog domain {cd} through the selected tunnel"
            );
            assert!(
                DOMESTIC_WHITELIST_DOMAINS.contains(&cd),
                "CRITICAL REGRESSION: Domain {} must be present in DOMESTIC_WHITELIST_DOMAINS",
                cd
            );
        }
    }

    #[test]
    fn test_v1_9_30_sync_disabled_providers_in_yaml() {
        let yaml = r#"proxy-groups:
  - name: Fastest
    type: url-test
    use:
      - sub1
      - sub2
  - name: Fallback
    type: fallback
    use: [sub1, sub2]
"#;

        // 1. Отключаем sub1 -> строка комментируется
        let disabled = vec!["sub1".to_string()];
        let synced = sync_disabled_providers_in_yaml(yaml, &disabled).expect("synced");
        assert!(synced.contains("# - sub1"), "Disabled provider must be commented out: {synced}");
        assert!(synced.contains("- sub2"), "Active provider must remain active: {synced}");
        assert!(synced.contains("use: [sub2]"), "Inline use must retain only active providers: {synced}");

        // 2. Включаем sub1 обратно -> строка раскомментируется
        let none_disabled: Vec<String> = Vec::new();
        let restored = sync_disabled_providers_in_yaml(&synced, &none_disabled).expect("restored");
        assert!(restored.contains("- sub1"), "Restored provider must be uncommented: {restored}");
        assert!(!restored.contains("# - sub1"), "No commented sub1 should remain: {restored}");
        assert!(restored.contains("- sub2"));
    }

    #[test]
    fn test_v1_9_30_apply_routing_filters_disabled_providers() {
        let base_yaml = r#"proxy-providers:
  active_sub:
    type: http
    url: "https://a.b/c"
  disabled_sub:
    type: http
    url: "https://d.e/f"
proxy-groups:
  - name: PROXY
    type: select
rules:
  - MATCH,DIRECT
"#;

        let mut cfg = crate::config::AppConfig::default();
        cfg.disabled_providers = vec!["disabled_sub".to_string()];
        cfg.device_routing.insert("192.168.2.50".to_string(), crate::config::DeviceRouting {
            servers: vec!["Fastest".to_string()],
            ..Default::default()
        });

        let (applied, _) = apply_routing(base_yaml, &cfg).expect("applied");
        // Проверяем, что в созданной группе устройства нет disabled_sub в секции use:
        assert!(applied.contains("- active_sub"), "active_sub must be present in device group use");
        assert!(!applied.contains("- disabled_sub"), "disabled_sub must NOT be present in device group use: {applied}");
    }

    #[test]
    fn test_apply_canary_rules_lifecycle() {
        let base_yaml = "port: 7890\nproxies:\n  - name: 'VPS-DE-01'\n    type: vless\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let applied = apply_canary_rules_with_providers(base_yaml, &["my_provider".to_string()]);

        // Listeners verification
        assert!(applied.contains(CANARY_LISTENERS_BEGIN));
        assert!(applied.contains(CANARY_LISTENERS_END));
        assert!(applied.contains("listeners:"));
        assert!(applied.contains("name: canary-probe-in"));
        assert!(applied.contains("type: mixed"));
        assert!(applied.contains("port: 7894"));
        assert!(applied.contains("listen: 127.0.0.1"));
        assert!(applied.contains("proxy: '🛡️ Canary Probe'"));

        // Proxy-groups verification
        assert!(applied.contains(CANARY_GROUP_BEGIN));
        assert!(applied.contains(CANARY_GROUP_END));
        assert!(applied.contains("- name: '🛡️ Canary Probe'"));
        assert!(applied.contains("type: select"));
        assert!(applied.contains("- DIRECT"));
        assert!(applied.contains("- REJECT"));
        assert!(applied.contains("- 'VPS-DE-01'") || applied.contains("- VPS-DE-01"));
        assert!(applied.contains("- my_provider"));

        // Clean removal verification
        let cleaned = remove_canary_rules(&applied);
        assert!(!cleaned.contains(CANARY_LISTENERS_BEGIN));
        assert!(!cleaned.contains(CANARY_GROUP_BEGIN));
        assert!(!cleaned.contains("canary-probe-in"));
        assert!(!cleaned.contains("🛡️ Canary Probe"));
        assert!(cleaned.contains("proxy-groups:"));
        assert!(cleaned.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_apply_canary_rules_idempotence() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let first = apply_canary_rules(base_yaml);
        let second = apply_canary_rules(&first);
        assert_eq!(first, second, "Applying canary rules twice must be strictly idempotent");
    }

    #[test]
    fn test_apply_canary_rules_preserves_existing_listeners() {
        let yaml_with_existing = "listeners:\n  - name: my-custom-in\n    type: http\n    port: 8080\nproxy-groups:\n  - name: PROXY\n    type: select\n";
        let applied = apply_canary_rules(yaml_with_existing);
        assert!(applied.contains("my-custom-in"));
        assert!(applied.contains("canary-probe-in"));

        let cleaned = remove_canary_rules(&applied);
        assert!(cleaned.contains("my-custom-in"));
        assert!(!cleaned.contains("canary-probe-in"));
        assert!(!cleaned.contains(CANARY_LISTENERS_BEGIN));
    }

    #[test]
    fn test_apply_canary_rules_includes_static_proxies_and_escapes() {
        let yaml_with_static = "proxies:\n  - name: 'VPS [DE] #1'\n    type: vless\n  - name: \"O'Reilly\"\n    type: trojan\nproxy-groups:\n  - name: PROXY\n    type: select\n";
        let applied = apply_canary_rules(yaml_with_static);
        assert!(applied.contains("- 'VPS [DE] #1'"));
        assert!(applied.contains("- 'O''Reilly'"));
    }

    #[test]
    fn test_apply_routing_with_failover_enabled_injects_canary_and_cleans_when_disabled() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.failover.enabled = true;

        let (applied, _) = apply_routing(base_yaml, &cfg).expect("apply_routing with failover.enabled=true");
        assert!(applied.contains("canary-probe-in"));
        assert!(applied.contains("🛡️ Canary Probe"));

        // Баг 84: изолированный speedtest использует тот же слушатель Canary Probe, поэтому группа
        // остаётся и при выключенном failover (и не дублируется при повторной пересборке).
        cfg.failover.enabled = false;
        let (kept, _) = apply_routing(&applied, &cfg).expect("apply_routing with failover.enabled=false");
        assert!(kept.contains("canary-probe-in"));
        assert_eq!(kept.matches("name: canary-probe-in").count(), applied.matches("name: canary-probe-in").count());
        assert!(kept.contains("🛡️ Canary Probe"));
    }

    #[test]
    fn regression_bug_39_whitelist_target_node_does_not_break_config() {
        // Баг 39: целевой узел БС из подписки подставлялся сырым именем в catch-all — Mihomo отвергал конфиг.
        let base = "mode: rule\nproxies:\n  - name: Static-1\n    type: ss\n    server: 1.1.1.1\n    port: 1\nproxy-providers:\n  sub1:\n    type: http\n    url: http://x\n    path: ./providers/sub1.yaml\nproxy-groups:\n  - name: PROXY\n    type: select\n    use: [sub1]\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.target_server = Some("RU Node From Subscription".to_string());
        let (yaml, _) = apply_routing(base, &cfg).unwrap();
        assert!(
            !yaml.lines().any(|l| l.contains(",RU Node From Subscription")),
            "правила не ссылаются на ноду подписки напрямую:\n{yaml}"
        );
        let user = "proxy-groups:\n  - name: PROXY\n    type: select\nrules:\n  - DOMAIN-REGEX,^a{1,3}\\.com$,PROXY\n  - MATCH,PROXY\n";
        let (kept, replaced) = sanitize_rule_targets(user);
        assert_eq!(kept, user, "пользовательские правила вне AUTO-блоков не меняются");
        assert!(replaced.is_empty());
    }

    #[test]
    fn regression_bug_12_gaming_exit_node_does_not_break_config() {
        // Баг 12: «Выходной узел» игрового устройства подставлялся сырым именем ноды подписки в правило.
        let base = "mode: rule\nproxies:\n  - name: Static-1\n    type: ss\n    server: 1.1.1.1\n    port: 1\nproxy-providers:\n  sub1:\n    type: http\n    url: http://x\n    path: ./providers/sub1.yaml\nproxy-groups:\n  - name: PROXY\n    type: select\n    use: [sub1]\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.gaming.enabled = true;
        cfg.gaming.mode = crate::config::GamingMode::Compatibility;
        cfg.gaming.devices.push(crate::config::GamingDevice {
            mac: "aa:bb:cc:dd:ee:01".into(), ip: "192.168.1.50".into(), ipv6: vec![], name: "PS5".into(),
            enabled: true, server: Some("DE Game Node".to_string()),
        });
        let (yaml, _) = apply_routing(base, &cfg).unwrap();
        assert!(!yaml.lines().any(|l| l.trim_start().starts_with("- ") && l.ends_with(",DE Game Node")), "правило не ссылается на ноду подписки:\n{yaml}");
    }

    #[test]
    fn regression_bug_51_flow_server_node_does_not_break_config() {
        // Баг 51: без группы «Google AI» правила Flow указывали на сырое имя ноды — каждое сохранение ломало конфиг.
        let base = "mode: rule\nproxies:\n  - name: Static-1\n    type: ss\n    server: 1.1.1.1\n    port: 1\nproxy-providers:\n  sub1:\n    type: http\n    url: http://x\n    path: ./providers/sub1.yaml\nproxy-groups:\n  - name: PROXY\n    type: select\n    use: [sub1]\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.flow_server = Some("US AI Node".to_string());
        let (yaml, _) = apply_routing(base, &cfg).unwrap();
        assert!(!yaml.lines().any(|l| l.trim_start().starts_with("- ") && l.ends_with(",US AI Node")), "правила Flow не ссылаются на ноду подписки:\n{yaml}");
    }

    #[test]
    fn regression_bug_124b_single_flow_group_detector() {
        use crate::routing::is_flow_group_name;

        // True cases: Google AI, Google-AI, GoogleFlow, Google Flow, Flow, 🤖 Google AI
        assert!(is_flow_group_name("Google AI"));
        assert!(is_flow_group_name("Google-AI"));
        assert!(is_flow_group_name("GoogleFlow"));
        assert!(is_flow_group_name("Google Flow"));
        assert!(is_flow_group_name("Flow"));
        assert!(is_flow_group_name("🤖 Google AI"));

        // False cases: AI, ChatGPT AI, Workflow, Overflow
        assert!(!is_flow_group_name("AI"));
        assert!(!is_flow_group_name("ChatGPT AI"));
        assert!(!is_flow_group_name("Workflow"));
        assert!(!is_flow_group_name("Overflow"));
    }

    #[test]
    fn regression_bug_52b_repair_flow_validates_before_write() {
        use crate::routing::plan_repair_flow;

        // Без подходящей группы Flow должно возвращать Err
        let empty_groups: Vec<String> = vec![];
        let res_empty = plan_repair_flow(&empty_groups, "US-Node");
        assert!(res_empty.is_err(), "Empty groups must return Err");

        let unrelated_groups = vec!["PROXY".to_string(), "DIRECT".to_string(), "ChatGPT AI".to_string(), "Workflow".to_string()];
        let res_unrelated = plan_repair_flow(&unrelated_groups, "US-Node");
        assert!(res_unrelated.is_err(), "Unrelated groups must return Err");

        // С группой Google-AI или Flow должно возвращать Ok(Plan)
        let flow_groups = vec!["PROXY".to_string(), "Google-AI".to_string()];
        let res_flow = plan_repair_flow(&flow_groups, "US-Node");
        assert!(res_flow.is_ok(), "Google-AI group must return Ok");
        let plan = res_flow.unwrap();
        assert_eq!(plan.target_server, "US-Node");
        assert_eq!(plan.group_name, "Google-AI");
    }

    #[test]
    fn regression_bug_a10_switch_flow_without_group_writes_nothing() {
        use crate::routing::{plan_switch_flow_server, apply_flow_rules};

        let empty_groups: Vec<String> = vec![];
        let res_empty = plan_switch_flow_server(&empty_groups, "US-Node");
        assert!(res_empty.is_err(), "Empty groups must return Err");

        let unrelated_groups = vec!["PROXY".to_string(), "DIRECT".to_string(), "AI".to_string(), "Workflow".to_string()];
        let res_unrelated = plan_switch_flow_server(&unrelated_groups, "US-Node");
        assert!(res_unrelated.is_err(), "Unrelated groups without Flow must return Err");

        // When groups are empty/err, verify YAML remains untouched byte for byte
        let sample_yaml = "rules:\n  - GEOIP,CN,DIRECT\n  - MATCH,PROXY\n";
        let mut yaml_copy = sample_yaml.to_string();
        if let Ok(plan) = res_empty {
            yaml_copy = apply_flow_rules(&yaml_copy, &plan.target_server, Some(&plan.group_name)).unwrap();
        }
        assert_eq!(sample_yaml.as_bytes(), yaml_copy.as_bytes(), "YAML must remain identical byte for byte when no flow group");

        // When Flow group exists, returns plan with group name, not raw server node name
        let flow_groups = vec!["PROXY".to_string(), "Google-AI".to_string()];
        let res_flow = plan_switch_flow_server(&flow_groups, "US-Node");
        assert!(res_flow.is_ok(), "Groups with Flow group must return Ok(plan)");
        let plan = res_flow.unwrap();
        assert_eq!(plan.group_name, "Google-AI");
        assert_eq!(plan.target_server, "US-Node");
    }

    #[test]
    fn regression_bug_77b_provider_yaml_is_escaped() {
        use crate::subscription_parser::yaml_quote;
        let yaml = "proxy-providers:\n  sub1:\n    type: http\n    url: \"https://foo.bar/sub\"\nrules:\n  - MATCH,DIRECT";
        let special_url = "https://a/b\\c/d";
        let special_ua = "MyUA\\Test-1.0";
        let special_hwid = "hwid\\123-abc";
        let special_hc = "https://hc.example.com/check\\path/test";

        let res = add_provider_to_yaml_full(
            yaml,
            "sub_special",
            special_url,
            Some(special_hc),
            Some(300),
            Some(special_hwid),
            Some(special_ua),
        ).expect("should succeed with escaped YAML");

        let quoted_url = yaml_quote(special_url);
        let quoted_ua = yaml_quote(special_ua);
        let quoted_hwid = yaml_quote(special_hwid);
        let quoted_hc = yaml_quote(special_hc);

        assert!(res.contains(&format!("url: {quoted_url}")), "URL must be formatted via yaml_quote: {res}");
        assert!(res.contains(&format!("User-Agent: [{quoted_ua}]")), "UA must be formatted via yaml_quote: {res}");
        assert!(res.contains(&format!("x-hwid: [{quoted_hwid}]")), "HWID must be formatted via yaml_quote: {res}");
        assert!(res.contains(&format!("url: {quoted_hc}")), "HC URL must be formatted via yaml_quote: {res}");

        // Валидация подпункта 77.2: невалидный URL health-check отвергается
        assert!(add_provider_to_yaml_full(yaml, "sub_hc_err", special_url, Some("ftp://hc.example.com"), None, None, None).is_err());
        assert!(add_provider_to_yaml_full(yaml, "sub_hc_ctrl", special_url, Some("https://hc.example.com/bad\x01url"), None, None, None).is_err());
    }

    #[test]
    fn test_quic_reject_rule_precedes_whitelist_catchall() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;

        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - DIRECT\nrules:\n  - MATCH,PROXY\n";
        let (applied, _) = apply_routing(base_yaml, &cfg).expect("routing generation");

        let quic_pos = applied.find(QUIC_REJECT_RULE).expect("QUIC reject rule present");
        let wl_pos = applied.find(WHITELIST_BEGIN).expect("whitelist block present");
        assert!(quic_pos < wl_pos, "QUIC reject rule must precede whitelist block to avoid shadowing");

        if let Some(src_pos) = applied.find("SRC-IP-CIDR,192.168.0.0/16") {
            assert!(quic_pos < src_pos, "QUIC reject rule must precede SRC-IP-CIDR rules");
        }

        assert!(validate_marker_pair(&applied, QUIC_REJECT_BEGIN, QUIC_REJECT_END).is_ok());

        // Test standard strategy as well
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::Standard;
        let (std_applied, _) = apply_routing(base_yaml, &cfg).expect("standard routing generation");
        let std_quic_pos = std_applied.find(QUIC_REJECT_RULE).expect("QUIC reject rule present in standard");
        let std_wl_pos = std_applied.find(WHITELIST_BEGIN).expect("whitelist block present in standard");
        assert!(std_quic_pos < std_wl_pos, "QUIC reject rule must precede whitelist block in standard mode");
    }

    #[test]
    fn test_tcp_concurrent_flag_persists_in_mihomo_yaml() {
        let yaml_without = "port: 7890\nunified-delay: true\nmode: rule\n";
        let out = ensure_tcp_concurrent(yaml_without);
        assert!(out.contains("tcp-concurrent: true"), "tcp-concurrent: true must be injected");

        let yaml_false = "port: 7890\nunified-delay: true\ntcp-concurrent: false\nmode: rule\n";
        let out2 = ensure_tcp_concurrent(yaml_false);
        assert!(out2.contains("tcp-concurrent: true"), "tcp-concurrent: false must be changed to true");
        assert!(!out2.contains("tcp-concurrent: false"), "tcp-concurrent: false must not remain");
    }

    #[tokio::test]
    async fn test_routing_helpers_section_offset_excluded_ips_and_cleanup() {
        let yaml = "proxies:\n  - name: p1\nrules:\n  - MATCH,DIRECT\n";
        let off = section_end_offset(yaml, "proxies:").expect("proxies section found");
        assert_eq!(&yaml[..off], "proxies:");

        let dup_yaml = "rules:\nrules:\n";
        assert!(section_end_offset(dup_yaml, "rules:").is_err());
        assert!(section_end_offset(yaml, "proxy-groups:").is_err());

        let mut zcfg = crate::config::ZapretConfig::default();
        zcfg.excluded_devices = vec!["192.168.1.50".to_string()];
        let ips = resolve_excluded_zapret_ips(&zcfg);
        assert!(ips.contains(&"192.168.1.50".to_string()));

        assert!(cleanup_stale_mode_ipsets().await.is_ok());
    }

    #[test]
    fn regression_bug_12b_gaming_device_server_gets_own_group() {
        let yaml = "proxy-providers:\n  sub1:\n    type: http\n    url: https://example.com\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\n  - name: Fastest\n    type: url-test\n    use:\n      - sub1\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.devices = vec![crate::config::GamingDevice {
            mac: "AA:BB:CC:DD:EE:10".into(),
            ip: "192.168.1.50".into(),
            ipv6: Vec::new(),
            name: "PS5".into(),
            enabled: true,
            server: Some("Node-A".into()),
        }];

        // 12.1: с провайдером создаётся отдельная select-группа, и правило ведёт в неё (а не в сырое имя Node-A)
        let out = apply_gaming_rules_with_zapret(yaml, &cfg, &["sub1".to_string()], false).unwrap();
        let rules_part = out.split("rules:").nth(1).unwrap_or("");
        let rule_line = rules_part
            .lines()
            .find(|l| l.contains("SRC-IP-CIDR,192.168.1.50/32,"))
            .expect("rule for gaming device must exist");
        let target_group = rule_line.rsplit(',').next().unwrap().trim();
        assert_ne!(target_group, "Node-A", "raw provider node name must not appear in rules");
        assert_ne!(target_group, GAMING_GROUP_NAME, "device with provider node must get its own group");
        let groups_part = out.split("rules:").next().unwrap_or("");
        assert!(
            groups_part.contains(&format!("- name: '{target_group}'")),
            "per-device selector group '{target_group}' must exist in proxy-groups:\n{groups_part}"
        );
        assert!(
            !rules_part.contains(",Node-A"),
            "raw node name Node-A must not appear in rules:\n{rules_part}"
        );

        // 12.2: если выбранный узел недоступен/удалён (нет провайдеров и статиков), устройство падает на Gaming
        let yaml_no_prov = "proxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - DIRECT\nrules:\n  - MATCH,PROXY\n";
        let out_fallback = apply_gaming_rules_with_zapret(yaml_no_prov, &cfg, &[], false).unwrap();
        let rules_fb = out_fallback.split("rules:").nth(1).unwrap_or("");
        assert!(
            rules_fb.contains(&format!("SRC-IP-CIDR,192.168.1.50/32,{GAMING_GROUP_NAME}")),
            "unavailable node must fall back to {GAMING_GROUP_NAME}:\n{rules_fb}"
        );
    }

    #[test]
    fn regression_bug_16b_fake_ip_filter_for_game_domains() {
        let base_yaml = "port: 7890\ndns:\n  enable: true\n  enhanced-mode: fake-ip\n  fake-ip-filter:\n    - '*.lan'\n    - 'localhost.ptlogin2.qq.com'\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - DIRECT\nrules:\n  - MATCH,PROXY\n";

        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.exclude_fake_ip_for_game_domains = true;
        cfg.platforms.steam = true;

        // С флагом true игровые домены добавляются в fake-ip-filter, а пользовательские записи сохраняются
        let with_filter = apply_gaming_fake_ip_filter(base_yaml, &cfg, false);
        assert!(with_filter.contains("fake-ip-filter:"), "fake-ip-filter section must exist:\n{with_filter}");
        assert!(with_filter.contains("'*.lan'") || with_filter.contains("*.lan"), "existing user fake-ip-filter entries must be preserved");
        assert!(with_filter.contains("steampowered.com"), "gaming domain must be added to fake-ip-filter:\n{with_filter}");

        // При выключении флага (или самого игрового режима) добавленные игровые домены удаляются, а пользовательские остаются
        cfg.exclude_fake_ip_for_game_domains = false;
        let without_filter = apply_gaming_fake_ip_filter(&with_filter, &cfg, false);
        assert!(without_filter.contains("*.lan"), "user fake-ip-filter entries must stay after disabling");
        assert!(!without_filter.contains("steampowered.com"), "gaming domains must be removed when flag is false:\n{without_filter}");
    }

    #[test]
    fn regression_bug_23b_flow_rules_do_not_override_user_rules() {
        // 1. google.com и play.google.com не должны быть в FLOW_DOMAINS
        assert!(!FLOW_DOMAINS.contains(&"google.com"), "google.com must not be in FLOW_DOMAINS");
        assert!(!FLOW_DOMAINS.contains(&"play.google.com"), "play.google.com must not be in FLOW_DOMAINS");

        // 2. Порядок AdBlock -> DIRECT/FORCE -> Flow (без БС)
        let base_yaml = "proxy-groups:\n  - name: PROXY\n    type: select\n  - name: 🎯 Google Flow\n    type: select\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.adblock_enabled = true;
        cfg.direct_domains = vec!["gemini.google.com".into()];
        cfg.force_domains = vec!["my-force.example".into()];
        cfg.flow_server = Some("US-Flow".into());

        let (applied, _) = apply_routing(base_yaml, &cfg).expect("apply_routing must succeed");
        let ab_pos = applied.find(ADBLOCK_BEGIN).expect("ADBLOCK_BEGIN");
        let dir_pos = applied.find("DOMAIN-SUFFIX,gemini.google.com,DIRECT").expect("user DIRECT");
        let frc_pos = applied.find("DOMAIN-SUFFIX,my-force.example,").expect("user FORCE");
        let ai_pos = applied.find(GOOGLE_AI_BEGIN).expect("GOOGLE_AI_BEGIN");

        assert!(ab_pos < ai_pos, "AdBlock must come before Flow:\n{applied}");
        assert!(dir_pos < ai_pos, "user DIRECT must come before Flow:\n{applied}");
        assert!(frc_pos < ai_pos, "user FORCE must come before Flow:\n{applied}");

        // 3. Zapret safeguard должен использовать flow_server/Flow-группу вместо PROXY
        let mut zcfg = crate::config::ZapretConfig::default();
        zcfg.enabled = true;
        zcfg.hybrid_youtube = true;
        let with_zapret = apply_zapret_hybrid_rules_with_force(base_yaml, &zcfg, &[]).unwrap();
        assert!(
            with_zapret.contains("DOMAIN-SUFFIX,gemini.google.com,🎯 Google Flow"),
            "Zapret safeguard must route FLOW_DOMAINS to Flow group, not PROXY:\n{with_zapret}"
        );
    }
}








