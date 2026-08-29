#!/bin/sh
# XKeen Route — установочный скрипт для Entware (Keenetic/Netcraze)
# Установка:  curl -Lsfo /tmp/xr-setup.sh <URL>/setup.sh && sh /tmp/xr-setup.sh
# Бета:       ... sh /tmp/xr-setup.sh beta

GREEN=$'\033[32m'
RED=$'\033[31m'
NC=$'\033[0m'

BIN="/opt/sbin/xkeen-route"
CONF_DIR="/opt/etc/xkeen-route"
INIT="/opt/etc/init.d/S99xkeen-route"

REPO="nickitafedorov2012-code/xkeen-ui-ext"

BETA=false
[ "$1" = "beta" ] && BETA=true

msg() { printf "%b\n" "$1"; }

get_arch() {
  case "$(opkg print-architecture)" in
    *aarch64*) echo 'arm64-v8a' ;;
    *mipsel*)  echo 'mips32le' ;;
    *mips*)    echo 'mips32' ;;
    *) msg "${RED} ❌ Не удалось определить архитектуру${NC}"; exit 1 ;;
  esac
}

ARCH=$(get_arch)
msg "${GREEN}ℹ️ Архитектура: $ARCH${NC}"

if [ "$BETA" = true ]; then
  DOWNLOAD_URL=$(curl -s https://api.github.com/repos/$REPO/releases | \
    grep -m1 '"browser_download_url".*xkeen-route-'"$ARCH" | cut -d '"' -f4)
else
  DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/xkeen-route-$ARCH"
fi

[ -z "$DOWNLOAD_URL" ] && { msg "${RED} ❌ Не удалось определить ссылку загрузки${NC}"; exit 1; }

msg "${GREEN}⬇️ Загрузка бинарника...${NC}"
curl -Lso "$BIN.tmp" "$DOWNLOAD_URL" && chmod +x "$BIN.tmp" && mv "$BIN.tmp" "$BIN" || {
  msg "${RED} ❌ Не удалось загрузить бинарник${NC}"; exit 1
}

msg "${GREEN}🧩 Init-скрипт...${NC}"
"$BIN" create-init || { msg "${RED} ❌ Ошибка создания init${NC}"; exit 1; }

mkdir -p "$CONF_DIR"

msg "${GREEN}🚀 Запуск...${NC}"
if [ -f "$INIT" ]; then
  sh "$INIT" restart
else
  "$BIN" -p 1001 &
fi

msg "${GREEN}✅ XKeen Route установлен: http://$(ip -4 addr show br0 2>/dev/null | grep -o 'inet [0-9.]*' | cut -d' ' -f2 | cut -d/ -f1):1001${NC}"
msg "   Порт/настройки: $CONF_DIR/config.json"
msg "   Управление: sh $INIT start|stop|restart|status"
