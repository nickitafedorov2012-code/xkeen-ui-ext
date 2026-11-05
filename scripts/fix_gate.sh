#!/usr/bin/env bash
# Контрольный шлюз для исправления багов из docs/BUG_AUDIT.md.
# Единственный легальный способ получить статус FIXED в docs/FIX_LEDGER.md.
# Логи пишутся ТОЛЬКО этим скриптом в docs/fix_logs/. Руками их создавать и править запрещено.
#
#   bash scripts/fix_gate.sh init                  — один раз: запомнить исходное состояние
#   bash scripts/fix_gate.sh rules                 — перечитать правила (печатает их и хеш)
#   bash scripts/fix_gate.sh red   <N> be:<фильтр> — тест ДО исправления, обязан упасть
#   bash scripts/fix_gate.sh red   <N> fe:<файл>[::<имя теста>]
#   bash scripts/fix_gate.sh already <N> be:<фильтр> — баг УЖЕ исправлен незакоммиченным пакетом 2:
#                                                    тест должен пройти; нужна цитата git diff в ledger
#   bash scripts/fix_gate.sh green <N> be:<фильтр> — после исправления: целевой тест + все наборы
#   bash scripts/fix_gate.sh status                — сводка по логам
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
LOGS="docs/fix_logs"
mkdir -p "$LOGS"
NOW="$(date '+%Y-%m-%d %H:%M:%S')"

BANNED='#\[ignore|\.skip\(|\.only\(|it\.todo|test\.todo|todo!\(|unimplemented!\(|assert!\(true|assert_eq!\(true, *true|toBe\(true\)\.toBe|expect\(true\)|#\[allow\(|#!\[allow\(|@ts-ignore|@ts-nocheck|@ts-expect-error|eslint-disable|as any\b|--no-verify'

rules_hash() { sha256sum docs/FIX_RULES.md | cut -c1-12; }
diff_hash()  { { git diff; git ls-files --others --exclude-standard | grep -v "^$LOGS/" | sort | xargs -r -d '\n' cat 2>/dev/null; } | sha256sum | cut -c1-12; }

count_tests() {
  local be fe
  be=$(grep -rhoE '#\[(tokio::)?test' backend/src backend/tests 2>/dev/null | wc -l)
  fe=$(grep -rhoE '^\s*(it|test)\(' frontend/src --include='*.test.ts' --include='*.test.tsx' 2>/dev/null | wc -l)
  echo "$be $fe"
}

banned_hits() {
  {
    git diff -U0 -- backend frontend scripts | grep -E '^\+' | grep -vE '^\+\+\+' | grep -E "$BANNED"
    git ls-files --others --exclude-standard -- backend frontend | grep -vE 'node_modules|target/' | while IFS= read -r f; do
      grep -HnE "$BANNED" "$f" 2>/dev/null
    done
  } | sort -u
}

run_target() { # $1 = be:<filter> | fe:<file>[::<name>]
  local spec="$1"
  case "$spec" in
    be:*)
      (cd backend && cargo test "${spec#be:}" 2>&1) ;;
    fe:*)
      local rest="${spec#fe:}" file name
      file="${rest%%::*}"; name=""
      [ "$rest" != "$file" ] && name="${rest#*::}"
      if [ -n "$name" ]; then (cd frontend && npx vitest run "$file" -t "$name" 2>&1)
      else (cd frontend && npx vitest run "$file" 2>&1); fi ;;
    *) echo "Спецификация теста должна начинаться с be: или fe:"; return 2 ;;
  esac
}

# Если фильтр не нашёл ни одного теста — это не RED и не GREEN, это обман.
ran_tests()     { grep -qE '[1-9][0-9]* (passed|failed)' "$1"; }
compile_error() { grep -qE 'could not compile|error\[E[0-9]+\]|Transform failed|SyntaxError|Failed to load url' "$1"; }

cmd="${1:-}"
case "$cmd" in
  init)
    if [ -f "$LOGS/baseline.txt" ]; then echo "baseline уже есть, повторный init запрещён"; exit 1; fi
    {
      echo "created: $NOW"
      echo "tests(be fe): $(count_tests)"
      echo "rules: $(rules_hash)"
      echo "gate: $(sha256sum scripts/fix_gate.sh | cut -c1-12)"
    } > "$LOGS/baseline.txt"
    banned_hits > "$LOGS/baseline_banned.txt"
    git diff --stat > "$LOGS/baseline_diffstat.txt"
    cat "$LOGS/baseline.txt"; echo "OK: исходное состояние записано" ;;

  rules)
    cat docs/FIX_RULES.md
    echo
    echo "=== ПРАВИЛА ПЕРЕЧИТАНЫ: $NOW | хеш $(rules_hash) ==="
    echo "$NOW rules $(rules_hash)" >> "$LOGS/rules_reads.log" ;;

  red)
    N="${2:?номер бага}"; SPEC="${3:?be:<фильтр> или fe:<файл>}"
    LOG="$LOGS/bug-$N-red.log"
    { echo "# RED bug $N | $NOW | spec $SPEC | diff $(diff_hash)"; run_target "$SPEC"; echo "# exit=$?"; } > "$LOG" 2>&1
    tail -40 "$LOG"
    if grep -q '^# exit=0$' "$LOG"; then
      echo "!!! RED НЕ ПОЛУЧЕН: тест проходит на старом коде, значит он НЕ ловит баг $N. Переписать тест."; exit 1
    fi
    if compile_error "$LOG"; then
      echo "# red_kind=compile" >> "$LOG"
      echo "=== RED по ОШИБКЕ КОМПИЛЯЦИИ. Допустимо только если тест вызывает новую функцию/поле, которого ещё нет. Объяснить в ledger. ==="
      exit 0
    fi
    if ! ran_tests "$LOG"; then echo "!!! RED НЕДЕЙСТВИТЕЛЕН: фильтр не нашёл ни одного теста"; exit 1; fi
    echo "# red_kind=assert" >> "$LOG"
    echo "=== RED OK bug $N: тест падает на старом коде. Лог: $LOG ===" ;;

  already)
    N="${2:?номер бага}"; SPEC="${3:?be:<фильтр> или fe:<файл>}"
    [ -f "$LOGS/bug-$N-red.log" ] && { echo "!!! для бага $N уже есть red — already запрещён"; exit 1; }
    LOG="$LOGS/bug-$N-already.log"
    { echo "# ALREADY bug $N | $NOW | spec $SPEC | diff $(diff_hash)"; run_target "$SPEC"; echo "# exit=$?"; echo "## git diff --stat"; git diff --stat; } > "$LOG" 2>&1
    tail -30 "$LOG"
    if ! grep -q '^# exit=0$' "$LOG" || ! ran_tests "$LOG"; then rm -f "$LOG"; echo "!!! тест не прошёл или не найден — баг НЕ исправлен, делай обычный red"; exit 1; fi
    echo "=== ALREADY записан для bug $N. В ledger ОБЯЗАТЕЛЬНО hunk из git diff, который это исправляет. ===" ;;

  green)
    N="${2:?номер бага}"; SPEC="${3:?be:<фильтр> или fe:<файл>}"
    [ -f "$LOGS/baseline.txt" ] || { echo "сначала: bash scripts/fix_gate.sh init"; exit 1; }
    if [ -f "$LOGS/bug-$N-red.log" ]; then
      grep -q '^# red_kind=' "$LOGS/bug-$N-red.log" || { echo "!!! RED-лог бага $N недействителен (тест не упал). Сначала нормальный red."; exit 1; }
    elif [ ! -f "$LOGS/bug-$N-already.log" ]; then
      echo "!!! нет RED-лога для бага $N. Сначала red."; exit 1
    fi
    LOG="$LOGS/bug-$N-green.log"; FAIL=0
    if [ "$(sed -n 's/^gate: //p' "$LOGS/baseline.txt")" != "$(sha256sum scripts/fix_gate.sh | cut -c1-12)" ]        || [ "$(sed -n 's/^rules: //p' "$LOGS/baseline.txt")" != "$(rules_hash)" ]; then
      echo "!!! scripts/fix_gate.sh или docs/FIX_RULES.md изменены после init. Это запрещено. Верни как было."; exit 1
    fi
    {
      echo "# GREEN bug $N | $NOW | spec $SPEC | diff $(diff_hash) | rules $(rules_hash)"
      echo "## 1. целевой тест"; run_target "$SPEC"; T=$?; echo "# target_exit=$T"
      echo "## 2. backend: cargo test"; (set -o pipefail; cd backend && cargo test 2>&1 | tail -60); B=$?; echo "# cargo_exit=$B"
      echo "## 3. frontend: vitest"; (set -o pipefail; cd frontend && npx vitest run 2>&1 | tail -40); F=$?; echo "# vitest_exit=$F"
      echo "## 4. frontend: tsc"; (set -o pipefail; cd frontend && npx tsc --noEmit 2>&1 | tail -40); C=$?; echo "# tsc_exit=$C"
      echo "## 5. запрещённые конструкции (новые относительно baseline)"
      banned_hits > "$LOGS/.banned_now.txt"
      NEWB=$(comm -13 "$LOGS/baseline_banned.txt" "$LOGS/.banned_now.txt")
      if [ -n "$NEWB" ]; then echo "$NEWB"; echo "# banned=FAIL"; else echo "нет"; echo "# banned=ok"; fi
      echo "## 6. число тестов (было -> стало)"
      read -r B0 F0 < <(sed -n 's/^tests(be fe): //p' "$LOGS/baseline.txt")
      read -r B1 F1 < <(count_tests)
      echo "backend $B0 -> $B1, frontend $F0 -> $F1"
      if [ "$B1" -lt "$B0" ] || [ "$F1" -lt "$F0" ]; then echo "# tests_count=FAIL (тесты удалены)"; else echo "# tests_count=ok"; fi
      echo "## 7. diffstat"; git diff --stat | tail -30
    } > "$LOG" 2>&1
    grep -qE '^# target_exit=0$' "$LOG" || FAIL=1
    grep -qE '^# cargo_exit=0$'  "$LOG" || FAIL=1
    grep -qE '^# vitest_exit=0$' "$LOG" || FAIL=1
    grep -qE '^# tsc_exit=0$'    "$LOG" || FAIL=1
    grep -q  '^# banned=ok$'     "$LOG" || FAIL=1
    grep -q  '^# tests_count=ok$' "$LOG" || FAIL=1
    sed -n '/## 1. целевой тест/,/target_exit/p' "$LOG" > "$LOGS/.t.txt"
    if ! ran_tests "$LOGS/.t.txt"; then FAIL=1; echo "# target=ZERO_TESTS" >> "$LOG"; fi
    grep -E '^# |^backend .* -> |^нет$' "$LOG"
    if [ $FAIL -ne 0 ]; then echo "!!! GATE FAIL bug $N. Статус FIXED ставить ЗАПРЕЩЕНО. Лог: $LOG"; exit 1; fi
    echo "# GATE OK" >> "$LOG"
    echo "=== GATE OK bug $N | diff $(diff_hash) | лог $LOG ===" ;;

  status)
    for f in "$LOGS"/bug-*-green.log; do
      [ -e "$f" ] || { echo "ещё нет ни одного green"; break; }
      n=$(basename "$f" | sed 's/bug-\(.*\)-green.log/\1/')
      k="red"; [ -f "$LOGS/bug-$n-already.log" ] && k="already"
      if grep -q '^# GATE OK$' "$f"; then echo "bug $n: GATE OK ($k)"; else echo "bug $n: FAIL"; fi
    done
    echo "перечитываний правил: $(wc -l < "$LOGS/rules_reads.log" 2>/dev/null || echo 0)" ;;

  *) sed -n '2,12p' "$0"; exit 2 ;;
esac
