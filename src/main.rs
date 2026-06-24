#![no_std] // Don't link the Rust standard library
#![no_main] // Disable the normal Rust-level entry point

//! Bare-metal entry for Raspberry Pi 4 (AArch64).
//! - Startup/boot in `src/start.s` sets the stack and branches to `not_main`.
//! - Linker script `raspi.ld` defines memory layout and stack location.
//! - We print to PL011 UART0 and run one of several no_std demos that mirror
//!   the aarnn-nsys examples, selectable via Cargo features.

use core::arch::global_asm;

mod panic;
mod uart;
#[cfg(feature = "usb-sim")]
mod usbsim;

// Bring the boot assembly into the binary.
global_asm!(include_str!("start.s"));

// Declare external symbols provided by `start.s`.
// In Rust 2024, `extern` blocks are unsafe because they declare FFI items with
// assumptions the compiler cannot verify. Marking the block `unsafe` makes this explicit.
// Declare external symbols provided by `start.s` only for the UART menu build.
#[cfg(not(feature = "usb-sim"))]
unsafe extern "C" {
    /// Puts the CPU into a low-power wait-for-interrupt loop.
    fn system_off();
}

// ---- aarnn-nsys bare-metal demo wiring ----
#[cfg(not(feature = "usb-sim"))]
use aarnn_nsys::bus;

#[cfg(not(feature = "usb-sim"))]
const DESC_CAPACITY: usize = 8;      // ring size (power of two)
#[cfg(not(feature = "usb-sim"))]
const SLOT_BYTES: usize = 64;        // per-slot payload bytes
#[cfg(not(feature = "usb-sim"))]
const BUF_LEN: usize = bus::min_buffer_size(DESC_CAPACITY, SLOT_BYTES);

// Static backing storage for the primary bus region used in no_std mode.
// Must be cacheline-aligned to satisfy the bus header/slot alignment (64B).
#[cfg(not(feature = "usb-sim"))]
#[repr(align(64))]
struct Aligned<const N: usize>([u8; N]);
#[cfg(not(feature = "usb-sim"))]
static mut BUS_MEM_A: Aligned<BUF_LEN> = Aligned([0u8; BUF_LEN]);

// Secondary buffer used by the relay demo.
#[cfg(not(feature = "usb-sim"))]
static mut BUS_MEM_B: Aligned<BUF_LEN> = Aligned([0u8; BUF_LEN]);

#[cfg(not(feature = "usb-sim"))]
#[inline(always)]
fn make_bus_from_a() -> Option<bus::BusHandle> {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(BUS_MEM_A.0) as *mut u8;
        let buf: &mut [u8] = core::slice::from_raw_parts_mut(ptr, BUF_LEN);
        bus::BusHandle::from_slice(buf, DESC_CAPACITY).ok()
    }
}

#[cfg(not(feature = "usb-sim"))]
#[inline(always)]
fn make_bus_from_b() -> Option<bus::BusHandle> {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(BUS_MEM_B.0) as *mut u8;
        let buf: &mut [u8] = core::slice::from_raw_parts_mut(ptr, BUF_LEN);
        bus::BusHandle::from_slice(buf, DESC_CAPACITY).ok()
    }
}

#[cfg(not(feature = "usb-sim"))]
fn demo_basic() {
    uart::puts(b"Raspi bare-metal booted. Initializing bus...\n");
    let Some(bus) = make_bus_from_a() else { uart::puts(b"Bus init error\n"); unsafe { system_off() }; return; };
    let sub = match bus.subscribe() { Ok(s) => s, Err(_) => { uart::puts(b"Subscribe failed\n"); unsafe { system_off() }; return; } };
    let prod = bus.producer();
    let msg = b"hello from aarnn-nsys bare bus";
    if let Err(_) = prod.publish(msg) { uart::puts(b"Publish failed\n"); unsafe { system_off() }; return; }
    let mut scratch = [0u8; SLOT_BYTES];
    match sub.try_recv(&mut scratch) {
        Ok(Some(n)) => { uart::puts(b"Received: "); uart::puts(&scratch[..n]); uart::puts(b"\n"); }
        Ok(None) => { uart::puts(b"No message available\n"); }
        Err(_) => { uart::puts(b"Recv error\n"); }
    }
    uart::puts(b"Done. Entering low-power wait.\n");
}

#[cfg(not(feature = "usb-sim"))]
fn demo_try_publish() {
    uart::puts(b"Demo: try_publish and backpressure\n");
    let Some(bus) = make_bus_from_a() else { uart::puts(b"Bus init error\n"); unsafe { system_off() }; return; };
    let sub = bus.subscribe().expect("subscribe");
    let prod = bus.producer();
    let payload = [0xABu8; 16];
    let mut published = 0usize;
    loop {
        match prod.try_publish(&payload) {
            Ok(true) => { published += 1; if published > DESC_CAPACITY { break; } }
            Ok(false) => { uart::puts(b"Backpressure observed after "); put_num(published as u64); uart::puts(b" publishes\n"); break; }
            Err(_) => { uart::puts(b"try_publish error\n"); break; }
        }
    }
    // Free one slot by receiving
    let mut scratch = [0u8; SLOT_BYTES];
    let _ = sub.try_recv(&mut scratch);
    match prod.try_publish(&payload) {
        Ok(true) => uart::puts(b"try_publish succeeded after advancing subscriber\n"),
        Ok(false) => uart::puts(b"Still backpressured (unexpected)\n"),
        Err(_) => uart::puts(b"try_publish error (unexpected)\n"),
    }
    uart::puts(b"Done. Entering low-power wait.\n");
}

#[cfg(not(feature = "usb-sim"))]
fn demo_fanout() {
    uart::puts(b"Demo: multi-subscriber fan-out\n");
    let Some(bus) = make_bus_from_a() else { uart::puts(b"Bus init error\n"); unsafe { system_off() }; return; };
    let sub0 = bus.subscribe().expect("sub0");
    let sub1 = bus.subscribe().expect("sub1");
    let sub2 = bus.subscribe().expect("sub2");
    let prod = bus.producer();
    let msg = b"fanout";
    let _ = prod.publish(msg);
    let mut scratch = [0u8; SLOT_BYTES];
    for (i, s) in [sub0, sub1, sub2].into_iter().enumerate() {
        match s.try_recv(&mut scratch) {
            Ok(Some(n)) => { uart::puts(b"Subscriber "); put_num(i as u64); uart::puts(b" got: "); uart::puts(&scratch[..n]); uart::puts(b"\n"); }
            _ => uart::puts(b"Subscriber missed message\n"),
        }
    }
    uart::puts(b"Done. Entering low-power wait.\n");
}

#[cfg(not(feature = "usb-sim"))]
fn demo_relay() {
    uart::puts(b"Demo: relay_once between two in-memory buses\n");
    let Some(bus_a) = make_bus_from_a() else { uart::puts(b"Bus A init error\n"); unsafe { system_off() }; return; };
    let Some(bus_b) = make_bus_from_b() else { uart::puts(b"Bus B init error\n"); unsafe { system_off() }; return; };
    let sub_a = bus_a.subscribe().expect("sub A");
    let sub_b = bus_b.subscribe().expect("sub B");
    let prod_a = bus_a.producer();
    let prod_b = bus_b.producer();
    let msg = b"relay";
    let _ = prod_a.publish(msg);
    let mut scratch = [0u8; SLOT_BYTES];
    match aarnn_nsys::bus::relay_once(&sub_a, &prod_b, &mut scratch) {
        Ok(Some(n)) => { uart::puts(b"Relayed "); put_num(n as u64); uart::puts(b" bytes\n"); }
        Ok(None) => uart::puts(b"No source message to relay\n"),
        Err(_) => uart::puts(b"Relay error\n"),
    }
    let mut scratch2 = [0u8; SLOT_BYTES];
    match sub_b.try_recv(&mut scratch2) {
        Ok(Some(n)) => { uart::puts(b"Bus B received: "); uart::puts(&scratch2[..n]); uart::puts(b"\n"); }
        _ => uart::puts(b"Bus B did not receive\n"),
    }
    uart::puts(b"Done. Entering low-power wait.\n");
}

#[cfg(not(feature = "usb-sim"))]
fn demo_msg_too_large() {
    uart::puts(b"Demo: MsgTooLarge behavior\n");
    let Some(bus) = make_bus_from_a() else { uart::puts(b"Bus init error\n"); unsafe { system_off() }; return; };
    let sub = bus.subscribe().expect("subscribe");
    let prod = bus.producer();
    let too_big = [0u8; SLOT_BYTES + 8];
    match prod.publish(&too_big) {
        Err(aarnn_nsys::bus::BusError::MsgTooLarge) => uart::puts(b"Got expected MsgTooLarge\n"),
        Ok(_) => uart::puts(b"Unexpectedly published oversize\n"),
        Err(_) => uart::puts(b"Publish error (unexpected)\n"),
    }
    let ok = [0x5Au8; 8];
    let _ = prod.publish(&ok);
    let mut scratch = [0u8; SLOT_BYTES];
    if let Ok(Some(n)) = sub.try_recv(&mut scratch) {
        uart::puts(b"Received ok payload len="); put_num(n as u64); uart::puts(b"\n");
    }
    uart::puts(b"Done. Entering low-power wait.\n");
}

#[cfg(not(feature = "usb-sim"))]
#[inline(always)]
fn put_num(mut x: u64) {
    // decimal printing
    let mut buf = [0u8; 20];
    let mut i = 0;
    if x == 0 { uart::putc(b'0'); return; }
    while x > 0 { buf[i] = b'0' + (x % 10) as u8; i += 1; x /= 10; }
    while i > 0 { i -= 1; uart::puts(&[buf[i]]); }
}

/// Rust entry called by `_start` in `start.s`.
#[unsafe(no_mangle)]
pub extern "C" fn not_main() {
    uart::init();

    // If building a usb-sim role, give external PTY connectors a moment to attach
    #[cfg(feature = "usb-sim")]
    unsafe {
        for _ in 0..50_000_000u32 { core::arch::asm!("nop"); }
        uart::puts(b"BOOT: usb-sim build\n");
    }

    // Auto-run usb-sim roles when enabled.
    #[cfg(all(feature = "usb-sim", feature = "usbsim-bridge"))]
    {
        uart::puts(b"Boot: usb-sim messagebus bridge\n");
        crate::usbsim::bridge_bm::run_bridge();
    }
    #[cfg(all(feature = "usb-sim", feature = "usbsim-responder", not(feature = "usbsim-bridge")))]
    {
        uart::puts(b"Boot: usb-sim responder role\n");
        crate::usbsim::run_responder();
    }
    #[cfg(all(feature = "usb-sim", feature = "usbsim-initiator", not(any(feature = "usbsim-responder", feature = "usbsim-bridge"))))]
    {
        uart::puts(b"Boot: usb-sim initiator role\n");
        crate::usbsim::run_initiator();
    }
    
    // Default interactive demos menu (only when usb-sim is not enabled)
    #[cfg(not(feature = "usb-sim"))]
    {
        loop {
            uart::puts(b"\n=== aarnn-nsys bare-metal demos ===\n");
            uart::puts(b"[1] Basic publish/subscribe\n");
            uart::puts(b"[2] try_publish and backpressure\n");
            uart::puts(b"[3] Multi-subscriber fan-out\n");
            uart::puts(b"[4] Relay between two buses\n");
            uart::puts(b"[5] MsgTooLarge error handling\n");
            uart::puts(b"[q] Quit to low-power wait\n> ");
            let ch = uart::getc();
            match ch {
                b'1' => { demo_basic(); }
                b'2' => { demo_try_publish(); }
                b'3' => { demo_fanout(); }
                b'4' => { demo_relay(); }
                b'5' => { demo_msg_too_large(); }
                b'q' | b'Q' => { break; }
                // Ignore common whitespace characters to make automation robust
                b'\r' | b'\n' | b' ' | b'\t' => { /* ignore whitespace */ }
                _ => { uart::puts(b"\nInvalid selection.\n"); }
            }
        }
        // Park only in the menu build to avoid unreachable warnings in usb-sim builds
        unsafe { system_off() }
    }
}