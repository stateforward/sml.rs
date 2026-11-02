//! Shared cells for test contexts.
//!
//! Generated machines keep their context private, so a test hands the
//! machine a shared reference to a context whose fields it can still read.
//! Async callbacks must stay `Send`, so these cells are `Sync`.

#![allow(
    dead_code,
    reason = "each test crate uses the subset of helpers it needs"
)]

use std::sync::{Mutex, MutexGuard, PoisonError};

/// A `Sync` counterpart of `Cell<T>` for `Copy` values.
#[derive(Debug, Default)]
pub struct SyncCell<T>(Mutex<T>);

impl<T: Copy> SyncCell<T> {
    pub const fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }

    pub fn get(&self) -> T {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set(&self, value: T) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = value;
    }
}

/// A `Sync` counterpart of `RefCell<T>`.
#[derive(Debug, Default)]
pub struct SyncRefCell<T>(Mutex<T>);

impl<T> SyncRefCell<T> {
    pub const fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }

    pub fn borrow(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn borrow_mut(&self) -> MutexGuard<'_, T> {
        self.borrow()
    }

    pub fn take(&self) -> T
    where
        T: Default,
    {
        core::mem::take(&mut *self.borrow())
    }
}
