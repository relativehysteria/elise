//! Scope based reference count

#![no_std]

#[cfg(test)] mod tests;

use core::sync::atomic::{AtomicUsize, Ordering};

/// Auto reference structure that allows for scope-based reference counting.
pub struct AutoRefCount(AtomicUsize);

impl AutoRefCount {
    /// Returns a new automatic reference count
    pub const fn new(init: usize) -> Self {
        Self(AtomicUsize::new(init))
    }

    /// Returns the current number of references
    pub fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }

    /// Increment the reference count and return the guard which will decrement
    /// the count automatically when it goes out of scope
    pub fn increment(&self) -> AutoRefCountGuard {
        // Increment the count
        let count = self.0.fetch_add(1, Ordering::SeqCst);

        // Make sure we didn't overflow
        count.checked_add(1).expect("Overflow on AutoRefCount decrement");

        // Return the guard
        AutoRefCountGuard(self)
    }
}

/// Guard structure which will automatically decrement the count when it goes
/// out of scope
pub struct AutoRefCountGuard<'a>(&'a AutoRefCount);

impl <'a> Drop for AutoRefCountGuard<'a> {
    fn drop(&mut self) {
        // Decrement the count
        let count = (self.0).0.fetch_sub(1, Ordering::SeqCst);

        // Make sure we didn't overflow
        count.checked_sub(1).expect("Overflow on AutoRefCount decrement");
    }
}
