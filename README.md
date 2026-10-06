# ore-edk-boot-opi

A U-Boot-free boot chain for the OrangePi R2S and OrangePi RV2 (SpacemiT K1, sold as Ky X1):

    BootROM
      -> oreboot bt0      DRAM init (training in Rust source, no DDR blob),
                          then loads the next stage itself
      -> OpenSBI          fw_dynamic, M-mode
      -> EDK2             UEFI, S-mode
      -> OS EFI loader    e.g. FreeBSD loader.efi from the ESP

## Status

| Board | bt0 loads next stage from | Result |
|---|---|---|
| OrangePi R2S | eMMC hardware partition boot1 | This repository's build (`fsbl.bin` in boot0, `next.img` in boot1, DEBUG EDK2) boots FreeBSD from eMMC on a warm reboot in about 85 s (Oct 2026). Cold boot pending |
| OrangePi RV2 (`orangepi-rv2`) | raw microSD sectors from LBA 8192 (the 4 MiB GPT partition "uboot") | This repository's build boots FreeBSD from NVMe on a cold power-on (Oct 2026): bt0 switches the M.2 slot supply on (GPIO 116), EDK2 brings PCIe up (Gen2 x2) and boots the NVMe ESP. UEFI variables live in the SPI NOR (verified) |
| OrangePi RV2 (`orangepi-rv2-nor`) | the 16 MiB SPI NOR, memory-mapped: bt0 at 0x20000/0x70000, FIT at 0xa0000, UEFI variables at 0x2a0000 | Boots FreeBSD from NVMe with the microSD card removed (Oct 2026); EDK2 keeps its variables in the NOR across boots |

Known gaps:
- **EEPROM:** EDK2's EEPROM (TLV) reads over I2C2 time out, so MAC addresses do not come from the EEPROM yet.
- **UEFI variables:** RAM only on the R2S (no NOR); in the SPI NOR on the RV2.
- **Bootinfo header:** none is generated. Installs keep the boot device's existing one.
- **Secure boot:** the BootROM image is signed with oreboot's packer keys. This works on boards without secure-boot fuses.

## Layout

| Path | What |
|---|---|
| `oreboot/` | oreboot (from [orangecms/oreboot](https://github.com/orangecms/oreboot) `k1x`, history kept) with the OrangePi K1 bt0 work in `src/mainboard/spacemit/k1x/bt0`: per-board rank count, eMMC driver, FIT loader, OpenSBI `fw_dynamic` handoff, EEPROM I2C setup; and `xtask` image packing (`--payload`) |
| `edk2-opi/OrangePiPkg` | EDK2 platforms `K1/R2S` and `K1/RV2` (derived from SpacemiT's MUSE-Pi-Pro platform), and `BareSatpOnExitDxe` (MMU off at ExitBootServices) |
| `edk2` | submodule: [bianbu-maker/edk2](https://github.com/bianbu-maker/edk2) `k1` (SpacemiT's EDK2 base) |
| `edk2-platforms` | submodule: [bianbu-maker/edk2-platforms](https://github.com/bianbu-maker/edk2-platforms) `k1` (SpacemiT K1 silicon package) |
| `opensbi` | submodule: OpenSBI v1.9 |
| `dts` | submodule: devicetree-rebasing `v7.2-dts` (board DTs) |
| `boards/<board>/board.conf` | per-board settings; `boards/fit.its.in` FIT template |
| `scripts/build.sh`, `Makefile` | build |

## Build

Requirements:
- a `riscv64-linux-gnu-` cross GCC;
- host GCC, Python 3, `dtc`/`fdtput`, `mkimage` (uboot-tools) and `cpp`;
- Rust via rustup. `oreboot/rust-toolchain.toml` pins the nightly.
- `llvm-objcopy` or `rust-objcopy` for oreboot's packer.

    git clone --recurse-submodules=dts --recurse-submodules=opensbi https://github.com/DaveWK/ore-edk-boot-opi.git
    cd ore-edk-boot-opi
    make submodules
    make orangepi-r2s            # TARGET=RELEASE for a release EDK2

`out/<board>/` then holds:
- `fsbl.bin`: the BootROM image (oreboot bt0, signed by oreboot's packer).
- `next.itb`: a FIT with OpenSBI `fw_dynamic` at 0x0, EDK2 at 0x200000 and the board DT with its memory nodes.
- `next.img`: `next.itb` zero-padded to the next-stage area (R2S: the 4 MiB eMMC boot1 partition; RV2: 4 MiB of microSD from LBA 8192).
- `SHA256SUMS`.

## Installing on the R2S

These steps write boot firmware. Keep a raw backup of eMMC boot0 and boot1 first. Download mode is the recovery path.

1. Write `next.img` to the eMMC boot1 hardware partition.
2. Write `fsbl.bin`, zero-padded to the existing FSBL slot, into eMMC boot0 at offset 512. Leave the bootinfo header in the first 512 bytes as it is.

To try an image without writing boot0, hold the download button and run:

    fastboot stage out/orangepi-r2s/fsbl.bin
    fastboot continue

bt0 then loads boot1 as usual.

## Installing on the RV2

These steps write boot firmware on the microSD card. Keep a raw backup of the card's first 8 MiB first.

1. Write `next.img` to LBA 8192 (GPT partition 2, "uboot").
2. Write `fsbl.bin`, zero-padded to the existing slot, at both FSBL copies the card's BootROM header lists (128 KiB and 512 KiB on our cards). Leave the first 512 bytes as they are.

## Licences

| Part | Licence |
|---|---|
| oreboot | GPL-2.0 (`oreboot/COPYING`) |
| EDK2 and `OrangePiPkg` | BSD-2-Clause-Patent |
| OpenSBI | BSD-2-Clause |
| Build scripts here | GPL-2.0-only (`LICENSE`) |
