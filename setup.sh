#!/bin/sh
# XKeen Route — установочный скрипт для Entware (Keenetic/Netcraze)
# Рекомендуемый способ установки (с проверкой):
#   curl -fLs -o setup.sh https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh
#   sh setup.sh
# Бета:      sh setup.sh beta
# Удаление:  sh setup.sh uninstall            (конфиги сохраняются)
# Удаление полностью (с конфигами):
#            sh setup.sh uninstall purge

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
    *armv7*|*cortex-a9*|*cortex-a7*|*arm_*) echo 'armv7-v7a' ;;
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
  # Проверяем наличие CA-сертификатов в Entware
  if [ ! -f /opt/etc/ssl/certs/ca-certificates.crt ] && which opkg >/dev/null 2>&1; then
    opkg update >/dev/null 2>&1
    opkg install ca-bundle ca-certificates >/dev/null 2>&1
  fi

  # Прямая ссылка + доверенные HTTPS-зеркала
  OK=0
  for P in "" "https://ghproxy.net/" "https://ghfast.top/"; do
    if [ -z "$P" ]; then
      msg "${NC}   пробую github.com (до 20 сек)...${NC}"
      T=20
    else
      msg "${NC}   пробую зеркало ${P} (до 60 сек)...${NC}"
      T=60
    fi
    curl -Ls --max-time "$T" --connect-timeout 10 "${P}${DOWNLOAD_URL}" -o "$BIN.tmp" </dev/null
    if [ -f "$BIN.tmp" ] && [ "$(wc -c < "$BIN.tmp")" -gt 1000000 ]; then
      OK=1
      break
    fi
    msg "${RED}   не удалось, пробую следующий источник...${NC}"
  done

  if [ "$OK" = 1 ]; then
    # Проверка SHA-256 контрольной суммы если доступен файл .sha256 в релизе
    SHA_URL="${DOWNLOAD_URL}.sha256"
    curl -sSL --max-time 15 "${SHA_URL}" -o "$BIN.tmp.sha256" 2>/dev/null
    if [ -s "$BIN.tmp.sha256" ] && which sha256sum >/dev/null 2>&1; then
      EXPECTED_SHA=$(awk '{print $1}' "$BIN.tmp.sha256")
      ACTUAL_SHA=$(sha256sum "$BIN.tmp" | awk '{print $1}')
      if [ -n "$EXPECTED_SHA" ] && [ "$EXPECTED_SHA" != "$ACTUAL_SHA" ]; then
        msg "${RED} ❌ Ошибка проверки целостности SHA-256:${NC}"
        msg "${RED}    Ожидалось: $EXPECTED_SHA${NC}"
        msg "${RED}    Получено:  $ACTUAL_SHA${NC}"
        rm -f "$BIN.tmp" "$BIN.tmp.sha256"
        return 1
      fi
      msg "${GREEN}✅ Контрольная сумма SHA-256 проверена успешно${NC}"
      rm -f "$BIN.tmp.sha256"
    fi

    # Проверка ELF-заголовка (\x7fELF)
    ELF_MAGIC=$(head -c 4 "$BIN.tmp" 2>/dev/null)
    if [ "$ELF_MAGIC" != "$(printf '\x7fELF')" ]; then
      msg "${RED} ❌ Загруженный файл поврежден или не является корректным ELF-бинарником${NC}"
      rm -f "$BIN.tmp"
      return 1
    fi
    chmod +x "$BIN.tmp" && mv "$BIN.tmp" "$BIN"
  else
    msg "${RED} ❌ Не удалось загрузить бинарник${NC}"
    rm -f "$BIN.tmp"
    return 1
  fi

  msg "${GREEN}🧩 Init-скрипт и Watchdog...${NC}"
  "$BIN" create-init || { msg "${RED} ❌ Ошибка создания init${NC}"; return 1; }

  mkdir -p "$CONF_DIR"

  # Настройка cron watchdog (перезапуск панели в случае падения или ночного обновления XKeen)
  if which crontab >/dev/null 2>&1; then
    (crontab -l 2>/dev/null | grep -v 'xkeen-route'; echo "*/5 * * * * pidof xkeen-route >/dev/null || /opt/etc/init.d/S99xkeen-route start >/dev/null 2>&1") | crontab -
  fi

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
  if which crontab >/dev/null 2>&1; then
    (crontab -l 2>/dev/null | grep -v 'xkeen-route') | crontab -
  fi
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
