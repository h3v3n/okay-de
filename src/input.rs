use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend,
        InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
        PointerMotionEvent,
    },
    input::{
        keyboard::{keysyms, FilterResult},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    reexports::wayland_server::Resource,
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::state::OkayState;

/// Actions triggered by global compositor keybindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyAction {
    SpawnTerminal,
    CloseFocused,
    SwitchWorkspace(usize),
    MoveToWorkspace(usize),
    CycleFocus,
    ExitCompositor,
}

/// Processes any Smithay `InputEvent` (from libinput or Winit).
pub fn process_input_event<I: InputBackend>(state: &mut OkayState, event: InputEvent<I>) {
    match event {
        InputEvent::Keyboard { event } => handle_keyboard_event::<I>(state, event),
        InputEvent::PointerMotion { event } => handle_pointer_motion::<I>(state, event),
        InputEvent::PointerMotionAbsolute { event } => handle_pointer_motion_absolute::<I>(state, event),
        InputEvent::PointerButton { event } => handle_pointer_button::<I>(state, event),
        InputEvent::PointerAxis { event } => handle_pointer_axis::<I>(state, event),
        _ => {}
    }
}

fn handle_keyboard_event<I: InputBackend>(state: &mut OkayState, event: I::KeyboardKeyEvent) {
    let serial = SERIAL_COUNTER.next_serial();
    let time = Event::time_msec(&event);
    let keycode = event.key_code();
    let key_state = event.state();

    let Some(keyboard) = state.seat.get_keyboard() else {
        return;
    };

    let action = keyboard.input::<KeyAction, _>(
        state,
        keycode,
        key_state,
        serial,
        time,
        |_state, modifiers, keysym| {
            if key_state != KeyState::Pressed {
                return FilterResult::Forward;
            }

            let raw_sym = keysym.modified_sym();
            let raw_u32 = raw_sym.raw();

            // Super + Escape -> Exit compositor cleanly
            if modifiers.logo && raw_u32 == keysyms::KEY_Escape {
                return FilterResult::Intercept(KeyAction::ExitCompositor);
            }

            // Super + Return -> Spawn terminal
            if modifiers.logo && raw_u32 == keysyms::KEY_Return {
                return FilterResult::Intercept(KeyAction::SpawnTerminal);
            }

            // Super + Q -> Close focused window
            if modifiers.logo && (raw_u32 == keysyms::KEY_q || raw_u32 == keysyms::KEY_Q) {
                return FilterResult::Intercept(KeyAction::CloseFocused);
            }

            // Super + Tab or Alt + Tab -> Cycle focus
            if (modifiers.logo || modifiers.alt) && raw_u32 == keysyms::KEY_Tab {
                return FilterResult::Intercept(KeyAction::CycleFocus);
            }

            // Super + [Shift] + 1..9 -> Workspace switching / moving
            if modifiers.logo {
                if let Some(ws) = keysym_to_workspace(raw_u32) {
                    if modifiers.shift {
                        return FilterResult::Intercept(KeyAction::MoveToWorkspace(ws));
                    } else {
                        return FilterResult::Intercept(KeyAction::SwitchWorkspace(ws));
                    }
                }
            }

            FilterResult::Forward
        },
    );

    if let Some(action) = action {
        execute_key_action(state, action);
    }
}

fn keysym_to_workspace(sym: u32) -> Option<usize> {
    match sym {
        keysyms::KEY_1 | keysyms::KEY_exclam => Some(1),
        keysyms::KEY_2 | keysyms::KEY_at => Some(2),
        keysyms::KEY_3 | keysyms::KEY_numbersign => Some(3),
        keysyms::KEY_4 | keysyms::KEY_dollar => Some(4),
        keysyms::KEY_5 | keysyms::KEY_percent => Some(5),
        keysyms::KEY_6 | keysyms::KEY_asciicircum => Some(6),
        keysyms::KEY_7 | keysyms::KEY_ampersand => Some(7),
        keysyms::KEY_8 | keysyms::KEY_asterisk => Some(8),
        keysyms::KEY_9 | keysyms::KEY_parenleft => Some(9),
        _ => None,
    }
}

fn execute_key_action(state: &mut OkayState, action: KeyAction) {
    match action {
        KeyAction::SpawnTerminal => spawn_terminal(&state.socket_name),
        KeyAction::CloseFocused => state.close_focused_window(),
        KeyAction::SwitchWorkspace(ws) => state.switch_workspace(ws),
        KeyAction::MoveToWorkspace(ws) => state.move_focused_to_workspace(ws),
        KeyAction::CycleFocus => state.cycle_focus(),
        KeyAction::ExitCompositor => {
            tracing::info!("Super+Escape pressed — shutting down OKay DE");
            state.running = false;
            state.loop_signal.stop();
        }
    }
}

fn spawn_terminal(socket_name: &std::ffi::OsStr) {
    crate::panel::spawn_terminal(socket_name);
}

fn handle_pointer_motion<I: InputBackend>(state: &mut OkayState, event: I::PointerMotionEvent) {
    let delta = event.delta();
    let new_pos = clamp_pointer_to_outputs(state, state.pointer_location + delta);
    move_pointer_to(state, new_pos, Event::time_msec(&event));
}

fn handle_pointer_motion_absolute<I: InputBackend>(
    state: &mut OkayState,
    event: I::PointerMotionAbsoluteEvent,
) {
    let output_size = state
        .space
        .outputs()
        .next()
        .and_then(|o| state.space.output_geometry(o))
        .map(|g| g.size)
        .unwrap_or_else(|| (1280, 720).into());

    let pos = event.position_transformed(output_size);
    let clamped = clamp_pointer_to_outputs(state, pos);
    move_pointer_to(state, clamped, Event::time_msec(&event));
}

fn move_pointer_to(state: &mut OkayState, pos: Point<f64, Logical>, time_msec: u32) {
    state.pointer_location = pos;
    let under = state.surface_under(pos);

    if let Some(pointer) = state.seat.get_pointer() {
        pointer.motion(
            state,
            under,
            &MotionEvent {
                location: pos,
                serial: SERIAL_COUNTER.next_serial(),
                time: time_msec,
            },
        );
        pointer.frame(state);
    }
}

fn clamp_pointer_to_outputs(
    state: &OkayState,
    pos: Point<f64, Logical>,
) -> Point<f64, Logical> {
    let mut best_clamped: Option<(f64, Point<f64, Logical>)> = None;

    for output in state.space.outputs() {
        let Some(geo) = state.space.output_geometry(output) else {
            continue;
        };
        if geo.to_f64().contains(pos) {
            return pos;
        }

        let max_x = (geo.loc.x + geo.size.w - 1).max(geo.loc.x) as f64;
        let max_y = (geo.loc.y + geo.size.h - 1).max(geo.loc.y) as f64;
        let clamped = Point::from((
            pos.x.clamp(geo.loc.x as f64, max_x),
            pos.y.clamp(geo.loc.y as f64, max_y),
        ));
        let dx = pos.x - clamped.x;
        let dy = pos.y - clamped.y;
        let dist_sq = dx * dx + dy * dy;

        if best_clamped.map(|(d, _)| dist_sq < d).unwrap_or(true) {
            best_clamped = Some((dist_sq, clamped));
        }
    }

    best_clamped
        .map(|(_, p)| p)
        .unwrap_or_else(|| Point::from((pos.x.max(0.0), pos.y.max(0.0))))
}

fn handle_pointer_button<I: InputBackend>(state: &mut OkayState, event: I::PointerButtonEvent) {
    let serial = SERIAL_COUNTER.next_serial();
    let button = event.button_code();
    let btn_state = event.state();

    if btn_state == ButtonState::Pressed {
        let pos = state.pointer_location;
        // BTN_LEFT (0x110 = 272): check native taskbar & launcher popup first
        if button == 0x110 && crate::panel::handle_panel_click(state, pos) {
            return;
        }
        if let Some((surface, _)) = state.surface_under(pos) {
            let matching_window = state
                .space
                .elements()
                .find(|w| {
                    state.workspaces.is_visible(w)
                        && w.toplevel()
                            .map(|t| t.wl_surface().id() == surface.id())
                            .unwrap_or(false)
                })
                .cloned();

            if let Some(win) = matching_window {
                state.focus_window(&win);
            } else if let Some(keyboard) = state.seat.get_keyboard() {
                keyboard.set_focus(state, Some(surface), serial);
            }
        }
    }

    if let Some(pointer) = state.seat.get_pointer() {
        pointer.button(
            state,
            &ButtonEvent {
                button,
                state: btn_state,
                serial,
                time: Event::time_msec(&event),
            },
        );
        pointer.frame(state);
    }
}

fn handle_pointer_axis<I: InputBackend>(state: &mut OkayState, event: I::PointerAxisEvent) {
    let source = event.source();
    let horizontal_amount = event.amount(Axis::Horizontal).unwrap_or_else(|| {
        event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.0
    });
    let vertical_amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
        event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.0
    });
    let horizontal_discrete = event.amount_v120(Axis::Horizontal);
    let vertical_discrete = event.amount_v120(Axis::Vertical);

    let mut frame = AxisFrame::new(Event::time_msec(&event)).source(source);
    if horizontal_amount != 0.0 {
        frame = frame.value(Axis::Horizontal, horizontal_amount);
        if let Some(discrete) = horizontal_discrete {
            frame = frame.v120(Axis::Horizontal, discrete as i32);
        }
    }
    if vertical_amount != 0.0 {
        frame = frame.value(Axis::Vertical, vertical_amount);
        if let Some(discrete) = vertical_discrete {
            frame = frame.v120(Axis::Vertical, discrete as i32);
        }
    }
    if source == AxisSource::Finger {
        if event.amount(Axis::Horizontal) == Some(0.0) {
            frame = frame.stop(Axis::Horizontal);
        }
        if event.amount(Axis::Vertical) == Some(0.0) {
            frame = frame.stop(Axis::Vertical);
        }
    }

    if let Some(pointer) = state.seat.get_pointer() {
        pointer.axis(state, frame);
        pointer.frame(state);
    }
}
