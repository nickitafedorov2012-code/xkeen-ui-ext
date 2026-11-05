#!/usr/bin/env bash
# Воспроизводимая сборка ядра VK-туннеля (клиент WDTT из amurcanov/proxy-turn-vk-android, GPL-3.0)
# под все архитектуры роутеров Keenetic. Результат: wdtt-client-linux-* и SHA256SUMS в OUT_DIR.
#
# Хеши из SHA256SUMS вписываются в backend/src/vkturn.rs (CORE_SHA256), а бинарники
# публикуются в релизе xkeen-ui-ext с тегом wdtt-core-<короткий коммит> (см. CORE_TAG).
#
# Использование: scripts/build-wdtt-core.sh <рабочий каталог> [путь к go]
set -euo pipefail

WDTT_REPO="https://github.com/amurcanov/proxy-turn-vk-android.git"
WDTT_COMMIT="7355598eae4a359d6874409f3437768f3548565b"   # 20.10.2026
GO_VERSION_REQUIRED="go1.27.3"

WORK="${1:?укажите рабочий каталог}"
GO="${2:-go}"
SRC="$WORK/src"
OUT_DIR="$WORK/out"
CLIENT_DIR="$SRC/app/src/main/assets/android-client"

"$GO" version | grep -q "$GO_VERSION_REQUIRED" || {
  echo "Нужен $GO_VERSION_REQUIRED (иначе хеши не совпадут): $("$GO" version)" >&2
  exit 1
}

if [ ! -d "$SRC/.git" ]; then
  git clone --quiet "$WDTT_REPO" "$SRC"
fi
git -C "$SRC" fetch --quiet --depth 1 origin "$WDTT_COMMIT" 2>/dev/null || true
git -C "$SRC" checkout --quiet --force "$WDTT_COMMIT"
git -C "$SRC" reset --quiet --hard "$WDTT_COMMIT"

# Патч: ручная капча ждёт решения 60 с — для роутера мало (человеку нужно открыть панель).
# Сессия капчи ВК живёт ~300 с (expired_at в redirect_uri), поэтому ждём 240 с.
VK_AUTH="$CLIENT_DIR/vk_auth.go"
grep -q 'captchaManualWebViewTimeout   = 60 \* time.Second' "$VK_AUTH" || {
  echo "Строка таймаута ручной капчи не найдена — проверьте патч для нового коммита" >&2
  exit 1
}
sed -i 's/captchaManualWebViewTimeout   = 60 \* time.Second/captchaManualWebViewTimeout   = 240 * time.Second/' "$VK_AUTH"

mkdir -p "$OUT_DIR"
export GOTOOLCHAIN=local CGO_ENABLED=0 GOFLAGS=-mod=readonly

build() { # name GOARCH [extra env]
  local name="$1" arch="$2"; shift 2
  env GOOS=linux GOARCH="$arch" "$@" \
    "$GO" -C "$CLIENT_DIR" build -trimpath -buildvcs=false -ldflags="-s -w -buildid=" \
    -o "$OUT_DIR/$name" .
  echo "собрано: $name"
}

build wdtt-client-linux-arm64  arm64
build wdtt-client-linux-mipsle mipsle GOMIPS=softfloat
build wdtt-client-linux-mips   mips   GOMIPS=softfloat
build wdtt-client-linux-armv7  arm    GOARM=7
build wdtt-client-linux-amd64  amd64

git -C "$SRC" checkout --quiet -- "$VK_AUTH"
(cd "$OUT_DIR" && sha256sum wdtt-client-linux-* > SHA256SUMS && cat SHA256SUMS)
