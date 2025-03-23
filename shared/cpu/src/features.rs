use crate::cpuid;

/// Structure representing the various CPU features which are supported on this
/// system. These can be detected with the `get_cpu_features` function
#[derive(Default, Debug)]
pub struct Features {
    pub max_cpuid: u32,
    pub max_extended_cpuid: u32,

    pub fpu: bool,
    pub vme: bool,
    pub de:  bool,
    pub pse: bool,
    pub tsc: bool,
    pub mmx: bool,
    pub fxsr: bool,
    pub sse: bool,
    pub sse2: bool,
    pub htt: bool,
    pub sse3: bool,
    pub ssse3: bool,
    pub sse4_1: bool,
    pub sse4_2: bool,
    pub x2apic: bool,
    pub aesni: bool,
    pub xsave: bool,
    pub avx: bool,
    pub apic: bool,

    pub vmx: bool,

    pub lahf: bool,
    pub lzcnt: bool,
    pub prefetchw: bool,

    pub syscall: bool,
    pub xd: bool,
    pub gbyte_pages: bool,
    pub rdtscp: bool,
    pub bits64: bool,

    pub avx512f: bool,
}

impl Features {
    /// Returns the set of CPU features
    pub fn get() -> Self {
        let mut features: Self = Default::default();

        unsafe {
            features.max_cpuid          = cpuid(0, 0).0;
            features.max_extended_cpuid = cpuid(0x80000000, 0).0;

            if features.max_cpuid >= 1 {
                let cpuid_1   = cpuid(1, 0);
                features.fpu  = ((cpuid_1.3 >>  0) & 1) == 1;
                features.vme  = ((cpuid_1.3 >>  1) & 1) == 1;
                features.de   = ((cpuid_1.3 >>  2) & 1) == 1;
                features.pse  = ((cpuid_1.3 >>  3) & 1) == 1;
                features.tsc  = ((cpuid_1.3 >>  4) & 1) == 1;
                features.apic = ((cpuid_1.3 >>  9) & 1) == 1;
                features.mmx  = ((cpuid_1.3 >> 23) & 1) == 1;
                features.fxsr = ((cpuid_1.3 >> 24) & 1) == 1;
                features.sse  = ((cpuid_1.3 >> 25) & 1) == 1;
                features.sse2 = ((cpuid_1.3 >> 26) & 1) == 1;
                features.htt  = ((cpuid_1.3 >> 28) & 1) == 1;

                features.sse3    = ((cpuid_1.2 >>  0) & 1) == 1;
                features.vmx     = ((cpuid_1.2 >>  5) & 1) == 1;
                features.ssse3   = ((cpuid_1.2 >>  9) & 1) == 1;
                features.sse4_1  = ((cpuid_1.2 >> 19) & 1) == 1;
                features.sse4_2  = ((cpuid_1.2 >> 20) & 1) == 1;
                features.x2apic  = ((cpuid_1.2 >> 21) & 1) == 1;
                features.aesni   = ((cpuid_1.2 >> 25) & 1) == 1;
                features.xsave   = ((cpuid_1.2 >> 26) & 1) == 1;
                features.avx     = ((cpuid_1.2 >> 28) & 1) == 1;
            }

            // Detect AVX-512 support
            if features.max_cpuid >= 7 {
                let cpuid_7 = cpuid(7, 0);
                features.avx512f = ((cpuid_7.1 >> 16) & 1) == 1;
            }

            if features.max_extended_cpuid >= 0x80000001 {
                let cpuid_e1 = cpuid(0x80000001, 0);

                features.lahf      = ((cpuid_e1.2 >> 0) & 1) == 1;
                features.lzcnt     = ((cpuid_e1.2 >> 5) & 1) == 1;
                features.prefetchw = ((cpuid_e1.2 >> 8) & 1) == 1;

                features.syscall     = ((cpuid_e1.3 >> 11) & 1) == 1;
                features.xd          = ((cpuid_e1.3 >> 20) & 1) == 1;
                features.gbyte_pages = ((cpuid_e1.3 >> 26) & 1) == 1;
                features.rdtscp      = ((cpuid_e1.3 >> 27) & 1) == 1;
                features.bits64      = ((cpuid_e1.3 >> 29) & 1) == 1;
            }
        }

        features
    }
}
