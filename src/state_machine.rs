//! The uniform machine interface and machine-owned action outputs.

use core::fmt::Debug;
use core::hash::Hash;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use crate::StateName;

/// Uniform interface of a generated machine with a single active leaf state.
///
/// It is implemented by every synchronous flat or composite table (nested
/// composite trees included) without a temporary call-scoped context.
///
/// Orthogonal machines (and composite trees with an orthogonal node) have more
/// than one active leaf and do not implement this trait; neither do machines
/// with async callbacks or a `temporary_context`.
///
/// The trait lets an owner (an actor shell, a journal, a replay harness) drive
/// any generated machine without knowing its shape: process an event, read the
/// active leaf state as a small `Copy` label, reach the context, and collect
/// the outputs the actions emitted.
pub trait StateMachine {
    /// The machine's name: the table name, or the `name: "..."` option.
    ///
    /// The value is a compile-time constant, so it is suitable as a
    /// bounded-cardinality telemetry or journal key.
    const NAME: &'static str;

    /// The generated `Events` enum.
    type Events;

    /// The machine's context type.
    type Context;

    /// A fieldless label for the active leaf state.
    ///
    /// For a flat table it names the current state; for a composite it names
    /// the deepest active state. A leaf state name that occurs in more than
    /// one table of a composite is qualified by its table name (for example
    /// `JoinX`). State payloads are not part of the label.
    type Label: Copy + Debug + Eq + Hash + StateName;

    /// The type actions emit through the machine's outbox: the `outputs: T`
    /// option, or `()` for a table without one.
    type Output;

    /// Enters the initial state and runs anonymous completions, like the
    /// generated inherent `initialize`; reports whether it succeeded.
    fn initialize(&mut self) -> bool;

    /// Processes one event to run-to-completion and reports whether it was
    /// accepted.
    ///
    /// Outputs that actions emitted before a failing guard or action stay in
    /// the outbox; on `false` the owner decides whether to execute or discard
    /// what [`drain_outputs`](Self::drain_outputs) yields.
    fn process(&mut self, event: Self::Events) -> bool;

    /// The label of the active leaf state.
    fn label(&self) -> Self::Label;

    /// Borrows the context.
    fn context(&self) -> &Self::Context;

    /// Mutably borrows the context.
    fn context_mut(&mut self) -> &mut Self::Context;

    /// Removes and yields every output the actions emitted since the last
    /// drain, in emit order.
    fn drain_outputs(&mut self) -> Drain<'_, Self::Output>;

    /// Returns true once an action called `Outbox::stop`. The request is
    /// sticky for the machine's lifetime: draining outputs does not clear it.
    fn stop_requested(&self) -> bool;
}

/// Iterator over drained machine outputs, returned by
/// [`StateMachine::drain_outputs`].
pub struct Drain<'a, O> {
    #[cfg(feature = "alloc")]
    inner: Option<alloc::vec::Drain<'a, O>>,
    // Covariant in `O` and `Send`/`Sync` exactly when `O` is, as
    // `alloc::vec::Drain` is, so feature unification never changes them.
    #[cfg(not(feature = "alloc"))]
    inner: core::marker::PhantomData<(&'a (), O)>,
}

impl<O> Drain<'_, O> {
    /// An iterator that yields nothing, for a machine without outputs.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            #[cfg(feature = "alloc")]
            inner: None,
            #[cfg(not(feature = "alloc"))]
            inner: core::marker::PhantomData,
        }
    }
}

impl<O> Iterator for Drain<'_, O> {
    type Item = O;

    #[inline]
    fn next(&mut self) -> Option<O> {
        #[cfg(feature = "alloc")]
        {
            self.inner.as_mut().and_then(Iterator::next)
        }
        #[cfg(not(feature = "alloc"))]
        {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        #[cfg(feature = "alloc")]
        {
            self.inner
                .as_ref()
                .map_or((0, Some(0)), Iterator::size_hint)
        }
        #[cfg(not(feature = "alloc"))]
        {
            (0, Some(0))
        }
    }
}

impl<O> Debug for Drain<'_, O> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Drain")
            .field("remaining", &self.size_hint().0)
            .finish()
    }
}

/// The machine-owned outbox of a table declared with `outputs: T`.
///
/// Every action of such a table receives `&mut Outbox<T>` as its first
/// parameter after `&mut self`. Actions emit outputs instead of performing
/// I/O; the machine's owner drains them after `process_event` (or
/// [`StateMachine::process`]) returns. Guards never receive the outbox.
///
/// Requires the `alloc` feature.
#[cfg(feature = "alloc")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outbox<O> {
    outputs: Vec<O>,
    stop: bool,
}

#[cfg(feature = "alloc")]
impl<O> Default for Outbox<O> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
impl<O> Outbox<O> {
    /// An empty outbox with no stop request.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            outputs: Vec::new(),
            stop: false,
        }
    }

    /// Appends one output.
    #[inline]
    pub fn emit(&mut self, output: O) {
        self.outputs.push(output);
    }

    /// Asks the owner to stop the machine after the current event has run to
    /// completion. The request is sticky.
    #[inline]
    pub const fn stop(&mut self) {
        self.stop = true;
    }

    /// Returns true once [`stop`](Self::stop) was called.
    #[inline]
    #[must_use]
    pub const fn stop_requested(&self) -> bool {
        self.stop
    }

    /// The outputs emitted and not yet drained, in emit order.
    #[inline]
    #[must_use]
    pub fn pending(&self) -> &[O] {
        &self.outputs
    }

    /// Removes and yields every pending output in emit order.
    #[inline]
    pub fn drain(&mut self) -> Drain<'_, O> {
        Drain {
            inner: Some(self.outputs.drain(..)),
        }
    }
}
