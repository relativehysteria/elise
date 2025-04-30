//! APIC and IO APIC implementations

mod system;
pub mod local;
// pub mod ioapic;

pub use system::*;
pub use local::LocalApic;
// pub use ioapic::IoApic;
