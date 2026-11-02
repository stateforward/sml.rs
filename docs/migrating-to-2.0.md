# Migrating to 2.0

Version 2.0 adds a scheduler policy slot, so a generated machine can be a
shared handle that many tasks call, and it makes every machine driven rather
than inspected. A machine's state and context can change under any reader, so
no generated machine exposes them: its public API is construction,
`initialize()`, `process_event`, and `Machine<E>`. An owner asks with an event
and learns what the step did from the outputs the step emitted, which the
generated step hands to the policy's step hook. The inspection API remains for
tests behind `TestingPolicy`.

## Event processing results

The inherent `process_event` returns `Result<bool, Error>` instead of
`Result<&States, Error>`:

| 1.x | 2.0 |
|---|---|
| `Ok(&state)` | `Ok(true)` |
| `Err(Error::InvalidEvent)`, `Err(Error::TransitionsFailed)` | `Ok(false)` |
| `Err(GuardFailed(e))`, `Err(ActionFailed(e))`, `Err(QueueFull)` | unchanged |

`initialize()` returns `Result<(), Error>`. `Machine::process_event` still
returns `bool`. A test that wrote `machine.process_event(e).unwrap();` to
assert the event was handled now needs `assert_eq!(machine.process_event(e),
Ok(true));`, because `Ok(false)` unwraps.

## Inspection is testing-only

These now require a policy whose `testing` slot is `TestingPolicy`:
`state()`, `states()`, `state(region)`, `is(...)`, `is_region(...)`,
`is_child(...)`, `child_is_active()`, typed child state, identity, active, and
visitor queries, `visit_current_state(s)`, `visit_current_events`,
`is_terminated()` and the generated `Terminated` impl, `context()`,
`context_mut()`, `logger()`, `logger_mut()`, `observer()`, `observer_mut()`,
and, as before, the setters.

```rust,ignore
sml::sml_policies!(Testing { testing: sml::TestingPolicy });

let mut machine = MyStateMachine::new_with_policy(context, Testing::default());
assert_eq!(machine.process_event(Start), Ok(true));
assert!(machine.is(&MyStates::Running));
```

## Outputs instead of reading the context

Production code that read the context after a step, or matched on the state,
moves to outputs: actions push what they produce into a buffer in the context,
the context implements `Emit<Output>`, and a step hook receives each output
after run-to-completion.

```rust,ignore
impl sml::Emit<Command> for MyContext {
    fn drain_outputs(&mut self, sink: impl FnMut(Command)) {
        self.commands.drain(..).for_each(sink);
    }
}

#[derive(Default)]
struct Journal { pending: Vec<Command> }

impl<L> sml::StepHook<L, MyEvents> for Journal {
    type Output = Command;

    fn before_step(&mut self, _logger: &mut L, event: &MyEvents) {
        // journal the input before it runs
    }

    fn output(&mut self, _logger: &mut L, command: Command) {
        self.pending.push(command);
    }

    fn after_step(&mut self, _logger: &mut L, step: sml::Step) {
        // journal `step` and hand `pending` to whoever executes it
    }
}

sml::sml_policies!(Production { step_hook: Journal });
```

Termination is learned the same way: once a machine reaches `X`, every event
returns `Ok(false)` (`false` through `Machine` or a scheduler).

## Policy bundles

`PolicyBundle` and `PolicyBundleParts` gained trailing, defaulted scheduler
and step-hook parameters, and `sml_policies!` accepts `scheduler:` and
`step_hook:`. `Policies::into_parts` returns `(parts, thread_safe, scheduler)`.
A bundle spelled with `sml_policies!` or `PolicyBundle<L, O, D, ...>` needs no
change.

Code generic over a machine's policy that processes events on the owned
machine adds the default scheduler and lets the compiler carry the hook
bounds, or names concrete policies:

```rust,ignore
// 1.x
impl<P: sml::Policies> Runner<P> { /* machine.process_event(..) */ }
// 2.0
impl<P: sml::Policies<Scheduler = ()>> Runner<P>
where
    MyStateMachine<MyContext, P>: sml::Machine<MyEvents>,
{ /* sml::Machine::process_event(&mut machine, ..) */ }
```

## Sharing a machine across tasks

```rust,ignore
sml::sml_policies!(Shared {
    scheduler: sml::FifoScheduler<1024>,
    step_hook: Journal,
});

let machine = MyStateMachine::new_with_policy(context, Shared::default());
let handle = machine.clone();
std::thread::spawn(move || smol::block_on(handle.process_event_async(Start)));
```

A scheduled machine's only operations are `process_event_async`,
`try_process_event`, and `Clone`. The event is queued when
`process_event_async` is called; the completion resolves to its acceptance
once it has run to completion, and dropping it does not cancel the event.
When a bounded queue (`FifoScheduler<N>`) is full, the event is queued only
once a poll of its completion finds room, so dropping the completion first
discards the event; `try_process_event` reports the full queue instead. A
queued machine runs its initial entry when constructed. `FifoScheduler<N>` and
`InlineScheduler` need the `std` feature. sml.rs ships no async-runtime
scheduler; the README's Schedulers section describes the traits one
implements.
