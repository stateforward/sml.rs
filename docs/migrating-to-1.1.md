# Migrating from 1.0 to 1.1

Version 1.1 aligns the generic `Machine<E>` interface with the `sml.cpp`
`co_sm` run-to-completion acceptance contract.

Update the dependency requirement:

```toml
sml = { package = "stateforward-sml", version = "1.1" }
```

## Generic machine processing

`Machine::process` and the `Machine::Error` associated type are replaced by:

- `Machine::process_event`, which returns whether the event was accepted.
- `Machine::process_event_async`, which returns an implementation-selected RTC
  completion future and resolves to the same acceptance value.

Generated synchronous flat machines use an allocation-free ready future for the
uncontended inline path. Orthogonal, composite, async-callback, and
temporary-context machines continue to use their inherent processing APIs.
Scheduler-backed adapters can provide a future that remains pending until queued
RTC processing completes.

Detailed generated errors remain available through each generated machine's
inherent `process_event` method, which continues to return `Result`.

Manual `Machine<E>` implementations define `process_event` and inherit the
allocation-free `Ready<bool>` async entry point. They may override
`process_event_async` with another concrete future type, allowing a
scheduler-backed adapter to remain pending without boxing or dynamic dispatch.
