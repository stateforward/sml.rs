//! # sml.rs
//!
//! A `no_std` state-machine library whose primary [`sml!`] procedural macro
//! mirrors the `sml.cpp` transition-table DSL.
#![doc = include_str!("../docs/dsl.md")]
#![no_std]

extern crate self as sml;

pub use sml_macros::sml;

pub mod utility;

/// Common synchronous interface implemented by generated state machines that
/// do not require a temporary context.
pub trait Machine<E> {
    /// Generated state enum.
    type State;

    /// Processes one event to run-to-completion and reports whether it was
    /// accepted.
    fn process_event(&mut self, event: E) -> bool;

    /// Processes one event to run-to-completion and returns its acceptance as a
    /// future. The default inline path does not allocate.
    #[inline]
    fn process_event_async(&mut self, event: E) -> impl core::future::Future<Output = bool> {
        core::future::ready(self.process_event(event))
    }
}

/// Reports whether a state machine has reached its terminal state.
pub trait Terminated {
    /// Returns true after entering the generated `X` state.
    fn is_terminated(&self) -> bool;
}
