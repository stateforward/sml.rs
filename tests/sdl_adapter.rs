use sml::sml;
use sml::utility::DispatchTable;

const SDL_KEYUP: usize = 1;
const SDL_MOUSEBUTTONUP: usize = 2;
const SDL_QUIT: usize = 3;
const SDLK_SPACE: i32 = 32;

#[derive(Clone, Copy)]
pub struct SdlEvent {
    key: i32,
}

pub struct KeyUp(SdlEvent);
pub struct MouseButtonUp(SdlEvent);
pub struct Quit(SdlEvent);

sml! {
    Sdl2 {
        *"idle"_s / initialize = "wait for user input"_s,
         "wait for user input"_s + event<KeyUp> [is_space] / space_pressed = "key pressed"_s,
         "key pressed"_s + event<MouseButtonUp> / mouse_pressed = X,

        *"waiting for quit"_s + event<Quit> / quit = X,
    }
}

#[derive(Default)]
struct Context {
    initialized: bool,
    space_pressed: bool,
    mouse_pressed: bool,
    quit: bool,
}

impl Sdl2StateMachineContext for Context {
    fn initialize(&mut self) -> Result<(), ()> {
        self.initialized = true;
        Ok(())
    }

    fn is_space(&self, event: &KeyUp) -> Result<bool, ()> {
        Ok(event.0.key == SDLK_SPACE)
    }

    fn space_pressed(&mut self, _: &KeyUp) -> Result<(), ()> {
        self.space_pressed = true;
        Ok(())
    }

    fn mouse_pressed(&mut self, event: &MouseButtonUp) -> Result<(), ()> {
        let _ = event.0;
        self.mouse_pressed = true;
        Ok(())
    }

    fn quit(&mut self, event: &Quit) -> Result<(), ()> {
        let _ = event.0;
        self.quit = true;
        Ok(())
    }
}

type Handler = fn(&mut Sdl2StateMachine<Context>, &SdlEvent) -> bool;

fn key_up(machine: &mut Sdl2StateMachine<Context>, event: &SdlEvent) -> bool {
    machine.process_event(KeyUp(*event)).is_ok()
}

fn mouse_button_up(machine: &mut Sdl2StateMachine<Context>, event: &SdlEvent) -> bool {
    machine.process_event(MouseButtonUp(*event)).is_ok()
}

fn quit(machine: &mut Sdl2StateMachine<Context>, event: &SdlEvent) -> bool {
    machine.process_event(Quit(*event)).is_ok()
}

#[test]
fn sdl_runtime_ids_dispatch_into_native_orthogonal_regions() {
    let handlers: [Handler; 3] = [key_up, mouse_button_up, quit];
    let mut machine = Sdl2StateMachine::new(Context::default());
    machine.initialize().unwrap();
    let mut dispatch = DispatchTable::new(&mut machine, SDL_KEYUP, &handlers);

    assert_eq!(
        dispatch.dispatch(&SdlEvent { key: SDLK_SPACE }, SDL_KEYUP),
        Some(true)
    );
    assert_eq!(
        dispatch.dispatch(&SdlEvent { key: 0 }, SDL_MOUSEBUTTONUP),
        Some(true)
    );
    assert_eq!(
        dispatch.dispatch(&SdlEvent { key: 0 }, SDL_QUIT),
        Some(true)
    );
    assert!(dispatch.machine().is_terminated());
    assert!(dispatch.machine().context().initialized);
    assert!(dispatch.machine().context().space_pressed);
    assert!(dispatch.machine().context().mouse_pressed);
    assert!(dispatch.machine().context().quit);
}
