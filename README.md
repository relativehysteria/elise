this was my attempt at an almost interrupt-less poll-based kernel.
IO APIC, TCP and kernel image download (on soft reboots) have yet to be
implemented fully.

the use case was for kernels that are heavy in compute and don't have to handle
spurious connections (over the net, such that a poll-based network stack would
be sufficient), but i got bored of the process, especially because lately i've
been wanting to write an async kernel but didn't want to rewrite this one.
