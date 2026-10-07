# SPDX-License-Identifier: GPL-2.0-only
# Three images:
#   make r2s       OrangePi R2S: bt0 in eMMC boot0, OpenSBI + EDK2 in eMMC boot1
#   make rv2       OrangePi RV2: bt0 and OpenSBI + EDK2 on the microSD card,
#                  UEFI variables in RAM (UEFI_VARS=nor: in the SPI NOR, with
#                  a warning: that erases NOR 0x2a0000-0x360000 on first boot)
#   make rv2-nor   OrangePi RV2: everything in the SPI NOR
#   make clean     remove build/ and out/
#   make sources   fetch the pinned source trees (patches/*/pin.env)
#   make mrproper  also remove oreboot's target/ and the fetched source trees
#   make help      list the targets
BOARDS := r2s rv2 rv2-nor
TARGET ?= DEBUG

.PHONY: all help $(BOARDS) sources clean mrproper
all: $(BOARDS)

help:
	@echo "r2s         OrangePi R2S: bt0 in eMMC boot0, OpenSBI + EDK2 in eMMC boot1"
	@echo "rv2         OrangePi RV2: bt0 and OpenSBI + EDK2 on the microSD card"
	@echo "rv2-nor     OrangePi RV2: everything in the SPI NOR"
	@echo "sources     fetch + patch edk2, edk2-platforms, opensbi and dts (patches/*/pin.env)"
	@echo "clean       remove build/ and out/"
	@echo "mrproper    also remove oreboot's target/ and the fetched source trees"
	@echo "TARGET=$(TARGET) (DEBUG or RELEASE)"

# edk2, edk2-platforms, opensbi and dts are fetched at the commits pinned in
# patches/*/pin.env, with patches/<group>/[<project>/]*.patch applied.
sources:
	python3 scripts/fetch-sources.py

$(BOARDS):
	scripts/build.sh $@ $(TARGET)

clean:
	rm -rf build out

mrproper: clean
	rm -rf oreboot/target edk2 edk2-platforms opensbi dts
