ENTRY(_start)
SECTIONS
{
    /*
     * Raspberry Pi loads the kernel at 0x80000.
     */
    . = 0x80000;

    .text.boot : { *(.text.boot) }
    .text : { *(.text) }
    .data : { *(.data) }
    .rodata : { *(.rodata) }
    .bss : { *(.bss) }

    /*
     * Set up the stack pointer at the end of our code.
     * We allocate 16KB for the stack.
     */
    . = ALIGN(16);
    . = . + 0x4000;
    LD_STACK_PTR = .;
}