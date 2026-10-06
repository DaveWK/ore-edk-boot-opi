//! Load a U-Boot style FIT (external data, `mkimage -E`) from an eMMC hardware
//! partition or raw SD card sectors, and start OpenSBI's fw_dynamic firmware
//! with the FIT's next stage and DT.

use core::arch::asm;
use core::ptr::{addr_of, copy_nonoverlapping};
use fdt::Fdt;

use crate::mmc::{Error as MmcError, Kind, Mmc, Partition};

const SECTOR: usize = 512;
// Largest FIT read (an eMMC boot partition, or the RV2's 4 MiB SD area).
const MAX_FIT: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub enum Error {
    Mmc(MmcError),
    NotFit,
    Missing(&'static str),
    TooBig(usize),
}

impl From<MmcError> for Error {
    fn from(e: MmcError) -> Self {
        Self::Mmc(e)
    }
}

pub struct Boot {
    pub firmware: usize,
    pub next: usize,
    pub fdt: usize,
}

fn be32(addr: usize) -> u32 {
    u32::from_be(unsafe { core::ptr::read_volatile(addr as *const u32) })
}

struct Image {
    offset: usize,
    size: usize,
    load: Option<usize>,
}

fn image(fit: &Fdt, name: &str) -> Result<Image, Error> {
    let node = fit
        .find_node("/images")
        .and_then(|i| i.children().find(|n| n.name == name))
        .ok_or(Error::Missing("image node"))?;
    let num = |p: &str| node.property(p).and_then(|v| v.as_usize());
    Ok(Image {
        offset: num("data-offset").ok_or(Error::Missing("data-offset"))?,
        size: num("data-size").ok_or(Error::Missing("data-size"))?,
        load: num("load"),
    })
}

/// Memory-mapped (XIP) window of the QSPI NOR flash.
const NOR_MMIO_BASE: usize = 0xb800_0000;

/// Where the FIT is: an MMC card (type and eMMC hardware partition) or the
/// memory-mapped SPI NOR, and its first 512-byte sector there.
pub enum Medium {
    Mmc(Kind, Partition),
    Nor,
}

pub struct Source {
    pub medium: Medium,
    pub lba: u32,
}

/// Reads sectors from an MMC card, or copies them out of the NOR window.
enum Reader {
    Mmc(Mmc),
    Nor,
}

impl Reader {
    fn read(&self, lba: u32, count: usize, dest: usize) -> Result<(), Error> {
        match self {
            Reader::Mmc(m) => Ok(m.read(lba, count, dest)?),
            Reader::Nor => {
                let src = NOR_MMIO_BASE + lba as usize * SECTOR;
                unsafe { copy_nonoverlapping(src as *const u8, dest as *mut u8, count * SECTOR) };
                Ok(())
            }
        }
    }

    fn select(&self, p: Partition) -> Result<(), Error> {
        match self {
            Reader::Mmc(m) => Ok(m.select(p)?),
            Reader::Nor => Ok(()),
        }
    }
}

pub fn load(src: &Source, staging: usize) -> Result<Boot, Error> {
    let mmc = match src.medium {
        Medium::Mmc(kind, partition) => {
            let m = Mmc::init(kind)?;
            m.select(partition)?;
            Reader::Mmc(m)
        }
        Medium::Nor => Reader::Nor,
    };
    // FIT header first, to learn its size and where the image data ends.
    mmc.read(src.lba, 1, staging)?;
    if be32(staging) != 0xd00d_feed {
        let _ = mmc.select(Partition::User);
        return Err(Error::NotFit);
    }
    let header = be32(staging + 4) as usize;
    let data_base = (header + 3) & !3;
    mmc.read(src.lba, header.div_ceil(SECTOR), staging)?;
    let fit = Fdt::new(unsafe { core::slice::from_raw_parts(staging as *const u8, header) })
        .map_err(|_| Error::NotFit)?;
    let conf = fit
        .find_node("/configurations")
        .ok_or(Error::Missing("configurations"))?;
    let default = conf
        .property("default")
        .and_then(|p| p.as_str())
        .ok_or(Error::Missing("default configuration"))?;
    let c = conf
        .children()
        .find(|n| n.name == default)
        .ok_or(Error::Missing("configuration node"))?;
    let s = |p: &'static str| c.property(p).and_then(|v| v.as_str()).ok_or(Error::Missing(p));
    let (fw, next, dt) = (image(&fit, s("firmware")?)?, image(&fit, s("loadables")?)?, image(&fit, s("fdt")?)?);
    let end = [&fw, &next, &dt]
        .iter()
        .map(|i| data_base + i.offset + i.size)
        .max()
        .unwrap();
    if end > MAX_FIT {
        let _ = mmc.select(Partition::User);
        return Err(Error::TooBig(end));
    }
    println!("[bt0] reading FIT at sector {} ({end} bytes)", src.lba);
    mmc.read(src.lba, end.div_ceil(SECTOR), staging)?;
    mmc.select(Partition::User)?;

    let place = |i: &Image, dest: usize| {
        unsafe {
            copy_nonoverlapping((staging + data_base + i.offset) as *const u8, dest as *mut u8, i.size)
        };
        dest
    };
    let firmware = place(&fw, fw.load.ok_or(Error::Missing("firmware load"))?);
    let next_addr = place(&next, next.load.ok_or(Error::Missing("loadable load"))?);
    // DT right after the next stage, as U-Boot's SPL places it.
    let fdt_addr = place(&dt, (next_addr + next.size + 0xffff) & !0xffff);
    Ok(Boot { firmware, next: next_addr, fdt: fdt_addr })
}

// OpenSBI include/sbi/fw_dynamic.h
#[repr(C)]
struct FwDynamicInfo {
    magic: usize,
    version: usize,
    next_addr: usize,
    next_mode: usize,
    options: usize,
    boot_hart: usize,
}

static mut FW_DYNAMIC_INFO: FwDynamicInfo = FwDynamicInfo {
    magic: 0x4942_534f,
    version: 2,
    next_addr: 0,
    next_mode: 1, // S-mode
    options: 0,
    boot_hart: 0,
};

pub fn start_opensbi(hart: usize, b: &Boot) -> ! {
    unsafe {
        FW_DYNAMIC_INFO.next_addr = b.next;
        FW_DYNAMIC_INFO.boot_hart = hart;
        asm!(
            "fence.i",
            "jr {entry}",
            entry = in(reg) b.firmware,
            in("a0") hart,
            in("a1") b.fdt,
            in("a2") addr_of!(FW_DYNAMIC_INFO) as usize,
            options(noreturn)
        );
    }
}
