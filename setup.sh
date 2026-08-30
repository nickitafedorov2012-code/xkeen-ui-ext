#!/bin/sh
# XKeen Route — установочный скрипт для Entware (Keenetic/Netcraze)
# Установка одной командой:
#   curl -Ls https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh
# Бета:      ... | sh -s -- beta
# Удаление:  ... | sh -s -- uninstall            (конфиги сохраняются)
# Удаление полностью (с конфигами):
#            ... | sh -s -- uninstall purge

GREEN=$'\033[32m'
RED=$'\033[31m'
NC=$'\033[0m'

BIN="/opt/sbin/xkeen-route"
CONF_DIR="/opt/etc/xkeen-route"
INIT="/opt/etc/init.d/S99xkeen-route"

REPO="nickitafedorov2012-code/xkeen-ui-ext"

BETA=false
ACTION="install"
UNINSTALL_MODE=""
case "$1" in
  beta) BETA=true ;;
  uninstall)
    ACTION="uninstall"
    [ "$2" = "purge" ] && UNINSTALL_MODE="purge"
    ;;
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
  # Через API: первый browser_download_url в списке релизов = самый свежий.
  # (прямой /releases/latest/download ненадёжен: «latest» может не иметь ассета)
  # busybox grep не умеет -m1 — берём head -1.
  curl -s https://api.github.com/repos/$REPO/releases | \
    grep '"browser_download_url".*xkeen-route-'"$ARCH" | head -1 | cut -d '"' -f4
}

do_install() {
  ARCH=$(get_arch) || return 1
  msg "${GREEN}ℹ️ Архитектура: $ARCH${NC}"

  DOWNLOAD_URL=$(download_url)
  [ -z "$DOWNLOAD_URL" ] && { msg "${RED} ❌ Не удалось определить ссылку загрузки${NC}"; return 1; }

  msg "${GREEN}⬇️ Загрузка бинарника...${NC}"
  # Прямая ссылка + зеркала (github может быть недоступен с некоторых сетей).
  # -k: на Entware часто нет CA-бандла (иначе curl отвечает rc=60).
  OK=0
  for P in "" "https://ghproxy.net/" "https://ghfast.top/" "http://ghproxy.net/"; do
    if [ -z "$P" ]; then
      msg "${NC}   пробую github.com (до 20 сек)...${NC}"
      T=20
    else
      msg "${NC}   пробую зеркало ${P} (до 60 сек)...${NC}"
      T=60
    fi
    curl -Lsk --max-time "$T" --connect-timeout 10 "${P}${DOWNLOAD_URL}" -o "$BIN.tmp" </dev/null
    if [ -f "$BIN.tmp" ] && [ "$(wc -c < "$BIN.tmp")" -gt 1000000 ]; then
      OK=1
      break
    fi
    msg "${RED}   не удалось, пробую следующий источник...${NC}"
  done
  [ "$OK" = 1 ] && chmod +x "$BIN.tmp" && mv "$BIN.tmp" "$BIN" || {
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
  PURGE="$1"
  msg "${GREEN}🛑 Остановка сервиса...${NC}"
  [ -f "$INIT" ] && sh "$INIT" stop 2>/dev/null
  rm -f "$INIT" "$BIN"
  if [ "$PURGE" = "purge" ]; then
    rm -rf "$CONF_DIR"
    msg "${GREEN}✅ XKeen Route полностью удалён (бинарь, init-скрипт, конфиги).${NC}"
  else
    msg "${GREEN}✅ XKeen Route удалён (бинарь и init-скрипт).${NC}"
    msg "   Конфиг сохранён: $CONF_DIR (удалить вручную или переустановить с purge)."
  fi
}

# Меню (как в XKeen-UI): баннер + выбор действия.
# Через curl|sh читаем выбор с /dev/tty; если tty нет — ставим без вопросов.

get_status() {
  if [ -x "$BIN" ] && [ -f "$INIT" ]; then
    VER=$("$BIN" version 2>/dev/null | grep -o 'v[0-9.]*' | head -1)
    if sh "$INIT" status 2>/dev/null | grep -q alive; then
      echo "статус: запущена ${VER:-}"
    else
      echo "статус: установлена ${VER:-}, но не запущена"
    fi
  else
    echo "статус: не установлена"
  fi
}

show_menu() {
  CLEAR=$'\033[2J'
  HOME_C=$'\033[H'
  printf "${CLEAR}${HOME_C}"
  cat <<'EOF'
  __  __ __                       __  __ ____
 | |/ / / //_/___   ___   ____      / / / //  _/
 |   / / ,<  / _ \ / _ \ / __ \    / / / / / /
 /   | / /| |/  __//  __// / / /   / /_/ /_/ /
/_/|_|/_/ |_|\___/ \___//_/ /_/    \____//___/
EOF
  printf "\n$(get_status)\n"
  printf "Архитектура: %s\n" "$(get_arch)"
  printf "\nДобро пожаловать! Выберите действие:\n"
  printf "  1. Установить/переустановить\n"
  printf "  2. Обновить\n"
  printf "  3. Удалить\n"
  printf "\n  0. Выйти\n\n"
}

if [ "$ACTION" = "install" ] && [ "$BETA" = false ]; then
  if [ -t 0 ] || [ -e /dev/tty ]; then
    show_menu
    printf ">: "
    if [ -t 0 ]; then
      read -r response
    else
      read -r response < /dev/tty
    fi
    case "$response" in
      1|"") do_install ;;
      2) do_install ;;
      3) do_uninstall "" ;;
      0) exit 0 ;;
      *) msg "${RED} ❌ Неверный выбор.${NC}"; exit 1 ;;
    esac
  else
    do_install
  fi
else
  case "$ACTION" in
    uninstall) do_uninstall "$UNINSTALL_MODE" ;;
    *) do_install ;;
  esac
fi
