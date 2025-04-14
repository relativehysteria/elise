//! A mutex-like spinlock implementation

#![no_std]

use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};

/// A dummy structure which can be used to implement an interrupt ignoring
/// `SpinLock`
pub struct DummyInterruptState;

impl InterruptState for DummyInterruptState {
    fn in_interrupt() -> bool { false }
    fn in_exception() -> bool { false }
    fn enter_lock() {}
    fn exit_lock() {}
}

/// Trait that allows access to OS-level constructs defining interrupt state,
/// exception state, unique core IDs, and enter/exit lock (for interrupt
/// disabling and enabling) primitives.
///
/// It's up to the callee to handle the nesting of the interrupt status.
pub trait InterruptState {
    /// Returns `true` if we're currently in an interrupt
    fn in_interrupt() -> bool;

    /// Returns `true` if we're currently in an exception. Which indicates that
    /// a lock cannot be held as we may have pre-empted a non-preemptable lock
    fn in_exception() -> bool;

    /// A lock which does not allow interrupting was taken, and thus interrupts
    /// must be disabled.
    fn enter_lock();

    /// A lock which does not allow interrupting was released, and thus
    /// interrupts can be enabled.
    fn exit_lock();
}

#[repr(C)]
/// A spinlock-guarded inner-mutable variable
pub struct SpinLock<T: ?Sized, I: InterruptState> {
    /// Ticket counter. A ticket is grabbed and when `release` is set to this
    /// ticket, you get your variable
    ticket: AtomicUsize,

    /// Current `ticket` value which has been released
    release: AtomicUsize,

    /// If set to `true`, it is required that interrupts are disabled prior to
    /// this lock being taken.
    disable_interrupts: bool,

    /// The phantom holder of the `InterruptState` for this lock.
    ///
    /// We have to save the `InterruptState` somewhere because we need to pass
    /// it to the `LockGuard` given out during `lock()`
    _interrupt_state: PhantomData<I>,

    // This has to be the last field to allow for `?Size` values
    /// The value guarded by this lock
    value: UnsafeCell<T>,
}

// Mark the SpinLock as thread safe
unsafe impl<T: ?Sized + Send, I: InterruptState> Send for SpinLock<T, I> {}
unsafe impl<T: ?Sized + Sync, I: InterruptState> Sync for SpinLock<T, I> {}

impl<T, I: InterruptState> SpinLock<T, I> {
    /// Move the `value` into a spinlock
    pub const fn new(value: T) -> Self {
        Self {
            ticket:             AtomicUsize::new(0),
            release:            AtomicUsize::new(0),
            value:              UnsafeCell::new(value),
            _interrupt_state:   PhantomData,
            disable_interrupts: false,
        }
    }

    pub const fn new_no_preempt(value: T) -> Self {
        Self {
            ticket:             AtomicUsize::new(0),
            release:            AtomicUsize::new(0),
            value:              UnsafeCell::new(value),
            _interrupt_state:   PhantomData,
            disable_interrupts: true,
        }
    }
}

impl<T: ?Sized, I: InterruptState> SpinLock<T, I> {
    /// Acquire exclusive access to the variable guarded by this spinlock
    pub fn lock(&self) -> SpinLockGuard<T, I> {
        // Make sure we don't use a non-preemptable lock during an interrupt.
        assert!(self.disable_interrupts || !I::in_interrupt(),
            "Attempted to take a non-preemptable lock in an interrupt");

        // Disable interrupts if needed
        if self.disable_interrupts {
            I::enter_lock();
        }

        // Get a ticket
        let ticket = self.ticket.fetch_add(1, Ordering::SeqCst);

        // Spin until we're free to use the value
        while ticket != self.release.load(Ordering::SeqCst) {
            core::hint::spin_loop();
        }

        SpinLockGuard::<T, I> {
            lock: self,
        }
    }

    /// Return a raw pointer to the internal locked value, bypassing the lock
    pub unsafe fn shatter(&self) -> *mut T {
        self.value.get()
    }
}

/// A guard which implements `Drop` so the locks can be released based on scope
pub struct SpinLockGuard<'a, T: ?Sized, I: InterruptState> {
    lock: &'a SpinLock<T, I>,
}

impl<'a, T: ?Sized, I: InterruptState> Drop for SpinLockGuard<'a, T, I> {
    fn drop(&mut self) {
        // Release the lock
        self.lock.release.fetch_add(1, Ordering::SeqCst);

        // Enable interrupts if needed
        if self.lock.disable_interrupts { I::exit_lock(); }
    }
}

impl<'a, T: ?Sized, I: InterruptState> Deref for SpinLockGuard<'a, T, I> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe {
            &*self.lock.value.get()
        }
    }
}

impl<'a, T: ?Sized, I: InterruptState> DerefMut for SpinLockGuard<'a, T, I> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe {
            &mut *self.lock.value.get()
        }
    }
}
