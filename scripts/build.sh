#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Build the oreboot bt0 + OpenSBI + EDK2 boot chain for one OrangePi K1 board.
#   scripts/build.sh <board> [DEBUG|RELEASE]
# Outputs in out/<board>/:
#   fsbl.bin   BootROM image (oreboot bt0, signed by oreboot's packer) for the
#              boot device's FSBL slot (eMMC boot0 + 512 on the R2S)
#   next.itb   FIT: OpenSBI fw_dynamic, EDK2 at 0x200000, board DT
#   next.img   next.itb zero-padded to the board's next-stage area
#              (R2S: eMMC boot1; RV2: microSD sectors from LBA 8192)
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
BOARD=${1:?board}
TARGET=${2:-DEBUG}
CONF=$ROOT/boards/$BOARD/board.conf
[ -f "$CONF" ] || { echo "unknown board $BOARD" >&2; exit 2; }
# shellcheck source=/dev/null
. "$CONF"
CROSS=${CROSS_COMPILE:-riscv64-linux-gnu-}
JOBS=${JOBS:-$(nproc)}
B=$ROOT/build/$BOARD
O=$ROOT/out/$BOARD
mkdir -p "$B" "$O"

need() { command -v "$1" >/dev/null || { echo "missing tool: $1" >&2; exit 2; }; }
for t in ${CROSS}gcc dtc fdtput mkimage cargo python3 make cpp; do need "$t"; done

echo "== OpenSBI"
make -s -C "$ROOT/opensbi" O="$B/opensbi" PLATFORM=generic CROSS_COMPILE="$CROSS" -j"$JOBS"
cp "$B/opensbi/platform/generic/firmware/fw_dynamic.bin" "$B/fw_dynamic.bin"

echo "== device tree ($DTS)"
cpp -nostdinc -undef -x assembler-with-cpp -D__DTS__ \
    -I "$ROOT/dts/include" -I "$ROOT/dts/src/riscv" \
    "$ROOT/dts/src/riscv/$DTS" | dtc -q -I dts -O dtb -o "$B/board.dtb" -
for range in $MEMORY; do
  base=$((${range%%:*})) size=$((${range##*:}))
  node=$(printf '/memory@%x' "$base")
  fdtput -c "$B/board.dtb" "$node"
  fdtput -t s "$B/board.dtb" "$node" device_type memory
  # fdtput -t x takes hexadecimal cells
  fdtput -t x "$B/board.dtb" "$node" reg $(printf '%x %x %x %x' $((base >> 32)) $((base & 0xffffffff)) $((size >> 32)) $((size & 0xffffffff)))
done

echo "== EDK2 ($EDK2_PLATFORM, $TARGET)"
(
  export WORKSPACE=$B/edk2-ws PACKAGES_PATH=$ROOT/edk2:$ROOT/edk2-platforms:$ROOT/edk2-opi
  export GCC5_RISCV64_PREFIX=$CROSS PYTHON_COMMAND=python3
  mkdir -p "$WORKSPACE"
  cd "$ROOT/edk2"
  # Host tools only; newer GCC adds warnings these sources treat as errors.
  [ -x BaseTools/Source/C/bin/GenFw ] || make -s -C BaseTools -j"$JOBS" EXTRA_OPTFLAGS=-Wno-error
  # edksetup.sh expects unset variables to be empty and returns non-zero
  # even when it succeeds.
  # Sourced scripts see the caller's arguments; give it none.
  set +eu --
  # shellcheck source=/dev/null
  . ./edksetup.sh >/dev/null
  set -e
  cd "$WORKSPACE"
  build -q -a RISCV64 -t GCC5 -b "$TARGET" -p "$EDK2_PLATFORM" -n "$JOBS"
  cp "Build/$(basename "$EDK2_PLATFORM" .dsc | sed 's/^/OrangePi-/')/${TARGET}_GCC5/FV/$EDK2_FD" "$B/edk2.fd"
)

echo "== FIT"
sed "s/@BOARD@/$BOARD/g" "$ROOT/boards/fit.its.in" > "$B/next.its"
(cd "$B" && mkimage -q -E -B 0x1000 -f next.its next.itb)
size=$(stat -c %s "$B/next.itb")
[ "$size" -le "$NEXT_STAGE_MAX" ] || { echo "next.itb is $size bytes, more than $NEXT_STAGE_MAX" >&2; exit 1; }
cp "$B/next.itb" "$O/next.itb"
python3 - "$O/next.itb" "$O/next.img" "$NEXT_STAGE_MAX" <<'PY'
import sys
data = open(sys.argv[1], "rb").read()
open(sys.argv[2], "wb").write(data + b"\0" * (int(sys.argv[3]) - len(data)))
PY

if [ -n "$BT0_BOOT" ]; then
  echo "== oreboot bt0 (K1_BOOT=$BT0_BOOT, K1_NEXT_LBA=$NEXT_LBA, K1_CS_NUM=$CS_NUM)"
  (cd "$ROOT/oreboot/src/mainboard/spacemit/k1x" &&
   K1_CS_NUM=$CS_NUM K1_BOOT=$BT0_BOOT K1_NEXT_LBA=$NEXT_LBA cargo --locked xtask make --release)
  cp "$ROOT/oreboot/target/riscv64imac-unknown-none-elf/release/spacemit-k1x-bt0.bin" "$O/fsbl.bin"
else
  echo "== oreboot bt0: no next-stage load path for $BOARD yet; fsbl.bin not built"
  rm -f "$O/fsbl.bin"
fi

(cd "$O" && rm -f SHA256SUMS && sha256sum -- * > SHA256SUMS && cat SHA256SUMS)
