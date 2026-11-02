use sml::sml;

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
    actions: usize,
}

macro_rules! action {
    ($name:ident, $event:ty) => {
        fn $name(&mut self, _event: &$event) -> Result<(), ()> {
            self.actions += 1;
            Ok(())
        }
    };
}

impl PlayerStateMachineContext for Context {
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
        self.actions += 1;
        Ok(())
    }

    fn exited_open(&mut self) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }
}

#[test]
fn cpp_shaped_cd_player_table_dispatches() {
    let mut sm = PlayerStateMachine::new(Context::default());

    sm.process_event(OpenClose).unwrap();
    assert!(matches!(sm.state(), PlayerStates::Open));
    sm.process_event(Play).unwrap();
    assert!(sm.is_terminated());
    assert_eq!(sm.context().actions, 4);
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
    entries: usize,
    exits: usize,
    actions: usize,
}

impl TransitionKindsStateMachineContext for TransitionContext {
    fn entered(&mut self) -> Result<(), ()> {
        self.entries += 1;
        Ok(())
    }

    fn exited(&mut self) -> Result<(), ()> {
        self.exits += 1;
        Ok(())
    }

    fn internal_action(&mut self, _event: &Ping) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }

    fn self_action(&mut self, _event: &Reset) -> Result<(), ()> {
        self.actions += 1;
        Ok(())
    }
}

#[test]
fn cpp_internal_and_self_transition_lifecycle_differs() {
    let mut sm = TransitionKindsStateMachine::new(TransitionContext::default());
    sm.initialize().unwrap();

    sm.process_event(Ping).unwrap();
    assert_eq!(sm.context().entries, 1);
    assert_eq!(sm.context().exits, 0);

    sm.process_event(Reset).unwrap();
    assert_eq!(sm.context().entries, 2);
    assert_eq!(sm.context().exits, 1);
    assert_eq!(sm.context().actions, 2);
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
    captured: Option<u32>,
    audits: usize,
}

impl EventDataStateMachineContext for EventContext {
    fn is_valid(&self, event: &Ack) -> Result<bool, ()> {
        Ok(event.valid)
    }

    fn capture(&mut self, event: &Ack) -> Result<(), ()> {
        self.captured = Some(event.id);
        Ok(())
    }

    fn audit(&mut self, _event: &Ack) -> Result<(), ()> {
        self.audits += 1;
        Ok(())
    }
}

#[test]
fn external_cpp_style_event_types_preserve_payloads() {
    let mut sm = EventDataStateMachine::new(EventContext::default());

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

    assert!(sm.is_terminated());
    assert_eq!(sm.context().captured, Some(42));
    assert_eq!(sm.context().audits, 1);
}

sml! {
    Anonymous {
        *"initial"_s / prepare = "ready"_s,
    }
}

#[derive(Default)]
struct AnonymousDslContext {
    prepared: bool,
}

impl AnonymousStateMachineContext for AnonymousDslContext {
    fn prepare(&mut self) -> Result<(), ()> {
        self.prepared = true;
        Ok(())
    }
}

#[test]
fn cpp_anonymous_transition_spelling_initializes() {
    let mut sm = AnonymousStateMachine::new(AnonymousDslContext::default());

    assert!(matches!(sm.initialize().unwrap(), AnonymousStates::Ready));
    assert!(sm.context().prepared);
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
    id: Option<u32>,
}

impl OriginCompletionStateMachineContext for OriginCompletionContext {
    fn finish_release(&mut self, event: &Release) -> Result<(), ()> {
        self.id = Some(event.id);
        Ok(())
    }
}

#[test]
fn cpp_origin_completion_preserves_external_event() {
    let mut sm = OriginCompletionStateMachine::new(OriginCompletionContext::default());

    sm.process_event(Release { id: 7 }).unwrap();

    assert!(sm.is_terminated());
    assert_eq!(sm.context().id, Some(7));
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
    recorded: bool,
}

impl DataActionSequenceStateMachineContext for DataActionSequenceContext {
    fn record_build(&mut self, _event: &Build) -> Result<(), ()> {
        self.recorded = true;
        Ok(())
    }

    fn make_value(&mut self, _event: &Build) -> Result<u32, ()> {
        Ok(42)
    }
}

#[test]
fn only_final_action_constructs_output_state_data() {
    let mut sm = DataActionSequenceStateMachine::new(DataActionSequenceContext::default());

    sm.process_event(Build).unwrap();

    assert!(matches!(sm.state(), DataActionSequenceStates::Ready(42)));
    assert!(sm.context().recorded);
}

sml! {
    NamedEvent {
        *"idle"_s + sml::on_entry<_> / named_entered,
         "idle"_s + "connected"_e = sml::X,
    }
}

#[derive(Default)]
struct NamedEventContext {
    entries: usize,
}
impl NamedEventStateMachineContext for NamedEventContext {
    fn named_entered(&mut self) -> Result<(), ()> {
        self.entries += 1;
        Ok(())
    }
}

#[test]
fn cpp_named_event_literal_is_accepted() {
    let mut sm = NamedEventStateMachine::new(NamedEventContext::default());
    sm.initialize().unwrap();
    assert_eq!(sm.context().entries, 1);
    assert!(sm.is(&NamedEventStates::Idle));
    sm.process_event(NamedEventEvents::Connected).unwrap();
    assert!(sm.is(&NamedEventStates::X));
    assert!(sm.is_terminated());
}
