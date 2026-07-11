//! Mechanical Rust translations of the programs under `../sml.cpp/example`.
//!
//! Each module keeps the upstream example name and asserts its observable
//! transition behavior. Callback lambdas become methods on the generated
//! context trait, which is the ownership-safe Rust spelling.

#![allow(private_interfaces)]

mod hello_world {
    use sml::sml;

    struct Release;
    struct Ack;
    struct Fin;
    struct Timeout;

    sml! {
        HelloWorldExample {
            *"established"_s + event<Release> / send_fin = "fin wait 1"_s,
             "fin wait 1"_s + event<Ack> [is_ack_valid] = "fin wait 2"_s,
             "fin wait 2"_s + event<Fin> [is_fin_valid] / send_ack_fin = "timed wait"_s,
             "timed wait"_s + event<Timeout> / send_ack_timeout = X,
        }
    }

    #[derive(Default)]
    struct Context(usize);
    impl HelloWorldExampleStateMachineContext for Context {
        fn is_ack_valid(&self, _: &Ack) -> Result<bool, ()> {
            Ok(true)
        }
        fn is_fin_valid(&self, _: &Fin) -> Result<bool, ()> {
            Ok(true)
        }
        fn send_fin(&mut self, _: &Release) -> Result<(), ()> {
            self.0 += 1;
            Ok(())
        }
        fn send_ack_fin(&mut self, _: &Fin) -> Result<(), ()> {
            self.0 += 1;
            Ok(())
        }
        fn send_ack_timeout(&mut self, _: &Timeout) -> Result<(), ()> {
            self.0 += 1;
            Ok(())
        }
    }

    #[test]
    fn translated_behavior() {
        let mut sm = HelloWorldExampleStateMachine::new(Context::default());
        sm.process_event(Release).unwrap();
        sm.process_event(Ack).unwrap();
        sm.process_event(Fin).unwrap();
        sm.process_event(Timeout).unwrap();
        assert!(sm.is_terminated());
        assert_eq!(sm.context().0, 3);
    }
}

mod transitions {
    use sml::sml;
    struct E1;
    struct E2;
    struct E3;

    sml! {
        TransitionsExample {
            *"idle"_s / anonymous = "s1"_s,
             "s1"_s + event<E1> / internal,
             "s1"_s + event<E2> / self_transition = "s1"_s,
             "s1"_s + on_entry<_> / entry,
             "s1"_s + on_exit<_> / exit,
             "s1"_s + event<E3> / external = X,
        }
    }

    #[derive(Default)]
    struct Context {
        entries: usize,
        exits: usize,
        actions: usize,
    }
    impl TransitionsExampleStateMachineContext for Context {
        fn anonymous(&mut self) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
        fn internal(&mut self, _: &E1) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
        fn self_transition(&mut self, _: &E2) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
        fn external(&mut self, _: &E3) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
        fn entry(&mut self) -> Result<(), ()> {
            self.entries += 1;
            Ok(())
        }
        fn exit(&mut self) -> Result<(), ()> {
            self.exits += 1;
            Ok(())
        }
    }

    #[test]
    fn translated_behavior() {
        let mut sm = TransitionsExampleStateMachine::new(Context::default());
        sm.initialize().unwrap();
        sm.process_event(E1).unwrap();
        sm.process_event(E2).unwrap();
        sm.process_event(E3).unwrap();
        assert!(sm.is_terminated());
        assert_eq!(
            (
                sm.context().entries,
                sm.context().exits,
                sm.context().actions
            ),
            (2, 2, 4)
        );
    }
}

mod events {
    use sml::sml;
    struct E1;
    struct E2 {
        value: bool,
    }
    struct E4 {
        value: i32,
    }

    sml! {
        EventsExample {
            *"idle"_s + event<E1> = "s1"_s,
             "s1"_s + event<E2> [valid] = "s2"_s,
             "s2"_s + "e3"_e = "s3"_s,
             "s3"_s + event<E4> / check = X,
        }
    }

    struct Context;
    impl EventsExampleStateMachineContext for Context {
        fn valid(&self, event: &E2) -> Result<bool, ()> {
            Ok(event.value)
        }
        fn check(&mut self, event: &E4) -> Result<(), ()> {
            assert_eq!(event.value, 42);
            Ok(())
        }
    }

    #[test]
    fn translated_behavior() {
        let mut sm = EventsExampleStateMachine::new(Context);
        sm.process_event(E1).unwrap();
        sm.process_event(E2 { value: true }).unwrap();
        sm.process_event(EventsExampleEvents::E3).unwrap();
        sm.process_event(E4 { value: 42 }).unwrap();
        assert!(sm.is_terminated());
    }
}

mod composite {
    use sml::sml;
    struct E1;
    struct E2;
    struct E3;
    struct E4;
    struct E5;

    sml! {
        CompositeSubExample {
            *"idle"_s + event<E3> = "s1"_s,
             "s1"_s + event<E4> = X,
        }
        CompositeExample {
            *"idle"_s + event<E1> = "s1"_s,
             "s1"_s + event<E2> = state<CompositeSubExample>,
             state<CompositeSubExample> + event<E5> = X,
        }
    }
    struct Context;
    impl CompositeExampleStateMachineContext for Context {}

    #[test]
    fn translated_behavior() {
        let mut sm = CompositeExampleStateMachine::new(Context);
        sm.process_event(E1).unwrap();
        sm.process_event(E2).unwrap();
        sm.process_event(E3).unwrap();
        sm.process_event(E4).unwrap();
        assert!(sm.is_child(&CompositeExampleCompositeSubExampleStates::X));
        sm.process_event(E5).unwrap();
        assert!(sm.is_terminated());
    }
}

mod orthogonal_regions {
    use sml::sml;
    struct E1;
    struct E2;
    struct E3;
    sml! {
        OrthogonalRegionsExample {
            *"idle"_s + event<E1> = "s1"_s,
             "s1"_s + event<E2> = X,
            *"idle2"_s + event<E2> = "s2"_s,
             "s2"_s + event<E3> = X,
        }
    }
    struct Context;
    impl OrthogonalRegionsExampleStateMachineContext for Context {}

    #[test]
    fn translated_behavior() {
        let mut sm = OrthogonalRegionsExampleStateMachine::new(Context);
        sm.process_event(E1).unwrap();
        sm.process_event(E2).unwrap();
        sm.process_event(E3).unwrap();
        assert!(sm.is_terminated());
    }
}

mod actions_guards {
    use sml::sml;
    struct E1;
    struct E2;
    struct E3;
    struct E4;
    struct E5;
    sml! {
        ActionsGuardsExample {
            *"idle"_s + event<E1> = "s1"_s,
             "s1"_s + event<E2> [guard_e2] / action_e2 = "s2"_s,
             "s2"_s + event<E3> [guard_e3 && !reject_e3] / (action_e3, action2) = "s3"_s,
             "s3"_s + event<E4> [!reject_e4 || guard_e4] / (action_e4, action3) = "s5"_s,
             "s5"_s + event<E5> [guard_e5] / action_e5 = X,
        }
    }
    #[derive(Default)]
    struct Context {
        actions: usize,
    }
    impl ActionsGuardsExampleStateMachineContext for Context {
        fn guard_e2(&self, _: &E2) -> Result<bool, ()> {
            Ok(true)
        }
        fn guard_e3(&self, _: &E3) -> Result<bool, ()> {
            Ok(true)
        }
        fn reject_e3(&self, _: &E3) -> Result<bool, ()> {
            Ok(false)
        }
        fn reject_e4(&self, _: &E4) -> Result<bool, ()> {
            Ok(false)
        }
        fn guard_e4(&self, _: &E4) -> Result<bool, ()> {
            Ok(false)
        }
        fn guard_e5(&self, _: &E5) -> Result<bool, ()> {
            Ok(true)
        }
        fn action_e2(&mut self, _: &E2) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
        fn action_e3(&mut self, _: &E3) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
        fn action2(&mut self, _: &E3) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
        fn action_e4(&mut self, _: &E4) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
        fn action3(&mut self, _: &E4) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
        fn action_e5(&mut self, _: &E5) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = ActionsGuardsExampleStateMachine::new(Context::default());
        sm.process_event(E1).unwrap();
        sm.process_event(E2).unwrap();
        sm.process_event(E3).unwrap();
        sm.process_event(E4).unwrap();
        sm.process_event(E5).unwrap();
        assert!(sm.is_terminated());
        assert_eq!(sm.context().actions, 6);
    }
}

mod states {
    use sml::sml;
    #[derive(Default)]
    struct Idle;
    #[derive(Default)]
    struct S2;
    struct E1;
    struct E2;
    struct E3;
    sml! {
        StatesExample {
            *state<Idle> + event<E1> = "s1"_s,
             "s1"_s + on_entry<_> / entered,
             "s1"_s + on_exit<_> / exited,
             "s1"_s + event<E2> = state<S2>,
             state<S2> + event<E3> = X,
        }
    }
    #[derive(Default)]
    struct Context {
        entries: usize,
        exits: usize,
    }
    impl StatesExampleStateMachineContext for Context {
        fn entered(&mut self) -> Result<(), ()> {
            self.entries += 1;
            Ok(())
        }
        fn exited(&mut self) -> Result<(), ()> {
            self.exits += 1;
            Ok(())
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = StatesExampleStateMachine::new(Context::default());
        sm.process_event(E1).unwrap();
        sm.process_event(E2).unwrap();
        sm.process_event(E3).unwrap();
        assert!(sm.is_terminated());
        assert_eq!((sm.context().entries, sm.context().exits), (1, 1));
    }
}

mod history {
    use sml::sml;
    sml! {
        HistorySubExample {
            *"idle"_s(H) + "e1"_e = "s1"_s,
             "s1"_s + "e2"_e = X,
        }
        HistoryExample {
            *"idle"_s + "e1"_e = state<HistorySubExample>,
             state<HistorySubExample> + "e3"_e = "s1"_s,
             "s1"_s + "e4"_e = state<HistorySubExample>,
        }
    }
    struct Context;
    impl HistoryExampleStateMachineContext for Context {}
    #[test]
    fn translated_behavior() {
        let mut sm = HistoryExampleStateMachine::new(Context);
        sm.process_event(HistoryExampleEvents::E1).unwrap();
        sm.process_event(HistoryExampleEvents::E1).unwrap();
        sm.process_event(HistoryExampleEvents::E3).unwrap();
        sm.process_event(HistoryExampleEvents::E4).unwrap();
        assert!(sm.is_child(&HistoryExampleHistorySubExampleStates::S1));
        sm.process_event(HistoryExampleEvents::E2).unwrap();
        assert!(sm.is_child(&HistoryExampleHistorySubExampleStates::X));
    }
}

mod dependencies {
    use sml::sml;
    struct E1 {
        i: i32,
    }
    sml! {
        DependenciesExample {
            *"idle"_s + event<E1> [dependency_empty] / update_dependency = "s1"_s,
             "s1"_s + event<E1> [dependency_matches] = X,
        }
    }
    #[derive(Default)]
    struct Context {
        dependency: i32,
    }
    impl DependenciesExampleStateMachineContext for Context {
        fn dependency_empty(&self, _: &E1) -> Result<bool, ()> {
            Ok(self.dependency == 0)
        }
        fn update_dependency(&mut self, event: &E1) -> Result<(), ()> {
            self.dependency = event.i + 42;
            Ok(())
        }
        fn dependency_matches(&self, event: &E1) -> Result<bool, ()> {
            Ok(self.dependency == event.i)
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = DependenciesExampleStateMachine::new(Context::default());
        sm.process_event(E1 { i: 0 }).unwrap();
        assert!(sm.process_event(E1 { i: 0 }).is_err());
        sm.process_event(E1 { i: 42 }).unwrap();
        assert!(sm.is_terminated());
    }
}

mod testing {
    use sml::sml;
    struct E1;
    struct E2;
    struct E3;
    sml! {
        TestingExample {
            *"idle"_s + event<E1> = "s1"_s,
             "s1"_s + event<E2> = "s2"_s,
             "s2"_s + event<E3> [ready] / mark = X,
        }
    }
    #[derive(Default)]
    struct Context {
        value: i32,
    }
    impl TestingExampleStateMachineContext for Context {
        fn ready(&self, _: &E3) -> Result<bool, ()> {
            Ok(self.value == 0)
        }
        fn mark(&mut self, _: &E3) -> Result<(), ()> {
            self.value = 42;
            Ok(())
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = TestingExampleStateMachine::new(Context::default());
        sm.set_state(TestingExampleStates::S2);
        sm.process_event(E3).unwrap();
        assert!(sm.is_terminated());
        assert_eq!(sm.context().value, 42);
    }
}

mod visitor {
    use sml::sml;
    struct E1;
    struct E2;
    struct E3;
    sml! {
        VisitorExample {
            *"idle"_s + event<E1> = "s1"_s,
             "s1"_s + event<E2> = "s2"_s,
             "s2"_s + event<E3> = X,
        }
    }
    struct Context;
    impl VisitorExampleStateMachineContext for Context {}
    #[test]
    fn translated_behavior() {
        let mut sm = VisitorExampleStateMachine::new(Context);
        sm.process_event(E1).unwrap();
        assert!(sm.visit_current_state(|s| matches!(s, VisitorExampleStates::S1)));
        sm.process_event(E2).unwrap();
        sm.process_event(E3).unwrap();
        assert!(sm.visit_current_state(|s| matches!(s, VisitorExampleStates::X)));
    }
}

mod logging {
    use core::cell::Cell;
    use sml::sml;
    struct E1;
    sml! {
        LoggingExample {
            *"idle"_s + event<E1> [guard_a && guard_b] / action = "s1"_s,
        }
    }
    #[derive(Default)]
    struct Context {
        guards: Cell<usize>,
        actions: Cell<usize>,
        transitions: Cell<usize>,
    }
    impl LoggingExampleStateMachineContext for Context {
        fn guard_a(&self, _: &E1) -> Result<bool, ()> {
            Ok(true)
        }
        fn guard_b(&self, _: &E1) -> Result<bool, ()> {
            Ok(true)
        }
        fn action(&mut self, _: &E1) -> Result<(), ()> {
            Ok(())
        }
        fn log_guard(&self, _: &'static str, _: bool) {
            self.guards.set(self.guards.get() + 1);
        }
        fn log_action(&self, _: &'static str) {
            self.actions.set(self.actions.get() + 1);
        }
        fn transition_callback(&self, _: &LoggingExampleStates, _: &LoggingExampleStates) {
            self.transitions.set(self.transitions.get() + 1);
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = LoggingExampleStateMachine::new(Context::default());
        sm.process_event(E1).unwrap();
        assert_eq!(
            (
                sm.context().guards.get(),
                sm.context().actions.get(),
                sm.context().transitions.get()
            ),
            (2, 1, 1)
        );
    }
}

mod data {
    use sml::sml;
    #[derive(Default)]
    struct Disconnected;
    #[derive(Default)]
    struct Connected {
        id: i32,
    }
    #[derive(Default)]
    struct Interrupted {
        id: i32,
    }
    struct Connect {
        id: i32,
    }
    struct Interrupt;
    struct Disconnect;
    sml! {
        DataExample {
            *state<Disconnected> + event<Connect> / connect = state<Connected>,
             state<Connected> + event<Interrupt> / interrupt = state<Interrupted>,
             state<Interrupted> + event<Connect> / reconnect = state<Connected>,
             state<Connected> + event<Disconnect> / disconnect = X,
        }
    }
    #[derive(Default)]
    struct Context {
        printed: Vec<i32>,
    }
    impl DataExampleStateMachineContext for Context {
        fn connect(&mut self, _: &Disconnected, event: &Connect) -> Result<Connected, ()> {
            self.printed.push(event.id);
            Ok(Connected { id: event.id })
        }
        fn interrupt(&mut self, state: &Connected, _: &Interrupt) -> Result<Interrupted, ()> {
            Ok(Interrupted { id: state.id })
        }
        fn reconnect(&mut self, state: &Interrupted, event: &Connect) -> Result<Connected, ()> {
            assert_eq!(state.id, 1024);
            self.printed.push(event.id);
            Ok(Connected { id: event.id })
        }
        fn disconnect(&mut self, state: &Connected, _: &Disconnect) -> Result<(), ()> {
            self.printed.push(state.id);
            Ok(())
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = DataExampleStateMachine::new(Context::default());
        sm.process_event(Connect { id: 1024 }).unwrap();
        sm.process_event(Interrupt).unwrap();
        sm.process_event(Connect { id: 1025 }).unwrap();
        sm.process_event(Disconnect).unwrap();
        assert!(sm.is_terminated());
        assert_eq!(sm.context().printed, [1024, 1025, 1025]);
    }
}

mod defer_and_process {
    use sml::sml;
    #[derive(Clone)]
    struct E1;
    struct E2;
    struct E3;
    struct E4;
    sml! {
        DeferAndProcessExample {
            *"idle"_s + event<E1> / defer,
             "idle"_s + event<E2> = "s1"_s,
             "s1"_s + event<E1> / process(E2 {}) = "s2"_s,
             "s2"_s + event<E3> / process(E4 {}),
             "s2"_s + event<E4> = X,
        }
    }
    struct Context;
    impl DeferAndProcessExampleStateMachineContext for Context {}
    #[test]
    fn translated_behavior() {
        let mut sm = DeferAndProcessExampleStateMachine::new(Context);
        sm.process_event(E1).unwrap();
        sm.process_event(E2).unwrap();
        assert!(sm.is(&DeferAndProcessExampleStates::S2));
        sm.process_event(E3).unwrap();
        assert!(sm.is_terminated());
    }
}

mod dependency_injection {
    use sml::sml;
    struct E1;
    struct E2;
    struct E3;
    sml! {
        DependencyInjectionExample {
            *"idle"_s + event<E1> = "s1"_s,
             "s1"_s + event<E2> [injected_guard] / injected_action = "s2"_s,
             "s2"_s + event<E3> = X,
        }
    }
    struct Context {
        integer: i32,
        real: f64,
    }
    impl DependencyInjectionExampleStateMachineContext for Context {
        fn injected_guard(&self, _: &E2) -> Result<bool, ()> {
            Ok(self.integer == 42 && self.real == 87.0)
        }
        fn injected_action(&mut self, _: &E2) -> Result<(), ()> {
            assert_eq!(self.integer, 42);
            Ok(())
        }
    }
    struct Controller<'a>(&'a mut DependencyInjectionExampleStateMachine<Context>);
    impl Controller<'_> {
        fn start(&mut self) {
            self.0.process_event(E1).unwrap();
            self.0.process_event(E2).unwrap();
            self.0.process_event(E3).unwrap();
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = DependencyInjectionExampleStateMachine::new(Context {
            integer: 42,
            real: 87.0,
        });
        Controller(&mut sm).start();
        assert!(sm.is_terminated());
    }
}

mod dispatch_policy {
    use sml::sml;
    struct Connect;
    struct Established;
    struct Ping;
    struct Disconnect;
    sml! {
        DispatchPolicyExample {
            *"disconnected"_s + event<Connect> = "connecting"_s,
             "connecting"_s + event<Established> = "connected"_s,
             "connected"_s + event<Ping> [valid],
             "connected"_s + event<Disconnect> = "disconnected"_s,
        }
    }
    struct Context;
    impl DispatchPolicyExampleStateMachineContext for Context {
        fn valid(&self, _: &Ping) -> Result<bool, ()> {
            Ok(true)
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = DispatchPolicyExampleStateMachine::new(Context);
        for _ in 0..2 {
            sm.process_event(Connect).unwrap();
            sm.process_event(Established).unwrap();
            sm.process_event(Ping).unwrap();
            sm.process_event(Disconnect).unwrap();
        }
        assert!(sm.is(&DispatchPolicyExampleStates::Disconnected));
    }
}

mod eval {
    use sml::sml;
    struct E1;
    sml! { EvalExample { *"idle"_s + event<E1> [guard] / (action, eval [guard] / action, action) = X, } }
    #[derive(Default)]
    struct Context(usize);
    impl EvalExampleStateMachineContext for Context {
        fn guard(&self, _: &E1) -> Result<bool, ()> {
            Ok(true)
        }
        fn action(&mut self, _: &E1) -> Result<(), ()> {
            self.0 += 1;
            Ok(())
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = EvalExampleStateMachine::new(Context::default());
        sm.process_event(E1).unwrap();
        assert_eq!(sm.context().0, 3);
        assert!(sm.is_terminated());
    }
}

mod euml_emulation {
    use sml::sml;
    struct E1;
    struct E2;
    struct E3;
    sml! { EumlExample { *"idle"_s + event<E1> = "s1"_s, "s1"_s + event<E2> [guard_e2] = "s2"_s, "s2"_s + event<E3> [guard_e3] / action = X, } }
    struct Context;
    impl EumlExampleStateMachineContext for Context {
        fn guard_e2(&self, _: &E2) -> Result<bool, ()> {
            Ok(true)
        }
        fn guard_e3(&self, _: &E3) -> Result<bool, ()> {
            Ok(true)
        }
        fn action(&mut self, _: &E3) -> Result<(), ()> {
            Ok(())
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = EumlExampleStateMachine::new(Context);
        sm.process_event(E1).unwrap();
        sm.process_event(E2).unwrap();
        sm.process_event(E3).unwrap();
        assert!(sm.is_terminated());
    }
}

mod in_place {
    use sml::sml;
    struct Start;
    sml! { InPlaceExample { *"idle"_s + event<Start> / action = X, } }
    #[derive(Default)]
    struct Context(bool);
    impl InPlaceExampleStateMachineContext for Context {
        fn action(&mut self, _: &Start) -> Result<(), ()> {
            self.0 = true;
            Ok(())
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = InPlaceExampleStateMachine::new(Context::default());
        sm.process_event(Start).unwrap();
        assert!(sm.is_terminated() && sm.context().0);
    }
}

mod nested {
    use sml::sml;
    struct E1;
    sml! { NestedOwnedExample { *"idle"_s + event<E1> = X, } }
    struct Context;
    impl NestedOwnedExampleStateMachineContext for Context {}
    struct Top {
        machine: NestedOwnedExampleStateMachine<Context>,
    }
    impl Top {
        fn process(&mut self) {
            self.machine.process_event(E1).unwrap();
        }
    }
    #[test]
    fn translated_behavior() {
        let mut top = Top {
            machine: NestedOwnedExampleStateMachine::new(Context),
        };
        top.process();
        assert!(top.machine.is_terminated());
    }
}

mod arduino {
    use sml::sml;
    struct Pressed(bool);
    sml! { ArduinoSwitcherExample { *"off"_s + event<Pressed> [pressed] / led_on = "on"_s, "on"_s + event<Pressed> [released] / led_off = "off"_s, } }
    #[derive(Default)]
    struct Pins {
        led: bool,
    }
    impl ArduinoSwitcherExampleStateMachineContext for Pins {
        fn pressed(&self, e: &Pressed) -> Result<bool, ()> {
            Ok(e.0)
        }
        fn released(&self, e: &Pressed) -> Result<bool, ()> {
            Ok(!e.0)
        }
        fn led_on(&mut self, _: &Pressed) -> Result<(), ()> {
            self.led = true;
            Ok(())
        }
        fn led_off(&mut self, _: &Pressed) -> Result<(), ()> {
            self.led = false;
            Ok(())
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = ArduinoSwitcherExampleStateMachine::new(Pins::default());
        sm.process_event(Pressed(true)).unwrap();
        assert!(sm.context().led);
        sm.process_event(Pressed(false)).unwrap();
        assert!(!sm.context().led);
    }
}

mod dispatch_table {
    use sml::{sml, utility::DispatchTable};
    #[derive(Clone, Copy)]
    struct RuntimeEvent;
    struct Event1;
    struct Event2;
    sml! { DispatchTableExample { *"idle"_s + event<Event1> = "s1"_s, "s1"_s + event<Event2> = X, } }
    struct Context;
    impl DispatchTableExampleStateMachineContext for Context {}
    type Handler = fn(&mut DispatchTableExampleStateMachine<Context>, &RuntimeEvent) -> bool;
    fn event1(sm: &mut DispatchTableExampleStateMachine<Context>, _: &RuntimeEvent) -> bool {
        sm.process_event(Event1).is_ok()
    }
    fn event2(sm: &mut DispatchTableExampleStateMachine<Context>, _: &RuntimeEvent) -> bool {
        sm.process_event(Event2).is_ok()
    }
    #[test]
    fn translated_behavior() {
        let handlers: [Handler; 2] = [event1, event2];
        let mut sm = DispatchTableExampleStateMachine::new(Context);
        let mut dispatch = DispatchTable::new(&mut sm, 1, &handlers);
        assert_eq!(dispatch.dispatch(&RuntimeEvent, 1), Some(true));
        assert_eq!(dispatch.dispatch(&RuntimeEvent, 2), Some(true));
        assert!(dispatch.machine().is_terminated());
    }
}

mod error_handling {
    use sml::sml;
    #[derive(Debug, PartialEq)]
    enum Failure {
        Specific,
        Generic,
    }
    struct Event1;
    struct Event2;
    struct SomeEvent;
    struct OtherEvent;
    sml! {
        ErrorHandlingExample {
            *"idle"_s + event<Event1> / fail_specific,
             "idle"_s + event<Event2> / fail_generic,
            *"exceptions handling"_s + exception<Failure> [is_specific] / catch_specific,
             "exceptions handling"_s + exception<_> / catch_generic = X,
            *"unexpected events handling"_s + unexpected_event<SomeEvent> / catch_some,
             "unexpected events handling"_s + unexpected_event<OtherEvent> / catch_other = X,
        }
    }
    #[derive(Default)]
    struct Context {
        failures: usize,
        unexpected: usize,
    }
    impl ErrorHandlingExampleStateMachineContext for Context {
        fn fail_specific(&mut self, _: &Event1) -> Result<(), Failure> {
            Err(Failure::Specific)
        }
        fn fail_generic(&mut self, _: &Event2) -> Result<(), Failure> {
            Err(Failure::Generic)
        }
        fn is_specific(&self, error: &Failure) -> Result<bool, Failure> {
            Ok(*error == Failure::Specific)
        }
        fn catch_specific(&mut self, _: &Failure) -> Result<(), Failure> {
            self.failures += 1;
            Ok(())
        }
        fn catch_generic(&mut self) -> Result<(), Failure> {
            self.failures += 1;
            Ok(())
        }
        fn catch_some(&mut self, _: &SomeEvent) -> Result<(), Failure> {
            self.unexpected += 1;
            Ok(())
        }
        fn catch_other(&mut self, _: &OtherEvent) -> Result<(), Failure> {
            self.unexpected += 1;
            Ok(())
        }
    }
    #[test]
    fn translated_behavior() {
        let mut sm = ErrorHandlingExampleStateMachine::new(Context::default());
        sm.process_event(Event1).unwrap();
        sm.process_event(Event2).unwrap();
        sm.process_event(SomeEvent).unwrap();
        sm.process_event(OtherEvent).unwrap();
        assert_eq!((sm.context().failures, sm.context().unexpected), (2, 2));
        assert!(sm.is_region(1, &ErrorHandlingExampleStates::X));
        assert!(sm.is_region(2, &ErrorHandlingExampleStates::X));
    }
}

mod plant_uml {
    use sml::sml;
    struct E1;
    struct E2;
    struct E3;
    sml! {
        PlantUmlExample {
            *"idle"_s + event<E1> = "s1"_s,
             "s1"_s + event<E2> [guard] / action = "s2"_s,
             "s2"_s + event<E3> = X,
        }
    }
    struct Context;
    impl PlantUmlExampleStateMachineContext for Context {
        fn guard(&self, _: &E2) -> Result<bool, ()> {
            Ok(true)
        }
        fn action(&mut self, _: &E2) -> Result<(), ()> {
            Ok(())
        }
    }
    #[test]
    fn translated_behavior_and_graphviz_input_compile() {
        let mut sm = PlantUmlExampleStateMachine::new(Context);
        sm.process_event(E1).unwrap();
        sm.process_event(E2).unwrap();
        sm.process_event(E3).unwrap();
        assert!(sm.is_terminated());
    }
}

mod sdl2 {
    use sml::{sml, utility::DispatchTable};
    const KEY_UP: usize = 1;
    const MOUSE_UP: usize = 2;
    const QUIT: usize = 3;
    #[derive(Clone, Copy)]
    struct SdlEvent {
        key: i32,
    }
    struct KeyUp(SdlEvent);
    struct MouseUp;
    struct Quit;
    sml! {
        Sdl2Example {
            *"idle"_s + event<KeyUp> [space] = "key pressed"_s,
             "key pressed"_s + event<MouseUp> = X,
            *"waiting for quit"_s + event<Quit> = X,
        }
    }
    struct Context;
    impl Sdl2ExampleStateMachineContext for Context {
        fn space(&self, e: &KeyUp) -> Result<bool, ()> {
            Ok(e.0.key == 32)
        }
    }
    type Handler = fn(&mut Sdl2ExampleStateMachine<Context>, &SdlEvent) -> bool;
    fn key(sm: &mut Sdl2ExampleStateMachine<Context>, e: &SdlEvent) -> bool {
        sm.process_event(KeyUp(*e)).is_ok()
    }
    fn mouse(sm: &mut Sdl2ExampleStateMachine<Context>, _: &SdlEvent) -> bool {
        sm.process_event(MouseUp).is_ok()
    }
    fn quit(sm: &mut Sdl2ExampleStateMachine<Context>, _: &SdlEvent) -> bool {
        sm.process_event(Quit).is_ok()
    }
    #[test]
    fn translated_behavior() {
        let handlers: [Handler; 3] = [key, mouse, quit];
        let mut sm = Sdl2ExampleStateMachine::new(Context);
        let mut dispatch = DispatchTable::new(&mut sm, KEY_UP, &handlers);
        dispatch.dispatch(&SdlEvent { key: 32 }, KEY_UP);
        dispatch.dispatch(&SdlEvent { key: 0 }, MOUSE_UP);
        dispatch.dispatch(&SdlEvent { key: 0 }, QUIT);
        assert!(dispatch.machine().is_terminated());
    }
}
