//! usb-sim: A Rust-only, USB-bulk-like framed duplex transport over the QEMU serial stream.
//!
//! This module provides a minimal framing protocol and two roles:
//! - Initiator: sends test frames and expects echo replies; measures integrity
//! - Responder: echoes any well-formed frame back to the sender
//!
//! It uses the existing blocking UART driver as the underlying byte stream.
//! The framing is designed to resemble a simple bulk pipe with two logical
//! channels (0 and 1). A TTL is included for loop demos.
//
use crate::uart;

// Bring system_off from start.s for initiator builds to ensure linkage without depending on main.rs cfg.
#[cfg(feature = "usbsim-initiator")]
unsafe extern "C" { fn system_off(); }

const MAGIC: u16 = 0xABCD;
const VER: u8 = 1;

// Default max frame size (payload), can be overridden at compile-time via cfg.
#[cfg(not(any(feature = "usbsim_frame_1024", feature = "usbsim_frame_2048", feature = "usbsim_frame_4096", feature = "usbsim_frame_8192")))]
pub const MAX_FRAME: usize = 4096;
#[cfg(feature = "usbsim_frame_1024")] pub const MAX_FRAME: usize = 1024;
#[cfg(feature = "usbsim_frame_2048")] pub const MAX_FRAME: usize = 2048;
#[cfg(feature = "usbsim_frame_4096")] pub const MAX_FRAME: usize = 4096;
#[cfg(feature = "usbsim_frame_8192")] pub const MAX_FRAME: usize = 8192;

#[inline(always)]
fn crc16_ccitt(mut crc: u16, data: &[u8]) -> u16 {
    // CRC-16/CCITT-FALSE: poly 0x1021, init 0xFFFF
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 { crc = (crc << 1) ^ 0x1021; } else { crc <<= 1; }
        }
    }
    crc
}

#[inline(always)]
#[allow(dead_code)]
fn put_u16(x: u16) { uart::putc((x & 0xFF) as u8); uart::putc((x >> 8) as u8); }
#[inline(always)]
fn get_u16() -> u16 { let lo = uart::getc() as u16; let hi = uart::getc() as u16; lo | (hi << 8) }

#[inline(always)]
#[allow(dead_code)]
pub(super) fn write_frame(chan: u8, ttl: u8, seq: u32, payload: &[u8]) {
    debug_assert!(payload.len() <= MAX_FRAME);
    let len = payload.len() as u16;
    // Header layout (little-endian on wire):
    // MAGIC(2) VER(1) CHAN(1) TTL(1) RSVD(1) LEN(2) SEQ(4) CRC16(2)
    put_u16(MAGIC);
    uart::putc(VER);
    uart::putc(chan);
    uart::putc(ttl);
    uart::putc(0); // reserved
    put_u16(len);
    // sequence
    uart::putc((seq & 0xFF) as u8);
    uart::putc(((seq >> 8) & 0xFF) as u8);
    uart::putc(((seq >> 16) & 0xFF) as u8);
    uart::putc(((seq >> 24) & 0xFF) as u8);
    // payload
    for &b in payload { uart::putc(b); }
    // crc over header (except magic) + payload for better detection
    let mut tmp = [0u8; 1+1+1+1+2+4];
    tmp[0] = VER; tmp[1] = chan; tmp[2] = ttl; tmp[3] = 0; tmp[4] = (len & 0xFF) as u8; tmp[5] = (len >> 8) as u8;
    tmp[6] = (seq & 0xFF) as u8; tmp[7] = ((seq >> 8) & 0xFF) as u8; tmp[8] = ((seq >> 16) & 0xFF) as u8; tmp[9] = ((seq >> 24) & 0xFF) as u8;
    let mut crc = crc16_ccitt(0xFFFF, &tmp);
    crc = crc16_ccitt(crc, payload);
    put_u16(crc);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxErr { BadCrc, LenTooBig }

#[inline(always)]
pub(super) fn read_frame(buf: &mut [u8]) -> Result<(u8 /*chan*/, u8 /*ttl*/, u32 /*seq*/, usize /*n*/ ), RxErr> {
    // Find magic (simple resync)
    loop {
        let b0 = uart::getc();
        if b0 as u16 != (MAGIC & 0xFF) { continue; }
        let b1 = uart::getc();
        if b1 as u16 != (MAGIC >> 8) { continue; }
        break;
    }
    let ver = uart::getc(); if ver != VER { /* accept only current */ }
    let chan = uart::getc();
    let ttl = uart::getc();
    let _rsv = uart::getc();
    let len = get_u16() as usize;
    if len > buf.len() || len > MAX_FRAME { return Err(RxErr::LenTooBig); }
    let mut seq: u32 = 0;
    seq |= uart::getc() as u32;
    seq |= (uart::getc() as u32) << 8;
    seq |= (uart::getc() as u32) << 16;
    seq |= (uart::getc() as u32) << 24;
    for i in 0..len { buf[i] = uart::getc(); }
    let crc_rx = get_u16();
    let mut hdr = [0u8; 10];
    hdr[0] = ver; hdr[1] = chan; hdr[2] = ttl; hdr[3] = 0; hdr[4] = (len as u16 & 0xFF) as u8; hdr[5] = ((len as u16) >> 8) as u8;
    hdr[6] = (seq & 0xFF) as u8; hdr[7] = ((seq >> 8) & 0xFF) as u8; hdr[8] = ((seq >> 16) & 0xFF) as u8; hdr[9] = ((seq >> 24) & 0xFF) as u8;
    let mut crc = crc16_ccitt(0xFFFF, &hdr);
    crc = crc16_ccitt(crc, &buf[..len]);
    if crc != crc_rx { return Err(RxErr::BadCrc); }
    Ok((chan, ttl, seq, len))
}

#[cfg(feature = "usbsim-responder")]
pub fn run_responder() -> ! {
    // Give the connector time to attach to the chardev
    unsafe { for _ in 0..10_000_000u32 { core::arch::asm!("nop"); } }
    uart::puts(b"usb-sim: responder ready\n");
    let mut buf = [0u8; MAX_FRAME];
    loop {
        match read_frame(&mut buf) {
            Ok((chan, ttl, seq, n)) => {
                // echo back; decrement TTL if non-zero
                let ttl2 = if ttl > 0 { ttl - 1 } else { 0 };
                write_frame(chan, ttl2, seq, &buf[..n]);
            }
            Err(_) => {
                // Resync on next magic. Optionally print a dot to show life.
                uart::puts(b".");
            }
        }
    }
}

// --- Messagebus bridge over usb-sim (bare-metal) ---
#[cfg(feature = "usbsim-bridge")]
pub mod bridge_bm {
    use aarnn_nsys::bus;
    use super::{uart, crc16_ccitt, MAX_FRAME};

    const DESC_CAPACITY: usize = 8;
    const SLOT_BYTES: usize = MAX_FRAME; // ensure frames fit
    const BUF_LEN: usize = bus::min_buffer_size(DESC_CAPACITY, SLOT_BYTES);

    #[repr(align(64))]
    struct Aligned<const N: usize>([u8; N]);
    static mut BUS_MEM: Aligned<BUF_LEN> = Aligned([0u8; BUF_LEN]);


    #[inline(always)]
    fn put_u16(x: u16) { uart::putc((x & 0xFF) as u8); uart::putc((x >> 8) as u8); }

    fn write_frame(chan: u8, ttl: u8, seq: u32, payload: &[u8]) {
        let len = payload.len() as u16;
        // Header: MAGIC, VER, CHAN, TTL, RSVD, LEN, SEQ
        put_u16(super::MAGIC);
        uart::putc(super::VER);
        uart::putc(chan);
        uart::putc(ttl);
        uart::putc(0);
        put_u16(len);
        uart::putc((seq & 0xFF) as u8);
        uart::putc(((seq >> 8) & 0xFF) as u8);
        uart::putc(((seq >> 16) & 0xFF) as u8);
        uart::putc(((seq >> 24) & 0xFF) as u8);
        for &b in payload { uart::putc(b); }
        // CRC over hdr (excluding magic) + payload
        let mut hdr = [0u8; 10];
        hdr[0] = super::VER; hdr[1] = chan; hdr[2] = ttl; hdr[3] = 0; hdr[4] = (len & 0xFF) as u8; hdr[5] = (len >> 8) as u8;
        hdr[6] = (seq & 0xFF) as u8; hdr[7] = ((seq >> 8) & 0xFF) as u8; hdr[8] = ((seq >> 16) & 0xFF) as u8; hdr[9] = ((seq >> 24) & 0xFF) as u8;
        let mut crc = crc16_ccitt(0xFFFF, &hdr);
        crc = crc16_ccitt(crc, payload);
        put_u16(crc);
    }

    pub fn run_bridge() -> ! {
        // Small startup delay to let the other VM and the PTY bridge attach
        unsafe { for _ in 0..50_000_000u32 { core::arch::asm!("nop"); } }
        uart::puts(b"usb-sim: bridge start\n");
        // Build local bus with SLOT_BYTES = MAX_FRAME
        let bus = unsafe {
            let ptr = core::ptr::addr_of_mut!(BUS_MEM.0) as *mut u8;
            let buf: &mut [u8] = core::slice::from_raw_parts_mut(ptr, BUF_LEN);
            bus::BusHandle::from_slice(buf, DESC_CAPACITY).expect("bus init")
        };
        let sub = bus.subscribe().expect("sub");
        let prod = bus.producer();

        // Probe payload and simple retransmit counter
        let probe = b"MBUS-PROBE";
        let mut retries: u32 = 0;
        // Tiny extra delay before first publish to reduce initial attach races on slower hosts
        unsafe { for _ in 0..5_000_000u32 { core::arch::asm!("nop"); } }
        let _ = prod.publish(probe);

        // Minimal receive buffer for bus
        let mut bus_buf = [0u8; SLOT_BYTES];
        // Simple parser state using blocking read to build frames is complex; rely on bus activity to drive UART reads opportunistically
        // For simplicity and determinism here, handle in two tight polling loops.
        let mut acked = false;
        let mut seq_ctr: u32 = 0;
        loop {
            // Periodically retransmit the probe until we observe an ACK from the peer
            if !acked {
                retries = retries.wrapping_add(1);
                if (retries & 0x3FFF) == 0 { let _ = prod.publish(probe); }
            }

            // 1) Drain local bus -> send frames (chan 0)
            if let Ok(Some(n)) = sub.try_recv(&mut bus_buf) {
                write_frame(0, 8, seq_ctr, &bus_buf[..n]);
                seq_ctr = seq_ctr.wrapping_add(1);
            }
            // 2) Opportunistically read full frames in a blocking manner using the simple reader from parent module
            // This can block; keep bursts small by checking UART FIFO quickly.
            if let Some(_) = uart::try_getc() {
                // There is at least one byte pending; use the blocking reader now.
                let mut rx = [0u8; SLOT_BYTES];
                match super::read_frame(&mut rx) {
                    Ok((chan, ttl, _seq, n)) => {
                        if chan == 0 {
                            // Publish into local bus and send ACK on chan 1
                            if n <= SLOT_BYTES { let _ = prod.publish(&rx[..n]); }
                            write_frame(1, if ttl>0 { ttl-1 } else { 0 }, 0xA000_0000, &rx[..n]);
                            uart::puts(b"[MBUS] RX on chan0 -> published + acked\n");
                        } else if chan == 1 {
                            // ACK observed
                            uart::puts(b"[MBUS] ACK received\n");
                            if !acked { uart::puts(b"[MBUS-BM] PASS\n"); acked = true; }
                        }
                    }
                    Err(_) => { /* ignore and continue */ }
                }
            }
        }
    }
}

#[cfg(feature = "usbsim-initiator")]
pub fn run_initiator() -> ! {
    uart::puts(b"usb-sim: initiator start\n");
    // Give the host bridge (socat or other) time to connect both PTYs before we send frames.
    // This avoids losing the very first frames during QEMU startup.
    unsafe {
        for _ in 0..20_000_000u32 { core::arch::asm!("nop"); }
    }
    const N_FRAMES: usize = 8; // send a few more frames to be robust against initial drops
    let mut payload = [0u8; MAX_FRAME];
    let mut rxbuf = [0u8; MAX_FRAME];
    for i in 0..payload.len() { payload[i] = (i & 0xFF) as u8; }
    let chan = 0u8;
    for seq in 0..(N_FRAMES as u32) {
        let len = 128 + ((seq as usize * 17) % 256).min(MAX_FRAME - 128);
        let ttl = 8;
        write_frame(chan, ttl, seq, &payload[..len]);
        // wait echo (with simple retry window)
        let mut retries = 0u32;
        loop {
            match read_frame(&mut rxbuf) {
                Ok((rchan, _ttl, rseq, n)) => {
                    if rchan == chan && rseq == seq && n == len && &rxbuf[..n] == &payload[..len] {
                        break;
                    } else {
                        // out-of-order or mismatch; keep waiting (initiator is strict)
                        uart::puts(b"!");
                    }
                }
                Err(_) => { /* resync */ }
            }
            // If we waited quite a bit without the right echo, retransmit the frame.
            retries = retries.wrapping_add(1);
            if retries % 2000 == 0 { write_frame(chan, ttl, seq, &payload[..len]); }
        }
        if (seq & 0x1) == 0 { uart::puts(b"#"); }
    }
    uart::puts(b"\n[USBSIM] PASS\n");
    // Park: call system_off in a loop so this function truly diverges
    loop {
        #[cfg(feature = "usbsim-initiator")]
        unsafe { system_off(); }
    }
}
