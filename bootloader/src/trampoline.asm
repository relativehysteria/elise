; System V AMD64 ABI is used here. Respectively, args are passed in:
; rdi rsi rdx rcx r8 r9 stack

[bits 64]

global .trampoline
.trampoline:
    ; rdi = kernel_entry
    ; rsi = kernel_stack
    ; rdx = kernel_table
    ; rcx = shared_paddr
    ; r8  = core_id

    ; Don't interrupt mid change
    cli

    ; Set up the new stack for this core
    mov rsp, rsi

    ; Switch to the kernel page table
    mov cr3, rdx

    ; Save the kernel entry point before we jump to it
    mov rax, rdi

    ; Set up the kernel arguments before the jump
    mov rdi, rcx ; shared_paddr
    mov rsi, r8  ; core_id

    ; Jump to the kernel
    jmp rax
