#!/bin/sh
# XKeen Route — установочный скрипт для Entware (Keenetic/Netcraze)
# Установка одной командой:
#   curl -Ls https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh
# Бета:      ... | sh -s -- beta
# Удаление:  ... | sh -s -- uninstall

GREEN=$'\033[32m'
RED=$'\033[31m'
NC=$'\033[0m'

BIN="/opt/sbin/xkeen-route"
CONF_DIR="/opt/etc/xkeen-route"
INIT="/opt/etc/init.d/S99xkeen-route"

REPO="nickitafedorov2012-code/xkeen-ui-ext"

BETA=false
ACTION="install"
case "$1" in
  beta) BETA=true ;;
  uninstall) ACTION="uninstall" ;;
esac

msg() { printf "%b\n" "$1"; }

get_arch() {
  case "$(opkg print-architecture)" in
    *aarch64*) echo 'arm64-v8a' ;;
    *mipsel*)  echo 'mips32le' ;;
    *mips*)    echo 'mips32' ;;
    *) msg "${RED} ❌ Не удалось определить архитектуру${NC}"; exit 1 ;;
  esac
}

download_url() {
  if [ "$BETA" = true ]; then
    curl -s https://api.github.com/repos/$REPO/releases | \
      grep -m1 '"browser_download_url".*xkeen-route-'"$ARCH" | cut -d '"' -f4
  else
    echo "https://github.com/$REPO/releases/latest/download/xkeen-route-$ARCH"
  fi
}

do_install() {
  ARCH=$(get_arch) || return 1
  msg "${GREEN}ℹ️ Архитектура: $ARCH${NC}"

  DOWNLOAD_URL=$(download_url)
  [ -z "$DOWNLOAD_URL" ] && { msg "${RED} ❌ Не удалось определить ссылку загрузки${NC}"; return 1; }

  msg "${GREEN}⬇️ Загрузка бинарника...${NC}"
  curl -Lso "$BIN.tmp" "$DOWNLOAD_URL" && chmod +x "$BIN.tmp" && mv "$BIN.tmp" "$BIN" || {
    msg "${RED} ❌ Не удалось загрузить бинарник${NC}"; return 1
  }

  msg "${GREEN}🧩 Init-скрипт...${NC}"
  "$BIN" create-init || { msg "${RED} ❌ Ошибка создания init${NC}"; return 1; }

  mkdir -p "$CONF_DIR"

  msg "${GREEN}🚀 Запуск...${NC}"
  if [ -f "$INIT" ]; then
    sh "$INIT" restart
  else
    "$BIN" -p 1001 &
  fi

  IP=$(ip -4 addr show br0 2>/dev/null | grep -o 'inet [0-9.]*' | cut -d' ' -f2 | cut -d/ -f1)
  msg "${GREEN}✅ XKeen Route установлен: http://${IP:-<router-ip>}:1001${NC}"
  msg "   Порт/настройки: $CONF_DIR/config.json"
  msg "   Управление: sh $INIT start|stop|restart|status"
}

do_uninstall() {
  msg "${GREEN}🛑 Остановка сервиса...${NC}"
  [ -f "$INIT" ] && sh "$INIT" stop 2>/dev/null
  rm -f "$INIT" "$BIN"
  msg "${GREEN}✅ XKeen Route удалён (бинарь и init-скрипт).${NC}"
  msg "   Конфиг сохранён: $CONF_DIR (удалить вручную при необходимости)."
}

# Интерактивное меню — только если запущено с терминала (не через curl | sh).
if [ -t 0 ] && [ "$ACTION" = "install" ] && [ "$BETA" = false ]; then
  echo "1) Установить / обновить"
  echo "2) Удалить"
  printf "Выбор [1]: "
  read -r choice
  [ "$choice" = "2" ] && ACTION="uninstall"
fi

case "$ACTION" in
  uninstall) do_uninstall ;;
  *) do_install ;;
esac
