; System V AMD64 ABI is used here. Respectively, args are passed in:
; rdi rsi rdx rcx r8 r9 stack

[bits 64]

global _trampoline
_trampoline:
    ; rdi = kernel_entry
    ; rsi = kernel_stack
    ; rdx = kernel_table
    ; rcx = shared_paddr
    ; r8  = core_id

    mov rax, 0x4141414141414141
    mov rbx, 0x4141414141414141
    mov rdx, 0x4141414141414141
    mov rsi, 0x4141414141414141
    mov rdi, 0x4141414141414141
    mov rcx, 0x4141414141414141
    mov rbp, 0x4141414141414141
    mov r8,  0x4141414141414141
    mov r9,  0x4141414141414141
    mov r10, 0x4141414141414141
    mov r11, 0x4141414141414141
    mov r12, 0x4141414141414141
    mov r13, 0x4141414141414141
    mov r14, 0x4141414141414141
    mov r15, 0x4141414141414141
    mov cr6, rax ; #UD

    ; Switch to the kernel page table
    mov cr3, rdx

    ; Set up the new stack for this core
    mov rsp, rsi

    ; Save the kernel entry point before we jump to it
    mov rax, rdi

    ; Set up the kernel arguments before the jump
    mov rdi, rcx ; shared_paddr
    mov rsi, r8  ; core_id

    ; Jump to the kernel
    jmp rax
