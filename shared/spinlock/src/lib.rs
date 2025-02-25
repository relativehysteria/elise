//! A mutex-like spinlock implementation

#![no_std]

use core::sync::atomic::{AtomicUsize, Ordering};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};

#[repr(C)]
/// A spinlock-guarded inner-mutable variable
pub struct SpinLock<T: ?Sized> {
    /// Ticket counter. A ticket is grabbed and when `release` is set to this
    /// ticket, you get your variable
    ticket: AtomicUsize,

    /// Current `ticket` value which has been released
    release: AtomicUsize,

    /// The value guarded by this lock
    value: UnsafeCell<T>,
}

// Mark the SpinLock as thread safe
unsafe impl<T: ?Sized + Send> Send for SpinLock<T> {}
unsafe impl<T: ?Sized + Sync> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    /// Move the `value` into a spinlock
    pub const fn new(value: T) -> Self {
        Self {
            ticket: AtomicUsize::new(0),
            release: AtomicUsize::new(0),
            value: UnsafeCell::new(value),
        }
    }
}

impl<T: ?Sized> SpinLock<T> {
    /// Acquire exclusive access to the variable guarded by this spinlock
    pub fn lock(&self) -> SpinLockGuard<T> {
        // Get a ticket
        let ticket = self.ticket.fetch_add(1, Ordering::SeqCst);

        // Spin until we're free to use the value
        while ticket != self.release.load(Ordering::SeqCst) {
            core::hint::spin_loop();
        }

        SpinLockGuard {
            lock: &self,
        }
    }

    /// Return a raw pointer to the internal locked value, bypassing the lock
    pub unsafe fn shatter(&self) -> *mut T {
        self.value.get()
    }
}

/// A guard which implements `Drop` so the locks can be released based on scope
pub struct SpinLockGuard<'a, T: ?Sized> {
    lock: &'a SpinLock<T>,
}

impl<'a, T: ?Sized> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.release.fetch_add(1, Ordering::SeqCst);
    }
}

impl<'a, T: ?Sized> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe {
            &*self.lock.value.get()
        }
    }
}

impl<'a, T: ?Sized> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe {
            &mut *self.lock.value.get()
        }
    }
}
