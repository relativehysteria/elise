this was my attempt at an almost interrupt-less poll-based kernel.

the use case was for kernels that are heavy in compute and don't have to handle
spurious connections (over the net, such that a poll-based network stack would
be sufficient), but i got bored of the process, especially because lately i've
been wanting to write an async kernel but didn't want to rewrite this one.

__here's how it works__

## bootloader
1) get execution from UEFI
2) get the physical memory map and exit boot services
3) create a kernel page table and map in the kernel
4) jump to the kernel through a trampoline
5) when the kernel exits, go to 3 and repeat

if i had finished the kernel TCP stack, i'd have implemented the soft reboot mechanism fully. that is, the kernel could download a new kernel image and put it somewhere into shared memory. the bootloader could then load the new image and boot that one.

## kernel
only 2 interrupts are active:
1) NMI (which is sent by a core to turn off other cores in case of a panic)
2) APIC timer (which is used to periodically check the serial port for input; an APIC timer is used here because the serial port has all interrupts masked)

the [`kernel/src/main.rs`](kernel/src/main.rs) file is very simple. feel free to check it out to see how the kernel gets set up

i still had planned to finish the TCP stack, implement the IO APIC scalably and then write some IPC code to allow for intercore communication, and all of that could be easily done within a week, but more importantly, i lost my motivation and i'm not happy with this kernel design anymore.

so published it is.
