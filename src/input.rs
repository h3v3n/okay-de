use std::time::{Duration, Instant};

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
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::Resource,
    },
    utils::{Logical, Point, Size, SERIAL_COUNTER},
};

use crate::{
    panel::LauncherItem,
    state::{OkayState, PointerGrabMode, SnapZone},
};

/// Actions triggered by global compositor keybindings.
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyAction {
    SpawnTerminal,
    ToggleLauncher,
    ToggleSettings,
    ToggleClipboard,
    LockScreen,
    TakeScreenshot,
    CloseFocused,
    ToggleFullscreen,
    ToggleTiling,
    ToggleAlwaysOnTop,
    SnapFocused(SnapZone),
    SwitchWorkspace(usize),
    MoveToWorkspace(usize),
    CycleFocus,
    LauncherTypeChar(char),
    LauncherBackspace,
    LauncherClose,
    UnlockAttempt,
    LockTypeChar(char),
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
        |state, modifiers, keysym| {
            state.super_pressed = modifiers.logo;
            if key_state != KeyState::Pressed {
                return FilterResult::Forward;
            }

            let raw_sym = keysym.modified_sym();
            let raw_u32 = raw_sym.raw();

            // Super + Escape -> Exit compositor cleanly
            if modifiers.logo && raw_u32 == keysyms::KEY_Escape {
                return FilterResult::Intercept(KeyAction::ExitCompositor);
            }

            // If session is locked, intercept all typing for the lockscreen
            if state.overlays.locked {
                if raw_u32 == keysyms::KEY_Return || raw_u32 == keysyms::KEY_Escape {
                    return FilterResult::Intercept(KeyAction::UnlockAttempt);
                }
                if (0x20..=0x7e).contains(&raw_u32) {
                    if let Some(ch) = char::from_u32(raw_u32) {
                        return FilterResult::Intercept(KeyAction::LockTypeChar(ch));
                    }
                }
                return FilterResult::Intercept(KeyAction::LockTypeChar('*'));
            }

            // Super + Return -> Spawn terminal
            if modifiers.logo && raw_u32 == keysyms::KEY_Return {
                return FilterResult::Intercept(KeyAction::SpawnTerminal);
            }

            // Super + D -> Toggle native app launcher menu
            if modifiers.logo && (raw_u32 == keysyms::KEY_d || raw_u32 == keysyms::KEY_D) {
                return FilterResult::Intercept(KeyAction::ToggleLauncher);
            }

            // Super + I -> Toggle native OKay Settings window
            if modifiers.logo && (raw_u32 == keysyms::KEY_i || raw_u32 == keysyms::KEY_I) {
                return FilterResult::Intercept(KeyAction::ToggleSettings);
            }

            // Super + V -> Toggle Clipboard History Manager
            if modifiers.logo && (raw_u32 == keysyms::KEY_v || raw_u32 == keysyms::KEY_V) {
                return FilterResult::Intercept(KeyAction::ToggleClipboard);
            }

            // Super + L -> Lock Screen
            if modifiers.logo && (raw_u32 == keysyms::KEY_l || raw_u32 == keysyms::KEY_L) {
                return FilterResult::Intercept(KeyAction::LockScreen);
            }

            // PrintScreen or Super + S -> Take Screenshot
            if raw_u32 == keysyms::KEY_Print
                || (modifiers.logo && (raw_u32 == keysyms::KEY_s || raw_u32 == keysyms::KEY_S))
            {
                return FilterResult::Intercept(KeyAction::TakeScreenshot);
            }

            // Super + Q -> Close focused window
            if modifiers.logo && (raw_u32 == keysyms::KEY_q || raw_u32 == keysyms::KEY_Q) {
                return FilterResult::Intercept(KeyAction::CloseFocused);
            }

            // Super + F -> Toggle fullscreen on focused window
            if modifiers.logo && (raw_u32 == keysyms::KEY_f || raw_u32 == keysyms::KEY_F) {
                return FilterResult::Intercept(KeyAction::ToggleFullscreen);
            }

            // Super + T -> Cycle layout mode (Floating / Master-Stack / Dwindle)
            if modifiers.logo && (raw_u32 == keysyms::KEY_t || raw_u32 == keysyms::KEY_T) {
                return FilterResult::Intercept(KeyAction::ToggleTiling);
            }

            // Super + A -> Toggle Always-on-Top
            if modifiers.logo && (raw_u32 == keysyms::KEY_a || raw_u32 == keysyms::KEY_A) {
                return FilterResult::Intercept(KeyAction::ToggleAlwaysOnTop);
            }

            // Super + Arrow Keys -> Snap window to halves / maximize
            if modifiers.logo {
                match raw_u32 {
                    keysyms::KEY_Left => {
                        return FilterResult::Intercept(KeyAction::SnapFocused(SnapZone::LeftHalf));
                    }
                    keysyms::KEY_Right => {
                        return FilterResult::Intercept(KeyAction::SnapFocused(SnapZone::RightHalf));
                    }
                    keysyms::KEY_Up => {
                        return FilterResult::Intercept(KeyAction::SnapFocused(SnapZone::Maximize));
                    }
                    keysyms::KEY_Down => {
                        return FilterResult::Intercept(KeyAction::SnapFocused(SnapZone::BottomHalf));
                    }
                    _ => {}
                }
            }

            // Super + Tab or Alt + Tab -> Cycle focus with live switcher overlay
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

            // Live search filter when Start Menu is open
            if state.panel.launcher_open && !modifiers.logo && !modifiers.ctrl && !modifiers.alt {
                if raw_u32 == keysyms::KEY_Escape {
                    return FilterResult::Intercept(KeyAction::LauncherClose);
                }
                if raw_u32 == keysyms::KEY_BackSpace {
                    return FilterResult::Intercept(KeyAction::LauncherBackspace);
                }
                if (0x20..=0x7e).contains(&raw_u32) {
                    if let Some(ch) = char::from_u32(raw_u32) {
                        return FilterResult::Intercept(KeyAction::LauncherTypeChar(ch));
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
        KeyAction::SpawnTerminal => {
            crate::panel::execute_launcher_item(state, LauncherItem::Terminal);
        }
        KeyAction::ToggleLauncher => {
            state.panel.launcher_open = !state.panel.launcher_open;
            if state.panel.launcher_open {
                state.panel.search_query.clear();
            }
        }
        KeyAction::ToggleSettings => {
            state.settings.open = !state.settings.open;
            if state.settings.open {
                state.settings.refresh_system_info();
            }
        }
        KeyAction::ToggleClipboard => {
            state.overlays.poll_clipboard_now();
            state.overlays.clipboard_open = !state.overlays.clipboard_open;
        }
        KeyAction::LockScreen => {
            state.overlays.locked = true;
            state.overlays.lock_input.clear();
        }
        KeyAction::TakeScreenshot => {
            crate::overlays::take_screenshot(state);
        }
        KeyAction::CloseFocused => state.close_focused_window(),
        KeyAction::ToggleFullscreen => state.toggle_fullscreen_focused(),
        KeyAction::ToggleTiling => state.toggle_tiling(),
        KeyAction::ToggleAlwaysOnTop => state.toggle_always_on_top_focused(),
        KeyAction::SnapFocused(zone) => {
            if let Some(win) = state.focused_window() {
                state.snap_window(&win, zone);
            }
        }
        KeyAction::SwitchWorkspace(ws) => state.switch_workspace(ws),
        KeyAction::MoveToWorkspace(ws) => state.move_focused_to_workspace(ws),
        KeyAction::CycleFocus => {
            state.cycle_focus();
            state.overlays.alttab_visible_until =
                Some(Instant::now() + Duration::from_millis(1400));
        }
        KeyAction::LauncherTypeChar(ch) => {
            if state.panel.search_query.len() < 24 {
                state.panel.search_query.push(ch);
            }
        }
        KeyAction::LauncherBackspace => {
            state.panel.search_query.pop();
        }
        KeyAction::LauncherClose => {
            state.panel.launcher_open = false;
            state.panel.search_query.clear();
        }
        KeyAction::UnlockAttempt => {
            state.overlays.locked = false;
            state.overlays.lock_input.clear();
        }
        KeyAction::LockTypeChar(ch) => {
            if state.overlays.lock_input.len() < 32 {
                state.overlays.lock_input.push(ch);
            }
        }
        KeyAction::ExitCompositor => {
            tracing::info!("Super+Escape pressed — shutting down OKay DE");
            state.running = false;
            state.loop_signal.stop();
        }
    }
}

fn handle_pointer_motion<I: InputBackend>(state: &mut OkayState, event: I::PointerMotionEvent) {
    let raw_delta = event.delta();
    let scaled_delta = Point::from((
        raw_delta.x * state.pointer_sensitivity,
        raw_delta.y * state.pointer_sensitivity,
    ));
    let new_pos = clamp_pointer_to_outputs(state, state.pointer_location + scaled_delta);
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

    if crate::settings::handle_settings_motion(state, pos) {
        return;
    }
    if crate::overlays::handle_overlays_motion(state, pos) {
        return;
    }

    // Handle active window drag-to-move / drag-to-resize grab
    if let Some(grab) = state.pointer_grab.clone() {
        match grab {
            PointerGrabMode::Move {
                window,
                initial_pointer,
                initial_window_loc,
            } => {
                let dx = (pos.x - initial_pointer.x).round() as i32;
                let dy = (pos.y - initial_pointer.y).round() as i32;
                let new_loc = Point::from((initial_window_loc.x + dx, initial_window_loc.y + dy));
                state.space.map_element(window, new_loc, false);
                return;
            }
            PointerGrabMode::Resize {
                window,
                initial_pointer,
                initial_size,
            } => {
                let dx = (pos.x - initial_pointer.x).round() as i32;
                let dy = (pos.y - initial_pointer.y).round() as i32;
                let new_w = (initial_size.w + dx).max(160);
                let new_h = (initial_size.h + dy).max(100);
                if let Some(toplevel) = window.toplevel() {
                    toplevel.with_pending_state(|s| {
                        s.states.set(xdg_toplevel::State::Resizing);
                        s.size = Some(Size::from((new_w, new_h)));
                    });
                    toplevel.send_configure();
                }
                return;
            }
        }
    }

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
        if state.overlays.locked {
            return;
        }

        // BTN_LEFT (0x110 = 272): check native taskbar, launcher, settings & overlays first
        if button == 0x110 && crate::panel::handle_panel_click(state, pos) {
            return;
        }
        if button == 0x110 && crate::settings::handle_settings_click(state, pos) {
            return;
        }
        if button == 0x110 && crate::overlays::handle_overlays_click(state, pos) {
            return;
        }

        // Right-click (0x111) on empty desktop -> Open Desktop Context Menu
        if button == 0x111 && !state.super_pressed && crate::overlays::handle_desktop_right_click(state, pos) {
            return;
        }

        // Super + Left Click (0x110) -> Drag-to-move window
        // Super + Right Click (0x111) -> Drag-to-resize window
        if state.super_pressed && (button == 0x110 || button == 0x111) {
            if let Some(win) = state.window_under(pos) {
                state.focus_window(&win);
                if button == 0x110 {
                    if let Some(win_loc) = state.space.element_location(&win) {
                        state.pointer_grab = Some(PointerGrabMode::Move {
                            window: win,
                            initial_pointer: pos,
                            initial_window_loc: win_loc,
                        });
                        return;
                    }
                } else {
                    let initial_size = win.geometry().size;
                    state.pointer_grab = Some(PointerGrabMode::Resize {
                        window: win,
                        initial_pointer: pos,
                        initial_size,
                    });
                    return;
                }
            }
        }

        // Server-side titlebar click (buttons [-] [+] [X] or drag-to-move)
        if button == 0x110 && state.handle_titlebar_click(pos) {
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
        } else if button == 0x110 && state.window_under(pos).is_none() {
            crate::overlays::start_desktop_rubberband(state, pos);
        }
    } else if btn_state == ButtonState::Released {
        if crate::settings::handle_settings_release(state) {
            return;
        }
        if crate::overlays::handle_overlays_release(state) {
            return;
        }
        if let Some(grab) = state.pointer_grab.take() {
            match grab {
                PointerGrabMode::Move { window, .. } => {
                    // Check if released at a screen edge/corner to trigger snap-to-edge/quarter
                    if let Some(out) = state.output_under_pointer() {
                        if let Some(geo) = state.space.output_geometry(&out) {
                            let px = state.pointer_location.x.round() as i32;
                            let py = state.pointer_location.y.round() as i32;
                            let left = px <= geo.loc.x + 12;
                            let right = px >= geo.loc.x + geo.size.w - 14;
                            let top = py <= geo.loc.y + 12;
                            let bot = py >= geo.loc.y + geo.size.h - 14;

                            if left && top {
                                state.snap_window(&window, SnapZone::TopLeftQuarter);
                            } else if right && top {
                                state.snap_window(&window, SnapZone::TopRightQuarter);
                            } else if left && bot {
                                state.snap_window(&window, SnapZone::BottomLeftQuarter);
                            } else if right && bot {
                                state.snap_window(&window, SnapZone::BottomRightQuarter);
                            } else if left {
                                state.snap_window(&window, SnapZone::LeftHalf);
                            } else if right {
                                state.snap_window(&window, SnapZone::RightHalf);
                            } else if top {
                                state.snap_window(&window, SnapZone::Maximize);
                            }
                        }
                    }
                }
                PointerGrabMode::Resize { window, .. } => {
                    if let Some(toplevel) = window.toplevel() {
                        toplevel.with_pending_state(|s| {
                            s.states.unset(xdg_toplevel::State::Resizing);
                        });
                        toplevel.send_configure();
                    }
                }
            }
            return;
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
    let dir = if state.natural_scroll { -1.0 } else { 1.0 };
    let horizontal_amount = event.amount(Axis::Horizontal).unwrap_or_else(|| {
        event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.0
    }) * dir;
    let vertical_amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
        event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.0
    }) * dir;
    let horizontal_discrete = event.amount_v120(Axis::Horizontal).map(|v| v * dir);
    let vertical_discrete = event.amount_v120(Axis::Vertical).map(|v| v * dir);

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
