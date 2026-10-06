# ore-edk-boot-opi

A FOSS boot chain for the OrangePi R2S and OrangePi RV2 (SpacemiT K1 / Ky X1), with no U-Boot:

    BootROM -> oreboot bt0 (DRAM init) -> OpenSBI (fw_dynamic) -> EDK2 (UEFI) -> OS EFI loader

Work in progress; see below as it fills in.
