//! Minimal PL011 UART (UART0) setup for Raspberry Pi 4 / QEMU raspi4b.
//!
//! This driver initializes the PL011 to 115200 8N1 and provides simple
//! blocking transmit routines. It configures GPIO14/15 to ALT0 for TXD0/RXD0.
//!
//! Assumptions:
//! - UART clock is 48 MHz (QEMU raspi machines default). If your environment
//!   uses a different UARTCLK (e.g., 3 MHz), adjust the baud divisor below.
//! - MMIO base addresses match BCM2711 (RPi4):
//!   - GPIO:  0xFE20_0000
//!   - UART0: 0xFE20_1000
//!
//! Safety: All MMIO accesses are done via volatile reads/writes. Public API is
//! safe where possible; internal helpers use `unsafe` to hit MMIO.

use core::ptr::{read_volatile, write_volatile};
use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

// Base addresses
const GPIO_BASE: usize = 0xFE20_0000;
const UART0_BASE: usize = 0xFE20_1000;

// GPIO registers (offsets)
const GPFSEL1: usize = 0x04; // Function select 1 (pins 10..19)

// PL011 registers (offsets)
const UART_DR: usize   = 0x00; // Data register
const UART_FR: usize   = 0x18; // Flag register
const UART_IBRD: usize = 0x24; // Integer baud rate divisor
const UART_FBRD: usize = 0x28; // Fractional baud rate divisor
const UART_LCR_H: usize= 0x2C; // Line control
const UART_CR: usize   = 0x30; // Control
const UART_ICR: usize  = 0x44; // Interrupt clear

// PL011 flag bits
const FR_TXFF: u32 = 1 << 5; // Transmit FIFO full
const FR_RXFE: u32 = 1 << 4; // Receive FIFO empty

// PL011 control and line control bits
const CR_UARTEN: u32 = 1 << 0;
const CR_TXE:    u32 = 1 << 8;
const CR_RXE:    u32 = 1 << 9;

const LCRH_FEN:  u32 = 1 << 4;     // FIFO enable
const LCRH_WLEN_8: u32 = 0b11 << 5; // 8-bit word length

// Helpers to get pointers
#[inline(always)]
fn reg32(addr: usize) -> *mut u32 { addr as *mut u32 }
#[inline(always)]
fn reg8(addr: usize) -> *mut u8 { addr as *mut u8 }

#[inline(always)]
fn gpio_reg(off: usize) -> *mut u32 { reg32(GPIO_BASE + off) }
#[inline(always)]
fn uart_reg(off: usize) -> *mut u32 { reg32(UART0_BASE + off) }

/// Small delay loop; not calibrated. Used between register writes when required.
#[inline(always)]
fn tiny_delay() {
    // Prevent the compiler from optimizing it away
    for _ in 0..64 { unsafe { asm!("nop") } }
}

/// Data synchronization barrier to ensure MMIO ordering.
#[inline(always)]
fn dsb_sy() { unsafe { asm!("dsb sy", options(nostack, preserves_flags)) } }

/// Configure GPIO14 (TXD0) and GPIO15 (RXD0) to ALT0 for PL011.
fn gpio_setup_uart0_alt0() {
    // Each pin has 3 function bits in GPFSEL registers.
    // For pins 14 and 15, both are in GPFSEL1.
    // ALT0 function value is 0b100 (4).
    unsafe {
        let fsel1 = gpio_reg(GPFSEL1);
        let mut v = read_volatile(fsel1);
        // Clear bits for pin 14 (bits 12..14) and pin 15 (bits 15..17)
        v &= !((0b111 << 12) | (0b111 << 15));
        // Set to ALT0 (0b100)
        v |= (0b100 << 12) | (0b100 << 15);
        write_volatile(fsel1, v);
        dsb_sy();
    }
}

/// Initialize PL011 UART0 to 115200 8N1.
///
/// Safe to call multiple times; reprograms the UART.
pub fn init() {
    // Configure the pins first
    gpio_setup_uart0_alt0();

    unsafe {
        // Disable UART before configuration
        write_volatile(uart_reg(UART_CR), 0);
        tiny_delay();

        // Clear pending interrupts
        write_volatile(uart_reg(UART_ICR), 0x7FF);

        // Baud rate divisors for UARTCLK = 48_000_000 Hz, baud = 115_200
        // Divider = 26.0416667 -> IBRD = 26, FBRD = round(0.0416667 * 64) = 3
        write_volatile(uart_reg(UART_IBRD), 26);
        write_volatile(uart_reg(UART_FBRD), 3);

        // 8N1, enable FIFO
        write_volatile(uart_reg(UART_LCR_H), LCRH_WLEN_8 | LCRH_FEN);

        // Enable UART, TX and RX
        write_volatile(uart_reg(UART_CR), CR_UARTEN | CR_TXE | CR_RXE);
        dsb_sy();
    }
}

/// Block until there is space in TX FIFO, then write one byte.
pub fn putc(ch: u8) {
    unsafe {
        while (read_volatile(uart_reg(UART_FR)) & FR_TXFF) != 0 {}
        write_volatile(reg8(UART0_BASE + UART_DR), ch);
    }
}

/// Simple global TX spinlock to serialize UART output across cores.
static TX_LOCK: AtomicBool = AtomicBool::new(false);

#[inline(always)]
fn lock_tx() {
    // Spin until we acquire the lock
    while TX_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        // hint to CPU
        unsafe { asm!("nop") }
    }
}

#[inline(always)]
fn unlock_tx() {
    TX_LOCK.store(false, Ordering::Release);
}

/// Write a buffer; translate `\n` to `\r\n` for terminal compatibility.
pub fn puts(buf: &[u8]) {
    lock_tx();
    for &b in buf {
        match b {
            b'\n' => { putc(b'\r'); putc(b'\n'); }
            _ => putc(b),
        }
    }
    unlock_tx();
}

/// Read one byte from UART0 (blocking until data is available).
pub fn getc() -> u8 {
    // Implement blocking read in terms of the non-blocking poll to ensure
    // `try_getc` is always referenced and compiled-in.
    loop {
        if let Some(b) = try_getc() { return b; }
        // brief wait
        unsafe { asm!("nop") }
    }
}

/// Non-blocking read: returns Some(byte) if data available, otherwise None.
pub fn try_getc() -> Option<u8> {
    unsafe {
        if (read_volatile(uart_reg(UART_FR)) & FR_RXFE) != 0 {
            None
        } else {
            Some(read_volatile(reg8(UART0_BASE + UART_DR)))
        }
    }
}
