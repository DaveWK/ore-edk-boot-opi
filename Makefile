# SPDX-License-Identifier: GPL-2.0-only
# Three images:
#   make r2s       OrangePi R2S: bt0 in eMMC boot0, OpenSBI + EDK2 in eMMC boot1
#   make rv2       OrangePi RV2: bt0 and OpenSBI + EDK2 on the microSD card,
#                  UEFI variables in the SPI NOR
#   make rv2-nor   OrangePi RV2: everything in the SPI NOR
BOARDS := r2s rv2 rv2-nor
TARGET ?= DEBUG

.PHONY: all $(BOARDS) submodules clean
all: $(BOARDS)

submodules:
	git submodule update --init dts edk2-platforms opensbi
	git submodule update --init edk2
	git -C edk2 submodule update --init --depth 1 \
	  BaseTools/Source/C/BrotliCompress/brotli MdeModulePkg/Library/BrotliCustomDecompressLib/brotli \
	  MdePkg/Library/BaseFdtLib/libfdt CryptoPkg/Library/OpensslLib/openssl \
	  CryptoPkg/Library/MbedTlsLib/mbedtls MdeModulePkg/Universal/RegularExpressionDxe/oniguruma \
	  MdePkg/Library/MipiSysTLib/mipisyst RedfishPkg/Library/JsonLib/jansson \
	  SecurityPkg/DeviceSecurity/SpdmLib/libspdm
	git -C edk2-platforms submodule update --init --depth 1

$(BOARDS):
	scripts/build.sh $@ $(TARGET)

clean:
	rm -rf build out
