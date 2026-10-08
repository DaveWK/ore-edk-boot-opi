#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Build the oreboot bt0 + OpenSBI + EDK2 boot chain for one OrangePi K1 board.
#   scripts/build.sh <r2s|rv2|rv2-nor> [DEBUG|RELEASE]
# Outputs in out/<board>/:
#   bt0.bin    BootROM image: oreboot bt0, signed by oreboot's packer, for the
#              boot device's first-stage slot(s) the BootROM header lists
#              (R2S: eMMC boot0 + 512; RV2: microSD or SPI NOR FSBL copies)
#   next.itb   FIT: OpenSBI fw_dynamic, EDK2 at 0x200000, board DT
#   next.img   next.itb zero-padded to the board's next-stage area
#              (R2S: eMMC boot1; RV2: microSD sectors from LBA 8192)
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
# Reproducible images: mkimage (FIT timestamps) and EDK2's tools (PE/FV time
# fields) take their time from SOURCE_DATE_EPOCH; default it to the commit's.
SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || echo 0)}
export SOURCE_DATE_EPOCH
BOARD=${1:?board}
TARGET=${2:-DEBUG}
CONF=$ROOT/boards/$BOARD/board.conf
[ -f "$CONF" ] || {
  echo "unknown board $BOARD" >&2
  exit 2
}
UEFI_VARS_ENV=${UEFI_VARS:-}
# shellcheck source=/dev/null
. "$CONF"
# UEFI variable store: ram (nothing written) or nor (SPI NOR 0x2a0000-0x360000).
# The board sets the default; UEFI_VARS in the environment overrides it.
UEFI_VARS=${UEFI_VARS_ENV:-${UEFI_VARS:-ram}}
case "$UEFI_VARS" in
ram) EMU_VARS=TRUE ;;
nor)
  EMU_VARS=FALSE
  [ "${NOR_VARS_OK:-no}" = yes ] || {
    echo "$BOARD has no SPI NOR variable store" >&2
    exit 2
  }
  if [ "$BT0_BOOT" != nor ]; then
    cat >&2 <<'WARN'
WARNING: UEFI_VARS=nor. This firmware keeps UEFI variables in the SPI NOR at
0x2a0000-0x360000 and ERASES and formats that range on first boot when it holds
no variable store. On a board whose NOR has other firmware there (for example
the vendor's), that firmware is destroyed. Back up the NOR first.
WARN
  fi
  ;;
*)
  echo "UEFI_VARS must be ram or nor, not $UEFI_VARS" >&2
  exit 2
  ;;
esac
CROSS=${CROSS_COMPILE:-riscv64-linux-gnu-}
JOBS=${JOBS:-$(nproc)}
B=$ROOT/build/$BOARD
O=$ROOT/out/$BOARD
mkdir -p "$B" "$O"

need() { command -v "$1" >/dev/null || {
  echo "missing tool: $1" >&2
  exit 2
}; }
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
  # shellcheck disable=SC2046 # four separate reg cells
  fdtput -t x "$B/board.dtb" "$node" reg $(printf '%x %x %x %x' $((base >> 32)) $((base & 0xffffffff)) $((size >> 32)) $((size & 0xffffffff)))
done

echo "== EDK2 ($EDK2_PLATFORM, $TARGET, UEFI variables in $UEFI_VARS)"
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
  build -q -a RISCV64 -t GCC5 -b "$TARGET" -p "$EDK2_PLATFORM" -n "$JOBS" \
    -D EMU_VARIABLE_NV_MODE_ENABLE="$EMU_VARS"
  cp "Build/$(basename "$EDK2_PLATFORM" .dsc | sed 's/^/OrangePi-/')/${TARGET}_GCC5/FV/$EDK2_FD" "$B/edk2.fd"
)

echo "== FIT"
sed "s/@BOARD@/$BOARD/g" "$ROOT/boards/fit.its.in" >"$B/next.its"
(cd "$B" && mkimage -q -E -B 0x1000 -f next.its next.itb)
size=$(stat -c %s "$B/next.itb")
[ "$size" -le "$NEXT_STAGE_MAX" ] || {
  echo "next.itb is $size bytes, more than $NEXT_STAGE_MAX" >&2
  exit 1
}
cp "$B/next.itb" "$O/next.itb"
python3 - "$O/next.itb" "$O/next.img" "$NEXT_STAGE_MAX" <<'PY'
import sys
data = open(sys.argv[1], "rb").read()
open(sys.argv[2], "wb").write(data + b"\0" * (int(sys.argv[3]) - len(data)))
PY

if [ -n "$BT0_BOOT" ]; then
  echo "== oreboot bt0 (K1_BOOT=$BT0_BOOT, K1_NEXT_PART=${NEXT_PART:-}, K1_NEXT_LBA=$NEXT_LBA, K1_CS_NUM=$CS_NUM)"
  (cd "$ROOT/oreboot/src/mainboard/spacemit/k1x" &&
    env K1_CS_NUM="$CS_NUM" K1_BOOT="$BT0_BOOT" K1_NEXT_LBA="$NEXT_LBA" ${NEXT_PART:+K1_NEXT_PART="$NEXT_PART"} K1_PCIE_PWR_GPIO="${PCIE_PWR_GPIO:-0}" K1_PRODUCT="orangepi-$BOARD" cargo --locked xtask make --release)
  cp "$ROOT/oreboot/target/riscv64imac-unknown-none-elf/release/spacemit-k1x-bt0.bin" "$O/bt0.bin"
else
  echo "== oreboot bt0: no next-stage load path for $BOARD yet; bt0.bin not built"
  rm -f "$O/bt0.bin"
fi

(cd "$O" && rm -f SHA256SUMS && sha256sum -- * >SHA256SUMS && cat SHA256SUMS)
