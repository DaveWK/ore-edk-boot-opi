## @file
#  OrangePi RV2 (SpacemiT K1) for the oreboot + EDK2 boot chain.
#  Derived from SpacemiT's MUSE-Pi-Pro platform (edk2-platforms k1).
#
#  Copyright (c) 2024, SpacemiT Co., Ltd. All rights reserved.
#
#  SPDX-License-Identifier: BSD-2-Clause-Patent
#
##

[Defines]
  PLATFORM_NAME                  = OrangePi-RV2
  PLATFORM_GUID                  = 1B7E4C92-3D68-4F05-A2C9-8E61D0F35B74
  PLATFORM_VERSION               = 0.5.0
  DSC_SPECIFICATION              = 0x0001001c
  OUTPUT_DIRECTORY               = Build/$(PLATFORM_NAME)
  SUPPORTED_ARCHITECTURES        = RISCV64
  BUILD_TARGETS                  = DEBUG|RELEASE|NOOPT
  SKUID_IDENTIFIER               = DEFAULT
  FLASH_DEFINITION               = OrangePiPkg/K1/RV2/RV2.fdf

  DEFINE DEBUG_ON_SERIAL_PORT    = TRUE
  # UEFI variables live in the SPI NOR (0x2A0000-0x360000, SpacemiT's layout),
  # whether the firmware was loaded from the NOR or from the microSD card.
  DEFINE EMU_VARIABLE_NV_MODE_ENABLE = FALSE

[SkuIds]
  0|DEFAULT

!include MdePkg/MdeLibs.dsc.inc
!include Silicon/Spacemit/Spacemit.dsc.inc
!include Features/Ext4Pkg/Ext4.dsc.inc

[LibraryClasses.common.SEC]
  # Serial via SBI (without poll)
  SerialPortLib|MdePkg/Library/BaseSerialPortLibRiscVSbiLib/BaseSerialPortLibRiscVSbiLib.inf

  SpacemitSecHelperLib|Silicon/Spacemit/Library/SpacemitSecHelperLib/SpacemitSecHelperLib.inf
  SpacemitSecLib|Silicon/Spacemit/K1/Library/SpacemitSecLib/SpacemitSecLib.inf

[LibraryClasses.common]
  # EmbeddedPkg's copy plus the RV2's XMC flash ID
  NorFlashInfoLib|OrangePiPkg/Library/NorFlashInfoLib/NorFlashInfoLib.inf
  PlatformBootManagerLib|Silicon/Spacemit/Library/PlatformBootManagerLib/PlatformBootManagerLib.inf

  NetLib|NetworkPkg/Library/DxeNetLib/DxeNetLib.inf
  ResetSystemLib|Silicon/Spacemit/K1/Library/ResetSystemLib/ResetSystemLib.inf

  # Serial via SBI (with poll)
  SerialPortLib|MdePkg/Library/BaseSerialPortLibRiscVSbiLib/BaseSerialPortLibRiscVSbiLibRam.inf

  # Lib for USB
  NonDiscoverableDeviceRegistrationLib|MdeModulePkg/Library/NonDiscoverableDeviceRegistrationLib/NonDiscoverableDeviceRegistrationLib.inf
  UefiUsbLib|MdePkg/Library/UefiUsbLib/UefiUsbLib.inf
  UefiScsiLib|MdePkg/Library/UefiScsiLib/UefiScsiLib.inf

  # PCI root port configuation and description
  PciHostBridgeLib|Silicon/Spacemit/Library/PciHostBridgeLib/PciHostBridgeLib.inf
  # PCI access library
  PciSegmentLib|Silicon/Spacemit/Library/DesignWarePciSegmentLib/DesignWarePciSegmentLib.inf
  DesignWarePcieControllerLib|Silicon/Spacemit/Library/DesignWarePcieControllerLib/DesignWarePcieControllerLib.inf

[PcdsFeatureFlag.common]
  gSpacemitTokenSpaceGuid.PcdEscEnterBootMenu|FALSE

[PcdsFixedAtBuild.common]
  # This UEFI memory region is used in SEC phase to create HOBs, load DXE, etc.
  #     Base = PcdSecStackBase + PcdSecStackSize - PcdSecUefiMemorySize
  #     Size = PcdSecUefiMemorySize
  # (The top of the UEFI memory region is reserved for the stack.)
  gSpacemitTokenSpaceGuid.PcdSecStackBase|0x23FF0000
  gSpacemitTokenSpaceGuid.PcdSecStackSize|0x10000
  gSpacemitTokenSpaceGuid.PcdSecUefiMemorySize|0x02000000

  # Indicates if to reset system when memory type information changes.
  # This platform doesn't support S4 state with EDK2, so set it FALSE.
  gEfiMdeModulePkgTokenSpaceGuid.PcdResetOnMemoryTypeInformationChange|FALSE

  # Set PcdBootManagerMenuFile to UiApp (FILE_GUID = 462CAA21-7614-4503-836E-8AB6F4662331)
  gEfiMdeModulePkgTokenSpaceGuid.PcdBootManagerMenuFile|{ 0x21, 0xaa, 0x2c, 0x46, 0x14, 0x76, 0x03, 0x45, 0x83, 0x6e, 0x8a, 0xb6, 0xf4, 0x66, 0x23, 0x31 }

  # Configurability to override RISC-V CPU Features
  # BIT 0 = Cache Management Operations. This bit is relevant only if
  # previous stage has feature enabled and user wants to disable it.
  # BIT 1 = Supervisor Time Compare (Sstc). This bit is relevant only if
  # previous stage has feature enabled and user wants to disable it.
  # BIT 2 = Page-Based Memory Types (Pbmt). This bit is relevant only if
  # previous stage has feature enabled and user wants to disable it.
  gEfiMdePkgTokenSpaceGuid.PcdRiscVFeatureOverride|0x07

  # Frequency of the core crystal clock in Hz
  gUefiCpuPkgTokenSpaceGuid.PcdCpuCoreCrystalClockFrequency|24000000

  #
  # Control the maximum SATP mode that MMU allowed to use.
  # 0 - Bare mode.
  # 8 - 39bit mode.
  # 9 - 48bit mode.
  # 10 - 57bit mode.
  #
  gUefiCpuPkgTokenSpaceGuid.PcdCpuRiscVMmuMaxSatpMode|8

  # For MMU type >= sv39, the width of physical address is 56-bit.
  gSpacemitTokenSpaceGuid.PcdMemoryAddressWidthMax|56
  gSpacemitTokenSpaceGuid.PcdIoAddressWidthMax|32

  # for QSPI controller in K1, Spi flash is map to address below
  gSpacemitTokenSpaceGuid.PcdSFMemMapBaseAddress|0xB8000000

  gSpacemitK1TokenSpaceGuid.PcdSpacemitMPMURegBase|0xd4050000
  gSpacemitK1TokenSpaceGuid.PcdSpacemitAPMURegBase|0xd4282800
  gSpacemitK1TokenSpaceGuid.PcdSpacemitAPBSpareRegBase|0xd4090000
  gSpacemitK1TokenSpaceGuid.PcdSpacemitAPBClockRegBase|0xd4015000
  gSpacemitK1TokenSpaceGuid.PcdSpacemitMFPRRegBase|0xd401e000
  gSpacemitK1TokenSpaceGuid.PcdSpacemitWDTRegBase|0xd4080000

  gSpacemitK1TokenSpaceGuid.PcdSpacemitHDMIRegBase|0xC0400500
  gSpacemitK1TokenSpaceGuid.PcdSpacemitHDMIDpuRegBase|0xc0440000

  # Sd/emmc host controller Configuration
  # microSD (card detect GPIO 80) and eMMC socket
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Num|2
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[0].RegBase|0xd4280000
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[0].SourceClkMHz|208
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[0].BusWidth|4
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[0].DetectGpio|80
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[0].PluginState|1
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[0].IsEmmc|FALSE
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[0].PIOTransferMode|TRUE
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[0].DisableMultiBlock|FALSE
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[1].RegBase|0xd4281000
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[1].SourceClkMHz|208
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[1].BusWidth|8
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[1].IsEmmc|TRUE
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[1].PIOTransferMode|TRUE
  gSpacemitK1TokenSpaceGuid.PcdMmcHostConfigs.Controller[1].DisableMultiBlock|FALSE

  gSpacemitK1TokenSpaceGuid.PcdQspiRegBase|0xd420c000
  gSpacemitK1TokenSpaceGuid.PcdQspiMaxFrequency|50000000

  # I2C Controller Configuration
  # Configure I2C2: ID=2, Base=0xd4012000, Clock=100KHz, Enable=1
  gSpacemitTokenSpaceGuid.PcdI2cControllerConfigs.Num|1
  gSpacemitTokenSpaceGuid.PcdI2cControllerConfigs.Data[0].ControllerId|2
  gSpacemitTokenSpaceGuid.PcdI2cControllerConfigs.Data[0].BaseAddress|0xd4012000
  gSpacemitTokenSpaceGuid.PcdI2cControllerConfigs.Data[0].ClockRate|100000
  gSpacemitTokenSpaceGuid.PcdI2cControllerConfigs.Data[0].Enable|TRUE

  # I2C Slave Configuration
  # Configure one slave device on I2C2 with address 0x50
  gSpacemitTokenSpaceGuid.PcdI2cSlaveConfig.Num|1
  gSpacemitTokenSpaceGuid.PcdI2cSlaveConfig.Data[0].BusNumber|2
  gSpacemitTokenSpaceGuid.PcdI2cSlaveConfig.Data[0].SlaveAddress|0x50

  # EEPROM Configuration
  gSpacemitTokenSpaceGuid.PcdEepromConfigs.Num|1
  gSpacemitTokenSpaceGuid.PcdEepromConfigs.Data[0].BusNumber|2
  gSpacemitTokenSpaceGuid.PcdEepromConfigs.Data[0].SlaveAddress|0x50
  gSpacemitTokenSpaceGuid.PcdEepromConfigs.Data[0].AddressWidth|1
  gSpacemitTokenSpaceGuid.PcdEepromConfigs.Data[0].PageSize|8

  # GPIO Controller Configuration
  gSpacemitTokenSpaceGuid.PcdGpioControllerBase|0xd4019000
  gSpacemitTokenSpaceGuid.PcdGpioControllerCount|1
  gSpacemitTokenSpaceGuid.PcdGpioPinCount|128

  # LED configuration
  gSpacemitTokenSpaceGuid.PcdLedConfigs.Num|1
  gSpacemitTokenSpaceGuid.PcdLedConfigs.Timing.ShortTimeMs|100
  gSpacemitTokenSpaceGuid.PcdLedConfigs.Timing.PauseTimeMs|200
  gSpacemitTokenSpaceGuid.PcdLedConfigs.Timing.LongTimeMs|3000
  gSpacemitTokenSpaceGuid.PcdLedConfigs.Data[0].GpioPin|96
  gSpacemitTokenSpaceGuid.PcdLedConfigs.Data[0].ActiveState|TRUE

  # Enable error status code reporting
  gEfiMdePkgTokenSpaceGuid.PcdReportStatusCodePropertyMask|0x07

  # Pcds for USB
  gSpacemitK1TokenSpaceGuid.PcdUsb0BaseAddr|0xc0900100
  gSpacemitK1TokenSpaceGuid.PcdUsb0PhyBaseAddr|0xc0940000
  gSpacemitK1TokenSpaceGuid.PcdUsb1BaseAddr|0xc0980100
  gSpacemitK1TokenSpaceGuid.PcdUsb1PhyBaseAddr|0xc09c0000
  gSpacemitK1TokenSpaceGuid.PcdUsb3BaseAddr|0xc0a00000
  gSpacemitK1TokenSpaceGuid.PcdUsb2PhyBaseAddr|0xc0a30000
  gSpacemitK1TokenSpaceGuid.PcdUsb3PhyBaseAddr|0xc0b10000
  gSpacemitK1TokenSpaceGuid.PcdCombPhySelAddr|0xd4282910
  gSpacemitK1TokenSpaceGuid.PcdUsb0Enable|FALSE
  gSpacemitK1TokenSpaceGuid.PcdUsb1Enable|FALSE
  gSpacemitK1TokenSpaceGuid.PcdUsb2Enable|TRUE
  gSpacemitK1TokenSpaceGuid.PcdUsb3Enable|TRUE

  # PCIe config tables.
  # Remember to change these Nums when adding/removing elements to/from the tables.
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Num|3
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayNum|3

  # PCIe 0
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[0].Reg.DbiBase|0xCA000000
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[0].Reg.DbiSize|0x1000
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[0].NumLanes|1
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[0].ControllerMode|0
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[0].CfgShiftModeEnabled|FALSE
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].Segment|0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].ConfigBase|0x8F000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].ConfigSize|0x100000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].BusBase|0x0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].BusLimit|0xFF
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].Io.PciBase|0x8F100000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].Io.PciSize|0x10000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].Io.CpuBase|0x8F100000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].Mem.PciBase|0x80000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].Mem.PciSize|0xF000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].Mem.CpuBase|0x80000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].PMem64.PciBase|0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].PMem64.PciSize|0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].PMem64.CpuBase|0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[0].IsEnabled|FALSE

  # PCIe 1
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[1].Reg.DbiBase|0xCA400000
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[1].Reg.DbiSize|0x1000
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[1].NumLanes|2
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[1].ControllerMode|0
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[1].CfgShiftModeEnabled|FALSE
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].Segment|1
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].ConfigBase|0x9F000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].ConfigSize|0x100000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].BusBase|0x0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].BusLimit|0xFF
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].Io.PciBase|0x9F100000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].Io.PciSize|0x10000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].Io.CpuBase|0x9F100000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].Mem.PciBase|0x90000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].Mem.PciSize|0xF000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].Mem.CpuBase|0x90000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].PMem64.PciBase|0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].PMem64.PciSize|0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].PMem64.CpuBase|0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[1].IsEnabled|TRUE

  # PCIe 2
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[2].Reg.DbiBase|0xCA800000
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[2].Reg.DbiSize|0x1000
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[2].NumLanes|2
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[2].ControllerMode|0
  gSpacemitTokenSpaceGuid.PcdDwPcieControllerConfigTable.Data[2].CfgShiftModeEnabled|FALSE
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].Segment|2
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].ConfigBase|0xAF000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].ConfigSize|0x100000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].BusBase|0x0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].BusLimit|0xFF
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].Io.PciBase|0xAF100000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].Io.PciSize|0x10000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].Io.CpuBase|0xAF100000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].Mem.PciBase|0xA0000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].Mem.PciSize|0xF000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].Mem.CpuBase|0xA0000000
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].PMem64.PciBase|0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].PMem64.PciSize|0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].PMem64.CpuBase|0
  gSpacemitTokenSpaceGuid.PcdBoardPciRootBridgeResourceConfigTable.ArrayData[2].IsEnabled|FALSE

[PcdsDynamicDefault.common]
  # The seconds that the firmware will wait before initiating the original default boot selection.
  # 0: immediately
  # 0xFFFF: firmware will wait for user input before booting
  gEfiMdePkgTokenSpaceGuid.PcdPlatformBootTimeOut|3

  # Console output rows and columns. 0 means max.
  gEfiMdeModulePkgTokenSpaceGuid.PcdConOutRow|0
  gEfiMdeModulePkgTokenSpaceGuid.PcdConOutColumn|0
  # Video horizontal and vertical resolution. 0 means highest.
  gEfiMdeModulePkgTokenSpaceGuid.PcdVideoHorizontalResolution|0
  gEfiMdeModulePkgTokenSpaceGuid.PcdVideoVerticalResolution|0

  # EDK firmware configuration
  gEfiMdeModulePkgTokenSpaceGuid.PcdFirmwareVendor|L"SPACEMIT"
  gEfiMdeModulePkgTokenSpaceGuid.PcdFirmwareVersionString|L"$(PLATFORM_VERSION)"

[Components]
  # Shell
#  ShellPkg/DynamicCommand/TftpDynamicCommand/TftpDynamicCommand.inf {
#    <PcdsFixedAtBuild>
#      gEfiShellPkgTokenSpaceGuid.PcdShellLibAutoInitialize|FALSE
#  }
#  ShellPkg/DynamicCommand/HttpDynamicCommand/HttpDynamicCommand.inf {
#    <PcdsFixedAtBuild>
#      gEfiShellPkgTokenSpaceGuid.PcdShellLibAutoInitialize|FALSE
#  }
  OvmfPkg/LinuxInitrdDynamicShellCommand/LinuxInitrdDynamicShellCommand.inf {
    <PcdsFixedAtBuild>
      gEfiShellPkgTokenSpaceGuid.PcdShellLibAutoInitialize|FALSE
  }
  ShellPkg/Application/Shell/Shell.inf {
    <LibraryClasses>
      ShellCommandLib|ShellPkg/Library/UefiShellCommandLib/UefiShellCommandLib.inf
      NULL|ShellPkg/Library/UefiShellLevel2CommandsLib/UefiShellLevel2CommandsLib.inf
      NULL|ShellPkg/Library/UefiShellLevel1CommandsLib/UefiShellLevel1CommandsLib.inf
      NULL|ShellPkg/Library/UefiShellLevel3CommandsLib/UefiShellLevel3CommandsLib.inf
      NULL|ShellPkg/Library/UefiShellDriver1CommandsLib/UefiShellDriver1CommandsLib.inf
      NULL|ShellPkg/Library/UefiShellDebug1CommandsLib/UefiShellDebug1CommandsLib.inf
#!if $(ACPIVIEW_ENABLE) == TRUE
#      NULL|ShellPkg/Library/UefiShellAcpiViewCommandLib/UefiShellAcpiViewCommandLib.inf
#!endif
      NULL|ShellPkg/Library/UefiShellInstall1CommandsLib/UefiShellInstall1CommandsLib.inf
      NULL|ShellPkg/Library/UefiShellNetwork1CommandsLib/UefiShellNetwork1CommandsLib.inf
#!if $(NETWORK_IP6_ENABLE) == TRUE
#      NULL|ShellPkg/Library/UefiShellNetwork2CommandsLib/UefiShellNetwork2CommandsLib.inf
#!endif
      NULL|Silicon/Spacemit/Applications/SpiTool/SpiFlashCmd.inf
      NULL|Silicon/Spacemit/Applications/EepromTool/EepromCmd.inf
      NULL|Silicon/Spacemit/Applications/I2cTool/I2cCmd.inf
      HandleParsingLib|ShellPkg/Library/UefiHandleParsingLib/UefiHandleParsingLib.inf
      PrintLib|MdePkg/Library/BasePrintLib/BasePrintLib.inf
      BcfgCommandLib|ShellPkg/Library/UefiShellBcfgCommandLib/UefiShellBcfgCommandLib.inf

    <PcdsFixedAtBuild>
      gEfiMdePkgTokenSpaceGuid.PcdDebugPropertyMask|0xFF
      gEfiShellPkgTokenSpaceGuid.PcdShellLibAutoInitialize|FALSE
      gEfiMdePkgTokenSpaceGuid.PcdUefiLibMaxPrintBufferSize|8000
  }

  # UiApp
  MdeModulePkg/Application/UiApp/UiApp.inf {
    <LibraryClasses>
      NULL|Silicon/Spacemit/Library/PlatformUiLib/PlatformManagerUiLib.inf
      NULL|MdeModulePkg/Library/DeviceManagerUiLib/DeviceManagerUiLib.inf
      NULL|MdeModulePkg/Library/BootManagerUiLib/BootManagerUiLib.inf
      NULL|MdeModulePkg/Library/BootMaintenanceManagerUiLib/BootMaintenanceManagerUiLib.inf
  }

  Silicon/RISC-V/ProcessorPkg/Universal/FdtDxe/FdtDxe.inf {
    <LibraryClasses>
      RiscVCpuLib|Silicon/RISC-V/ProcessorPkg/Library/RiscVCpuLib/RiscVCpuLib.inf
  }

  # Device tree for K1
  Platform/Spacemit/K1/DeviceTree/K1DeviceTree.inf
  OrangePiPkg/Drivers/BareSatpOnExitDxe/BareSatpOnExitDxe.inf

  # FDT fixup protocol
  Silicon/Spacemit/Drivers/FdtFixupDxe/FdtFixupDxe.inf

  #
  # SD/MMC support
  #
  Silicon/Spacemit/Drivers/MmcDxe/MmcDxe.inf
  Silicon/Spacemit/K1/Drivers/SdHostDxe/SdHostDxe.inf

  #
  # USB Support
  #
  Silicon/Spacemit/Override/MdeModulePkg/Bus/Pci/EhciDxe/EhciDxe.inf
  MdeModulePkg/Bus/Pci/XhciDxe/XhciDxe.inf
  Silicon/Spacemit/Override/MdeModulePkg/Bus/Pci/NonDiscoverablePciDeviceDxe/NonDiscoverablePciDeviceDxe.inf
  MdeModulePkg/Bus/Usb/UsbBusDxe/UsbBusDxe.inf
  MdeModulePkg/Bus/Usb/UsbKbDxe/UsbKbDxe.inf
  MdeModulePkg/Bus/Usb/UsbMassStorageDxe/UsbMassStorageDxe.inf
  Silicon/Spacemit/K1/Drivers/UsbHcdInitDxe/UsbHcd.inf

  #
  # Spinor flash support
  #
  Silicon/Spacemit/K1/Drivers/QspiDxe/QspiDxe.inf
  Silicon/Spacemit/Drivers/Spi/SpiNorFlashDxe/SpiNorFlashDxe.inf

  #
  # GOP support
  #
  Silicon/Spacemit/K1/Drivers/HdmiGraphicsOutputDxe/HdmiGraphicsOutputDxe.inf

  # boot logo
  Silicon/Spacemit/Drivers/LogoDxe/LogoDxe.inf

  #
  # Firmware Volume Block service
  #
  Silicon/Spacemit/Drivers/FlashFvbDxe/FlashFvbDxe.inf

  #
  # I2C support
  #
  MdeModulePkg/Bus/I2c/I2cDxe/I2cDxe.inf
  Silicon/Spacemit/Drivers/I2CMaster/I2CMaster/I2cMaster.inf

  #
  # EEPROM support
  #
  Silicon/Spacemit/Drivers/I2CMaster/Eeprom/EepromDxe.inf

  # GPIO support
  #
  Silicon/Spacemit/K1/Drivers/GPIO/K1xGpioDxe.inf

  # LED support
  Silicon/Spacemit/K1/Drivers/LedControl/LedControl.inf

  # TLV EEPROM support
  Silicon/Spacemit/Drivers/I2CMaster/Tlv_Eeprom/TlvEeprom.inf

  # platform info
  Silicon/Spacemit/K1/Drivers/PlatformInfoDxe/PlatformInfoDxe.inf

  #
  # PCI Support
  #
  Silicon/Spacemit/Drivers/PciCpuIo2Dxe/PciCpuIo2Dxe.inf
  Silicon/Spacemit/Override/MdeModulePkg/Bus/Pci/PciHostBridgeDxe/PciHostBridgeDxe.inf {
    <LibraryClasses>
      NULL|Silicon/Spacemit/K1/Drivers/PcieHostBridgeControllerDxe/PcieHostBridgeControllerDxe.inf
  }
  Silicon/Spacemit/Override/MdeModulePkg/Bus/Pci/PciBusDxe/PciBusDxe.inf

  #
  # NVMe boot devices
  #
  MdeModulePkg/Bus/Pci/NvmExpressDxe/NvmExpressDxe.inf

  #
  # SMBIOS info
  #
  Silicon/Spacemit/K1/Drivers/PlatformSmbiosDxe/PlatformSmbiosDxe.inf

  #
  # HII
  #
