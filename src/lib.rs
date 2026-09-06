//! # sml.rs
//!
//! A `no_std` state-machine library whose primary [`sml!`] procedural macro
//! mirrors the `sml.cpp` transition-table DSL.
#![doc = include_str!("../docs/dsl.md")]
#![no_std]

extern crate self as sml;

pub use sml_macros::sml;

mod policy;
pub mod utility;

pub use policy::{
    BranchStm, DefaultQueuePolicy, DeferQueue, Dispatch, EventName, JumpTable, Logger, NoPolicy,
    Observer, Policies, PolicyBundle, ProcessQueue, Queue, QueuePolicy, RawMutex, StateName,
    SwitchStm, Testing, TestingAccess, TestingPolicy, ThreadSafe, ThreadSafety,
};

/// Marker and name access for a static typed event payload.
///
/// Static event payloads can be identified with
/// `core::any::TypeId::of::<E>()` in an [`EventVisitor`] implementation. The
/// generated `Events` enum also exposes its exact variant names through its
/// `EVENT_NAMES` const table.
pub trait Event: 'static {
    /// Returns the final component of the Rust type name.
    fn name() -> &'static str {
        let type_name = core::any::type_name::<Self>();
        type_name.rsplit("::").next().unwrap_or(type_name)
    }
}

impl<T: 'static> Event for T {}

/// Receives the typed event payloads that a generated machine can accept from
/// its current state.
pub trait EventVisitor {
    /// Visits one typed event payload. The method is called at most once per
    /// payload type for each query; `E::name()` and `TypeId::of::<E>()` identify
    /// the type.
    fn visit<E: Event>(&mut self);
}

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
