//! The deferred-event drain shared by the flat, composite, and orthogonal
//! generators.
//!
//! The drain follows sml.cpp (`process_defer_events`): it runs once per
//! top-level step, after the step changed the state, and dispatches without
//! draining again, so nothing recurses. Each pass retries the events that were
//! deferred when it began, front to back; an event deferred again keeps its
//! place among them. When a retried event changes the state, the pass starts
//! over from the front, so retained events are retried in the new state ahead
//! of the events not yet retried. An event deferred again without a state
//! change (an internal row or an external self-transition) therefore waits for
//! the next state change instead of being retried forever.
//!
//! The queue is rotated in place: an event is popped before it is dispatched,
//! and a re-deferral pushes it to the back, so the queue never holds more
//! events than it did when the pass began and the policy's queue is the only
//! queue used. `deferred_len` counts the events in the queue, since `Queue`
//! does not report its length. While a pass runs the queue holds the events
//! not yet retried followed by those retained; moving the former behind the
//! latter (on a state change or a propagated error) restores queue order.

use proc_macro2::TokenStream;
use quote::quote;

/// Defers `event`, counting it; `?` returns `QueueFull` when the queue is full.
pub fn defer(event: &TokenStream, error: &TokenStream) -> TokenStream {
    quote! {
        ::sml::Queue::defer(&mut self.deferred, #event)
            .map_err(|_| #error::QueueFull)?;
        self.deferred_len = self.deferred_len.saturating_add(1);
    }
}

/// Moves the `__sml_unvisited` events at the front of the queue behind the
/// retained ones. Each push follows a pop, so a queue that held the events a
/// moment ago has room for them.
fn rotate_unvisited(error: &TokenStream) -> TokenStream {
    quote! {
        for _ in 0..__sml_unvisited {
            if let Some(event) = ::sml::Queue::pop(&mut self.deferred) {
                if ::sml::Queue::defer(&mut self.deferred, event).is_err() {
                    self.deferred_len = self.deferred_len.saturating_sub(1);
                    return Err(#error::QueueFull);
                }
            }
        }
    }
}

/// The synchronous drain, run after a top-level dispatch that changed the
/// state. `dispatch` is an expression dispatching `deferred_event` without
/// draining and evaluating to a `Result`; `state_key` is an expression that
/// compares equal exactly when the machine is in the same states.
pub fn sync_drain(
    state_key: &TokenStream,
    dispatch: &TokenStream,
    error: &TokenStream,
) -> TokenStream {
    let rotate = rotate_unvisited(error);
    quote! {
        let mut __sml_unvisited: usize = self.deferred_len;
        while __sml_unvisited > 0 {
            __sml_unvisited = __sml_unvisited.saturating_sub(1);
            let Some(deferred_event) = ::sml::Queue::pop(&mut self.deferred) else {
                break;
            };
            self.deferred_len = self.deferred_len.saturating_sub(1);
            let __sml_before = #state_key;
            let __sml_result = (#dispatch).map(|_| ());
            match __sml_result {
                Ok(()) | Err(#error::InvalidEvent | #error::TransitionsFailed) => {}
                Err(error) => {
                    #rotate
                    return Err(error);
                }
            }
            if __sml_before != #state_key {
                #rotate
                __sml_unvisited = self.deferred_len;
            }
        }
    }
}

/// The asynchronous event loop for machines with a defer queue: events queued
/// with `process` run first in, and each one that changes the state is
/// followed by a drain pass before the next queued event, as in sml.cpp.
/// `dispatch` dispatches `event` without draining.
pub fn async_loop(
    state_key: &TokenStream,
    dispatch: &TokenStream,
    error: &TokenStream,
    return_state: &TokenStream,
) -> TokenStream {
    let rotate = rotate_unvisited(error);
    quote! {
        let mut __sml_unvisited: usize = 0;
        loop {
            let (event, __sml_deferred) = if __sml_unvisited > 0 {
                __sml_unvisited = __sml_unvisited.saturating_sub(1);
                let Some(event) = ::sml::Queue::pop(&mut self.deferred) else {
                    __sml_unvisited = 0;
                    continue;
                };
                self.deferred_len = self.deferred_len.saturating_sub(1);
                (event, true)
            } else if let Some(event) = ::sml::Queue::pop(&mut self.pending) {
                (event, false)
            } else {
                break;
            };
            let __sml_before = #state_key;
            let __sml_result = (#dispatch).map(|_| ());
            match __sml_result {
                Ok(()) => {}
                Err(#error::InvalidEvent | #error::TransitionsFailed) if __sml_deferred => {}
                Err(error) => {
                    #rotate
                    return Err(error);
                }
            }
            if __sml_before != #state_key {
                #rotate
                __sml_unvisited = self.deferred_len;
            }
        }
        Ok(#return_state)
    }
}
