# raspi-bare-metal — Wiki Home — Wiki Home

A bootable bare-metal AArch64 image for Raspberry Pi 4 that demonstrates the [aarnn-nsys](https://github.com/neuralmimicry/aarnn-nsys) message bus in a `no_std` environment — no OS, no allocator, boots directly from the UART.

> ☕ [Support NeuralMimicry on Crowdfunder](https://www.crowdfunder.co.uk/p/qr/aWggxwPW?utm_campaign=sharemodal&utm_medium=referral&utm_source=shortlink)

---

## Quick navigation

| Page | Description |
|---|---|
| [Getting Started](Getting-Started) | Build, configure, and run |
| [Contributing](Contributing) | How to raise issues and submit pull requests |

---

## Build

Requires a Rust AArch64 bare-metal toolchain:

```bash
# Install the target
rustup target add aarch64-unknown-none

# Build
cargo build --release
```

The linker script (`raspi.ld`) and startup assembly (`src/start.s`) handle boot.

## What it demonstrates

1. Boots without an OS using `start.s` and `raspi.ld`
2. Initialises PL011 UART0 and prints to serial console
3. Allocates a static in-memory region and instantiates the aarnn-nsys bus
4. Creates a producer + subscriber, publishes a message, receives it, and prints the payload



## Get involved

- 🐛 [Report a bug or request a feature](https://github.com/neuralmimicry/raspi-bare-metal/issues)
- 💬 [Join the discussion](https://github.com/neuralmimicry/raspi-bare-metal/discussions)
- 📧 Direct support from the founder: [info@neuralmimicry.ai](mailto:info@neuralmimicry.ai) · **£1,000/day + VAT**
- 🌐 [neuralmimicry.ai](https://neuralmimicry.ai)
