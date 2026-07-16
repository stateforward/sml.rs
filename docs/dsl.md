# `sml!` transition-table DSL

The primary macro deliberately mirrors an `sml.cpp` transition table:

```rust
use sml::sml;

pub struct E1;
pub struct E2 { pub value: u32 }

sml! {
    Example {
        *"idle"_s + event<E1> / start = "running"_s,
         "done"_s <= "running"_s + event<E2> [valid] / (capture, audit),
         "done"_s / finish = X,
    }
}
```

The machine name replaces the C++ transition-table type. It prefixes generated
items: `ExampleStateMachine`, `ExampleStateMachineContext`, `ExampleStates`,
`ExampleEvents`, and `ExampleError`.

## Mechanical C++ to Rust mapping

| `sml.cpp` table expression | `sml.rs` macro expression |
|---|---|
| `*"idle"_s + event<E> = "run"_s` | identical |
| `"run"_s <= *"idle"_s + event<E>` | identical |
| `"idle"_s + event<E> [guard] / action = X` | identical |
| `/ (first, second)` | identical for named Rust callbacks |
| `"idle"_s + event<E> / action` | identical internal transition |
| `"idle"_s + event<E> = "idle"_s` | identical external self-transition |
| `*"initial"_s / action = "ready"_s` | identical anonymous transition |
| `"name"_e` | identical named-event trigger |
| `state + on_entry<_> / action` | identical |
| `state + on_exit<_> / action` | identical |
| `state + sml::on_entry<_> / action` | accepted qualified spelling |
| `... = sml::X` | accepted qualified terminal spelling |
| `state + unexpected_event<E> / action` | identical |
| `state + unexpected_event<_> / action` | identical wildcard form |
| origin-aware completion | `state + completion<E> / action = target` |
| `/ defer` | identical bounded deferred-event action |
| `/ process(E {})` | identical processed-event action |

Rust callback bodies are implemented on the generated context trait rather
than written as C++ lambdas inside the table. External `event<E>` values are
ordinary Rust types and can be passed directly to `process_event`.

## Grammar

```text
sml! {
    MachineName [<event generic parameters>] [where bounds] {
        transition (, transition)* [,]
    }
}

transition := [*] state + trigger [guard] [/ action] [= state]
            | state <= [*] state + trigger [guard] [/ action]
            | [*] state [/ action] = state

state   := "state name"_s | RustIdentifier | X
trigger := event<RustType>
         | "named event"_e
         | on_entry<_> | on_exit<_>
         | unexpected_event<RustType> | unexpected_event<_>
         | completion<RustType> | completion<_>
         | exception<RustError> | exception<_>
guard   := [name] | [!name] | [(a && !b) || c]
action  := name | (first, second, ...)
         | (..., eval [guard] / action, ...)
```

A leading `*` selects the initial state. Omitting the target makes a true
internal transition: no exit or entry callbacks run. An explicit target equal
to the source is an external self-transition, so exit and entry callbacks do
run. `X` is the terminal state.

## Generic event types

Flat tables declare Rust lifetime, type, and const parameters immediately after
the machine name. Bounds may be inline or in an ordinary `where` clause:

```rust
use core::fmt::Debug;
use sml::sml;

pub struct Owned<T, const N: usize>(pub T, pub [u8; N]);
pub struct Operation<T, const N: usize> {
    pub input: [T; N],
    pub result: Option<[T; N]>,
}

sml! {
    Typed<'operation, T, const N: usize>
    where
        T: Clone + Debug + 'operation,
        [T; N]: AsRef<[T]>,
    {
        *"idle"_s + event<Owned<T, N>> / consume,
         "idle"_s + event<&'operation mut Operation<T, N>> / complete,
    }
}
```

The trigger accepts a named type path (`event<Message<T>>`) or a shared or
mutable reference to one (`event<&'a Message<T>>` and
`event<&'a mut Operation<T, N>>`). Qualified paths, concrete lifetimes such as
`'static`, and const arguments are
accepted. `unexpected_event<...>` uses the same typed form. Tuple, slice,
array, pointer, function, trait-object, and other anonymous trigger types are
rejected; put such data in a named event type.

Generated APIs preserve static dispatch:

- `TypedEvents<'operation, T, N>` stores the concrete event or reference.
- Each affected context callback is a generic trait method with the declared
  bounds and `where` clause.
- The inherent `process_event` method and synchronous `Machine<Events<...>>`
  implementation are monomorphized for each concrete argument set.
- Event-only lifetimes remain on the dispatch call. They are not added to
  `TypedStateMachine`, cannot be stored in machine state, and mutable borrows
  must end before dispatch returns.
- The generated machine uses no type erasure, `Any`, trait object, allocation,
  downcast, unsafe code, or dispatch-local storage.

Every directly dispatched external event must mention every declared type and
const parameter. A typed internal event that uses any declared generic must do
the same. The generated API owns one `Events<...>` family, so an event that
omitted one of those arguments would leave dispatch or its generic callback
unable to infer the family. The macro rejects that declaration directly instead
of adding hidden routing. Lifetimes may remain event-specific: they are inferred
at the call and cannot escape dispatch. A single machine value may process, for
example, both `Owned<u32, 4>` and `Owned<String, 8>` when its generic callback
implementation satisfies the declared bounds. Parameter defaults are rejected
because Rust does not permit defaults on the generated generic callback and
dispatch methods.

Generic event declarations currently apply to flat tables. Orthogonal and
composite generators have fixed event enums and reject them with a targeted
diagnostic. Generic events also reject `/ defer` and `/ process(...)`: those
operations store events in the machine's fixed queue, which cannot hold a
dispatch-scoped family of monomorphizations. Concrete event types such as
`event<Message<u32>>` remain available in every machine shape and may use the
normal queue features.

Multiple leading `*` states define orthogonal regions exactly as in `sml.cpp`:

```rust
use sml::sml;

pub struct E1;
pub struct E2;
pub struct E3;

sml! {
    Regions {
        *"idle"_s  + event<E1> = "s1"_s,
         "s1"_s   + event<E2> = X,
        *"idle2"_s + event<E2> = "s2"_s,
         "s2"_s   + event<E3> = X,
    }
}
```

Each event is borrowed and broadcast to every active region, so `E2` can move
both regions during one call. Orthogonal machines expose `states()`,
`state(region)`, `is(&[...])`, and `is_region(region, &state)`, and are
terminated only when every region is `X`. Specific and wildcard unexpected
handlers are resolved independently per region. Anonymous rows stabilize every
region during `initialize()` and again after each handled broadcast. Prefixing
any orthogonal guard or action with `async` generates an async broadcast and
stabilization path and awaits callbacks in every region.

State strings are converted to PascalCase generated variants; for example,
`"fin wait 1"_s` becomes `States::FinWait1`. Named events are converted the
same way. Generated event enum variants remain useful for named events, while
typed `event<E>` transitions generate `From<E>` and support direct dispatch.

In a flat table, `state<T>` creates a `States::T(T)` payload variant. Initial
typed states and actionless typed targets use `T::default()`. When a transition
action constructs the target, its final action returns `T`; this is the
ownership-safe Rust counterpart to sml.cpp injecting a mutable destination
state object. Use `new_with_state_data(context, value)` to override an inferred
initial value.

## Context callbacks

Implement the generated `MachineStateMachineContext` trait. Guards borrow the
event and return `Result<bool, Error>`; actions borrow external events and
return `Result<(), Error>`. If a transition constructs a data-bearing output
state, the final action returns that state's data. Earlier actions in a
sequence return `()`.

```rust,ignore
impl ExampleStateMachineContext for Context {
    fn valid(&self, event: &E2) -> Result<bool, ()> {
        Ok(event.value != 0)
    }

    fn capture(&mut self, event: &E2) -> Result<(), ()> {
        self.value = event.value;
        Ok(())
    }

    fn audit(&mut self, _event: &E2) -> Result<(), ()> {
        Ok(())
    }
}
```

Call `initialize()` once after construction to run initial entry behavior and
anonymous-transition stabilization. Normal event processing automatically
stabilizes subsequent anonymous and completion transitions.

`state()` borrows the generated state enum, while `is(&States::Idle)` performs
the payload-insensitive state identity check corresponding to `sm.is("idle"_s)`
in C++.

Prefix a callback with `async` in a guard or action position to generate an
async machine, for example `[async ready] / async send`. Rust futures provide
the coroutine behavior directly.

Generated synchronous flat machines without a temporary call-scoped context
implement `Machine<E>`. Generic callers can use `process_event` directly or
await `process_event_async` for the same run-to-completion operation. Both
report event acceptance as `bool`, matching the `sml.cpp` `co_sm` contract; the
generated machine's inherent `process_event` retains the detailed Rust
`Result`. The async trait entry point is an allocation-free future using the
uncontended inline RTC fast path. This is independent of async guards and
actions. Generic callers pass the generated event enum; inherent methods also
accept external event types that convert into that enum. Orthogonal, composite,
async-callback, and temporary-context machines retain their shape-specific
inherent processing APIs. A scheduler-backed `co_sm` adapter for pending RTC
completion remains separate work.

Action sequences accept `eval [guard] / action` in any position. The nested
action runs only when its guard expression passes, while surrounding actions
retain their original order. Both the eval guard and action may be `async`.

For flat machines, `state + exception<_> / recover = target` handles a guard
or action that returned `Err`. A typed `exception<MyError>` additionally
infers `MyError` as the machine callback-error type and injects `&MyError` into
the handler action. The original `GuardFailed`/`ActionFailed` is intercepted
and the exception transition becomes the event result. Sync and async
callbacks are supported. A machine currently uses one concrete Rust error
type; use an enum when several error variants need typed routing.

The reserved `/ defer` action stores the current event in a generated,
allocation-free queue and retries it after the next state change. The reserved
`/ process(Event {})` action dispatches the supplied event after the current
transition has installed its target state. Queue actions work in sync/async
flat, orthogonal, and composite tables through allocation-free iterative
dispatch. Deferring an owned structural payload requires that payload to
implement `Clone`.

## Composite machines

Place the child and parent tables adjacently in one `sml!` invocation and use
the same `state<Sub>` spelling as C++:

```rust
use sml::sml;
pub struct Enter;
pub struct ChildEvent;
pub struct Leave;

sml! {
    Sub {
        *"idle"_s + event<ChildEvent> = X,
    }

    Parent {
        *"idle"_s + event<Enter> = state<Sub>,
         state<Sub> + event<Leave> = X,
    }
}
```

The generated parent owns both state values and one unified context. While
`state<Sub>` is active, events route to the child first and bubble to the
parent only when the child has no matching transition. Leaving and re-entering
the child resets it to its initial state by default. Mark the child's initial
state as `*"idle"_s(H)` to retain shallow history across re-entry, exactly as
in `sml.cpp`. `child_state()`, `is_child(...)`, and
`child_is_active()` expose typed composite queries. Parent and child lifecycle
rows and unexpected handlers participate in the same child-first ordering.
Sync or async guards/actions are supported. A parent anonymous transition from
`state<Sub>` becomes eligible only after the child reaches `X`, matching
composite completion semantics.

The public macro surface is `sml!`.
