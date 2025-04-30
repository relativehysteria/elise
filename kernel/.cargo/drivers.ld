SECTIONS {
    .probe_functions :
    {
        __start_pci_probes = .;
        KEEP(*(.pci_probes))
        __end_pci_probes = .;
    }
}
INSERT AFTER .text
