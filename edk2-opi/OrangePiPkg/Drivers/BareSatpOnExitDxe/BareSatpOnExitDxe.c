/** @file
  Return to bare (MMU off) translation at ExitBootServices().

  CpuDxe enables paging with page tables in boot services memory. After
  ExitBootServices() that memory belongs to the OS loader, which may
  overwrite it while still translating through those tables: FreeBSD's
  riscv loader copies the kernel into place at that point and dies with no
  output. U-Boot never enables paging, and Linux's EFI stub clears satp
  itself; do the same here so any OS loader starts from bare translation.
  The mapping is the identity, so execution continues at the same addresses.

  SPDX-License-Identifier: BSD-2-Clause-Patent
**/

#include <Uefi.h>
#include <Library/BaseLib.h>
#include <Library/BaseRiscVMmuLib.h>
#include <Library/DebugLib.h>
#include <Library/UefiBootServicesTableLib.h>

STATIC EFI_EVENT  mExitBootServicesEvent;

STATIC
VOID
EFIAPI
BareSatpOnExit (
  IN EFI_EVENT  Event,
  IN VOID       *Context
  )
{
  RiscVSetSupervisorAddressTranslationRegister (0);
  RiscVLocalTlbFlushAll ();
}

EFI_STATUS
EFIAPI
BareSatpOnExitDxeEntry (
  IN EFI_HANDLE        ImageHandle,
  IN EFI_SYSTEM_TABLE  *SystemTable
  )
{
  EFI_STATUS  Status;

  Status = gBS->CreateEvent (
                  EVT_SIGNAL_EXIT_BOOT_SERVICES,
                  TPL_NOTIFY,
                  BareSatpOnExit,
                  NULL,
                  &mExitBootServicesEvent
                  );
  ASSERT_EFI_ERROR (Status);
  return Status;
}
