.globl _start
.globl system_off
.extern LD_STACK_PTR

.section ".text.boot"

_start:
    // Determine core ID from MPIDR_EL1 (Affinity level 0)
    mrs     x0, mpidr_el1
    and     x0, x0, #0xFF          // x0 = core ID (Aff0)
    cbnz    x0, system_off         // non-primary cores park immediately

    // Primary core (core 0): set stack pointer
    ldr     x30, =LD_STACK_PTR
    mov     sp, x30
    // Ensure visibility of earlier writes and synchronize context before jumping into Rust
    dsb     sy
    isb

    // Call our Rust main function
    bl      not_main

// A simple 'shutdown' / park for bare-metal
// Non-primary cores land here and wait forever; primary core calls this on exit.
system_off:
    wfi     // Wait For Interrupt
    b       system_off
