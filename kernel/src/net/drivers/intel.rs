//! Intel e1000/e drivers

use alloc::sync::Arc;
use crate::pci::{DeviceConfig, Device};

/// The Intel gigabit network driver
struct Driver {
}

impl Device for Driver {
    fn purge(&self) {
        println_shatter!("Purge called!");
    }
}

fn probe(_cfg: &DeviceConfig) -> Option<Arc<dyn Device>> {
    println!("Registered driver! for cfg: {_cfg:?}");
    Some(Arc::new(Driver{}))
}
crate::register_pci_driver!(probe);
