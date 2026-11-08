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
//! Synchronous machines dispatch `process(...)` events recursively, so a step
//! may leave and re-enter its starting state through nested dispatches. Every
//! dispatch, nested or not, goes through a tracking wrapper that records a
//! state change in `deferred_changed`, so such a step still counts as a change.
//!
//! The queue is rotated in place, so the policy's queue is the only queue
//! used, and `deferred_len` counts its events, since `Queue` does not report
//! its length. During a synchronous pass the queue holds three runs:
//! `[unvisited..., new..., retained...]`. The pass pops the unvisited events
//! from the front, and an event deferred again by its own retry (a dispatch at
//! depth one) is retained by pushing it to the back. An event deferred by a
//! nested `process(...)` dispatch is new: it is inserted after the other new
//! events, ahead of the retained ones. Rotating the unvisited and new events
//! behind the retained ones restores queue order, `[retained..., unvisited...,
//! new...]`: the order the events had before the pass, with the new events
//! behind them. That happens on a state change, on a propagated error, and
//! when the pass ends.

use proc_macro2::TokenStream;
use quote::quote;

/// The fields tracking the defer queue, declared after the `deferred` queue.
/// Asynchronous machines dispatch `process(...)` events through their pending
/// queue rather than recursively, so they track only the length.
pub fn fields(sync: bool) -> TokenStream {
    if sync {
        quote! {
            deferred_len: usize,
            deferred_draining: bool,
            deferred_depth: usize,
            deferred_unvisited: usize,
            deferred_new: usize,
            deferred_changed: bool,
        }
    } else {
        quote! { deferred_len: usize, }
    }
}

/// Initializers for [`fields`].
pub fn field_inits(sync: bool) -> TokenStream {
    if sync {
        quote! {
            deferred_len: 0,
            deferred_draining: false,
            deferred_depth: 0,
            deferred_unvisited: 0,
            deferred_new: 0,
            deferred_changed: false,
        }
    } else {
        quote! { deferred_len: 0, }
    }
}

/// Moves `count` events from the front of the queue to the back. Each push
/// follows a pop, so a queue that held the events a moment ago has room.
fn rotate(count: &TokenStream, error: &TokenStream) -> TokenStream {
    quote! {
        for _ in 0..#count {
            if let Some(event) = ::sml::Queue::pop(&mut self.deferred) {
                if ::sml::Queue::defer(&mut self.deferred, event).is_err() {
                    self.deferred_len = self.deferred_len.saturating_sub(1);
                    return Err(#error::QueueFull);
                }
            }
        }
    }
}

/// Defers `event`, counting it; returns `QueueFull` when the queue is full.
/// In a synchronous pass, an event deferred by a nested dispatch is inserted
/// ahead of the retained events.
pub fn defer(event: &TokenStream, error: &TokenStream, sync: bool) -> TokenStream {
    if !sync {
        return quote! {
            ::sml::Queue::defer(&mut self.deferred, #event)
                .map_err(|_| #error::QueueFull)?;
            self.deferred_len = self.deferred_len.saturating_add(1);
        };
    }
    let to_retained = rotate(&quote! { __sml_ahead }, error);
    let past_retained = rotate(&quote! { __sml_retained }, error);
    quote! {
        if self.deferred_draining && self.deferred_depth > 1 {
            let __sml_ahead = self.deferred_unvisited.saturating_add(self.deferred_new);
            let __sml_retained = self.deferred_len.saturating_sub(__sml_ahead);
            #to_retained
            if ::sml::Queue::defer(&mut self.deferred, #event).is_err() {
                #past_retained
                return Err(#error::QueueFull);
            }
            #past_retained
            self.deferred_new = self.deferred_new.saturating_add(1);
        } else {
            ::sml::Queue::defer(&mut self.deferred, #event)
                .map_err(|_| #error::QueueFull)?;
        }
        self.deferred_len = self.deferred_len.saturating_add(1);
    }
}

/// The body of the synchronous tracking wrapper, `__sml_dispatch_event`:
/// dispatches through `inner` (an expression evaluating to a `Result`) one
/// level deeper, and records whether the state changed. `state_key` compares
/// equal exactly when the machine is in the same states.
pub fn tracked_dispatch(state_key: &TokenStream, inner: &TokenStream) -> TokenStream {
    quote! {
        let __sml_before = #state_key;
        self.deferred_depth = self.deferred_depth.saturating_add(1);
        let __sml_result = (#inner).map(|_| ());
        self.deferred_depth = self.deferred_depth.saturating_sub(1);
        if __sml_before != #state_key {
            self.deferred_changed = true;
        }
        __sml_result
    }
}

/// Restores queue order at the end of a synchronous pass, or before it starts
/// over or returns an error.
fn restore(error: &TokenStream) -> TokenStream {
    let rotate = rotate(&quote! { __sml_ahead }, error);
    quote! {
        let __sml_ahead = self.deferred_unvisited.saturating_add(self.deferred_new);
        self.deferred_unvisited = 0;
        self.deferred_new = 0;
        self.deferred_draining = false;
        #rotate
    }
}

/// The synchronous top-level step: runs `dispatch` (through the tracking
/// wrapper) and, when the step changed the state, drains the queue. `retry`
/// dispatches `deferred_event` through the tracking wrapper. Both evaluate to
/// a `Result<(), _>`.
pub fn sync_step(dispatch: &TokenStream, retry: &TokenStream, error: &TokenStream) -> TokenStream {
    let restore = restore(error);
    quote! {
        // A top-level step starts at depth zero, outside any pass. A callback
        // panic that a caller caught (as `FifoScheduler` does) can leave the
        // tracking state of the interrupted step behind: restore queue order
        // for an interrupted pass and reset the depth.
        if self.deferred_draining {
            #restore
        }
        self.deferred_depth = 0;
        self.deferred_changed = false;
        #dispatch?;
        if self.deferred_changed && self.deferred_len > 0 {
            self.deferred_draining = true;
            self.deferred_unvisited = self.deferred_len;
            self.deferred_new = 0;
            while self.deferred_unvisited > 0 {
                self.deferred_unvisited = self.deferred_unvisited.saturating_sub(1);
                let Some(deferred_event) = ::sml::Queue::pop(&mut self.deferred) else {
                    break;
                };
                self.deferred_len = self.deferred_len.saturating_sub(1);
                self.deferred_changed = false;
                match #retry {
                    Ok(()) | Err(#error::InvalidEvent | #error::TransitionsFailed) => {}
                    Err(error) => {
                        #restore
                        return Err(error);
                    }
                }
                if self.deferred_changed {
                    #restore
                    self.deferred_draining = true;
                    self.deferred_unvisited = self.deferred_len;
                }
            }
            #restore
        }
    }
}

/// The asynchronous event loop for machines with a defer queue: events queued
/// with `process` run first in, and each one that changes the state is
/// followed by a drain pass before the next queued event, as in sml.cpp.
/// `dispatch` dispatches `event` without draining. Nested `process(...)`
/// events go to the pending queue, so a pass never defers new events and the
/// defer queue holds only `[unvisited..., retained...]`.
pub fn async_loop(
    state_key: &TokenStream,
    dispatch: &TokenStream,
    error: &TokenStream,
    return_state: &TokenStream,
) -> TokenStream {
    let rotate = rotate(&quote! { __sml_unvisited }, error);
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
