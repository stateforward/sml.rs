//! # sml.rs
//!
//! A `no_std` state-machine library whose primary [`sml!`] procedural macro
//! mirrors the `sml.cpp` transition-table DSL.
#![doc = include_str!("../docs/dsl.md")]
#![no_std]
#![forbid(unsafe_code)]
#![deny(warnings)]
#![deny(
    elided_lifetimes_in_paths,
    missing_docs,
    rust_2018_idioms,
    unsafe_op_in_unsafe_fn,
    unused_must_use
)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::allow_attributes_without_reason,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::dbg_macro,
    clippy::exit,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::float_arithmetic,
    clippy::get_unwrap,
    clippy::infinite_loop,
    clippy::indexing_slicing,
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    clippy::large_stack_arrays,
    clippy::let_underscore_must_use,
    clippy::lossy_float_literal,
    clippy::mem_forget,
    clippy::mixed_read_write_in_expression,
    clippy::modulo_arithmetic,
    clippy::option_env_unwrap,
    clippy::panicking_overflow_checks,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::rc_buffer,
    clippy::rc_mutex,
    clippy::string_slice,
    clippy::unseparated_literal_suffix,
    clippy::transmute_ptr_to_ptr,
    clippy::transmute_undefined_repr,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable,
    clippy::uninit_assumed_init,
    clippy::unwrap_in_result,
    clippy::unwrap_used
)]

// Generated code refers to the runtime through the stable `::sml` path,
// including when a machine is declared from this crate's own tests.
#[allow(
    unused_extern_crates,
    reason = "the generated code uses the stable crate alias `::sml`"
)]
extern crate self as sml;

pub use sml_macros::sml;

mod policy;
pub mod utility;

pub use policy::{
    BranchStm, DefaultQueuePolicy, DeferQueue, Dispatch, EventName, JumpTable, Logger, NoPolicy,
    Observer, Policies, PolicyBundle, PolicyBundleParts, PolicyParts, ProcessQueue, Queue,
    QueuePolicy, RawMutex, StateName, SwitchStm, Testing, TestingAccess, TestingPolicy, ThreadSafe,
    ThreadSafety,
};

/// Marker and name access for a static typed event payload.
///
/// Static event payloads can be identified with
/// `core::any::TypeId::of::<E>()` in an [`EventVisitor`] implementation. The
/// generated `Events` enum also exposes its exact variant names through its
/// `EVENT_NAMES` const table.
pub trait Event: 'static {
    /// Returns the final component of the Rust type name.
    #[must_use]
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

#[cfg(test)]
mod tests {
    use super::Event;

    struct TestEvent;

    #[test]
    fn event_name_uses_the_final_type_path_component() {
        assert_eq!(TestEvent::name(), "TestEvent");
    }
}
