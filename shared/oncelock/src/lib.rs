//! A synchronization primitive which can nominally be written to only once.

#![no_std]

use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

/// A synchronization primitive which can nominally be written to only once.
///
/// If accessed before set, or if set multiple times, this lock will panic.
#[repr(C)]
pub struct OnceLock<T: Sized> {
    /// Whether the value has been initialized
    initialized: AtomicBool,

    /// The value guarded by this lock
    value: UnsafeCell<MaybeUninit<T>>,
}

// Mark the lock as thread safe
unsafe impl<T: Send> Send for OnceLock<T> {}
unsafe impl<T: Sync> Sync for OnceLock<T> {}

impl<T> OnceLock<T> {
    /// Create a new oncelock prepared to hold `T`
    pub const fn new() -> Self {
        OnceLock {
            initialized: AtomicBool::new(false),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// Returns the value in this lock.
    ///
    /// Panics if the value hasn't been set yet.
    #[track_caller]
    pub fn get(&self) -> &T {
        if !self.initialized() {
            panic!("OnceLock value is not initialized");
        }

        unsafe { &*(*self.value.get()).as_ptr() }
    }

    /// Initializes the value in this lock.
    ///
    /// Panics if the value has been set already.
    #[track_caller]
    pub fn set(&self, value: T) {
        assert!(!self.initialized.swap(true, Ordering::SeqCst),
            "OnceLock is already initialized");

        unsafe { (*self.value.get()).as_mut_ptr().write(value); }
    }

    /// Returns whether the value in the lock has been initialized already
    pub fn initialized(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }
}
