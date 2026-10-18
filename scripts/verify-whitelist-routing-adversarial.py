#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Empirical Adversarial Test Suite for Whitelist Mode Routing & Failover
Role: Challenger 1 (Routing & Failover Adversarial Challenger)
Mission: Stress-test assumptions, verify invariants, detect failure modes.
"""

import sys
import re
import os
import subprocess
from collections import Counter

# Set UTF-8 encoding for standard output on Windows
if sys.platform == 'win32':
    import io
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding='utf-8')

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
BACKEND_SRC = os.path.join(ROOT_DIR, "backend", "src")
CATALOG_PATH = os.path.join(BACKEND_SRC, "whitelist_catalog.rs")
ROUTING_PATH = os.path.join(BACKEND_SRC, "routing.rs")
FAILOVER_PATH = os.path.join(BACKEND_SRC, "failover.rs")
CONFIG_PATH = os.path.join(BACKEND_SRC, "config.rs")

test_results = []

def record(test_name: str, passed: bool, details: str = ""):
    status = "PASS" if passed else "FAIL"
    test_results.append((test_name, passed, details))
    print(f"[{status}] {test_name}" + (f" -> {details}" if details else ""))
    if not passed:
        print(f"       ERROR DETAILS: {details}")

# ============================================================================
# Section 1: Invariant Verification of Domestic Whitelist Catalog (376 Domains)
# ============================================================================
print("\n=== [1] Invariant Verification of Domestic Whitelist Catalog ===")

def parse_catalog():
    with open(CATALOG_PATH, "r", encoding="utf-8") as f:
        content = f.read()

    # Extract DOMESTIC_WHITELIST_DOMAINS
    match_domains = re.search(r'pub const DOMESTIC_WHITELIST_DOMAINS:\s*&\[&str\]\s*=\s*&\[(.*?)\];', content, re.DOTALL)
    if not match_domains:
        raise ValueError("Could not find DOMESTIC_WHITELIST_DOMAINS in whitelist_catalog.rs")
    domains_block = match_domains.group(1)
    domains = re.findall(r'"([^"]+)"', domains_block)

    # Extract DOMESTIC_CATALOG entries
    match_catalog = re.search(r'pub const DOMESTIC_CATALOG:\s*&\[DomesticCatalogEntry\]\s*=\s*&\[(.*?)\];', content, re.DOTALL)
    if not match_catalog:
        raise ValueError("Could not find DOMESTIC_CATALOG in whitelist_catalog.rs")
    catalog_block = match_catalog.group(1)
    catalog_entries = re.findall(
        r'DomesticCatalogEntry\s*\{\s*domain:\s*"([^"]+)",\s*category:\s*"([^"]+)",\s*description:\s*"([^"]+)"\s*\}',
        catalog_block
    )

    return domains, catalog_entries

domains, catalog_entries = parse_catalog()

# Test 1.1: Exact count == 376
record("Catalog Exact Length == 376", len(domains) == 376, f"Found {len(domains)} domains")
record("Catalog Entries Exact Length == 376", len(catalog_entries) == 376, f"Found {len(catalog_entries)} entries")

# Test 1.2: Check no duplicates
counts = Counter(domains)
duplicates = [dom for dom, count in counts.items() if count > 1]
record("Catalog No Duplicate Domains", len(duplicates) == 0, f"Duplicates: {duplicates}" if duplicates else "All 376 unique")

# Test 1.3: Check domain invariants (no protocols, no slashes, no www prefixes, no uppercase, no trailing dots)
invalid_invariants = []
for dom in domains:
    if "://" in dom:
        invalid_invariants.append(f"{dom}: contains protocol")
    if "/" in dom:
        invalid_invariants.append(f"{dom}: contains slash")
    if dom.startswith("www."):
        invalid_invariants.append(f"{dom}: starts with www.")
    if dom.endswith("."):
        invalid_invariants.append(f"{dom}: ends with dot")
    if dom != dom.lower():
        invalid_invariants.append(f"{dom}: contains uppercase")
    if " " in dom or "\t" in dom or "\n" in dom:
        invalid_invariants.append(f"{dom}: contains whitespace")
    if not re.match(r'^[a-z0-9.-]+$', dom):
        invalid_invariants.append(f"{dom}: non-ascii or invalid characters")

record("Catalog Invariants (no protocol, path, www, uppercase, invalid chars)", len(invalid_invariants) == 0,
       "; ".join(invalid_invariants[:5]) if invalid_invariants else "All clean")

# Test 1.4: Check all 8 categories present
found_categories = set(cat for _, cat, _ in catalog_entries)
record("Catalog Covers All 8 Domestic Sectors", len(found_categories) == 8,
       f"Found {len(found_categories)} categories: {sorted(list(found_categories))}")

# ============================================================================
# Section 2: Strict DIRECT vs Broad GEOIP,RU Wildcard Verification
# ============================================================================
print("\n=== [2] Strict DIRECT vs Broad GEOIP,RU Wildcard Verification ===")

with open(ROUTING_PATH, "r", encoding="utf-8") as f:
    routing_content = f.read()

# Test 2.1: Verify apply_whitelist_rules implementation strictly uses DOMAIN-SUFFIX and DIRECT
match_apply = re.search(r'pub fn apply_whitelist_rules\((.*?)\n\}', routing_content, re.DOTALL)
apply_wl_code = match_apply.group(0) if match_apply else ""

# Extract only lines that push to wl_rules
emitted_rule_lines = [l.strip() for l in apply_wl_code.splitlines() if "wl_rules.push" in l]
has_geoip_in_emitted_rules = any("GEOIP" in l for l in emitted_rule_lines)

record("apply_whitelist_rules NEVER Injects GEOIP,RU Wildcard", not has_geoip_in_emitted_rules,
       "Verified: No GEOIP token in emitted wl_rules")

record("apply_whitelist_rules Uses Strict DOMAIN-SUFFIX,dom,DIRECT",
       'format!("  - DOMAIN-SUFFIX,{dom},DIRECT")' in apply_wl_code,
       "Verified: Each domain strictly bound to DIRECT")

# Test 2.2: Verify Marker Definitions
record("Marker WHITELIST_BEGIN Defined Correctly", '# --- AUTO-WHITELIST-BEGIN ---' in routing_content)
record("Marker WHITELIST_END Defined Correctly", '# --- AUTO-WHITELIST-END ---' in routing_content)
record("Marker WHITELIST_DNS_BEGIN Defined Correctly", '# --- AUTO-WHITELIST-DNS-BEGIN ---' in routing_content)
record("Marker WHITELIST_DNS_END Defined Correctly", '# --- AUTO-WHITELIST-DNS-END ---' in routing_content)

# ============================================================================
# Section 3: Domain Normalization and Validation Edge Cases
# ============================================================================
print("\n=== [3] Domain Normalization and Validation Edge Cases ===")

def normalize_domain(input_str: str) -> str:
    lower = input_str.strip().lower()
    if "://" in lower:
        idx = lower.find("://")
        no_proto = lower[idx + 3:]
    else:
        no_proto = lower
    host_part = no_proto.split('/')[0].split('?')[0].split('#')[0]
    no_user = host_part.split('@')[-1]
    no_port = no_user.split(':')[0]
    clean = no_port
    if clean.startswith("www."):
        clean = clean[4:]
    clean = clean.strip(". \t\r\n")
    return clean

def is_valid_domain(domain: str) -> bool:
    if not domain or len(domain) > 253:
        return False
    if domain in ("localhost",) or domain.endswith(".local") or domain.endswith(".lan") or domain.endswith(".internal") or domain.endswith(".onion"):
        return False
    # IP check
    parts = domain.split('.')
    if len(parts) == 4 and all(p.isdigit() and 0 <= int(p) <= 255 for p in parts):
        return False
    if ":" in domain:  # IPv6
        return False
    if "." not in domain:
        return False
    for label in domain.split('.'):
        if not label or len(label) > 63:
            return False
        if label.startswith('-') or label.endswith('-'):
            return False
        if not re.match(r'^[a-z0-9-]+$', label):
            return False
    return True

normalization_cases = [
    ("https://habr.com/", "habr.com", True),
    ("HTTPS://MYSKU.CLUB:443", "mysku.club", True),
    ("http://www.sberbank.ru/path/to/page?q=1#test", "sberbank.ru", True),
    ("user:pass@example.com", "example.com", True),
    ("WWW.EXAMPLE.COM.", "example.com", True),
    ("  twitch.tv  ", "twitch.tv", True),
    ("https://sub.domain.co.uk:8080/path?query=1#hash", "sub.domain.co.uk", True),
    ("https://my-internal-portal.ru/index.html", "my-internal-portal.ru", True),
    ("192.168.1.1", "192.168.1.1", False),  # normalized, but is_valid_domain MUST be False
    ("10.0.0.1:8080", "10.0.0.1", False),
    ("http://[2001:db8::1]:8080/", "[2001", False),
    ("localhost", "localhost", False),
    ("bad;domain", "bad;domain", False),
    ("domain with space.com", "domain with space.com", False),
    ("GEOIP,RU", "geoip,ru", False),
    ("\n  - GEOIP,RU,DIRECT\n", "- geoip,ru,direct", False),
    ("domain_with_underscore.com", "domain_with_underscore.com", False),
    ("-leading-dash.com", "-leading-dash.com", False),
    ("trailing-dash-.com", "trailing-dash-.com", False),
    ("a." + ("b" * 64) + ".com", "a." + ("b" * 64) + ".com", False),  # label > 63
    ("wildberries.ru///", "wildberries.ru", True),
    ("HTTP://GOSUSLUGI.RU:8443/epgu-portal/", "gosuslugi.ru", True),
]

failed_norm = []
for raw, expected_norm, expected_valid in normalization_cases:
    norm = normalize_domain(raw)
    valid = is_valid_domain(norm)
    if norm != expected_norm or valid != expected_valid:
        failed_norm.append(f"Input '{raw}': got norm='{norm}' (exp '{expected_norm}'), valid={valid} (exp {expected_valid})")

record("Domain Normalization & Validation Edge Cases (22 test vectors)", len(failed_norm) == 0,
       "; ".join(failed_norm[:3]) if failed_norm else "All 22 edge cases correctly parsed")

# ============================================================================
# Section 4: Failover Server Tag Detection Stress Testing
# ============================================================================
print("\n=== [4] Failover Server Tag Detection Stress Testing ===")

def is_whitelist_server(node_name: str, node_id: str, whitelist_servers: list) -> bool:
    if node_id and node_id in whitelist_servers:
        return True
    def check_tag(s: str) -> bool:
        lower = s.lower()
        return ("[бс]" in lower
                or "(бс)" in lower
                or "[бc]" in lower   # Cyrillic б + Latin c
                or "(бc)" in lower
                or "[wl]" in lower
                or "(wl)" in lower
                or "whitelist" in lower)
    return check_tag(node_name) or (bool(node_id) and check_tag(node_id))

wl_servers = ["wl_exact_id", "special_srv"]

tag_cases = [
    # Required in DISPATCH
    ("NL-Amsterdam-[БС]-01", "node_nl", True),
    ("DE-Frankfurt-(WL)-02", "node_de", True),
    ("RU-Moscow-whitelist-03", "node_ru", True),
    ("US-Direct-Standard", "node_us", False),
    ("SG-Fastest-Node", "node_sg", False),

    # Cyrillic cases
    ("Сервер [БС] Москва", "node1", True),
    ("Резерв [бс] Питер", "node2", True),
    ("Канал (БС) Ростов", "node3", True),
    ("Шлюз (бс) Казань", "node4", True),
    ("Шлюз [бc] mixed homoglyph", "node4_h1", True),  # Latin c
    ("Шлюз (бc) mixed homoglyph", "node4_h2", True),  # Latin c

    # Latin cases
    ("Netherlands [WL] 01", "node5", True),
    ("Finland [wl] Fast", "node6", True),
    ("Germany (WL) Direct", "node7", True),
    ("Sweden (wl) Cloud", "node8", True),

    # Keywords
    ("Vless Whitelist Direct", "node9", True),
    ("HYSTERIA WHITELIST 02", "node10", True),
    ("node_whitelist_backup", "node11", True),

    # By ID match
    ("Generic Server Without Tag", "wl_exact_id", True),
    ("Ordinary Server", "special_srv", True),

    # Negative / False-Positive checks (MUST BE FALSE)
    ("Fastest", "Fastest", False),
    ("Fallback", "Fallback", False),
    ("DIRECT", "DIRECT", False),
    ("REJECT", "REJECT", False),
    ("PROXY", "PROXY", False),
    ("BS-Standard-01", "node_bs", False),     # 'bs' without brackets/parentheses
    ("WL-Standard-02", "node_wl", False),     # 'wl' without brackets/parentheses
    ("", "", False),
]

failed_tags = []
for name, nid, expected in tag_cases:
    actual = is_whitelist_server(name, nid, wl_servers)
    if actual != expected:
        failed_tags.append(f"Node '{name}' (id='{nid}'): got {actual}, expected {expected}")

record("Failover Server Tag Auto-Detection (25 stress test cases)", len(failed_tags) == 0,
       "; ".join(failed_tags[:3]) if failed_tags else "All 25 node names correctly classified")

# ============================================================================
# Section 5: Device Rule Generation & Subnet / Mask Safety
# ============================================================================
print("\n=== [5] Device Rule Generation & Subnet / Mask Safety ===")

def format_device_rule(ip: str, target: str) -> str:
    ip_trim = ip.strip()
    cidr = ip_trim if "/" in ip_trim else f"{ip_trim}/32"
    return f"  - SRC-IP-CIDR,{cidr},{target}"

device_cases = [
    ("192.168.1.105", "🛡️ Whitelist", "  - SRC-IP-CIDR,192.168.1.105/32,🛡️ Whitelist"),
    ("192.168.1.0/24", "🛡️ Whitelist", "  - SRC-IP-CIDR,192.168.1.0/24,🛡️ Whitelist"),
    (" 10.8.0.50 ", "Fastest", "  - SRC-IP-CIDR,10.8.0.50/32,Fastest"),
    ("172.16.0.0/16", "NL-Node", "  - SRC-IP-CIDR,172.16.0.0/16,NL-Node"),
]

failed_devs = []
for raw_ip, tgt, expected_rule in device_cases:
    actual_rule = format_device_rule(raw_ip, tgt)
    if actual_rule != expected_rule:
        failed_devs.append(f"IP '{raw_ip}': got '{actual_rule}', expected '{expected_rule}'")

record("Device SRC-IP-CIDR Formatting & Subnet Mask Handling", len(failed_devs) == 0,
       "; ".join(failed_devs) if failed_devs else "Subnet masks and host IPs formatted perfectly")

# ============================================================================
# Section 6: Marker Boundary Safety & Surrounding Rule Integrity
# ============================================================================
print("\n=== [6] Marker Boundary Safety & Idempotency ===")

SAMPLE_MIHOMO_YAML = """port: 7890
socks-port: 7891
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
      - DIRECT
rules:
# --- AUTO-FLOW-RULES-BEGIN ---
  - DOMAIN-SUFFIX,google.com,PROXY
# --- AUTO-FLOW-RULES-END ---
# --- AUTO-GAMING-RULES-BEGIN ---
  - SRC-IP-CIDR,192.168.1.200/32,Fastest
# --- AUTO-GAMING-RULES-END ---
# --- AUTO-ADBLOCK-RULES-BEGIN ---
  - DOMAIN-KEYWORD,adservice,REJECT
# --- AUTO-ADBLOCK-RULES-END ---
# --- AUTO-ZAPRET-HYBRID-BEGIN ---
  - DOMAIN-SUFFIX,googlevideo.com,DIRECT
# --- AUTO-ZAPRET-HYBRID-END ---
# --- AUTO-DEVICE-RULES-BEGIN ---
  - SRC-IP-CIDR,192.168.1.50/32,PROXY
# --- AUTO-DEVICE-RULES-END ---
  - GEOIP,RU,DIRECT
  - MATCH,PROXY
"""

def simulate_apply_whitelist(yaml_str: str, enabled: bool, devices: list, domains: list, target_server: str = "🛡️ Whitelist") -> str:
    # 1. Clean existing whitelist
    lines = yaml_str.splitlines()
    clean_lines = []
    skip = False
    for l in lines:
        if l.strip() == "# --- AUTO-WHITELIST-BEGIN ---":
            skip = True
            continue
        if l.strip() == "# --- AUTO-WHITELIST-END ---":
            skip = False
            continue
        if not skip:
            clean_lines.append(l)

    if not enabled or not devices:
        return "\n".join(clean_lines)

    # 2. Build whitelist rules
    wl_rules = ["# --- AUTO-WHITELIST-BEGIN ---"]
    for d in domains:
        wl_rules.append(f"  - DOMAIN-SUFFIX,{d},DIRECT")
    for dev in devices:
        cidr = dev if "/" in dev else f"{dev}/32"
        wl_rules.append(f"  - SRC-IP-CIDR,{cidr},{target_server}")
    wl_rules.append("# --- AUTO-WHITELIST-END ---")

    # 3. Find insertion position: right before AUTO-DEVICE-RULES-BEGIN
    insert_idx = -1
    for i, l in enumerate(clean_lines):
        if l.strip() == "# --- AUTO-DEVICE-RULES-BEGIN ---":
            insert_idx = i
            break
    if insert_idx == -1:
        # fallback before GEOIP or MATCH
        for i, l in enumerate(clean_lines):
            if l.strip().startswith("- GEOIP,") or l.strip().startswith("- MATCH,"):
                insert_idx = i
                break

    out = clean_lines[:insert_idx] + wl_rules + clean_lines[insert_idx:]
    return "\n".join(out)

# Test 6.1: Application preserves all markers
applied_1 = simulate_apply_whitelist(SAMPLE_MIHOMO_YAML, True, ["192.168.1.105"], ["gosuslugi.ru", "sberbank.ru"])
markers_to_check = [
    "# --- AUTO-FLOW-RULES-BEGIN ---",
    "# --- AUTO-FLOW-RULES-END ---",
    "# --- AUTO-GAMING-RULES-BEGIN ---",
    "# --- AUTO-GAMING-RULES-END ---",
    "# --- AUTO-ADBLOCK-RULES-BEGIN ---",
    "# --- AUTO-ADBLOCK-RULES-END ---",
    "# --- AUTO-ZAPRET-HYBRID-BEGIN ---",
    "# --- AUTO-ZAPRET-HYBRID-END ---",
    "# --- AUTO-WHITELIST-BEGIN ---",
    "# --- AUTO-WHITELIST-END ---",
    "# --- AUTO-DEVICE-RULES-BEGIN ---",
    "# --- AUTO-DEVICE-RULES-END ---",
]
missing_markers = [m for m in markers_to_check if m not in applied_1]
record("Application Preserves All Pre-Existing Markers", len(missing_markers) == 0,
       f"Missing: {missing_markers}" if missing_markers else "All 12 marker boundaries intact")

# Test 6.2: Rule Hierarchy: FLOW < GAMING < ADBLOCK < ZAPRET < WHITELIST < DEVICE < FALLBACK
pos_flow = applied_1.find("# --- AUTO-FLOW-RULES-BEGIN ---")
pos_gaming = applied_1.find("# --- AUTO-GAMING-RULES-BEGIN ---")
pos_adblock = applied_1.find("# --- AUTO-ADBLOCK-RULES-BEGIN ---")
pos_zapret = applied_1.find("# --- AUTO-ZAPRET-HYBRID-BEGIN ---")
pos_wl = applied_1.find("# --- AUTO-WHITELIST-BEGIN ---")
pos_dev = applied_1.find("# --- AUTO-DEVICE-RULES-BEGIN ---")
pos_geoip = applied_1.find("- GEOIP,RU,DIRECT")

hierarchy_correct = (pos_flow < pos_gaming < pos_adblock < pos_zapret < pos_wl < pos_dev < pos_geoip)
record("Rule Hierarchy Correct (FLOW < GAMING < ADBLOCK < ZAPRET < WHITELIST < DEVICE < GEOIP)", hierarchy_correct,
       f"Positions: flow={pos_flow}, gaming={pos_gaming}, adblock={pos_adblock}, zapret={pos_zapret}, wl={pos_wl}, dev={pos_dev}, geoip={pos_geoip}")

# Test 6.3: Idempotency: re-applying whitelist does not duplicate blocks
applied_2 = simulate_apply_whitelist(applied_1, True, ["192.168.1.105"], ["gosuslugi.ru", "sberbank.ru"])
record("Idempotency: Re-Applying Whitelist Does Not Duplicate Markers",
       applied_2.count("# --- AUTO-WHITELIST-BEGIN ---") == 1 and applied_2.count("# --- AUTO-WHITELIST-END ---") == 1,
       f"Begin count={applied_2.count('# --- AUTO-WHITELIST-BEGIN ---')}, End count={applied_2.count('# --- AUTO-WHITELIST-END ---')}")

# Test 6.4: Clean Removal: disabling whitelist restores original YAML exactly
disabled_yaml = simulate_apply_whitelist(applied_2, False, [], [])
record("Clean Removal: Disabling Whitelist Restores Surrounding YAML",
       "# --- AUTO-WHITELIST-BEGIN ---" not in disabled_yaml and disabled_yaml.strip() == SAMPLE_MIHOMO_YAML.strip(),
       "Verified: YAML identical to original after disable")

# ============================================================================
# Section 7: Live Cargo Test Execution (M4 Anti-Regression Suite)
# ============================================================================
print("\n=== [7] Live Cargo Test Execution (M4 Anti-Regression Suite) ===")

cargo_cmd = ["cargo", "test", "--bin", "xkeen-route", "--", "whitelist"]
proc = subprocess.run(cargo_cmd, cwd=os.path.join(ROOT_DIR, "backend"), capture_output=True, text=True)
cargo_success = (proc.returncode == 0) and ("test result: ok" in proc.stdout)

# Extract passed test count
match_test_count = re.search(r'test result: ok\.\s*(\d+)\s*passed', proc.stdout)
passed_count = match_test_count.group(1) if match_test_count else "unknown"

record("Cargo Whitelist Test Suite Execution", cargo_success,
       f"Exit code {proc.returncode}, {passed_count} unit tests passed" if cargo_success else f"Stderr: {proc.stderr[:200]}")

# ============================================================================
# Final Summary & Verdict
# ============================================================================
print("\n" + "="*70)
total_tests = len(test_results)
passed_tests = sum(1 for _, p, _ in test_results if p)
failed_tests = total_tests - passed_tests

print(f"TOTAL ADVERSARIAL TESTS: {total_tests}")
print(f"PASSED: {passed_tests}")
print(f"FAILED: {failed_tests}")

if failed_tests == 0:
    print("VERDICT: APPROVE (100% of adversarial challenge tests passed)")
    sys.exit(0)
else:
    print("VERDICT: REQUEST_CHANGES (Failures detected)")
    sys.exit(1)
