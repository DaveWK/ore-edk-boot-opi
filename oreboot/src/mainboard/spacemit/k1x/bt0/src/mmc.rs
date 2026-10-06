//! Minimal polled (PIO) eMMC / SD reader for the SpacemiT K1 SDHCI.
//!
//! eMMC is SDH2 at 0xd428_1000 (OrangePi R2S), SD is SDH0 at 0xd428_0000
//! (OrangePi RV2). When the mask ROM booted from the card, the controller is
//! clocked, pinmuxed and the card powered; for download-mode boots the
//! controller clock and reset are enabled here. The card is re-initialised at
//! a low clock in 1-bit mode and read in 512-byte sectors with CMD17. It is
//! only meant to fetch the next boot stage.

use util::mmio::{read16, read32, read8, write16, write32, write8};

pub const EMMC_BASE: usize = 0xd428_1000;
pub const SD_BASE: usize = 0xd428_0000;

// APMU clock/reset control (U-Boot include/soc/spacemit/k1-syscon.h)
const APMU: usize = 0xd428_2800;
// SDH AXI: bit 3 clock, bit 0 reset release; SDH0: bit 4 clock, bit 1 reset release
const APMU_SDH0_CLK_RES_CTRL: usize = APMU + 0x054;
// SDH2: bit 4 clock, bit 1 reset release
const APMU_SDH2_CLK_RES_CTRL: usize = APMU + 0x0e0;

// SDHCI registers
const BLOCK_SIZE: usize = 0x04;
const BLOCK_COUNT: usize = 0x06;
const ARGUMENT: usize = 0x08;
const TRANSFER_MODE: usize = 0x0c;
const COMMAND: usize = 0x0e;
const RESPONSE: usize = 0x10;
const BUFFER: usize = 0x20;
const PRESENT_STATE: usize = 0x24;
const HOST_CONTROL: usize = 0x28;
const POWER_CONTROL: usize = 0x29;
const CLOCK_CONTROL: usize = 0x2c;
const TIMEOUT_CONTROL: usize = 0x2e;
const SOFTWARE_RESET: usize = 0x2f;
const INT_STATUS: usize = 0x30;
const INT_ENABLE: usize = 0x34;
const SIGNAL_ENABLE: usize = 0x38;
const CAPABILITIES: usize = 0x40;

// SpacemiT K1 vendor registers (see U-Boot drivers/mmc/spacemit_sdhci.c)
const OP_EXT: usize = 0x108;
const MMC_CTRL: usize = 0x114;
const TX_CFG: usize = 0x11c;
const PHY_CTRL: usize = 0x160;
const PHY_PADCFG: usize = 0x178;

const PRESENT_CMD_INHIBIT: u32 = 1 << 0;
const PRESENT_DAT_INHIBIT: u32 = 1 << 1;

const INT_CMD_COMPLETE: u32 = 1 << 0;
const INT_XFER_COMPLETE: u32 = 1 << 1;
const INT_BUF_READ_READY: u32 = 1 << 5;
const INT_ERROR: u32 = 1 << 15;

const CLOCK_INT_EN: u16 = 1 << 0;
const CLOCK_INT_STABLE: u16 = 1 << 1;
const CLOCK_CARD_EN: u16 = 1 << 2;

// Command register flags
const RESP_NONE: u16 = 0;
const RESP_136: u16 = 1;
const RESP_48: u16 = 2;
const RESP_48_BUSY: u16 = 3;
const CHECK_CRC: u16 = 1 << 3;
const CHECK_INDEX: u16 = 1 << 4;
const DATA_PRESENT: u16 = 1 << 5;

const R1: u16 = RESP_48 | CHECK_CRC | CHECK_INDEX;
const R1B: u16 = RESP_48_BUSY | CHECK_CRC | CHECK_INDEX;
const R2: u16 = RESP_136 | CHECK_CRC;
const R3: u16 = RESP_48;
const R6: u16 = R1;
const R7: u16 = R1;

// RCA given to an eMMC card (SD cards publish their own).
const EMMC_RCA: u32 = 2;
const SECTOR: usize = 512;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Emmc,
    Sd,
}

/// eMMC hardware partitions (EXT_CSD PARTITION_ACCESS).
#[derive(Clone, Copy)]
pub enum Partition {
    User = 0,
    Boot0 = 1,
    Boot1 = 2,
}

#[derive(Debug)]
pub enum Error {
    Timeout(&'static str),
    Command(u8, u32),
    NotReady,
}

fn spin(n: usize) {
    for _ in 0..n {
        core::hint::spin_loop();
    }
}

fn wait<F: Fn() -> bool>(what: &'static str, done: F) -> Result<(), Error> {
    for _ in 0..50_000_000 {
        if done() {
            return Ok(());
        }
    }
    Err(Error::Timeout(what))
}


pub struct Mmc {
    base: usize,
    kind: Kind,
    rca: u32,
    // Sector (block) addressing: eMMC sector mode, SDHC/SDXC.
    block_addressing: bool,
}

impl Mmc {
    fn r8(&self, o: usize) -> u8 {
        read8(self.base + o)
    }
    fn w8(&self, o: usize, v: u8) {
        write8(self.base + o, v)
    }
    fn r16(&self, o: usize) -> u16 {
        read16(self.base + o)
    }
    fn w16(&self, o: usize, v: u16) {
        write16(self.base + o, v)
    }
    fn r32(&self, o: usize) -> u32 {
        read32(self.base + o)
    }
    fn w32(&self, o: usize, v: u32) {
        write32(self.base + o, v)
    }

    /// Base clock in MHz from the capabilities register; 0 means unknown.
    fn base_clock_mhz(&self) -> u32 {
        (self.r32(CAPABILITIES) >> 8) & 0xff
    }

    fn set_clock(&self, khz: u32) -> Result<(), Error> {
        let base = match self.base_clock_mhz() {
            0 => 200,
            b => b,
        } * 1000;
        // SDHCI v3 10-bit divided clock: f = base / (2 * N), N = 0 means base.
        let mut n = base.div_ceil(2 * khz);
        if n > 0x3ff {
            n = 0x3ff;
        }
        self.w16(CLOCK_CONTROL, 0);
        let div = (((n & 0xff) << 8) | ((n >> 8) & 0x3) << 6) as u16;
        self.w16(CLOCK_CONTROL, div | CLOCK_INT_EN);
        wait("internal clock", || self.r16(CLOCK_CONTROL) & CLOCK_INT_STABLE != 0)?;
        self.w16(CLOCK_CONTROL, div | CLOCK_INT_EN | CLOCK_CARD_EN);
        spin(100_000);
        Ok(())
    }


    fn command(&self, index: u8, arg: u32, flags: u16) -> Result<[u32; 4], Error> {
        let busy = flags & 3 == RESP_48_BUSY || flags & DATA_PRESENT != 0;
        wait("cmd inhibit", || self.r32(PRESENT_STATE) & PRESENT_CMD_INHIBIT == 0)?;
        if busy {
            wait("dat inhibit", || self.r32(PRESENT_STATE) & PRESENT_DAT_INHIBIT == 0)?;
        }
        self.w32(INT_STATUS, 0xffff_ffff);
        self.w32(ARGUMENT, arg);
        if flags & DATA_PRESENT != 0 {
            self.w16(BLOCK_SIZE, SECTOR as u16);
            self.w16(BLOCK_COUNT, 1);
            // Single block, card to host
            self.w16(TRANSFER_MODE, 1 << 4);
        } else {
            self.w16(TRANSFER_MODE, 0);
        }
        self.w16(COMMAND, ((index as u16) << 8) | flags);
        wait("command complete", || {
            self.r32(INT_STATUS) & (INT_CMD_COMPLETE | INT_ERROR) != 0
        })?;
        let status = self.r32(INT_STATUS);
        if status & INT_ERROR != 0 {
            self.w32(INT_STATUS, 0xffff_ffff);
            // Reset the command and data lines after an error.
            self.w8(SOFTWARE_RESET, 0x06);
            let _ = wait("reset", || self.r8(SOFTWARE_RESET) & 0x06 == 0);
            return Err(Error::Command(index, status));
        }
        self.w32(INT_STATUS, INT_CMD_COMPLETE);
        let resp = [
            self.r32(RESPONSE),
            self.r32(RESPONSE + 4),
            self.r32(RESPONSE + 8),
            self.r32(RESPONSE + 12),
        ];
        if flags & 3 == RESP_48_BUSY {
            wait("busy", || self.r32(INT_STATUS) & (INT_XFER_COMPLETE | INT_ERROR) != 0)?;
            self.w32(INT_STATUS, INT_XFER_COMPLETE);
        }
        Ok(resp)
    }


    /// EXT_CSD write with CMD6 (access: 1 = set bits, 2 = clear bits, 3 = write byte).
    fn switch(&self, access: u32, index: u32, value: u32) -> Result<(), Error> {
        self.command(6, (access << 24) | (index << 16) | (value << 8), R1B)?;
        // Wait until the card is back in the transfer state.
        for _ in 0..100_000 {
            let r = self.command(13, self.rca << 16, R1)?[0];
            if (r >> 9) & 0xf == 4 && r & (1 << 8) != 0 {
                return Ok(());
            }
        }
        Err(Error::NotReady)
    }


    /// Re-initialise the card in 1-bit legacy mode.
    pub fn init(kind: Kind) -> Result<Self, Error> {
        let base = match kind {
            Kind::Emmc => EMMC_BASE,
            Kind::Sd => SD_BASE,
        };
        let mut m = Self { base, kind, rca: 0, block_addressing: true };
        // The mask ROM does not touch a card it did not boot from (or any
        // card in download mode): make sure the controller is clocked and
        // out of reset.
        write32(APMU_SDH0_CLK_RES_CTRL, read32(APMU_SDH0_CLK_RES_CTRL) | (1 << 3) | (1 << 0));
        match kind {
            Kind::Emmc => write32(
                APMU_SDH2_CLK_RES_CTRL,
                read32(APMU_SDH2_CLK_RES_CTRL) | (1 << 4) | (1 << 1),
            ),
            Kind::Sd => write32(
                APMU_SDH0_CLK_RES_CTRL,
                read32(APMU_SDH0_CLK_RES_CTRL) | (1 << 4) | (1 << 1),
            ),
        }
        spin(100_000);
        // Controller set-up as U-Boot's spacemit_sdhci_phy_init() does.
        match kind {
            Kind::Emmc => {
                // eMMC card mode and the PHY in functional mode.
                m.w32(MMC_CTRL, m.r32(MMC_CTRL) | (1 << 12));
                m.w32(PHY_CTRL, m.r32(PHY_CTRL) | 0x3);
                m.w32(PHY_PADCFG, m.r32(PHY_PADCFG) | (1 << 5));
            }
            Kind::Sd => {
                m.w32(TX_CFG, m.r32(TX_CFG) | (1 << 30));
            }
        }
        // Keep the card clock running while we poll.
        m.w32(OP_EXT, m.r32(OP_EXT) | (1 << 11) | (1 << 12));

        m.w8(SOFTWARE_RESET, 0x06);
        wait("reset", || m.r8(SOFTWARE_RESET) & 0x06 == 0)?;
        m.w32(INT_ENABLE, 0xffff_ffff);
        m.w32(SIGNAL_ENABLE, 0);
        m.w8(TIMEOUT_CONTROL, 0x0e);
        // 1-bit, normal speed, no DMA.
        m.w8(HOST_CONTROL, 0);
        if m.r8(POWER_CONTROL) & 1 == 0 {
            // The bus is unpowered: eMMC I/O is 1.8 V, SD starts at 3.3 V.
            let volts = match kind {
                Kind::Emmc => 0x5,
                Kind::Sd => 0x7,
            };
            m.w8(POWER_CONTROL, (volts << 1) | 1);
            spin(1_000_000);
        }
        println!(
            "[mmc] {} base clock {} MHz, power {:02x}",
            if kind == Kind::Emmc { "eMMC" } else { "SD" },
            m.base_clock_mhz(),
            m.r8(POWER_CONTROL)
        );
        m.set_clock(400)?;

        m.command(0, 0, RESP_NONE)?;
        spin(1_000_000);
        match kind {
            Kind::Emmc => m.init_emmc()?,
            Kind::Sd => m.init_sd()?,
        }
        m.command(7, m.rca << 16, R1B)?;
        if !m.block_addressing {
            m.command(16, SECTOR as u32, R1)?;
        }
        m.set_clock(25_000)?;
        println!("[mmc] card ready (RCA {})", m.rca);
        Ok(m)
    }

    fn init_emmc(&mut self) -> Result<(), Error> {
        // Sector addressing, 1.7-1.95 V and 2.7-3.6 V.
        let mut ready = false;
        for _ in 0..1000 {
            let ocr = self.command(1, 0x40ff_8080, R3)?[0];
            if ocr & (1 << 31) != 0 {
                ready = true;
                break;
            }
            spin(1_000_000);
        }
        if !ready {
            return Err(Error::NotReady);
        }
        self.command(2, 0, R2)?;
        self.rca = EMMC_RCA;
        self.command(3, self.rca << 16, R1)?;
        Ok(())
    }

    fn init_sd(&mut self) -> Result<(), Error> {
        // CMD8: 2.7-3.6 V, check pattern 0xaa. Version 1 cards do not answer.
        let v2 = match self.command(8, 0x1aa, R7) {
            Ok(r) => r[0] & 0xfff == 0x1aa,
            Err(_) => false,
        };
        let hcs = if v2 { 1 << 30 } else { 0 };
        let mut ocr = 0;
        for _ in 0..1000 {
            self.command(55, 0, R1)?;
            ocr = self.command(41, hcs | 0x00ff_8000, R3)?[0];
            if ocr & (1 << 31) != 0 {
                break;
            }
            spin(1_000_000);
        }
        if ocr & (1 << 31) == 0 {
            return Err(Error::NotReady);
        }
        // CCS: SDHC/SDXC use block addresses.
        self.block_addressing = ocr & (1 << 30) != 0;
        self.command(2, 0, R2)?;
        self.rca = self.command(3, 0, R6)?[0] >> 16;
        Ok(())
    }

    /// Select an eMMC hardware partition, keeping the boot configuration bits.
    pub fn select(&self, p: Partition) -> Result<(), Error> {
        if self.kind != Kind::Emmc {
            return Ok(());
        }
        // PARTITION_CONFIG (EXT_CSD 179): clear PARTITION_ACCESS, then set it.
        self.switch(2, 179, 0x07)?;
        if p as u32 != 0 {
            self.switch(1, 179, p as u32)?;
        }
        Ok(())
    }

    /// Read `count` sectors starting at `lba` into memory at `dest`.
    pub fn read(&self, lba: u32, count: usize, dest: usize) -> Result<(), Error> {
        for i in 0..count {
            let sector = lba + i as u32;
            let arg = if self.block_addressing { sector } else { sector * SECTOR as u32 };
            self.command(17, arg, R1 | DATA_PRESENT)?;
            wait("read buffer", || {
                self.r32(INT_STATUS) & (INT_BUF_READ_READY | INT_ERROR) != 0
            })?;
            let status = self.r32(INT_STATUS);
            if status & INT_ERROR != 0 {
                self.w32(INT_STATUS, 0xffff_ffff);
                return Err(Error::Command(17, status));
            }
            self.w32(INT_STATUS, INT_BUF_READ_READY);
            let base = dest + i * SECTOR;
            for w in 0..SECTOR / 4 {
                write32(base + w * 4, self.r32(BUFFER));
            }
            wait("transfer complete", || {
                self.r32(INT_STATUS) & (INT_XFER_COMPLETE | INT_ERROR) != 0
            })?;
            self.w32(INT_STATUS, INT_XFER_COMPLETE);
            if i % 2048 == 2047 {
                print!(".");
            }
        }
        Ok(())
    }
}
