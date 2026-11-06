#!/usr/bin/env bash
# Сборка ядра VK-туннеля — клиента csqtt из Afteroid/csqttr («CSQTT-Rebirth»,
# PolyForm Noncommercial 1.0.0, © amurcanov) — для роутеров Keenetic с процессором ARM64.
# Результат: csqtt-client-linux-arm64 и SHA256SUMS в <рабочий каталог>/out.
#
# Хеш из SHA256SUMS вписывается в backend/src/vkturn.rs (CORE_SHA256), бинарник публикуется
# в релизе xkeen-ui-ext с тегом csqtt-core-<короткий коммит> (см. CORE_TAG) вместе с LICENSE csqttr.
#
# Единственное изменение исходников: Lua-движок встроенного Gooby (TURN поверх TCP/TLS с
# пользовательскими Lua-стратегиями) заменён заглушкой scripts/csqtt-router-patch/gooby_lua_stub.rs.
# VK-туннель панели работает по UDP и Gooby не включает, а LuaJIT не собирается кросс-компиляцией под musl.
#
# Нужны: git, rustup (тулчейн 1.97.1), zig и cargo-zigbuild (как в rust-server/build_linux.sh csqttr).
# Использование: scripts/build-csqtt-core.sh <рабочий каталог>
set -euo pipefail

CSQTT_REPO="https://github.com/Afteroid/csqttr.git"
CSQTT_COMMIT="0e90df237583738741b50e86e4f51f87e96c94da"   # 05.11.2026
RUST_TOOLCHAIN="1.97.1"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

WORK="${1:?укажите рабочий каталог}"
SRC="$WORK/src"
OUT_DIR="$WORK/out"

if [ ! -d "$SRC/.git" ]; then
  git clone --quiet "$CSQTT_REPO" "$SRC"
fi
git -C "$SRC" fetch --quiet origin "$CSQTT_COMMIT" 2>/dev/null || true
git -C "$SRC" checkout --quiet --force "$CSQTT_COMMIT"
git -C "$SRC" clean -fdq rust-client

# Патч для роутера: без Lua в Gooby
sed -i '/^mlua = /d' "$SRC/rust-client/vendor/gooby/Cargo.toml"
cp "$HERE/csqtt-router-patch/gooby_lua_stub.rs" "$SRC/rust-client/vendor/gooby/src/lua.rs"

rustup toolchain install "$RUST_TOOLCHAIN" --profile minimal >/dev/null
rustup target add aarch64-unknown-linux-musl --toolchain "$RUST_TOOLCHAIN" >/dev/null

mkdir -p "$OUT_DIR"
(
  cd "$SRC/rust-client"
  cargo +"$RUST_TOOLCHAIN" zigbuild --release --target aarch64-unknown-linux-musl --bin client
)
cp "$SRC/rust-client/target/aarch64-unknown-linux-musl/release/client" "$OUT_DIR/csqtt-client-linux-arm64"
cp "$SRC/LICENSE" "$OUT_DIR/LICENSE-csqttr.md"
(cd "$OUT_DIR" && sha256sum csqtt-client-linux-arm64 > SHA256SUMS && cat SHA256SUMS)
