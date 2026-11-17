/* The watch's builds laid out for QEMU's mps2-an386: a Cortex-M4 with an
   FPU and 4 MB of RAM at 0, which QEMU loads the ELF into. The vector
   table is first: the stack's top, then the reset and fault handlers
   (rt.rs). */
MEMORY
{
  RAM : ORIGIN = 0x00000000, LENGTH = 4M
}

ENTRY(reset);

SECTIONS
{
  .text :
  {
    LONG(ORIGIN(RAM) + LENGTH(RAM));
    KEEP(*(.vectors));
    *(.text .text.*);
  } > RAM

  .rodata : ALIGN(4)
  {
    *(.rodata .rodata.*);
  } > RAM

  .data : ALIGN(4)
  {
    *(.data .data.*);
  } > RAM

  .bss (NOLOAD) : ALIGN(4)
  {
    __bss_start = .;
    *(.bss .bss.* COMMON);
    . = ALIGN(4);
    __bss_end = .;
  } > RAM
}
