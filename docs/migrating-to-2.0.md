# Migrating to 2.0

Version 2.0 adds a scheduler policy slot, so a generated machine can be a
shared handle that many tasks call. A shared machine cannot lend out
references to its state or context, so 2.0 also narrows every generated
machine's public surface to what `sml.cpp`'s `sm` exposes: construction,
event processing, state identity queries, visitors, and termination. Getters
that hand out references move behind `TestingPolicy`.

## Event processing results

The inherent `process_event` returns `Result<bool, Error>` instead of
`Result<&States, Error>`:

| 1.x | 2.0 |
|---|---|
| `Ok(&state)` | `Ok(true)` |
| `Err(Error::InvalidEvent)`, `Err(Error::TransitionsFailed)` | `Ok(false)` |
| `Err(GuardFailed(e))`, `Err(ActionFailed(e))`, `Err(QueueFull)` | unchanged |

`initialize()` returns `Result<(), Error>`. `Machine::process_event` still
returns `bool`, so code written against the trait is unaffected.

## State, context, and policy access

`state()`, `states()`, `state(region)`, typed child state getters,
`context()`, `context_mut()`, `logger()`, `logger_mut()`, `observer()`, and
`observer_mut()` now require a policy whose `testing` slot is
`TestingPolicy`, like `set_current_states` already did:

```rust,ignore
sml::sml_policies!(Testing { testing: sml::TestingPolicy });

let mut machine = MyStateMachine::new_with_policy(context, Testing::default());
assert_eq!(machine.process_event(Start), Ok(true));
assert!(matches!(machine.state(), MyStates::Running));
```

Production code observes a machine through `is(&States::Running)`,
`is_terminated()`, `visit_current_state(...)`, and `visit_current_events`. To
read what an action produced, keep the output in the context and take it in a
step hook (below), or share it with the owner through a handle the context
holds.

## Policy bundles

`PolicyBundle` and `PolicyBundleParts` gained trailing, defaulted scheduler
and step-hook parameters, and `sml_policies!` accepts `scheduler:` and
`step_hook:`. `Policies::into_parts` returns `(parts, thread_safe, scheduler)`.
A downstream bundle spelled with `sml_policies!` or with
`PolicyBundle<L, O, D, ...>` needs no change.

Code generic over a machine's policy that processes events on the owned
machine adds the default scheduler:

```rust,ignore
// 1.x
impl<P: sml::Policies> Runner<P> { /* machine.process_event(..) */ }
// 2.0
impl<P: sml::Policies<Scheduler = ()>> Runner<P> { /* machine.process_event(..) */ }
```

## Sharing a machine across tasks

Pick a scheduler:

```rust,ignore
sml::sml_policies!(Shared {
    scheduler: sml::TokioScheduler,
    step_hook: Journal,
});

let machine = MyStateMachine::new_with_policy(context, Shared::default());
let handle = machine.clone();
tokio::spawn(async move { handle.process_event_async(Start).await });
```

The event is queued when `process_event_async` is called; awaiting the
completion yields its acceptance once it has run to completion, and dropping
the completion does not cancel it. A queued machine runs its initial entry
when constructed. `FifoScheduler<N>` and `InlineScheduler` need the `std`
feature; `TokioScheduler` needs `tokio`.

The step hook replaces an owner loop that stepped the machine and then read
its context:

```rust,ignore
impl<L> sml::StepHook<MyContext, L, MyEvents> for Journal {
    fn before_step(&mut self, _context: &mut MyContext, _logger: &mut L, event: &MyEvents) {
        // record the event before it runs
    }

    fn after_step(&mut self, context: &mut MyContext, _logger: &mut L, step: sml::Step) {
        // journal `step` and drain `context`'s outputs
    }
}
```
