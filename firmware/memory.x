MEMORY
{
  FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 2048K
  RAM (rwx)  : ORIGIN = 0x20000000, LENGTH = 320K
  CCRAM (rwx): ORIGIN = 0x10000000, LENGTH = 64K
}

_entry_point = Reset;

SECTIONS
{
  .text :
  {
    *(.text .text.*)
  } > FLASH

  .rodata :
  {
    *(.rodata .rodata.*)
  } > FLASH

  .data :
  {
    _sidata = LOADADDR(.data);
    _sdata = .;
    *(.data .data.*);
    _edata = .;
  } > RAM AT > FLASH

  .bss :
  {
    _sbss = .;
    *(.bss .bss.*);
    _ebss = .;
  } > RAM

  _stack_start = ORIGIN(RAM) + LENGTH(RAM);
}
