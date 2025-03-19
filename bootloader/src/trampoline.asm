; System V AMD64 ABI is used here. Respectively, args are passed in:
; rdi rsi rdx rcx r8 r9 stack

[bits 64]

global .trampoline
.trampoline:
    ; The following arguments are required for the trampoline:
    ;   rdi = entry
    ;   rsi = stack
    ;   rdx = table

    ; The following arguments are required when jumping to the kernel:
    ;   rcx = core_id

    ; Don't interrupt mid change
    cli

    ; Set up the stack for this core
    mov rsp, rsi

    ; Switch to the specified page table
    mov cr3, rdx

    ; Save the entry point before we jump to it
    mov rax, rdi

    ; Set up the _kernel_ arguments before the jump.
    ; The bootloader takes no arguments so if we're jumping to the bootloader,
    ; this doesn't do anything
    mov rdi, rcx ; core_id

    ; Jump to the entry point
    jmp rax
