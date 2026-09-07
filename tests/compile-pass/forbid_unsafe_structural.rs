#![deny(warnings)]
#![forbid(unsafe_code)]

use sml::sml;

pub struct EnterChild;
pub struct ChildDone;
pub struct LeftDone;
pub struct RightDone;

sml! {
    SafeCompositeChild {
        *ChildIdle + event<ChildDone> = X,
    }

    SafeCompositeParent {
        *Outside + event<EnterChild> = state<SafeCompositeChild>,
        state<SafeCompositeChild> + event<ChildDone> = X,
    }
}

sml! {
    SafeOrthogonal {
        *Left + event<LeftDone> = X,
        *Right + event<RightDone> = X,
    }
}

struct CompositeContext;
impl SafeCompositeParentStateMachineContext for CompositeContext {}

struct OrthogonalContext;
impl SafeOrthogonalStateMachineContext for OrthogonalContext {}

fn main() {
    let mut composite = SafeCompositeParentStateMachine::new(CompositeContext);
    composite.process_event(EnterChild).unwrap();

    let mut orthogonal = SafeOrthogonalStateMachine::new(OrthogonalContext);
    orthogonal.process_event(LeftDone).unwrap();
}
