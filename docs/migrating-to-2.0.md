# Migrating from 1.x to 2.0

Version 2.0 aligns the generated machine's inspection API with `sml.cpp`,
renames the policy types, and adds scheduler policies. Event processing is
unchanged: the inherent `process_event` and `initialize` keep their return
types, and `Machine<E>` is untouched.

Update the dependency requirement:

```toml
sml = { package = "stateforward-sml", version = "2.0" }
```

## Policy names

| 1.x | 2.0 |
|---|---|
| `PolicyBundle<...>` (struct) | `Policies<...>` |
| `Policies` (sealed trait) | `Policy` |
| `PolicyBundleParts` | `sml::__private::PoliciesParts` |
| `PolicyParts` | `sml::__private::PolicyParts` |

`sml_policies!` keeps its name and slots, and gains a `scheduler:` slot.
`Policies::new`, `with_dispatch`, `into_parts`, and `Policy::into_parts` keep
their signatures. `Policy` gains a `Scheduler` associated type (`()` for
`NoPolicy` and a default `Policies`) and an `into_scheduled_parts` split. The
owned machine's API (`process_event`, `initialize`, `Machine<E>`, and the
testing setters) is bounded on `P: Policy<Scheduler = ()>`, so code that is
generic over the policy of an owned machine names that bound:

```rust,ignore
// 1.x
fn drive<P: sml::Policies>(sm: &mut PlayerStateMachine<Context, P>) { /* ... */ }
// 2.0
fn drive<P: sml::Policy<Scheduler = ()>>(sm: &mut PlayerStateMachine<Context, P>) { /* ... */ }
```

`is`, `visit_current_states`, and `visit_current_events` need only
`P: Policy`, since they also work on a scheduled handle.

`sml::__private` is macro support for generated code. It is not public API and
is exempt from semantic versioning; code that named `PolicyParts` or
`PolicyBundleParts` directly should stop doing so.

## State queries: `is` and `visit_current_states`

The getters are gone. `is(...)` is the only state query and
`visit_current_states(visitor)` is the only visitor, as in `sml.cpp`.

| 1.x | 2.0 |
|---|---|
| `matches!(sm.state(), States::Idle)` | `sm.is(&States::Idle)` |
| `match sm.state() { States::Ready(data) => ... }` | `sm.visit_current_states(\|state\| if let States::Ready(data) = state { ... })` |
| `sm.visit_current_state(\|s\| ...)` | `sm.visit_current_states(\|s\| ...)` (called once per active state; returns `()`) |
| `sm.is_terminated()` | `sm.is(&States::X)` |
| orthogonal `sm.states()`, `sm.state(region)`, `sm.is_region(region, &s)` | `sm.is(&[a, b])`, or `visit_current_states`, which visits the regions in order |
| composite `sm.is_child(&ChildStates::Idle)` | `sm.is(&ChildStates::Idle)` |
| composite `sm.child_is_active()`, `sm.<child>_is_active()` | `sm.is(&ParentStates::Child)` (for a grandchild, also `sm.is(&MiddleStates::Child)`) |
| composite `sm.child_state()`, `sm.<child>_state()`, `sm.visit_<child>_state(...)` | `sm.is(&ChildStates::...)` |
| composite `sm.<child>_states()`, `sm.<child>_state(region)` | `sm.is(&[ChildStates::A, ChildStates::B])` |
| `sml::Terminated::is_terminated(&sm)` | `sm.is(&States::X)` |

`is` accepts any type implementing `IsState` for the machine: the root states
enum (an array of them for an orthogonal root), and each child machine's
states enum (an array for a child with regions). It compares variants and
ignores payloads, so `sm.is(&ChildStates::Loaded(Loaded(0)))` matches any
`Loaded` payload; read payloads in `visit_current_states` or in a callback.
A child query compares the child's stored state, as `is_child` did, so it also
answers while the child is inactive; add `sm.is(&ParentStates::Child)` when the
child must be active.
A child machine may appear only once in a tree, which the generator already
enforces, so each child states enum names one place.

Generated machines no longer implement `Terminated`. Code that passes a
generated machine to `utility::Hierarchical` implements it in its own crate:

```rust,ignore
impl sml::Terminated for ChildStateMachine<Context> {
    fn is_terminated(&self) -> bool {
        self.is(&ChildStates::X)
    }
}
```

`set_current_states`, `set_child_state`, and the per-child
`set_<child>_state` setters remain behind `TestingPolicy`, and
`visit_current_events` remains.

## Contexts, loggers, and observers belong to the caller

`context()`, `context_mut()`, `logger()`, `logger_mut()`, `observer()`, and
`observer_mut()` are removed. As in `sml.cpp`, the caller owns the
dependencies it gives the machine:

- Implement the context trait for a reference and keep the value:
  `impl MyStateMachineContext for &Context`, then
  `let context = Context::default(); let mut sm = MyStateMachine::new(&context);`.
  Fields the caller reads between events use `Cell`/`RefCell` (or `Mutex` and
  atomics when async callbacks must stay `Send`).
- When every read follows the last event, `impl ... for &mut Context` and
  `MyStateMachine::new(&mut context)` keep `&mut self` callback bodies as they
  were; the borrow ends with the machine's last use.
- A shared handle such as `Rc<RefCell<...>>` or `Arc<Mutex<...>>` inside an
  owned context works as well.
- Values needed for a single call can still be passed through the table's
  per-call temporary context.
- A logger or observer records into a sink the caller also holds, for example
  `struct Log(Rc<RefCell<Vec<&'static str>>>)` passed as `log.clone()`.

## Scheduler policies

New in 2.0: `sml_policies!(Shared { scheduler: FifoScheduler<1024> })` makes the
generated machine a cloneable handle with `process_event_async(&self, ...)`,
`try_process_event(&self, ...)`, `is`, `visit_current_states`, and
`visit_current_events`. The default `()` scheduler keeps the owned machine
described above. See the README's Schedulers section.
