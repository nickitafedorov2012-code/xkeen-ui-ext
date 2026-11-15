#!/bin/sh
# Dead-Man's Switch для роутера Keenetic (BusyBox ash / Entware):
# Если за заданное время (по умолчанию 60 сек) не получен сигнал отмены (/tmp/xkeen_deadman.cancel),
# автоматически переключает панель XKeen Route обратно на предыдущий рабочий сервер по localhost.
#
# Использование:
#   sh /tmp/router-deadman-switch.sh "<PREV_SERVER_ID>" [TIMEOUT_SEC] [API_PORT]

set -u

PREV_SERVER="${1:-}"
TIMEOUT_SEC="${2:-60}"
API_PORT="${3:-1001}"

PID_FILE="/tmp/xkeen_deadman.pid"
CANCEL_FILE="/tmp/xkeen_deadman.cancel"
LOG_FILE="/tmp/xkeen_deadman.log"

if [ -z "$PREV_SERVER" ]; then
    echo "Usage: $0 <PREV_SERVER_ID> [TIMEOUT_SEC] [API_PORT]" >&2
    exit 2
fi

# Снимаем возможный предыдущий таймер
if [ -f "$PID_FILE" ]; then
    OLD_PID="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$OLD_PID" ] && [ "$OLD_PID" != "$$" ]; then
        kill "$OLD_PID" 2>/dev/null || true
    fi
fi

rm -f "$CANCEL_FILE"
echo "$$" > "$PID_FILE"
echo "$(date '+%Y-%m-%d %H:%M:%S') [DEADMAN] Armed for ${TIMEOUT_SEC}s; fallback server: ${PREV_SERVER}" > "$LOG_FILE"

ELAPSED=0
while [ "$ELAPSED" -lt "$TIMEOUT_SEC" ]; do
    if [ -f "$CANCEL_FILE" ]; then
        echo "$(date '+%Y-%m-%d %H:%M:%S') [DEADMAN] Disarmed cleanly at ${ELAPSED}s" >> "$LOG_FILE"
        rm -f "$PID_FILE" "$CANCEL_FILE"
        exit 0
    fi
    sleep 2
    ELAPSED=$((ELAPSED + 2))
done

if [ -f "$CANCEL_FILE" ]; then
    echo "$(date '+%Y-%m-%d %H:%M:%S') [DEADMAN] Disarmed at deadline" >> "$LOG_FILE"
    rm -f "$PID_FILE" "$CANCEL_FILE"
    exit 0
fi

# Экранируем кавычки в имени сервера для JSON
ESCAPED_SERVER=$(printf '%s' "$PREV_SERVER" | sed 's/\\/\\\\/g; s/"/\\"/g')
PAYLOAD="{\"server_id\":\"${ESCAPED_SERVER}\"}"
URL="http://127.0.0.1:${API_PORT}/api/servers/switch"

echo "$(date '+%Y-%m-%d %H:%M:%S') [DEADMAN] Timeout reached! Rolling back to ${PREV_SERVER}" >> "$LOG_FILE"

if command -v curl >/dev/null 2>&1; then
    curl -sS -m 10 -X POST "$URL" -H "Content-Type: application/json" -d "$PAYLOAD" >> "$LOG_FILE" 2>&1 || true
elif command -v wget >/dev/null 2>&1; then
    wget -q -T 10 -O - --post-data="$PAYLOAD" --header="Content-Type: application/json" "$URL" >> "$LOG_FILE" 2>&1 || true
else
    echo "$(date '+%Y-%m-%d %H:%M:%S') [DEADMAN] ERROR: neither curl nor wget found" >> "$LOG_FILE"
fi

rm -f "$PID_FILE" "$CANCEL_FILE"
exit 0
