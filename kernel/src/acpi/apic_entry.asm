; This is the entry point for all cores in the system. Basically, it quickly
; enters long mode and jumps to the bootloader.

[org 0x8000]
[bits 16]

entry:
    ; Disable interrupts, clear direction flag
    cli
    cld

    ; Set the A20
    in    al, 0x92
    or    al, 2
    out 0x92, al

    ; DS is undefined after boot -- clear it
    xor ax, ax
    mov ds, ax

    ; Load the kernel GDT
    lgdt [gdt]

    ; Enable protected mode
    mov eax, cr0
    or  eax, 1
    mov cr0, eax

    ; Jump to protected mode
    jmp 0x18:pm_entry

[bits 32]

pm_entry:
    ; Set up the data selectors
    mov ax, 0x20 ; 0x20 is the 32-bit data entry in the GDT
    mov es, ax
    mov ds, ax
    mov gs, ax
    mov fs, ax
    mov ss, ax

	; Set NXE (NX enable) and LME (long mode enable)
	mov edx, 0
	mov eax, 0x00000900
	mov ecx, 0xc0000080
	wrmsr

	xor eax, eax
	or  eax, (1 <<  9) ; OSFXSR
	or  eax, (1 << 10) ; OSXMMEXCPT
	or  eax, (1 <<  5) ; PAE
	or  eax, (1 <<  3) ; DE
	mov cr4, eax

    ; Set up the CR3
    mov esi, kernel_fill_in
    mov eax, [esi]
    mov edx, [esi + 4]
    mov cr3, eax

	xor eax, eax
	or  eax,  (1 <<  0) ; Protected mode enable
    or  eax,  (1 <<  1) ; Monitor co-processor
	and eax, ~(1 <<  2) ; Clear Emulation flag
	or  eax,  (1 << 16) ; Write protect
	or  eax,  (1 << 31) ; Paging enable
	mov cr0, eax

    ; Jump to the bootloader!
    jmp 0x28:lm_entry

[bits 64]

lm_entry:
    mov rsi, kernel_fill_in
    mov rsp, [rsi + 16] ; Set up the stack
    jmp qword [rsi + 8]

align 8
gdt_base:
    dq 0x0000000000000000 ; 0x00 | null
    dq 0x00009A008000FFFF ; 0x08 | 16-bit, present, code, base 0x8000
    dq 0x000092000000FFFF ; 0x10 | 16-bit, present, data, base 0
    dq 0x00cF9A000000FFFF ; 0x18 | 32-bit, present, code, base 0
    dq 0x00CF92000000FFFF ; 0x20 | 32-bit, present, data, base 0
    dq 0x00209A0000000000 ; 0x28 | 64-bit, present, code, base 0
    dq 0x0000920000000000 ; 0x30 | 64-bit, present, data, base 0

gdt:
    dw (gdt - gdt_base) - 1
    dd gdt_base

; DATA SEGMENT -----------------------------------------------------------------

; These last 3 pointers MUST BE the last thing in this code as that is what the
; kernel will fill in. This is just `shared_data::BootloaderState`

align 8
kernel_fill_in:
    dq 0xCAFECAFECAFECAFE ; bootloader page_table
    dq 0xCAFECAFECAFECAFE ; bootloader entry point
    dq 0xCAFECAFECAFECAFE ; bootloader stack
