#![no_std]
#![no_main]
// TODO: remove when done debugging crap
#![allow(unused)]

#[macro_use]
extern crate log;

use core::{
    arch::{asm, naked_asm},
    intrinsics::transmute,
    panic::PanicInfo,
    ptr::{self, addr_of, addr_of_mut},
    slice::from_raw_parts as slice_from,
};

use embedded_hal_nb::serial::Write;
use riscv::register::{marchid, mhartid, mimpid, mip, mvendorid};

mod dram;
mod fit;
mod mmc;
mod uart;

use uart::K1XSerial;
use util::{
    mem::{copy, dump_block},
    mmio::{read32, write32, write8},
};

pub type EntryPoint = unsafe extern "C" fn();

const SRAM0_BASE: usize = 0x0020_0000;
const SRAM0_SIZE: usize = 0x0002_0000;

const DRAM_BASE: usize = 0x0000_0000;
const FLASH_BASE: usize = 0xb800_0000;

const FLASH_SIZE: usize = 16 * 1024 * 1024;

// vendor partitions, taken from OrangePi RV 2 U-Boot SPL; U-Boot ends at
// - 0x0020_3fb0 (OrangePi RV2)
// - 0x0029_ce80 (Jupiter)
// 64K@0(bootinfo)
// 64K@64K(private)
// 256K@128K(fsbl),
// 64K@384K(env)
// 192K@448K(opensbi)
// -@640K(uboot)
const PART_BOOTINFO: usize = 0x0;
const PART_RESERVED: usize = 0x0001_0000;
const PART_FSBL: usize = 0x0002_0000;
const PART_UBOOT_ENV: usize = 0x0006_0000;
const PART_OPENSBI: usize = 0x0007_0000;
const PART_UBOOT: usize = 0x000a_0000;

// ours
const ORE_MAIN_OFFSET: usize = 4 * 1024 * 1024;

const LOAD_ADDR: usize = DRAM_BASE + 0x1000;

const MEM_TEST: bool = true;
const MEM_TEST_FULL: bool = false;

const DUMP_FLASH: bool = false;

// K1_BOOT=emmc|sd: load a FIT (OpenSBI, next stage, DT) from the eMMC boot1
// partition (OrangePi R2S) or from raw SD card sectors starting at
// K1_NEXT_LBA (OrangePi RV2: 8192, its 4 MiB "uboot" GPT partition).
const fn env_eq(v: Option<&str>, want: &str) -> bool {
    match v {
        None => false,
        Some(v) => {
            let (a, b) = (v.as_bytes(), want.as_bytes());
            if a.len() != b.len() {
                return false;
            }
            let mut i = 0;
            while i < a.len() {
                if a[i] != b[i] {
                    return false;
                }
                i += 1;
            }
            true
        }
    }
}

const fn env_u32(v: Option<&str>) -> u32 {
    match v {
        None => 0,
        Some(v) => {
            let b = v.as_bytes();
            let mut n = 0u32;
            let mut i = 0;
            while i < b.len() {
                n = n * 10 + (b[i] - b'0') as u32;
                i += 1;
            }
            n
        }
    }
}

const BOOT_FIT_EMMC: bool = env_eq(option_env!("K1_BOOT"), "emmc");
const BOOT_FIT_SD: bool = env_eq(option_env!("K1_BOOT"), "sd");
// K1_BOOT=nor: the FIT is in the SPI NOR, read through its memory-mapped
// window; K1_NEXT_LBA counts 512-byte sectors from the start of the flash.
const BOOT_FIT_NOR: bool = env_eq(option_env!("K1_BOOT"), "nor");
const BOOT_EMMC_FIT: bool = BOOT_FIT_EMMC || BOOT_FIT_SD || BOOT_FIT_NOR;
const NEXT_LBA: u32 = env_u32(option_env!("K1_NEXT_LBA"));
// K1_PCIE_PWR_GPIO: a GPIO that switches a PCIe slot's 3.3 V supply on (OrangePi
// RV2: 116, the M.2 slot's vpcie3v3 regulator). 0 means none.
const PCIE_PWR_GPIO: u32 = env_u32(option_env!("K1_PCIE_PWR_GPIO"));
// Where the FIT is read to before its images are copied out.
const FIT_STAGING_ADDR: usize = 0x1000_0000;

// Otherwise hand off to a next stage appended to this image (see main()).
const PAYLOAD_HANDOFF: bool = !BOOT_EMMC_FIT;
const PAYLOAD_OFFSET: usize = 0x0001_0000;
const PAYLOAD_ADDR: usize = 0x0400_0000;
// Up to the end of the 256K SRAM the BootROM loads the image into.
const PAYLOAD_MAX_SIZE: usize = 0xc084_0000 - (0xc080_1000 + PAYLOAD_OFFSET);
const BOOT_FLASH: bool = false;

const STORAGE_API_P_ADDR: usize = 0xC083_8498;
const USB_BOOT_ENTRY: usize = 0xc083_81a0;
// 0xffe0_3b36
const SDCARD_API_ENTRY: usize = 0xFFE0_A548;

const GPIO_BASE: usize = 0xd401_9000;

const PINCTRL_BASE: usize = 0xd401_e000;

const GPIO68: usize = GPIO_BASE + 68 * 4;
// const GPIO68: usize = GPIO_BASE + 0x0110;

const PULL_DOWN: u32 = (5 << 13);
const PAD_1V8_DS2: u32 = (2 << 11);
const EDGE_NONE: u32 = (1 << 6);
const MUX_MODE2: u32 = 2;

// octacore, 2 clusters of 4x X60
const BOOT_HART_ID: usize = 0;

const STACK_SIZE: usize = 8 * 1024;

#[link_section = ".bss.uninit"]
static mut BT0_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

/// Set up stack and jump to executable code.
///
/// # Safety
///
/// Naked function.
#[unsafe(naked)]
#[export_name = "start"]
#[link_section = ".text.entry"]
#[allow(named_asm_labels)]
pub unsafe extern "C" fn start() -> ! {
    naked_asm!(
        "auipc  s4, 0",

        "csrw   mstatus, zero",
        "csrw   mie, zero",
        "ld     t0, {start}",
        "csrw   mtvec, t0",
        // 1. suspend non-boot hart
        "li     t1, {boothart}",
        "csrr   t0, mhartid",
        "bne    t0, t1, .nonboothart",
        // 2. prepare stack
        // NOTE: non-boot harts need no stack here, they skip this
        "la     sp, {stack}",
        "li     t0, {stack_size}",
        "add    sp, sp, t0",
        "j      .boothart",
        // wait for multihart to get back into the game
        ".nonboothart:",
        // "csrw   mie, (1 << 3)",
        "wfi",
        "call   {payload}",
        ".boothart:",

        "call   {reset}",
        boothart   = const BOOT_HART_ID,
        stack      = sym BT0_STACK,
        stack_size = const STACK_SIZE,
        payload    = sym exec_payload,
        reset      = sym reset,
        start      = sym start
    )
}

/// Initialize RAM: Clear BSS and set up data.
/// See https://docs.rust-embedded.org/embedonomicon/main.html
///
/// # Safety
/// :shrug:
#[no_mangle]
pub unsafe extern "C" fn reset() {
    extern "C" {
        static mut _sbss: u8;
        static mut _ebss: u8;

        static mut _sdata: u8;
        static mut _edata: u8;
        static _sidata: u8;
    }

    let bss_size = addr_of!(_ebss) as usize - addr_of!(_sbss) as usize;
    // FIXME: why is this broken now, Rust?!
    if false {
        ptr::write_bytes(addr_of_mut!(_sbss), 0, bss_size);
    }
    let data_size = addr_of!(_edata) as usize - addr_of!(_sdata) as usize;
    ptr::copy_nonoverlapping(addr_of!(_sidata), addr_of_mut!(_sdata), data_size);
    // Call user entry point
    main();
}

fn vendorid_to_name<'a>(vendorid: usize) -> &'a str {
    match vendorid {
        0x0489 => "SiFive",
        0x0710 => "SpacemiT",
        _ => "unknown",
    }
}

// https://sifive.cdn.prismic.io/sifive/2dd11994-693c-4360-8aea-5453d8642c42_u74mc_core_complex_manual_21G3.pdf
fn impid_to_name<'a>(impid: usize) -> &'a str {
    match impid {
        0x0421_0427 => "21G1.02.00 / llama.02.00-general",
        0x1000_0000_4977_2200 => "X60",
        _ => "unknown",
    }
}

/// Print RISC-V core information:
/// - vendor
/// - arch
/// - implementation
/// - hart ID
fn print_ids() {
    let vid = mvendorid::read().map(|r| r.bits()).unwrap_or(0);
    let aid = marchid::read().map(|r| r.bits()).unwrap_or(0);
    let iid = mimpid::read().map(|r| r.bits()).unwrap_or(0);
    println!("RISC-V arch {aid:08x}");
    let vendor_name = vendorid_to_name(vid);
    println!("RISC-V core vendor: {vendor_name} (0x{vid:04x})");
    let imp_name = impid_to_name(iid);
    println!("RISC-V implementation: {imp_name} (0x{iid:08x})");
    let hart_id = mhartid::read();
    println!("RISC-V hart ID {hart_id}");
}

static mut SERIAL: Option<uart::K1XSerial> = None;

fn init_logger(s: uart::K1XSerial) {
    unsafe {
        SERIAL.replace(s);
        if let Some(m) = SERIAL.as_mut() {
            log::init(m);
        }
    }
}

#[no_mangle]
fn main() {
    let mut ini_pc: usize = 0;
    unsafe { asm!("mv {}, s4", out(reg) ini_pc) };

    let s = uart::K1XSerial::noinit();
    init_logger(s);
    println!("oreboot 🦀 bt0");
    println!("initial program counter (PC) {ini_pc:016x}");

    print_ids();

    let boot_mode = read32(STORAGE_API_P_ADDR);
    println!("Boot mode register: 0x{boot_mode:08x}");
    let boot_entry = read32(boot_mode as usize);
    println!("Boot entry: 0x{boot_entry:08x}");

    dram::init();

    const DRAM_SIZE: usize = 0x8000_0000;
    if MEM_TEST {
        if MEM_TEST_FULL {
            // NOTE: The full test will take _very long_.
            // FIXME: We need a little offset from address 0 because Rust
            // currently errors otherwise.
            util::mem::test(DRAM_BASE + 0x10, DRAM_SIZE - 0x10);
        } else {
            // Test a small amount of DRAM only.
            util::mem::test(LOAD_ADDR, 2 * 1024 * 1024);
        }
    }

    if DUMP_FLASH {
        dump_block(FLASH_BASE, FLASH_SIZE, 32);
    }

    if BOOT_EMMC_FIT {
        // Without U-Boot SPL nothing else sets up the board EEPROM's I2C
        // bus, which the next stage (EDK2) reads early.
        eeprom_i2c_init();
        if PCIE_PWR_GPIO != 0 {
            // Switch the slot supply on early so the next stage finds the
            // link; nothing else on this boot path drives the regulator.
            gpio_output_high(PCIE_PWR_GPIO);
            println!("[bt0] PCIe slot power on (GPIO {PCIE_PWR_GPIO})");
        }
        // Load OpenSBI, the next stage and its DT from a FIT in eMMC boot1,
        // then start OpenSBI (fw_dynamic) with that next stage.
        let src = fit::Source {
            medium: if BOOT_FIT_NOR {
                fit::Medium::Nor
            } else if BOOT_FIT_SD {
                fit::Medium::Mmc(mmc::Kind::Sd, mmc::Partition::User)
            } else {
                fit::Medium::Mmc(mmc::Kind::Emmc, mmc::Partition::Boot1)
            },
            lba: NEXT_LBA,
        };
        match fit::load(&src, FIT_STAGING_ADDR) {
            Ok(b) => {
                println!(
                    "[bt0] OpenSBI @{:08x}, next @{:08x}, DT @{:08x}",
                    b.firmware, b.next, b.fdt
                );
                fit::start_opensbi(BOOT_HART_ID, &b);
            }
            Err(e) => {
                println!("[bt0] FIT boot failed: {e:?}");
                unsafe { riscv::asm::wfi() };
            }
        }
    }

    if PAYLOAD_HANDOFF {
        // The FSBL image carries the next stage (U-Boot SPL, linked to run
        // from DRAM) at a fixed offset behind bt0. Copy it out of SRAM and
        // run it with DRAM up; it loads OpenSBI and U-Boot proper.
        let src = ini_pc + PAYLOAD_OFFSET;
        println!("[bt0] Copy next stage {src:08x} -> {PAYLOAD_ADDR:08x} ({PAYLOAD_MAX_SIZE} bytes)");
        copy(src, PAYLOAD_ADDR, PAYLOAD_MAX_SIZE);
        println!("[bt0] Jump to next stage @{PAYLOAD_ADDR:08x}");
        unsafe {
            asm!(
                "fence.i",
                "jr {entry}",
                entry = in(reg) PAYLOAD_ADDR,
                in("a0") BOOT_HART_ID,
                in("a1") 0usize,
                options(noreturn)
            );
        }
    }

    if BOOT_FLASH {
        copy(FLASH_BASE + ORE_MAIN_OFFSET, LOAD_ADDR, 0x8000);

        // GO!
        println!("[bt0] Jump to main stage @{LOAD_ADDR:08x}");
        exec_payload(LOAD_ADDR);

        println!("[bt0] Exit from main stage, resetting...");
    } else {
        println!("DRAM init done, WFI...");
    }

    unsafe {
        // udelay(0x0100_0000);
        riscv::asm::wfi()
    };
}

// jump to main stage or payload
// I2C2 (TWSI2) carries the board TLV EEPROM; U-Boot SPL's i2c_early_init()
// and SpacemiT's FSBL set it up the same way.
const APBC_TWSI2_CLK_RST: usize = 0xd401_5038; // bit 0 bus clk, bit 1 func clk, bit 2 reset
const MFP_GPIO_84: usize = 0xd401_e154;
const MFP_GPIO_85: usize = 0xd401_e158;
// MUX_MODE4 | EDGE_NONE | PULL_UP | PAD_1V8_DS2
const I2C_PIN_CONFIG: u32 = 4 | (1 << 6) | (6 << 13) | (1 << 12);

fn eeprom_i2c_init() {
    write32(APBC_TWSI2_CLK_RST, (read32(APBC_TWSI2_CLK_RST) | 0b11) & !(1 << 2));
    write32(MFP_GPIO_84, I2C_PIN_CONFIG);
    write32(MFP_GPIO_85, I2C_PIN_CONFIG);
}

// K1 GPIO and pad mux (U-Boot drivers/gpio/spacemit_gpio.c and
// drivers/pinctrl/spacemit/pinctrl-k1.c).
const GPIO_BASE_ADDR: usize = 0xd401_9000;
const PAD_MUX_BASE: usize = 0xd401_e000;

fn gpio_bank_offset(pin: u32) -> usize {
    match pin / 32 {
        0 => 0x0,
        1 => 0x4,
        2 => 0x8,
        _ => 0x100,
    }
}

fn pad_mux_reg(pin: u32) -> usize {
    let p = pin as usize;
    let offset = 1 + match p {
        0..=85 => p,
        86..=92 => p + 36,
        93..=97 => p + 23,
        98 => 92,
        99 => 91,
        100 => 90,
        101 => 89,
        102 => 94,
        103 => 93,
        104..=110 => p + 5,
        _ => p + 19,
    };
    PAD_MUX_BASE + (offset << 2)
}

fn gpio_mux(pin: u32) -> u32 {
    match pin {
        70..=73 | 93..=103 => 1,
        104..=109 => 4,
        _ => 0,
    }
}

fn gpio_output_high(pin: u32) {
    let bank = GPIO_BASE_ADDR + gpio_bank_offset(pin);
    let bit = 1u32 << (pin % 32);
    write32(bank + 0x18, bit); // GPSR: drive high
    write32(bank + 0x54, bit); // GSDR: output
    let pad = pad_mux_reg(pin);
    write32(pad, (read32(pad) & !0x7) | gpio_mux(pin));
}

fn exec_payload(addr: usize) {
    unsafe {
        let f: EntryPoint = transmute(addr);
        asm!("fence.i");
        f();
    }
}

#[cfg_attr(not(test), panic_handler)]
fn panic(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        println!(
            "[bt0] panic in '{}' line {}",
            location.file(),
            location.line(),
        );
    } else {
        println!("[bt0] panic at unknown location");
    };
    let msg = info.message();
    println!("[bt0]   {msg}");
    loop {
        core::hint::spin_loop();
    }
}
