; System V AMD64 ABI is used here. Respectively, args are passed in:
; rdi rsi rdx rcx r8 r9 stack

[bits 64]

global .trampoline
.trampoline:
    ; The following arguments are required for the trampoline:
    ;   rdi = entry
    ;   rsi = stack
    ;   rdx = table

    ; Don't interrupt mid change
    cli

    ; Set up the stack for this core
    mov rsp, rsi

    ; Switch to the specified page table
    mov cr3, rdx

    ; Save the entry point before we jump to it
    mov rax, rdi

    ; Jump to the entry point
    call rax
