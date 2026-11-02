use sml::sml;

/// Returns whether the machine's active root state matches `$pattern`.
macro_rules! in_state {
    ($machine:expr, $pattern:pat $(if $guard:expr)?) => {{
        let mut matched = false;
        $machine.visit_current_states(|state| matched |= matches!(state, $pattern $(if $guard)?));
        matched
    }};
}

pub struct OpenClose;
pub struct CdDetected;
pub struct Play;
pub struct Pause;
pub struct EndPause;
pub struct Stop;

sml! {
    Player {
         "open"_s <= *"empty"_s + event<OpenClose> / open_drawer,
         "open"_s + event<OpenClose> / close_drawer = "empty"_s,
         "empty"_s + event<CdDetected> / store_cd_info = "stopped"_s,
         "stopped"_s + event<Play> / start_playback = "playing"_s,
         "playing"_s + event<Pause> / pause_playback = "paused"_s,
         "paused"_s + event<EndPause> / resume_playback = "playing"_s,
         "playing"_s + event<Stop> / stop_playback = "stopped"_s,
         "paused"_s + event<Stop> / stop_playback = "stopped"_s,
         "stopped"_s + event<Stop> / stopped_again = "stopped"_s,
         "stopped"_s + event<OpenClose> / open_drawer = "open"_s,
         "paused"_s + event<OpenClose> / stop_and_open = "open"_s,
         "playing"_s + event<OpenClose> / stop_and_open = "open"_s,
         "open"_s + unexpected_event<Play> / invalid_play = X,
         "open"_s + on_entry<_> / entered_open,
         "open"_s + on_exit<_> / exited_open,
    }
}

#[derive(Default)]
struct Context {
    actions: core::cell::Cell<usize>,
}

macro_rules! action {
    ($name:ident, $event:ty) => {
        fn $name(&mut self, _event: &$event) -> Result<(), ()> {
            self.actions.set(self.actions.get() + 1);
            Ok(())
        }
    };
}

impl PlayerStateMachineContext for &Context {
    action!(open_drawer, OpenClose);
    action!(close_drawer, OpenClose);
    action!(store_cd_info, CdDetected);
    action!(start_playback, Play);
    action!(pause_playback, Pause);
    action!(resume_playback, EndPause);
    action!(stop_playback, Stop);
    action!(stopped_again, Stop);
    action!(stop_and_open, OpenClose);
    action!(invalid_play, Play);

    fn entered_open(&mut self) -> Result<(), ()> {
        self.actions.set(self.actions.get() + 1);
        Ok(())
    }

    fn exited_open(&mut self) -> Result<(), ()> {
        self.actions.set(self.actions.get() + 1);
        Ok(())
    }
}

#[test]
fn cpp_shaped_cd_player_table_dispatches() {
    let context = Context::default();
    let mut sm = PlayerStateMachine::new(&context);

    sm.process_event(OpenClose).unwrap();
    assert!(sm.is(&PlayerStates::Open));
    sm.process_event(Play).unwrap();
    assert!(sm.is(&PlayerStates::X));
    assert_eq!(context.actions.get(), 4);
}

pub struct Ping;
pub struct Reset;

sml! {
    TransitionKinds {
        *"idle"_s + on_entry<_> / entered,
         "idle"_s + on_exit<_> / exited,
         "idle"_s + event<Ping> / internal_action,
         "idle"_s + event<Reset> / self_action = "idle"_s,
    }
}

#[derive(Default)]
struct TransitionContext {
    entries: core::cell::Cell<usize>,
    exits: core::cell::Cell<usize>,
    actions: core::cell::Cell<usize>,
}

impl TransitionKindsStateMachineContext for &TransitionContext {
    fn entered(&mut self) -> Result<(), ()> {
        self.entries.set(self.entries.get() + 1);
        Ok(())
    }

    fn exited(&mut self) -> Result<(), ()> {
        self.exits.set(self.exits.get() + 1);
        Ok(())
    }

    fn internal_action(&mut self, _event: &Ping) -> Result<(), ()> {
        self.actions.set(self.actions.get() + 1);
        Ok(())
    }

    fn self_action(&mut self, _event: &Reset) -> Result<(), ()> {
        self.actions.set(self.actions.get() + 1);
        Ok(())
    }
}

#[test]
fn cpp_internal_and_self_transition_lifecycle_differs() {
    let context = TransitionContext::default();
    let mut sm = TransitionKindsStateMachine::new(&context);
    sm.initialize().unwrap();

    sm.process_event(Ping).unwrap();
    assert_eq!(context.entries.get(), 1);
    assert_eq!(context.exits.get(), 0);

    sm.process_event(Reset).unwrap();
    assert_eq!(context.entries.get(), 2);
    assert_eq!(context.exits.get(), 1);
    assert_eq!(context.actions.get(), 2);
}

pub struct Ack {
    valid: bool,
    id: u32,
}

sml! {
    EventData {
        *"waiting"_s + event<Ack> [is_valid] / (capture, audit) = X,
    }
}

#[derive(Default)]
struct EventContext {
    captured: core::cell::Cell<Option<u32>>,
    audits: core::cell::Cell<usize>,
}

impl EventDataStateMachineContext for &EventContext {
    fn is_valid(&self, event: &Ack) -> Result<bool, ()> {
        Ok(event.valid)
    }

    fn capture(&mut self, event: &Ack) -> Result<(), ()> {
        self.captured.set(Some(event.id));
        Ok(())
    }

    fn audit(&mut self, _event: &Ack) -> Result<(), ()> {
        self.audits.set(self.audits.get() + 1);
        Ok(())
    }
}

#[test]
fn external_cpp_style_event_types_preserve_payloads() {
    let context = EventContext::default();
    let mut sm = EventDataStateMachine::new(&context);

    assert!(sm
        .process_event(Ack {
            valid: false,
            id: 1,
        })
        .is_err());
    sm.process_event(Ack {
        valid: true,
        id: 42,
    })
    .unwrap();

    assert!(sm.is(&EventDataStates::X));
    assert_eq!(context.captured.get(), Some(42));
    assert_eq!(context.audits.get(), 1);
}

sml! {
    Anonymous {
        *"initial"_s / prepare = "ready"_s,
    }
}

#[derive(Default)]
struct AnonymousDslContext {
    prepared: core::cell::Cell<bool>,
}

impl AnonymousStateMachineContext for &AnonymousDslContext {
    fn prepare(&mut self) -> Result<(), ()> {
        self.prepared.set(true);
        Ok(())
    }
}

#[test]
fn cpp_anonymous_transition_spelling_initializes() {
    let context = AnonymousDslContext::default();
    let mut sm = AnonymousStateMachine::new(&context);

    assert!(matches!(sm.initialize().unwrap(), AnonymousStates::Ready));
    assert!(context.prepared.get());
}

#[derive(Clone)]
pub struct Release {
    id: u32,
}

sml! {
    OriginCompletion {
        *"idle"_s + event<Release> = "step1"_s,
         "step1"_s + completion<Release> / finish_release = X,
    }
}

#[derive(Default)]
struct OriginCompletionContext {
    id: core::cell::Cell<Option<u32>>,
}

impl OriginCompletionStateMachineContext for &OriginCompletionContext {
    fn finish_release(&mut self, event: &Release) -> Result<(), ()> {
        self.id.set(Some(event.id));
        Ok(())
    }
}

#[test]
fn cpp_origin_completion_preserves_external_event() {
    let context = OriginCompletionContext::default();
    let mut sm = OriginCompletionStateMachine::new(&context);

    sm.process_event(Release { id: 7 }).unwrap();

    assert!(sm.is(&OriginCompletionStates::X));
    assert_eq!(context.id.get(), Some(7));
}

pub struct Build;

sml! {
    DataActionSequence {
        *"idle"_s + event<Build> / (record_build, make_value) = "ready"_s(u32),
         "ready"_s(u32) + event<Build> = X,
    }
}

#[derive(Default)]
struct DataActionSequenceContext {
    recorded: core::cell::Cell<bool>,
}

impl DataActionSequenceStateMachineContext for &DataActionSequenceContext {
    fn record_build(&mut self, _event: &Build) -> Result<(), ()> {
        self.recorded.set(true);
        Ok(())
    }

    fn make_value(&mut self, _event: &Build) -> Result<u32, ()> {
        Ok(42)
    }
}

#[test]
fn only_final_action_constructs_output_state_data() {
    let context = DataActionSequenceContext::default();
    let mut sm = DataActionSequenceStateMachine::new(&context);

    sm.process_event(Build).unwrap();

    assert!(in_state!(sm, DataActionSequenceStates::Ready(42)));
    assert!(context.recorded.get());
}

sml! {
    NamedEvent {
        *"idle"_s + sml::on_entry<_> / named_entered,
         "idle"_s + "connected"_e = sml::X,
    }
}

#[derive(Default)]
struct NamedEventContext {
    entries: core::cell::Cell<usize>,
}
impl NamedEventStateMachineContext for &NamedEventContext {
    fn named_entered(&mut self) -> Result<(), ()> {
        self.entries.set(self.entries.get() + 1);
        Ok(())
    }
}

#[test]
fn cpp_named_event_literal_is_accepted() {
    let context = NamedEventContext::default();
    let mut sm = NamedEventStateMachine::new(&context);
    sm.initialize().unwrap();
    assert_eq!(context.entries.get(), 1);
    assert!(sm.is(&NamedEventStates::Idle));
    sm.process_event(NamedEventEvents::Connected).unwrap();
    assert!(sm.is(&NamedEventStates::X));
    assert!(sm.is(&NamedEventStates::X));
}
