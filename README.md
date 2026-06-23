# raspi-bare-metal — aarnn-nsys on Raspberry Pi (no_std)

This crate is a tiny, bootable bare‑metal image for Raspberry Pi 4 (AArch64) that demonstrates how to use `aarnn-nsys` in a `no_std` environment. It:

- Boots without an OS using a minimal `start.s` and `raspi.ld` linker script
- Initializes the PL011 UART0 and prints to the serial console
- Allocates a static in‑memory region and instantiates the aarnn‑nsys bus inside it
- Creates a producer + subscriber, publishes a message, receives it, and prints the payload

See the `aarnn-nsys/README.md` for a deeper library overview and the Linux shared‑memory backend.

## Prerequisites
- Rust nightly toolchain (pinned via `rust-toolchain.toml`)
- Target: `aarch64-unknown-none` (added by `rust-toolchain.toml`)
- Build-std configured for `core` and `compiler_builtins` (via `.cargo/config.toml`)
- AArch64 cross binutils if you plan to generate a raw `kernel8.img` (optional; `llvm-objcopy` also works)
- For emulation: `qemu-system-aarch64` with machine `raspi4b`

## Project layout
- `src/start.s` — AArch64 entry point: sets the stack and jumps to `not_main`
- `src/main.rs` — `no_std` entry; initializes UART and the in‑memory bus
- `src/uart.rs` — Minimal PL011 UART0 driver (115200 8N1)
- `src/panic.rs` — Minimal panic handler (infinite loop)
- `raspi.ld` — Linker script: places text/data and reserves a 16 KiB stack
- `.cargo/config.toml` — Sets the target and passes the linker script to rustc
- `rust-toolchain.toml` — Pins nightly and required components

## Build
```
cd raspi-bare-metal
cargo build --release
```
This produces an ELF image at:
```
target/aarch64-unknown-none/release/raspi-bare-metal
```

## Run in QEMU (recommended for quick test)
```
qemu-system-aarch64 \
  -M raspi4b \
  -kernel target/aarch64-unknown-none/release/raspi-bare-metal \
  -nographic -monitor none -serial stdio
```
Notes:
- QEMU machine `raspi4b` enforces a minimum of 4 CPUs; `-smp 1` is not supported on this machine.
- If you want to share the terminal between QEMU monitor and serial, use `-serial mon:stdio` instead of separate `-monitor`.
  - In this mode, Ctrl-C will not terminate QEMU because stdio is multiplexed. Use `Ctrl-a x` to quit immediately, or `Ctrl-a c` to enter the monitor and then type `quit`.
- If you prefer Ctrl-C to terminate QEMU, use the default shown above: `-monitor none -serial stdio` (no multiplexing).

### Smoke test (automated)
A non-interactive smoke test script is provided to validate the basic demo under QEMU and assert the expected three lines appear.

From the repository root:
```
./scripts/smoke_qemu.sh
```
It will:
- Build the release ELF
- Boot QEMU
- Automatically press `1` to run the basic publish/subscribe demo
- Assert the following lines are printed within a few seconds:
  - `Raspi bare-metal booted. Initializing bus...`
  - `Received: hello from aarnn-nsys bare bus`
  - `Done. Entering low-power wait.`
Returns exit code 0 on success (prints `[SMOKE] PASS`).

## Select and run demos at runtime
This single binary contains a tiny UART menu to run multiple demos that mirror the `aarnn-nsys` examples. After boot you will see:
```
=== aarnn-nsys bare-metal demos ===
[1] Basic publish/subscribe
[2] try_publish and backpressure
[3] Multi-subscriber fan-out
[4] Relay between two buses
[5] MsgTooLarge error handling
[q] Quit to low-power wait
> 
```
- Press the corresponding key to run a demo. After completion, the menu is shown again.
- Press `q` to park the CPU in low-power wait.

Expected serial output by demo:
- 1 Basic publish/subscribe:
```
Raspi bare-metal booted. Initializing bus...
Received: hello from aarnn-nsys bare bus
Done. Entering low-power wait.
```
- 2 try_publish and backpressure:
```
Demo: try_publish and backpressure
Backpressure observed after <N> publishes
try_publish succeeded after advancing subscriber
Done. Entering low-power wait.
```
- 3 Multi-subscriber fan-out:
```
Demo: multi-subscriber fan-out
Subscriber 0 got: fanout
Subscriber 1 got: fanout
Subscriber 2 got: fanout
Done. Entering low-power wait.
```
- 4 Relay between two buses:
```
Demo: relay_once between two in-memory buses
Relayed 5 bytes
Bus B received: relay
Done. Entering low-power wait.
```
- 5 MsgTooLarge error handling:
```
Demo: MsgTooLarge behavior
Got expected MsgTooLarge
Received ok payload len=8
Done. Entering low-power wait.
```

Notes:
- The UART is configured for 115200 8N1, and QEMU’s `raspi4b` machine defaults to a 48 MHz UART clock — the divisors in `uart.rs` assume this.
- Multi-core: The boot code parks non-primary cores in a low-power WFI loop using `MPIDR_EL1`, so only core 0 prints. You can run with the default number of cores, or add `-smp 1` to run single-core for debugging.

## Run on real Raspberry Pi 4
The RPi firmware expects a raw `kernel8.img` at the FAT boot partition root. Convert the ELF to a raw image:

Using llvm-objcopy (installed with Rust/LLVM toolchains):
```
llvm-objcopy \
  -O binary \
  target/aarch64-unknown-none/release/raspi-bare-metal \
  kernel8.img
```
Copy `kernel8.img` to the RPi’s boot partition alongside the usual `config.txt`, `fixup4.dat`, and `start4.elf` files. Connect a serial adapter to UART0 (GPIO14 TXD0, GPIO15 RXD0) at 115200 8N1 and power on. You should see the same output as with QEMU.

## How aarnn‑nsys is used here
The bus is fully in‑memory; you provide one contiguous byte slice to hold metadata and the payload slab. In this demo, we size it at compile time and store it in a `static mut`.

Key lines from `src/main.rs`:
```
use aarnn_nsys::bus;

const DESC_CAPACITY: usize = 8;      // ring size (power of two)
const SLOT_BYTES: usize = 64;        // payload bytes per message
const BUF_LEN: usize = bus::min_buffer_size(DESC_CAPACITY, SLOT_BYTES);

static mut BUS_MEM: [u8; BUF_LEN] = [0u8; BUF_LEN];

// ... inside not_main():
let bus = unsafe {
    let ptr = core::ptr::addr_of_mut!(BUS_MEM) as *mut u8;
    let buf: &mut [u8] = core::slice::from_raw_parts_mut(ptr, BUF_LEN);
    bus::BusHandle::from_slice(buf, DESC_CAPACITY).expect("bus init")
};

let sub = bus.subscribe().expect("subscribe");
let prod = bus.producer();
prod.publish(b"hello from aarnn-nsys bare bus").unwrap();
let mut scratch = [0u8; SLOT_BYTES];
if let Ok(Some(n)) = sub.try_recv(&mut scratch) {
    // prints the received payload via UART
}
```

### Choosing sizes (compile‑time)
- `DESC_CAPACITY` must be a power of two. Larger values reduce backpressure at the cost of more RAM.
- `SLOT_BYTES` is the fixed per‑slot payload size. All published messages must fit in this many bytes.
- Total bytes reserved: `bus::min_buffer_size(DESC_CAPACITY, SLOT_BYTES)`.

Use the helper if you want to understand the metadata overhead for a given ring size:
```
const HDR: usize = bus::header_layout_size(DESC_CAPACITY);
```

### Safety notes (Rust 2024)
- Do not take `&mut`/`&` references to `static mut` storage. The code uses raw pointers + `from_raw_parts_mut` to construct the slice view.
- `publish` is blocking and will busy‑wait under backpressure in `no_std` — there is no OS to yield to.
- The bus uses atomics and `unsafe` internally; `BusHandle::from_slice` performs bounds and layout checks for you.

## Tweaking the demo
- Change `DESC_CAPACITY` and `SLOT_BYTES` at the top of `src/main.rs` to match your needs.
- Replace the simple publish/receive with your own application logic. You can construct additional producers/subscribers; the API is `no_std`‑friendly.

## Troubleshooting
- E0463 “can’t find crate for `test`”: Don’t run `cargo test` for this target. Build with `cargo build --release`.
- No serial output in QEMU: Ensure you used `-M raspi4b -serial stdio -nographic` and that the built path matches the command.
- Ctrl-C doesn’t stop QEMU: You are likely using the stdio multiplexer (`-serial mon:stdio`). Use `Ctrl-a x` to quit, or `Ctrl-a c` then type `quit`. If you prefer Ctrl-C to terminate, use `-monitor none -serial stdio` instead of the multiplexer.
- Garbled UART on real hardware: Verify 115200 8N1 and wiring to UART0 (GPIO14/15). Some boards expose mini‑UART on different pins; this demo targets PL011.
- Stuck at “Bus init error”: Ensure `DESC_CAPACITY` is a power of two and that the buffer length (`BUF_LEN`) was computed with `min_buffer_size`.

## Going further
- The same bus core also works on Linux with the `linux-shm` backend (feature `std`): see `aarnn-nsys` CLI and examples.
- You can run multiple producers/subscribers, or even wire a relay between two in‑memory buses (see `bus::relay_once`).
