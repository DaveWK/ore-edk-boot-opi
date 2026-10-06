//! Minimal polled (PIO) eMMC reader for the SpacemiT K1 SDHCI at 0xd428_1000.
//!
//! The mask ROM has already clocked, reset and pinmuxed the controller and
//! powered the card to load bt0 from boot0. This re-initialises the card at
//! a low clock in 1-bit mode, selects a hardware partition, and reads 512-byte
//! sectors with CMD17. It is only meant to fetch the next boot stage.

use util::mmio::{read16, read32, read8, write16, write32, write8};

const BASE: usize = 0xd428_1000;

// APMU clock/reset control (U-Boot include/soc/spacemit/k1-syscon.h)
const APMU: usize = 0xd428_2800;
const APMU_SDH0_CLK_RES_CTRL: usize = APMU + 0x054; // SDH AXI: bit 3 clock, bit 0 reset release
const APMU_SDH2_CLK_RES_CTRL: usize = APMU + 0x0e0; // SDH2: bit 4 clock, bit 1 reset release

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

const RCA: u32 = 2;
const SECTOR: usize = 512;

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

fn r8(o: usize) -> u8 {
    read8(BASE + o)
}
fn w8(o: usize, v: u8) {
    write8(BASE + o, v)
}
fn r16(o: usize) -> u16 {
    read16(BASE + o)
}
fn w16(o: usize, v: u16) {
    write16(BASE + o, v)
}
fn r32(o: usize) -> u32 {
    read32(BASE + o)
}
fn w32(o: usize, v: u32) {
    write32(BASE + o, v)
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

/// Base clock in MHz from the capabilities register; 0 means unknown.
fn base_clock_mhz() -> u32 {
    (r32(CAPABILITIES) >> 8) & 0xff
}

fn set_clock(khz: u32) -> Result<(), Error> {
    let base = match base_clock_mhz() {
        0 => 200,
        b => b,
    } * 1000;
    // SDHCI v3 10-bit divided clock: f = base / (2 * N), N = 0 means base.
    let mut n = base.div_ceil(2 * khz);
    if n > 0x3ff {
        n = 0x3ff;
    }
    w16(CLOCK_CONTROL, 0);
    let div = (((n & 0xff) << 8) | ((n >> 8) & 0x3) << 6) as u16;
    w16(CLOCK_CONTROL, div | CLOCK_INT_EN);
    wait("internal clock", || r16(CLOCK_CONTROL) & CLOCK_INT_STABLE != 0)?;
    w16(CLOCK_CONTROL, div | CLOCK_INT_EN | CLOCK_CARD_EN);
    spin(100_000);
    Ok(())
}

fn command(index: u8, arg: u32, flags: u16) -> Result<[u32; 4], Error> {
    let busy = flags & 3 == RESP_48_BUSY || flags & DATA_PRESENT != 0;
    wait("cmd inhibit", || r32(PRESENT_STATE) & PRESENT_CMD_INHIBIT == 0)?;
    if busy {
        wait("dat inhibit", || r32(PRESENT_STATE) & PRESENT_DAT_INHIBIT == 0)?;
    }
    w32(INT_STATUS, 0xffff_ffff);
    w32(ARGUMENT, arg);
    if flags & DATA_PRESENT != 0 {
        w16(BLOCK_SIZE, SECTOR as u16);
        w16(BLOCK_COUNT, 1);
        // Single block, card to host
        w16(TRANSFER_MODE, 1 << 4);
    } else {
        w16(TRANSFER_MODE, 0);
    }
    w16(COMMAND, ((index as u16) << 8) | flags);
    wait("command complete", || {
        r32(INT_STATUS) & (INT_CMD_COMPLETE | INT_ERROR) != 0
    })?;
    let status = r32(INT_STATUS);
    if status & INT_ERROR != 0 {
        w32(INT_STATUS, 0xffff_ffff);
        // Reset the command and data lines after an error.
        w8(SOFTWARE_RESET, 0x06);
        let _ = wait("reset", || r8(SOFTWARE_RESET) & 0x06 == 0);
        return Err(Error::Command(index, status));
    }
    w32(INT_STATUS, INT_CMD_COMPLETE);
    let resp = [
        r32(RESPONSE),
        r32(RESPONSE + 4),
        r32(RESPONSE + 8),
        r32(RESPONSE + 12),
    ];
    if flags & 3 == RESP_48_BUSY {
        wait("busy", || r32(INT_STATUS) & (INT_XFER_COMPLETE | INT_ERROR) != 0)?;
        w32(INT_STATUS, INT_XFER_COMPLETE);
    }
    Ok(resp)
}

/// EXT_CSD write with CMD6 (access: 1 = set bits, 2 = clear bits, 3 = write byte).
fn switch(access: u32, index: u32, value: u32) -> Result<(), Error> {
    command(6, (access << 24) | (index << 16) | (value << 8), R1B)?;
    // Wait until the card is back in the transfer state.
    for _ in 0..100_000 {
        let r = command(13, RCA << 16, R1)?[0];
        if (r >> 9) & 0xf == 4 && r & (1 << 8) != 0 {
            return Ok(());
        }
    }
    Err(Error::NotReady)
}

pub struct Emmc;

impl Emmc {
    /// Re-initialise the eMMC card in 1-bit legacy mode.
    pub fn init() -> Result<Self, Error> {
        // The mask ROM does not touch the eMMC when it enters download
        // mode, so make sure the controller is clocked and out of reset.
        write32(APMU_SDH0_CLK_RES_CTRL, read32(APMU_SDH0_CLK_RES_CTRL) | (1 << 3) | (1 << 0));
        write32(APMU_SDH2_CLK_RES_CTRL, read32(APMU_SDH2_CLK_RES_CTRL) | (1 << 4) | (1 << 1));
        spin(100_000);
        // eMMC card mode and the PHY in functional mode, as U-Boot does.
        w32(MMC_CTRL, r32(MMC_CTRL) | (1 << 12));
        w32(PHY_CTRL, r32(PHY_CTRL) | 0x3);
        w32(PHY_PADCFG, r32(PHY_PADCFG) | (1 << 5));
        // Keep the card clock running while we poll.
        w32(OP_EXT, r32(OP_EXT) | (1 << 11) | (1 << 12));

        w8(SOFTWARE_RESET, 0x06);
        wait("reset", || r8(SOFTWARE_RESET) & 0x06 == 0)?;
        w32(INT_ENABLE, 0xffff_ffff);
        w32(SIGNAL_ENABLE, 0);
        w8(TIMEOUT_CONTROL, 0x0e);
        // 1-bit, normal speed, no DMA.
        w8(HOST_CONTROL, 0);
        if r8(POWER_CONTROL) & 1 == 0 {
            // The mask ROM left the bus unpowered; select 1.8 V and power on.
            w8(POWER_CONTROL, (0x5 << 1) | 1);
            spin(1_000_000);
        }
        println!(
            "[emmc] base clock {} MHz, power {:02x}",
            base_clock_mhz(),
            r8(POWER_CONTROL)
        );
        set_clock(400)?;

        command(0, 0, RESP_NONE)?;
        spin(1_000_000);
        // Sector addressing, 1.7-1.95 V and 2.7-3.6 V.
        let mut ready = false;
        for _ in 0..1000 {
            let ocr = command(1, 0x40ff_8080, R3)?[0];
            if ocr & (1 << 31) != 0 {
                ready = true;
                break;
            }
            spin(1_000_000);
        }
        if !ready {
            return Err(Error::NotReady);
        }
        command(2, 0, R2)?;
        command(3, RCA << 16, R1)?;
        command(7, RCA << 16, R1B)?;
        set_clock(25_000)?;
        println!("[emmc] card ready (RCA {RCA})");
        Ok(Self)
    }

    /// Select a hardware partition, keeping the boot configuration bits.
    pub fn select(&self, p: Partition) -> Result<(), Error> {
        // PARTITION_CONFIG (EXT_CSD 179): clear PARTITION_ACCESS, then set it.
        switch(2, 179, 0x07)?;
        if p as u32 != 0 {
            switch(1, 179, p as u32)?;
        }
        Ok(())
    }

    /// Read `count` sectors starting at `lba` into memory at `dest`.
    pub fn read(&self, lba: u32, count: usize, dest: usize) -> Result<(), Error> {
        for i in 0..count {
            command(17, lba + i as u32, R1 | DATA_PRESENT)?;
            wait("read buffer", || {
                r32(INT_STATUS) & (INT_BUF_READ_READY | INT_ERROR) != 0
            })?;
            let status = r32(INT_STATUS);
            if status & INT_ERROR != 0 {
                w32(INT_STATUS, 0xffff_ffff);
                return Err(Error::Command(17, status));
            }
            w32(INT_STATUS, INT_BUF_READ_READY);
            let base = dest + i * SECTOR;
            for w in 0..SECTOR / 4 {
                write32(base + w * 4, r32(BUFFER));
            }
            wait("transfer complete", || {
                r32(INT_STATUS) & (INT_XFER_COMPLETE | INT_ERROR) != 0
            })?;
            w32(INT_STATUS, INT_XFER_COMPLETE);
            if i % 2048 == 2047 {
                print!(".");
            }
        }
        Ok(())
    }
}
