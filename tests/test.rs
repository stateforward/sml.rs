extern crate sml;

use derive_more::Display;

use sml::sml;

mod internal_macros {
    #[macro_export]
    macro_rules! assert_transition {
        ($sm:expr, $context:expr, $event:expr, $expected_state:expr, $expected_count:expr) => {{
            let state = $sm.process_event($event).unwrap().clone();
            println!("-> {:?} : {:?}", state, $context);
            assert_eq!($expected_state, state);
            assert!($sm.is(&$expected_state));
            assert_eq!($expected_count, $context.count.get());
        }};
    }
    #[macro_export]
    macro_rules! assert_transition_ok {
        ($sm:expr, $context:expr, $event:expr, $expected_action:expr, $expected_result:pat) => {{
            if let Ok(result) = $sm.process_event($event) {
                let result = result.clone();
                println!("-> {:?} : {:?}", result, $context);
                match result {
                    $expected_result => {}
                    _ => panic!(
                        "Assertion failed:\n expected: {:?},\n actual:    {:?}",
                        stringify!($expected_result),
                        result
                    ),
                }
                assert_eq!($expected_action, $context.action.get());
            } else {
                panic!("assert_transition_ok failed")
            }
        }};
    }
}

#[test]
fn compile_fail_tests() {
    let t = trybuild::TestCases::new();
    t.pass("tests/compile-pass/*.rs");
    t.compile_fail("tests/compile-fail/*.rs");
}
#[test]
fn wildcard_after_input_state() {
    sml! {
        _ {
            *State1 + Event1 = State2,
            _ + Event1 = Fault,
        }
    }

    struct Context;
    impl StateMachineContext for Context {}

    let mut sm = StateMachine::new(Context);

    sm.process_event(Events::Event1).unwrap();
    assert!(sm.is(&States::State2));

    sm.process_event(Events::Event1).unwrap();
    assert!(sm.is(&States::Fault));
}

#[test]
fn multiple_lifetimes() {
    pub struct X;
    pub struct Y;
    pub struct Z;

    sml! {
        _ {
            *State1 + Event1(&'a X) [guard1] / action1 = State2(&'a X),
            State2(&'a X) + Event2(&'b Y) [guard2] / action2 = State3((&'a X, &'b Y)),
            State4 + Event(&'c Z) [guard3] / action3 = State5,
        }
    }

    #[allow(dead_code, reason = "the fixture exercises generated state variants")]
    struct Context;

    impl StateMachineContext for Context {
        fn guard1(&self, _event_data: &X) -> Result<bool, ()> {
            Ok(true)
        }

        fn guard2(&self, _state_data: &X, _event_data: &Y) -> Result<bool, ()> {
            Ok(true)
        }

        fn guard3(&self, _event_data: &Z) -> Result<bool, ()> {
            Ok(true)
        }

        fn action1<'a>(&mut self, event_data: &'a X) -> Result<&'a X, ()> {
            Ok(event_data)
        }

        fn action2<'a, 'b>(
            &mut self,
            state_data: &'a X,
            event_data: &'b Y,
        ) -> Result<(&'a X, &'b Y), ()> {
            Ok((state_data, event_data))
        }

        fn action3(&mut self, _event_data: &Z) -> Result<(), ()> {
            Ok(())
        }
    }

    #[allow(dead_code, reason = "the fixture exercises generated state variants")]
    struct WrappedStates<'a, 'b>(States<'a, 'b>);

    #[allow(dead_code, reason = "the fixture exercises generated state variants")]
    struct WrappedEvents<'a, 'b, 'c>(Events<'a, 'b, 'c>);
}

#[test]
fn derive_display_events_states() {
    sml! {
        _[
            events_attr: #[derive(Debug,Display)],
            states_attr: #[derive(Debug,Display)]
        ] {
            *Init + Event = End,
        }
    }

    struct Context;
    impl StateMachineContext for Context {}

    let mut sm = StateMachine::new(Context);
    assert!(sm.is(&States::Init));

    let event = Events::Event;
    assert_eq!(format!("{event}"), "Event");

    sm.process_event(event).unwrap();
    assert!(sm.is(&States::End));
}

#[test]
fn named_derive_display_events_states() {
    sml! {
        SM[
            events_attr: #[derive(Debug,Display)],
            states_attr: #[derive(Debug,Display)]
        ] {
            *Init + Event = End,
        }
    }

    struct Context;
    impl SMStateMachineContext for Context {}

    let mut sm = SMStateMachine::new(Context);
    assert!(sm.is(&SMStates::Init));

    let event = SMEvents::Event;
    assert_eq!(format!("{event}"), "Event");

    sm.process_event(event).unwrap();
    assert!(sm.is(&SMStates::End));
}

#[test]
fn async_guards_and_actions() {
    use smol;

    smol::block_on(async {
        sml! {
        _ {
                *State1 + Event1 [async guard1] / async action1 = State2,
                _ + Event1 = Fault,
            }
        }

        struct Context;

        impl StateMachineContext for Context {
            async fn guard1(&self) -> Result<bool, ()> {
                core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
                Ok(true)
            }

            async fn action1(&mut self) -> Result<(), ()> {
                core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
                Ok(())
            }
        }

        let mut sm = StateMachine::new(Context);

        sm.process_event(Events::Event1).await.unwrap();
        assert!(sm.is(&States::State2));

        sm.process_event(Events::Event1).await.unwrap();
        assert!(sm.is(&States::Fault));
    });
}

#[test]
fn async_on_entry_and_exit() {
    use smol;

    smol::block_on(async {
        sml! {
            _[entry_exit_async] {
                *State1 + Event1 = State2,
                State2 + Event2 = State3,
            }
        }

        struct Context;

        impl StateMachineContext for Context {
            async fn on_entry_state1(&mut self) {
                core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
            }

            async fn on_exit_state1(&mut self) {
                core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
            }

            async fn on_entry_state2(&mut self) {
                core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
            }

            async fn on_exit_state2(&mut self) {
                core::future::poll_fn(|_| core::task::Poll::Ready(())).await;
            }
        }

        let mut sm = StateMachine::new(Context);

        sm.process_event(Events::Event1).await.unwrap();
        assert!(sm.is(&States::State2));

        sm.process_event(Events::Event2).await.unwrap();
        assert!(sm.is(&States::State3));
    });
}

#[test]
fn guard_expressions() {
    #[derive(PartialEq, Eq, Display)]
    pub struct Entry(pub u32);

    sml! {
        _[states_attr: #[derive(Display, Debug)]] {
            *Init + Login(&'a Entry) [valid_entry] / attempt = LoggedIn,
            Init + Login(&'a Entry) [!valid_entry && !too_many_attempts] / attempt = Init,
            Init + Login(&'a Entry) [!valid_entry && too_many_attempts] / attempt = LoginDenied,
            LoggedIn + Logout / reset = Init,
        }
    }

    /// Context
    pub struct Context {
        password: u32,
        attempts: core::cell::Cell<u32>,
    }
    impl StateMachineContext for &Context {
        fn valid_entry(&self, e: &Entry) -> Result<bool, ()> {
            Ok(e.0 == self.password)
        }
        fn too_many_attempts(&self, _e: &Entry) -> Result<bool, ()> {
            Ok(self.attempts.get() >= 3)
        }
        fn reset(&mut self) -> Result<(), ()> {
            self.attempts.set(0);
            Ok(())
        }
        fn attempt(&mut self, _e: &Entry) -> Result<(), ()> {
            self.attempts.set(self.attempts.get() + 1);
            Ok(())
        }
    }

    let context = Context {
        password: 42,
        attempts: core::cell::Cell::new(0),
    };
    let mut sm = StateMachine::new(&context);
    assert!(sm.is(&States::Init));

    let bad_entry = Entry(10);
    let good_entry = Entry(42);

    let _ = sm.process_event(Events::Login(&bad_entry));
    assert_eq!(context.attempts.get(), 1);
    assert!(sm.is(&States::Init));

    let _ = sm.process_event(Events::Login(&bad_entry));
    assert_eq!(context.attempts.get(), 2);
    assert!(sm.is(&States::Init));

    let _ = sm.process_event(Events::Login(&good_entry));
    assert_eq!(context.attempts.get(), 3);
    assert!(sm.is(&States::LoggedIn));

    let _ = sm.process_event(Events::Logout);
    assert_eq!(context.attempts.get(), 0);
    assert!(sm.is(&States::Init));

    let _ = sm.process_event(Events::Login(&bad_entry));
    let _ = sm.process_event(Events::Login(&bad_entry));
    let _ = sm.process_event(Events::Login(&bad_entry));
    assert_eq!(context.attempts.get(), 3);
    assert!(sm.is(&States::Init));

    // exhausted attempts
    let _ = sm.process_event(Events::Login(&bad_entry));
    assert_eq!(context.attempts.get(), 4);
    assert!(sm.is(&States::LoginDenied));

    // Invalid event, as we are in a final state
    assert_eq!(
        sm.process_event(Events::Login(&good_entry)),
        Err(Error::InvalidEvent)
    );
    assert_eq!(context.attempts.get(), 4);
    assert!(sm.is(&States::LoginDenied));
}
#[test]
fn guarded_transition_before_unguarded() {
    use sml::sml;
    sml! {
        _[states_attr: #[derive(Debug, Clone, Copy)]] {
            *State1 + Event1 [guard] / disable = State2,
            State1 + Event1 = Fault,
            State2 + Event1 = State1,
        }
    }
    struct Context {
        pub enabled: bool,
    }
    impl StateMachineContext for Context {
        fn guard(&self) -> Result<bool, ()> {
            Ok(self.enabled)
        }

        fn disable(&mut self) -> Result<(), ()> {
            self.enabled = false;
            Ok(())
        }
    }
    let mut sm = StateMachine::new(Context { enabled: true });
    sm.process_event(Events::Event1).unwrap();
    assert!(sm.is(&States::State2));

    sm.process_event(Events::Event1).unwrap();
    assert!(sm.is(&States::State1));

    sm.process_event(Events::Event1).unwrap();
    assert!(sm.is(&States::Fault));
}

#[test]
fn guard_errors() {
    use sml::sml;
    sml! {
        _ {
            *Init + Event1 [guard] = Done,
        }
    }

    struct Context {
        pub guard_passable: core::cell::Cell<bool>,
        pub guard_errors: core::cell::Cell<bool>,
    }
    impl StateMachineContext for &Context {
        fn guard(&self) -> Result<bool, ()> {
            if self.guard_errors.get() {
                Err(())
            } else {
                Ok(self.guard_passable.get())
            }
        }
    }

    let context = Context {
        guard_passable: core::cell::Cell::new(false),
        guard_errors: core::cell::Cell::new(true),
    };
    let mut sm = StateMachine::new(&context);

    // Test attempting to transition when the guard fails.
    context.guard_errors.set(true);
    assert!(sm.process_event(Events::Event1).is_err());
    assert!(sm.is(&States::Init));

    // Test attempting to transition when the guard is not passable.
    context.guard_errors.set(false);
    assert!(sm.process_event(Events::Event1).is_err());
    assert!(sm.is(&States::Init));

    assert!(sm.process_event(Events::Event1).is_err());
    assert!(sm.is(&States::Init));

    context.guard_passable.set(true);
    sm.process_event(Events::Event1).unwrap();
    assert!(sm.is(&States::Done));
}
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the fixture keeps the transition table, its shared context, and every scenario together"
)]
fn test_internal_transition_with_data() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct State1Data(pub ActionId);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct State3Data(pub ActionId);

    sml! {
        _[states_attr: #[derive(Debug, Clone, Copy, Eq)]] {
            *State1(State1Data) + Event2 / action12 = State2,
            State1(State1Data) + Event3 / action13 = State3(State3Data),
            State1(State1Data) + Event4 / action14 = State4(State3Data),

            State2 + Event3 / action23 = State3(State3Data),
            State4(State3Data) + Event1 / action44 = _, // Same as State4(State3Data) + Event1 / action44

            // TRANSITION : _ + Event3 / increment_count = _, IS EQUIVALENT TO THE FOLLOWING TWO:
            // State3(State3Data) + Event3 / action_3 = State3(State3Data),
            // State4(State3Data) + Event3 / action_3 = State4(State3Data),
            _ + Event3 / action_3 = _,
        }
    }
    /// Context
    #[derive(Default, Debug, PartialEq, Eq)]
    pub struct Context {
        action: core::cell::Cell<ActionId>,
    }
    #[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
    enum ActionId {
        #[default]
        Init,
        Action3,
        Action12,
        Action13,
        Action14,
        Action23,
        Action44,
    }
    impl StateMachineContext for &Context {
        fn action_3(&mut self, d: &State3Data) -> Result<State3Data, ()> {
            self.action.set(ActionId::Action3);
            Ok(*d)
        }

        fn action44(&mut self, _d: &State3Data) -> Result<State3Data, ()> {
            self.action.set(ActionId::Action44);
            Ok(State3Data(ActionId::Action44))
        }
        fn action14(&mut self, _d: &State1Data) -> Result<State3Data, ()> {
            self.action.set(ActionId::Action14);
            Ok(State3Data(ActionId::Action14))
        }
        fn action12(&mut self, _d: &State1Data) -> Result<(), ()> {
            self.action.set(ActionId::Action12);
            Ok(())
        }
        fn action13(&mut self, _d: &State1Data) -> Result<State3Data, ()> {
            self.action.set(ActionId::Action13);
            Ok(State3Data(ActionId::Action13))
        }
        fn action23(&mut self) -> Result<State3Data, ()> {
            self.action.set(ActionId::Action23);
            Ok(State3Data(ActionId::Action23))
        }
    }

    {
        let context = Context::default();
        let mut sm = StateMachine::new(&context, State1Data(ActionId::Init));
        matches!(States::State2, States::State2);
        assert_transition_ok!(
            sm,
            context,
            Events::Event2,
            ActionId::Action12,
            States::State2
        );
        assert!(sm.process_event(Events::Event1).is_err());
        assert!(sm.process_event(Events::Event2).is_err());
        assert!(sm.process_event(Events::Event4).is_err());
        assert_transition_ok!(
            sm,
            context,
            Events::Event3,
            ActionId::Action23,
            States::State3(_)
        );
        assert_transition_ok!(
            sm,
            context,
            Events::Event3,
            ActionId::Action3,
            States::State3(_)
        );
        assert_transition_ok!(
            sm,
            context,
            Events::Event3,
            ActionId::Action3,
            States::State3(_)
        );
        assert!(sm.process_event(Events::Event1).is_err());
        assert!(sm.process_event(Events::Event2).is_err());
        assert!(sm.process_event(Events::Event4).is_err());
    }
    {
        let context = Context::default();
        let mut sm = StateMachine::new(&context, State1Data(ActionId::Init));
        assert_transition_ok!(
            sm,
            context,
            Events::Event3,
            ActionId::Action13,
            States::State3(_)
        );
        assert!(sm.process_event(Events::Event1).is_err());
        assert!(sm.process_event(Events::Event2).is_err());
        assert!(sm.process_event(Events::Event4).is_err());
    }
    {
        let context = Context::default();
        let mut sm = StateMachine::new(&context, State1Data(ActionId::Init));
        assert_transition_ok!(
            sm,
            context,
            Events::Event4,
            ActionId::Action14,
            States::State4(_)
        );
        assert_transition_ok!(
            sm,
            context,
            Events::Event1,
            ActionId::Action44,
            States::State4(_)
        );
        assert_transition_ok!(
            sm,
            context,
            Events::Event3,
            ActionId::Action3,
            States::State4(_)
        );
    }
}
#[test]
fn test_wildcard_states_and_internal_transitions() {
    sml! {
        _[states_attr: #[derive(Debug, Clone, Copy)]] {
            *State1 + Event2 = State2,
            State2 + Event3 = State3,
            _ + Event1 / increment_count,      // Internal transition (implicit: omitting target state)
            _ + Event3 / increment_count = _ , // Internal transition (explicit: using _ as target state)
        }
    }
    #[derive(Debug)]
    pub struct Context {
        count: core::cell::Cell<u32>,
    }
    impl StateMachineContext for &Context {
        fn increment_count(&mut self) -> Result<(), ()> {
            self.count.set(self.count.get() + 1);
            Ok(())
        }
    }

    let context = Context {
        count: core::cell::Cell::new(0),
    };
    let mut sm = StateMachine::new(&context);

    assert_transition!(sm, context, Events::Event1, States::State1, 1);
    assert_transition!(sm, context, Events::Event2, States::State2, 1);
    assert_transition!(sm, context, Events::Event3, States::State3, 1);
    assert_transition!(sm, context, Events::Event1, States::State3, 2);
    assert_transition!(sm, context, Events::Event3, States::State3, 3);

    assert!(sm.process_event(Events::Event2).is_err()); // InvalidEvent
    assert!(sm.is(&States::State3));
}
#[test]
fn test_specify_attrs() {
    #![deny(non_camel_case_types)]
    use serde::Serialize;
    sml! {
        _[
            states_attr: #[derive(Debug, Clone, Copy, Serialize)] #[non_exhaustive] #[repr(u8)] #[serde(tag="type")],
            events_attr: #[derive(Debug)] #[allow(
                non_camel_case_types,
                reason = "the fixture preserves the source DSL's event naming"
            )]
        ] {
            *State1 + tostate2 = State2,
            State2 + tostate3 / increment_count = State3
        }
    }

    #[derive(Debug, Default)]
    pub struct Context {
        count: core::cell::Cell<u32>,
    }

    impl StateMachineContext for &Context {
        fn increment_count(&mut self) -> Result<(), ()> {
            self.count.set(self.count.get() + 1);
            Ok(())
        }
    }
    let context = Context::default();
    let mut sm = StateMachine::new(&context);

    assert!(sm.is(&States::State1));
    assert_transition!(sm, context, Events::tostate2, States::State2, 0);
    assert_transition!(sm, context, Events::tostate3, States::State3, 1);
}

mod machine_trait_api {
    use sml::{sml, Machine};

    pub struct SyncFlatEvent;
    pub struct RejectedEvent;

    #[derive(Debug, PartialEq)]
    enum PrivateFailure {
        Rejected,
    }

    sml! {
        TraitSyncFlat {
            *"idle"_s + event<SyncFlatEvent> = "done"_s,
        }
    }

    sml! {
        TraitPrivateError[custom_error] {
            *"idle"_s + event<RejectedEvent> / reject = "done"_s,
        }
    }

    struct Context;

    impl TraitSyncFlatStateMachineContext for Context {}
    impl TraitPrivateErrorStateMachineContext for Context {
        type Error = PrivateFailure;

        fn reject(&mut self, _event: &RejectedEvent) -> Result<(), PrivateFailure> {
            Err(PrivateFailure::Rejected)
        }
    }

    fn process_sync<M, E>(machine: &mut M, event: E) -> bool
    where
        M: Machine<E>,
    {
        Machine::process_event(machine, event)
    }

    async fn process_async<M, E>(machine: &mut M, event: E) -> bool
    where
        M: Machine<E>,
    {
        Machine::process_event_async(machine, event).await
    }

    #[test]
    fn synchronous_trait_dispatches_flat_machine() {
        let mut flat = TraitSyncFlatStateMachine::new(Context);
        assert!(process_sync(
            &mut flat,
            TraitSyncFlatEvents::SyncFlatEvent(SyncFlatEvent)
        ));
        assert!(flat.is(&TraitSyncFlatStates::Done));
    }

    #[test]
    fn asynchronous_trait_dispatches_flat_machine() {
        smol::block_on(async {
            let mut flat = TraitSyncFlatStateMachine::new(Context);
            assert!(
                process_async(&mut flat, TraitSyncFlatEvents::SyncFlatEvent(SyncFlatEvent)).await
            );
            assert!(flat.is(&TraitSyncFlatStates::Done));
        });
    }

    #[test]
    fn trait_rejection_preserves_private_inherent_error_contract() {
        let mut inherent = TraitPrivateErrorStateMachine::new(Context);
        assert!(matches!(
            inherent.process_event(RejectedEvent),
            Err(TraitPrivateErrorError::ActionFailed(
                PrivateFailure::Rejected
            ))
        ));
        assert!(inherent.is(&TraitPrivateErrorStates::Idle));

        let mut synchronous = TraitPrivateErrorStateMachine::new(Context);
        assert!(!Machine::process_event(
            &mut synchronous,
            TraitPrivateErrorEvents::RejectedEvent(RejectedEvent),
        ));
        assert!(synchronous.is(&TraitPrivateErrorStates::Idle));

        smol::block_on(async {
            let mut asynchronous = TraitPrivateErrorStateMachine::new(Context);
            assert!(
                !Machine::process_event_async(
                    &mut asynchronous,
                    TraitPrivateErrorEvents::RejectedEvent(RejectedEvent),
                )
                .await
            );
            assert!(asynchronous.is(&TraitPrivateErrorStates::Idle));
        });
    }
}
