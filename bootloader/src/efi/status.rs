//! Status codes returned by EFI routines

use core::convert::From;

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
/// Status codes returned by EFI routines
pub enum Status {
    /// Operation completed successfully
    Success,

    /// Operation completed with a warning
    Warning(Warning),

    /// Operation failed with an error
    Error(Error),
}

impl From<usize> for Status {
    fn from(val: usize) -> Status {
        // Sign extend the code to make it not tied to a specific bitness
        let val = val as i32 as u32 as u64;
        let code = (val & !(1 << 63)) as usize;

        match val {
            0 => Self::Success,
            0x0000000000000001..0x8000000000000000 =>
                Self::Error(Error::from(code)),
            0x8000000000000000..=u64::MAX =>
                Self::Warning(Warning::from(code)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
/// Warning codes returned by EFI routines
pub enum Warning {
    /// The string contained one or more characters that the device could not
    /// render and were skipped
    UnknownGlyph = 1,

    /// The handle was closed, but the file was not deleted
    DeleteFailure = 2,

    /// The handle was closed, but the data to the file was not flushed properly
    WriteFailure = 3,

    /// The resulting buffer was too small, and the data was truncated to the
    /// buffer size
    BufferTooSmall = 4,

    /// The data has not been updated within the timeframe set by local policy
    /// for this type of data
    StaleData = 5,

    /// The resulting buffer contains UEFI-compliant file system
    FileSystem = 6,

    /// The operation will be processed across a system reset
    ResetRequired = 7,

    /// Warning not defined by the UEFI spec, likely OEM defined
    Undefined,
}

impl From<usize> for Warning {
    fn from(val: usize) -> Warning {
        match val {
            1 => Warning::UnknownGlyph,
            2 => Warning::DeleteFailure,
            3 => Warning::WriteFailure,
            4 => Warning::BufferTooSmall,
            5 => Warning::StaleData,
            6 => Warning::FileSystem,
            7 => Warning::ResetRequired,
            _ => Warning::Undefined,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
/// Error codes returned by EFI routines
pub enum Error {
    /// Image failed to load
    LoadError = 1,

    /// A parameter was incorrect
    InvalidParameter = 2,

    /// The operation is not supported
    Unsupported = 3,

    /// The buffer was not the proper size for the request
    BadBufferSize = 4,

    /// The buffer is not large enough to hold the requested data. The required
    /// buffer size is returned in the appropriate parameter when this error
    /// occurs
    BufferTooSmall = 5,

    /// There is no data pending upon return
    NotReady = 6,

    /// The physical device reported an error while attempting the operation
    DeviceError = 7,

    /// The device cannot be written to
    WriteProtected = 8,

    /// A resource has run out
    OutOfResources = 9,

    /// An inconsistency was detected on the file system causing the operation
    /// to fail
    VolumeCorrupted = 10,

    /// There is no more space on the file system
    VolumeFull = 11,

    /// The device does not contain any medium to perform the operation
    NoMedia = 12,

    /// The medium in the device has changed since the last access
    MediaChanged = 13,

    /// The item was not found
    NotFound = 14,

    /// Access was denied
    AccessDenied = 15,

    /// The server was not found or did not respond to the request
    NoResponse = 16,

    /// A mapping to a device does not exist
    NoMapping = 17,

    /// The timeout time expired
    Timeout = 18,

    /// The protocol has not been started
    NotStarted = 19,

    /// The protocol has already been started
    AlreadyStarted = 20,

    /// The operation was aborted
    Aborted = 21,

    /// An ICMP error occurred during the network operation
    IcmpError = 22,

    /// A TFTP error occurred during the network operation
    TftpError = 23,

    /// A protocol error occurred during the network operation
    ProtocolError = 24,

    /// The function encountered an internal version that was incompatible with
    /// a version requested by the caller
    IncompatibleVersion = 25,

    /// The function was not performed due to a security violation
    SecurityViolation = 26,

    /// A CRC error was detected
    CrcError = 27,

    /// Beginning or end of media was reached
    EndOfMedia = 28,

    /// The end of the file was reached
    EndOfFile = 31,

    /// The language specified was invalid
    InvalidLanguage = 32,

    /// The security status of the data is unknown or compromised and the data
    /// must be updated or replaced to restore a valid security status
    CompromisedData = 33,

    /// There is an address conflict address allocation
    IpAddressConflict = 34,

    /// A HTTP error occurred during the network operation
    HttpError = 35,

    /// Error not defined by the UEFI spec, likely OEM defined
    Undefined,
}

impl From<usize> for Error {
    fn from(val: usize) -> Error {
        match val {
             1 => Error::LoadError,
             2 => Error::InvalidParameter,
             3 => Error::Unsupported,
             4 => Error::BadBufferSize,
             5 => Error::BufferTooSmall,
             6 => Error::NotReady,
             7 => Error::DeviceError,
             8 => Error::WriteProtected,
             9 => Error::OutOfResources,
            10 => Error::VolumeCorrupted,
            11 => Error::VolumeFull,
            12 => Error::NoMedia,
            13 => Error::MediaChanged,
            14 => Error::NotFound,
            15 => Error::AccessDenied,
            16 => Error::NoResponse,
            17 => Error::NoMapping,
            18 => Error::Timeout,
            19 => Error::NotStarted,
            20 => Error::AlreadyStarted,
            21 => Error::Aborted,
            22 => Error::IcmpError,
            23 => Error::TftpError,
            24 => Error::ProtocolError,
            25 => Error::IncompatibleVersion,
            26 => Error::SecurityViolation,
            27 => Error::CrcError,
            28 => Error::EndOfMedia,
            31 => Error::EndOfFile,
            32 => Error::InvalidLanguage,
            33 => Error::CompromisedData,
            34 => Error::IpAddressConflict,
            35 => Error::HttpError,
            __ => Error::Undefined,
        }
    }
}
