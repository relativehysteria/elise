my kernels usually have one requirement: network boot.  
by booting the kernel over the network, one can implement a soft reboot by
re-downloading the newest kernel image. this saves time by not having to go
through a hardware reboot and POST (which can take _minutes_ on high end
hardware) and makes it easy to write small changes to the kernel on each boot.

i usually implement the kernels in two ways:
1) by using BIOS and its PXE services.
2) by using UEFI and its boot services.
   this is for simple experiments where soft reboot is not required.  
   once the UEFI boot services are exited, PXE routines are no longer
   accessible which forces the bootloader to, at the very least, implement a
   small net stack if soft reboot is required. if the kernel has its own net
   stack, this leads to duplicated code -- not ideal.

this time i do things little differently so i can stop writing BIOS based
systems. the bootloader downloads the first kernel over PXE, exits the boot
services and then relies on the kernel to download new kernel images and handing
them over to the bootloader on soft reboots.
