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
    /// Generated processing error.
    type Error;

    /// Processes one event.
    fn process(&mut self, event: E) -> Result<&Self::State, Self::Error>;
}

/// Reports whether a state machine has reached its terminal state.
pub trait Terminated {
    /// Returns true after entering the generated `X` state.
    fn is_terminated(&self) -> bool;
}
