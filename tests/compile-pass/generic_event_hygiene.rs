use sml::sml;

pub struct StaticMessage<'a>(&'a str);

sml! {
    StaticLifetime {
        *Idle + event<StaticMessage<'static>> = X,
    }
}

struct StaticContext;
impl StaticLifetimeStateMachineContext for StaticContext {}

pub struct CollisionEvent<A, B, C>(A, B, C);

sml! {
    Collision<EventInput, __SmlContext, __SmlEventInput> {
        *Idle + event<CollisionEvent<EventInput, __SmlContext, __SmlEventInput>> / accept,
    }
}

struct CollisionContext;

impl CollisionStateMachineContext for CollisionContext {
    fn accept<EventInput, __SmlContext, __SmlEventInput>(
        &mut self,
        _event: &CollisionEvent<EventInput, __SmlContext, __SmlEventInput>,
    ) -> Result<(), ()> {
        Ok(())
    }
}

fn main() {
    StaticLifetimeStateMachine::new(StaticContext)
        .process_event(StaticMessage("static"))
        .unwrap();
    CollisionStateMachine::new(CollisionContext)
        .process_event(CollisionEvent(1_u8, 2_u16, 3_u32))
        .unwrap();
}
