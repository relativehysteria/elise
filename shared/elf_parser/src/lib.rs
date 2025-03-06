//! Parser for basic x86_64 statically linked little endian ELF files.

#![no_std]

/// Read bytes and little-endian interpret them as a given type
macro_rules! get_bytes {
    ($bytes:expr, $offset:expr, $type:ty) => {{
        use core::mem::size_of;
        let range = ($offset as usize)..(($offset as usize)
            .checked_add(size_of::<$type>()).ok_or(Error::ParseFailure)?);
        <$type>::from_le_bytes($bytes.get(range).ok_or(Error::ParseFailure)?
            .try_into().ok().ok_or(Error::ParseFailure)?)
    }}
}

/// Virtual address type for better readability
pub type VirtAddr = u64;

/// Virtual size type for better readability
pub type VirtSize = u64;

pub enum Error {
    /// The byte data couldn't be parsed
    ParseFailure,

    /// The ELF to be parsed didn't have enough bytes
    NotEnoughBytes,

    /// The ELF file had the wrong magic bytes
    WrongMagic([u8; 4]),

    /// The file was not 64-bit
    WrongBitness,

    /// The file was not little endian
    WrongEndian,

    /// The ELF version was incorrect
    WrongVersion(u8),

    /// The ELF type was not EXECUTABLE
    WrongType(u16),

    /// A different machine type was expected
    WrongMachine(u16),

    /// The virtual address of the section was not aligned
    Unaligned,

    /// The raw size of the segment in the file was larger than the virtual size
    RawSizeTooLarge,

    /// The closure that was called on each segment failed
    SegmentsClosureFailed,
}

/// Permission bits for memory segments
pub struct Permissions {
    /// Marks the memory as readable
    pub read: bool,

    /// Marks the memory as writeable
    pub write: bool,

    /// Marks the memory as executable
    pub execute: bool,
}

impl Permissions {
    /// Returns a new permissions struct encoding the arguments
    pub fn new(read: bool, write: bool, execute: bool) -> Self {
        Self { read, write, execute }
    }

    /// Returns a new permission struct encoding the ELF program header flags
    pub fn from_flags(flags: u32) -> Self {
        let execute = (flags & (1 << 0)) == 1;
        let write   = (flags & (1 << 1)) == 1;
        let read    = (flags & (1 << 2)) == 1;
        Self::new(read, write, execute)
    }
}

/// A validated ELF file
pub struct Elf<'a> {
    /// Raw bytes of the ELF file
    bytes: &'a [u8],

    /// Offset into the ELF file to the start of the program header table
    ph_offset: usize,

    /// Size of the program header table entries
    ph_entry_size: u16,

    /// Number of program header entries
    ph_num: usize,

    /// Address of the entry point
    pub entry: VirtAddr,
}

impl<'a> Elf<'a> {
    /// Parse an ELF file and return its parsed representation
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let bytes: &[u8] = bytes.as_ref();

        // Check for the ELF header
        if bytes.get(..0x04) != Some(b"\x7FELF") {
            return Err(Error::WrongMagic(bytes[0x00..0x04]
                    .try_into().unwrap()));
        }

        // Make sure we have a 64-bit file
        if get_bytes!(bytes, 0x04, u8) != 2 {
            return Err(Error::WrongBitness);
        }

        // Make sure we have a little endian file
        if get_bytes!(bytes, 0x05, u8) != 1 {
            return Err(Error::WrongEndian);
        }

        // Make sure we have the expected version
        if get_bytes!(bytes, 0x06, u8) != 1 {
            return Err(Error::WrongVersion(bytes[0x06]))
        }

        // Make sure we have an executable
        if get_bytes!(bytes, 0x10, u16) != 2 {
            return Err(Error::WrongType(u16::from_le_bytes(
                        bytes[0x10..0x12].try_into().unwrap())))
        }

        // Make sure we have an amd64 file
        if get_bytes!(bytes, 0x12, u16) != 0x3E {
            return Err(Error::WrongMachine(u16::from_le_bytes(
                        bytes[0x12..0x14].try_into().unwrap())))
        }

        // Get the entry point
        let entry = get_bytes!(bytes, 0x18, u64);

        // Get the offset to the start of the program header table
        let ph_offset = get_bytes!(bytes, 0x20, u64) as usize;

        // Get the size of program header table entries
        let ph_entry_size = get_bytes!(bytes, 0x36, u16);

        // Get the number of program header table entries
        let ph_num = get_bytes!(bytes, 0x38, u16) as usize;

        // Make sure that all the entries are in bounds of the bytes
        let table_size = ph_offset.checked_add(
            ph_num.checked_mul(ph_entry_size as usize)
                .ok_or(Error::ParseFailure)?
            ).ok_or(Error::ParseFailure)?;

        if bytes.len() < table_size {
            return Err(Error::NotEnoughBytes);
        }

        // Return the parsed ELF
        Ok(Self { bytes, entry, ph_offset, ph_entry_size, ph_num })
    }

    /// Invoke a closure on every loadable segment with the format
    /// (vaddr, offset from vaddr, vsize, raw segment bytes, permissions)
    /// where
    ///     * `vaddr` is the aligned virtual address of the segment
    ///     * `offset` is the offset from the `vaddr` where the segment bytes
    ///       should be placed
    ///     * `vsise` is the size of the raw segment bytes
    ///     * `raw segment bytes` are the raw segment bytes
    ///     * `permissions` are the memory permissions for this segment
    pub fn segments<F>(&self, mut closure: F) -> Result<(), Error>
    where F: FnMut(VirtAddr, u64, VirtSize, &[u8], Permissions) -> Option<()> {
        let bytes = self.bytes;

        // Go through every segment
        for segment in 0..self.ph_num {
            // Get the offset to the beginning of this program header.
            // This calculation won't overflow; it's been checked during parsing
            let offset = self.ph_offset + segment * self.ph_entry_size as usize;

            // Skip segments that are not loadable
            if get_bytes!(bytes, offset + 0x00, u32) != 0x00000001 { continue; }

            // Get the segment memory permissions
            let perms = Permissions::from_flags(
                get_bytes!(bytes, offset + 0x04, u32));

            // Get the offset of the segment in the file image
            let raw_offset = get_bytes!(bytes, offset + 0x08, u64) as usize;

            // Get the virtual address of the segment in memory
            let vaddr = get_bytes!(bytes, offset + 0x10, u64);

            // Get the size of the segment in file (may be 0)
            let raw_size = get_bytes!(bytes, offset + 0x20, u64) as usize;

            // Get the size of the segment in memory
            let vsize = get_bytes!(bytes, offset + 0x28, u64);

            // The segment size in the file should never be larger than the
            // virtual size
            if raw_size as u64 > vsize { return Err(Error::RawSizeTooLarge); }

            // Get the required alignment mask for this segment
            let align_mask = get_bytes!(bytes, offset + 0x30, u64) - 1;

            // Get the aligned virtual address and the offset for this segment
            let aligned_vaddr = vaddr & (!align_mask);
            let virtual_offset = vaddr - aligned_vaddr;

            // Invoke the closure
            closure(
                aligned_vaddr,
                virtual_offset,
                vsize,
                bytes.get(raw_offset..raw_offset.checked_add(raw_size)
                    .ok_or(Error::ParseFailure)?).ok_or(Error::ParseFailure)?,
                perms
            ).ok_or(Error::SegmentsClosureFailed)?;
        }
        Ok(())
    }
}
