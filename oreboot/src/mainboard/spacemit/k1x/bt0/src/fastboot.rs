//! fastboot over USB, for writing the board's storage from a workstation.
//!
//! bt0 serves this when the BootROM started it from USB download mode
//! (`fastboot stage bt0.bin && fastboot continue`): it re-enumerates as a
//! fastboot device and takes
//!
//!   fastboot getvar <name>
//!   fastboot flash <target> <file>   raw or Android sparse images
//!   fastboot continue | reboot       boot the board's normal chain
//!
//! Targets: `emmc` (eMMC user area), `boot0` and `boot1` (eMMC boot
//! partitions), `sd` (microSD card), and `disk`, the board's boot medium.
//! Images larger than max-download-size arrive as several sparse pieces,
//! which the fastboot tool makes itself.

use core::fmt::Write;

use crate::mmc::{Kind, Mmc, Partition};
use crate::usb::{Descriptors, Usb};

const SECTOR: usize = 512;
/// Download buffer in DRAM.
const BUF: usize = 0x1000_0000;
const MAX_DOWNLOAD: usize = 0x2000_0000;
/// Pattern buffer for sparse FILL chunks, after the download buffer.
const FILL_BUF: usize = BUF + MAX_DOWNLOAD;
const FILL_LEN: usize = 1 << 20;

const SPARSE_MAGIC: u32 = 0xed26_ff3a;
const CHUNK_RAW: u16 = 0xcac1;
const CHUNK_FILL: u16 = 0xcac2;
const CHUNK_DONT_CARE: u16 = 0xcac3;
const CHUNK_CRC32: u16 = 0xcac4;

pub struct Board {
    /// Reported as `product`.
    pub name: &'static str,
    /// The medium `disk` names.
    pub disk: Kind,
}

/// A response line: OKAY, FAIL, INFO or DATA plus up to 60 bytes.
struct Line {
    buf: [u8; 64],
    len: usize,
}

impl Line {
    fn new(kind: &str) -> Self {
        let mut l = Self { buf: [0; 64], len: 0 };
        let _ = l.write_str(kind);
        l
    }
}

impl Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let n = s.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

fn reply(usb: &mut Usb, kind: &str, msg: core::fmt::Arguments) {
    let mut l = Line::new(kind);
    let _ = l.write_fmt(msg);
    if kind != "INFO" {
        println!("[fastboot] {}", core::str::from_utf8(&l.buf[..l.len]).unwrap_or("?"));
    }
    let _ = usb.send(&l.buf[..l.len]);
}

fn target(name: &str, disk: Kind) -> Option<(Kind, Partition)> {
    Some(match name {
        "emmc" | "user" | "mmc0" => (Kind::Emmc, Partition::User),
        "boot0" => (Kind::Emmc, Partition::Boot0),
        "boot1" => (Kind::Emmc, Partition::Boot1),
        "sd" => (Kind::Sd, Partition::User),
        "disk" => (disk, Partition::User),
        _ => return None,
    })
}

fn le16(a: usize) -> u16 {
    unsafe { core::ptr::read_unaligned(a as *const u16) }
}

fn le32(a: usize) -> u32 {
    unsafe { core::ptr::read_unaligned(a as *const u32) }
}

/// The open card: initialised once per medium, with the bus widened.
struct Disk {
    kind: Kind,
    mmc: Mmc,
    width: u8,
    /// Index into FALLBACK once a write has failed.
    fallback: usize,
}

/// Bus settings tried in turn when a write fails: width, TX hold time,
/// clock in kHz.
const FALLBACK: [(u8, bool, u32); 7] = [
    (4, true, 25_000),
    (1, true, 25_000),
    (1, false, 25_000),
    (4, false, 25_000),
    (8, false, 25_000),
    (1, true, 5_000),
    (1, false, 5_000),
];

impl Disk {
    fn open(slot: &mut Option<Disk>, kind: Kind) -> Result<&mut Disk, &'static str> {
        if slot.as_ref().map(|d| d.kind) != Some(kind) {
            *slot = None;
            let mut mmc = Mmc::init(kind).map_err(|_| "card did not initialise")?;
            let width = mmc.widen();
            println!("[fastboot] {}-bit bus", width);
            *slot = Some(Disk { kind, mmc, width, fallback: 0 });
        }
        Ok(slot.as_mut().unwrap())
    }

    /// Write, stepping through FALLBACK when the card rejects the data.
    fn write(&mut self, lba: u32, count: usize, src: usize) -> Result<(), &'static str> {
        loop {
            match self.mmc.write(lba, count, src) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    println!("[fastboot] write error at {lba} ({}-bit): {e:?}", self.width);
                    loop {
                        if self.fallback == FALLBACK.len() {
                            return Err("write failed (see the serial console)");
                        }
                        let (width, hold, khz) = FALLBACK[self.fallback];
                        self.fallback += 1;
                        if width > 4 && self.kind == Kind::Sd {
                            continue;
                        }
                        self.mmc.set_tx_hold(hold);
                        if self.mmc.set_width(width).is_ok() && self.mmc.set_khz(khz).is_ok() {
                            self.width = width;
                            println!("[fastboot] retrying: {width}-bit bus, TX hold {hold}, {khz} kHz");
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// Capacity in sectors, when the card reports it (eMMC).
fn capacity(d: &Disk, part: Partition) -> Option<u64> {
    match d.kind {
        Kind::Emmc => d.mmc.sectors(part).ok(),
        Kind::Sd => None,
    }
}

fn write_raw(usb: &mut Usb, d: &mut Disk, lba: u64, src: usize, len: usize) -> Result<(), &'static str> {
    let sectors = len.div_ceil(SECTOR);
    let mut done = 0;
    // 32 MiB at a time, with progress for the host.
    while done < sectors {
        let n = (sectors - done).min(65536);
        d.write((lba + done as u64) as u32, n, src + done * SECTOR)?;
        done += n;
        if sectors > 65536 {
            reply(usb, "INFO", format_args!("{} of {} MiB", done >> 11, sectors >> 11));
        }
    }
    Ok(())
}

fn fill(value: u32) {
    for i in 0..FILL_LEN / 4 {
        unsafe { core::ptr::write_volatile((FILL_BUF + 4 * i) as *mut u32, value) };
    }
}

fn write_sparse(usb: &mut Usb, d: &mut Disk, cap: Option<u64>, len: usize) -> Result<(), &'static str> {
    let hdr_size = le16(BUF + 8) as usize;
    let chunk_hdr = le16(BUF + 10) as usize;
    let blk = le32(BUF + 12) as usize;
    let total_blks = le32(BUF + 16) as u64;
    let chunks = le32(BUF + 20);
    if le16(BUF + 4) != 1 || hdr_size < 28 || chunk_hdr < 12 || blk == 0 || blk % SECTOR != 0 {
        return Err("unsupported sparse image");
    }
    let spb = (blk / SECTOR) as u64;
    if let Some(c) = cap {
        if total_blks * spb > c {
            return Err("image larger than the partition");
        }
    }
    let mut off = hdr_size;
    let mut at = 0u64; // in blocks
    let mut pattern = None;
    for _ in 0..chunks {
        if off + chunk_hdr > len {
            return Err("sparse image truncated");
        }
        let kind = le16(BUF + off);
        let count = le32(BUF + off + 4) as u64;
        let total = le32(BUF + off + 8) as usize;
        let data = BUF + off + chunk_hdr;
        let bytes = count as usize * blk;
        match kind {
            CHUNK_RAW => {
                if total != chunk_hdr + bytes || off + total > len {
                    return Err("bad raw chunk");
                }
                write_raw(usb, d, at * spb, data, bytes)?;
            }
            CHUNK_FILL => {
                let value = le32(data);
                if pattern != Some(value) {
                    fill(value);
                    pattern = Some(value);
                }
                let mut left = bytes;
                let mut lba = at * spb;
                while left > 0 {
                    let n = left.min(FILL_LEN);
                    d.write(lba as u32, n / SECTOR, FILL_BUF)?;
                    lba += (n / SECTOR) as u64;
                    left -= n;
                }
            }
            CHUNK_DONT_CARE | CHUNK_CRC32 => {}
            _ => return Err("unknown sparse chunk"),
        }
        at += count;
        off += total;
    }
    Ok(())
}

fn flash(usb: &mut Usb, disk: &mut Option<Disk>, board: &Board, name: &str, len: usize) -> Result<(), &'static str> {
    if len == 0 {
        return Err("download an image first");
    }
    let (kind, part) = target(name, board.disk).ok_or("unknown target")?;
    let d = Disk::open(disk, kind)?;
    if kind == Kind::Emmc {
        d.mmc.select(part).map_err(|_| "cannot select the eMMC partition")?;
    }
    let cap = capacity(d, part);
    if len >= 28 && le32(BUF) == SPARSE_MAGIC {
        return write_sparse(usb, d, cap, len);
    }
    if let Some(c) = cap {
        if len.div_ceil(SECTOR) as u64 > c {
            return Err("image larger than the partition");
        }
    }
    // Zero the tail of a partial last sector.
    for a in BUF + len..BUF + len.next_multiple_of(SECTOR) {
        unsafe { core::ptr::write_volatile(a as *mut u8, 0) };
    }
    write_raw(usb, d, 0, BUF, len)
}

fn getvar(usb: &mut Usb, disk: &mut Option<Disk>, board: &Board, var: &str) {
    match var {
        "version" => reply(usb, "OKAY", format_args!("0.4")),
        "product" => reply(usb, "OKAY", format_args!("{}", board.name)),
        "serialno" => reply(usb, "OKAY", format_args!("k1")),
        "max-download-size" => reply(usb, "OKAY", format_args!("0x{:08x}", MAX_DOWNLOAD)),
        "is-userspace" | "secure" | "unlocked" => {
            reply(usb, "OKAY", format_args!("{}", if var == "unlocked" { "yes" } else { "no" }))
        }
        _ => {
            if let Some(p) = var.strip_prefix("partition-type:") {
                if target(p, board.disk).is_some() {
                    return reply(usb, "OKAY", format_args!("raw"));
                }
            } else if let Some(p) = var.strip_prefix("partition-size:") {
                if let Some((kind, part)) = target(p, board.disk) {
                    if let Ok(d) = Disk::open(disk, kind) {
                        if let Some(c) = capacity(d, part) {
                            return reply(usb, "OKAY", format_args!("0x{:x}", c * SECTOR as u64));
                        }
                    }
                }
            } else if var.starts_with("has-slot:") || var.starts_with("is-logical:") {
                return reply(usb, "OKAY", format_args!("no"));
            }
            reply(usb, "FAIL", format_args!("unknown variable"))
        }
    }
}

fn hex(s: &str) -> Option<usize> {
    usize::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}

/// Serve fastboot until the host sends `continue` or `reboot`.
pub fn run(board: &Board) {
    let mut usb = Usb::init(Descriptors {
        vendor: 0x18d1, // the IDs the fastboot tool and U-Boot use
        product: 0x4ee0,
        class: [0xff, 0x42, 0x03],
        strings: ["oreboot", board.name, "k1", "fastboot"],
    });
    let mut disk: Option<Disk> = None;
    let mut downloaded = 0;
    let mut cmd = [0u8; 64];
    loop {
        usb.wait_configured();
        let n = match usb.recv(&mut cmd) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let c = core::str::from_utf8(&cmd[..n]).unwrap_or("");
        println!("[fastboot] < {c}");
        if let Some(var) = c.strip_prefix("getvar:") {
            getvar(&mut usb, &mut disk, board, var);
        } else if let Some(size) = c.strip_prefix("download:") {
            match hex(size) {
                Some(len) if len > 0 && len <= MAX_DOWNLOAD => {
                    reply(&mut usb, "DATA", format_args!("{:08x}", len));
                    match usb.recv_into(BUF, len) {
                        Ok(()) => {
                            downloaded = len;
                            reply(&mut usb, "OKAY", format_args!(""));
                        }
                        Err(e) => {
                            downloaded = 0;
                            println!("[fastboot] download failed: {e:?}");
                        }
                    }
                }
                _ => reply(&mut usb, "FAIL", format_args!("bad download size")),
            }
        } else if let Some(name) = c.strip_prefix("flash:") {
            match flash(&mut usb, &mut disk, board, name, downloaded) {
                Ok(()) => reply(&mut usb, "OKAY", format_args!("")),
                Err(e) => reply(&mut usb, "FAIL", format_args!("{e}")),
            }
        } else if c == "continue" || c.starts_with("reboot") {
            reply(&mut usb, "OKAY", format_args!(""));
            usb.disconnect();
            return;
        } else {
            reply(&mut usb, "FAIL", format_args!("unknown command"));
        }
    }
}
