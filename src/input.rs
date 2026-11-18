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
    utils::{IsAlive, Logical, Point, Size, SERIAL_COUNTER},
};

use crate::{
    panel::LauncherItem,
    state::{OkayState, PointerGrabMode, ResizeEdges, SnapZone},
};

const KEY_F2_SYM: u32 = 0xffbf;
const KEY_ISO_LEFT_TAB_SYM: u32 = 0xfe20;
const KEY_BRACKETLEFT_SYM: u32 = 0x005b;
const KEY_BRACKETRIGHT_SYM: u32 = 0x005d;
const KEY_MINUS_SYM: u32 = 0x002d;
const KEY_EQUAL_SYM: u32 = 0x003d;
const KEY_PLUS_SYM: u32 = 0x002b;
const KEY_XF86_AUD_LOWER: u32 = 0x1008ff11;
const KEY_XF86_AUD_MUTE: u32 = 0x1008ff12;
const KEY_XF86_AUD_RAISE: u32 = 0x1008ff13;

/// Actions triggered by global compositor keybindings.
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyAction {
    SpawnTerminal,
    ToggleLauncher,
    ToggleOverview,
    ToggleSettings,
    ToggleClipboard,
    LockScreen,
    TakeScreenshot,
    AreaScreenshot,
    CancelAreaScreenshot,
    CloseFocused,
    ToggleFullscreen,
    ToggleTiling,
    ToggleAlwaysOnTop,
    MinimizeFocused,
    RestoreMinimized,
    CenterFocused,
    FocusNextPrev(bool),
    SwapTiled(bool),
    AdjustMasterRatio(i32),
    SnapFocused(SnapZone),
    SwitchWorkspace(usize),
    MoveToWorkspace(usize),
    CycleWorkspaceDir(i32),
    CycleFocusDir(bool),
    VolumeStep(i32),
    ToggleMute,
    LauncherTypeChar(char),
    LauncherBackspace,
    LauncherClose,
    UnlockAttempt,
    LockBackspace,
    LockTypeChar(char),
    NewFolderTypeChar(char),
    NewFolderBackspace,
    ConfirmNewFolder,
    CancelNewFolder,
    BeginRenameSelectedIcon,
    RenameIconTypeChar(char),
    RenameIconBackspace,
    ConfirmRenameIcon,
    CancelRenameIcon,
    WifiPwTypeChar(char),
    WifiPwBackspace,
    ConfirmWifiPw,
    CancelWifiPw,
    AdShieldTypeChar(char),
    AdShieldBackspace,
    AdShieldUnfocus,
    ClipboardTypeChar(char),
    ClipboardBackspace,
    CloseAnyOverlay,
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
                if raw_u32 == keysyms::KEY_Return {
                    return FilterResult::Intercept(KeyAction::UnlockAttempt);
                }
                if raw_u32 == keysyms::KEY_Escape {
                    if state.overlays.lock_input.is_empty() {
                        return FilterResult::Intercept(KeyAction::UnlockAttempt);
                    }
                    return FilterResult::Intercept(KeyAction::LockBackspace);
                }
                if raw_u32 == keysyms::KEY_BackSpace {
                    return FilterResult::Intercept(KeyAction::LockBackspace);
                }
                if (0x20..=0x7e).contains(&raw_u32) {
                    if let Some(ch) = char::from_u32(raw_u32) {
                        return FilterResult::Intercept(KeyAction::LockTypeChar(ch));
                    }
                }
                return FilterResult::Intercept(KeyAction::LockTypeChar('*'));
            }

            // Escape cancels active interactive area screenshot selection
            if state.overlays.screenshot_selecting && raw_u32 == keysyms::KEY_Escape {
                return FilterResult::Intercept(KeyAction::CancelAreaScreenshot);
            }

            // Inline desktop icon renaming (F2)
            if state.overlays.renaming_icon.is_some()
                && !modifiers.logo
                && !modifiers.ctrl
                && !modifiers.alt
            {
                if raw_u32 == keysyms::KEY_Return {
                    return FilterResult::Intercept(KeyAction::ConfirmRenameIcon);
                }
                if raw_u32 == keysyms::KEY_Escape {
                    return FilterResult::Intercept(KeyAction::CancelRenameIcon);
                }
                if raw_u32 == keysyms::KEY_BackSpace {
                    return FilterResult::Intercept(KeyAction::RenameIconBackspace);
                }
                if (0x20..=0x7e).contains(&raw_u32) {
                    if let Some(ch) = char::from_u32(raw_u32) {
                        if ch != '/' {
                            return FilterResult::Intercept(KeyAction::RenameIconTypeChar(ch));
                        }
                    }
                }
                return FilterResult::Intercept(KeyAction::RenameIconTypeChar('\0'));
            }

            // Inline Wi-Fi password prompt inside OKay Settings -> Wi-Fi & Network
            if state.settings.open
                && state.settings.wifi_password_prompt.is_some()
                && !modifiers.logo
                && !modifiers.ctrl
                && !modifiers.alt
            {
                if raw_u32 == keysyms::KEY_Return {
                    return FilterResult::Intercept(KeyAction::ConfirmWifiPw);
                }
                if raw_u32 == keysyms::KEY_Escape {
                    return FilterResult::Intercept(KeyAction::CancelWifiPw);
                }
                if raw_u32 == keysyms::KEY_BackSpace {
                    return FilterResult::Intercept(KeyAction::WifiPwBackspace);
                }
                if (0x20..=0x7e).contains(&raw_u32) {
                    if let Some(ch) = char::from_u32(raw_u32) {
                        return FilterResult::Intercept(KeyAction::WifiPwTypeChar(ch));
                    }
                }
                return FilterResult::Intercept(KeyAction::WifiPwTypeChar('\0'));
            }

            // Inline AdShield domain input inside OKay Settings -> AdShield & Privacy
            if state.settings.open
                && state.settings.active_tab == crate::settings::SettingsTab::AdShield
                && state.settings.adshield_input_focused
                && !modifiers.logo
                && !modifiers.ctrl
                && !modifiers.alt
            {
                if raw_u32 == keysyms::KEY_Escape || raw_u32 == keysyms::KEY_Return {
                    return FilterResult::Intercept(KeyAction::AdShieldUnfocus);
                }
                if raw_u32 == keysyms::KEY_BackSpace {
                    return FilterResult::Intercept(KeyAction::AdShieldBackspace);
                }
                if (0x21..=0x7e).contains(&raw_u32) {
                    if let Some(ch) = char::from_u32(raw_u32) {
                        return FilterResult::Intercept(KeyAction::AdShieldTypeChar(ch));
                    }
                }
                return FilterResult::Intercept(KeyAction::AdShieldTypeChar('\0'));
            }

            // Super + Return -> Spawn terminal
            if modifiers.logo && raw_u32 == keysyms::KEY_Return {
                return FilterResult::Intercept(KeyAction::SpawnTerminal);
            }

            // Super + D -> Toggle native app launcher menu
            if modifiers.logo && (raw_u32 == keysyms::KEY_d || raw_u32 == keysyms::KEY_D) {
                return FilterResult::Intercept(KeyAction::ToggleLauncher);
            }

            // Super + O -> Toggle live Window Overview / Expose
            if modifiers.logo && (raw_u32 == keysyms::KEY_o || raw_u32 == keysyms::KEY_O) {
                return FilterResult::Intercept(KeyAction::ToggleOverview);
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

            // Super + Shift + S -> Interactive Area Screenshot
            if modifiers.logo && modifiers.shift && (raw_u32 == keysyms::KEY_s || raw_u32 == keysyms::KEY_S) {
                return FilterResult::Intercept(KeyAction::AreaScreenshot);
            }

            // PrintScreen or Super + S -> Take Full Screenshot
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

            // Super + C -> Center focused window
            if modifiers.logo && (raw_u32 == keysyms::KEY_c || raw_u32 == keysyms::KEY_C) {
                return FilterResult::Intercept(KeyAction::CenterFocused);
            }

            // Super + [Shift] + M -> Restore last minimized / Minimize focused
            if modifiers.logo && (raw_u32 == keysyms::KEY_m || raw_u32 == keysyms::KEY_M) {
                if modifiers.shift {
                    return FilterResult::Intercept(KeyAction::RestoreMinimized);
                }
                return FilterResult::Intercept(KeyAction::MinimizeFocused);
            }

            // Super + [Shift] + J / K -> Swap or focus windows cleanly
            if modifiers.logo && (raw_u32 == keysyms::KEY_j || raw_u32 == keysyms::KEY_J) {
                if modifiers.shift {
                    return FilterResult::Intercept(KeyAction::SwapTiled(true));
                }
                return FilterResult::Intercept(KeyAction::FocusNextPrev(true));
            }
            if modifiers.logo && (raw_u32 == keysyms::KEY_k || raw_u32 == keysyms::KEY_K) {
                if modifiers.shift {
                    return FilterResult::Intercept(KeyAction::SwapTiled(false));
                }
                return FilterResult::Intercept(KeyAction::FocusNextPrev(false));
            }

            // Super + [ / - or ] / = / + -> Adjust Master-Stack split ratio
            if modifiers.logo
                && (raw_u32 == KEY_BRACKETLEFT_SYM || raw_u32 == KEY_MINUS_SYM)
            {
                return FilterResult::Intercept(KeyAction::AdjustMasterRatio(-5));
            }
            if modifiers.logo
                && (raw_u32 == KEY_BRACKETRIGHT_SYM
                    || raw_u32 == KEY_EQUAL_SYM
                    || raw_u32 == KEY_PLUS_SYM)
            {
                return FilterResult::Intercept(KeyAction::AdjustMasterRatio(5));
            }

            // Super + Shift + Left/Right -> Prev/Next Workspace; Super + Arrows -> Snap
            if modifiers.logo {
                if modifiers.shift && raw_u32 == keysyms::KEY_Left {
                    return FilterResult::Intercept(KeyAction::CycleWorkspaceDir(-1));
                }
                if modifiers.shift && raw_u32 == keysyms::KEY_Right {
                    return FilterResult::Intercept(KeyAction::CycleWorkspaceDir(1));
                }
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

            // Super + Tab or Alt + Tab (+Shift / ISO_Left_Tab) -> Cycle focus with switcher overlay
            if (modifiers.logo || modifiers.alt)
                && (raw_u32 == keysyms::KEY_Tab || raw_u32 == KEY_ISO_LEFT_TAB_SYM)
            {
                let forward = !modifiers.shift && raw_u32 != KEY_ISO_LEFT_TAB_SYM;
                return FilterResult::Intercept(KeyAction::CycleFocusDir(forward));
            }

            // Hardware media volume keys
            if raw_u32 == KEY_XF86_AUD_RAISE {
                return FilterResult::Intercept(KeyAction::VolumeStep(5));
            }
            if raw_u32 == KEY_XF86_AUD_LOWER {
                return FilterResult::Intercept(KeyAction::VolumeStep(-5));
            }
            if raw_u32 == KEY_XF86_AUD_MUTE {
                return FilterResult::Intercept(KeyAction::ToggleMute);
            }

            // F2 -> Rename currently selected desktop icon when no compositor modal is open
            if raw_u32 == KEY_F2_SYM
                && !modifiers.logo
                && !modifiers.ctrl
                && !modifiers.alt
                && state.overlays.selected_icons.len() == 1
                && !state.panel.launcher_open
                && !state.settings.open
                && !state.overlays.overview_open
            {
                return FilterResult::Intercept(KeyAction::BeginRenameSelectedIcon);
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

            // Inline new folder name editing on the desktop
            if state.overlays.creating_folder.is_some()
                && !modifiers.logo
                && !modifiers.ctrl
                && !modifiers.alt
            {
                if raw_u32 == keysyms::KEY_Return {
                    return FilterResult::Intercept(KeyAction::ConfirmNewFolder);
                }
                if raw_u32 == keysyms::KEY_Escape {
                    return FilterResult::Intercept(KeyAction::CancelNewFolder);
                }
                if raw_u32 == keysyms::KEY_BackSpace {
                    return FilterResult::Intercept(KeyAction::NewFolderBackspace);
                }
                if (0x20..=0x7e).contains(&raw_u32) {
                    if let Some(ch) = char::from_u32(raw_u32) {
                        if ch != '/' {
                            return FilterResult::Intercept(KeyAction::NewFolderTypeChar(ch));
                        }
                    }
                }
                return FilterResult::Intercept(KeyAction::NewFolderTypeChar('\0'));
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

            // Clipboard search typing when Super+V popup is open
            if state.overlays.clipboard_open && !modifiers.logo && !modifiers.ctrl && !modifiers.alt {
                if raw_u32 == keysyms::KEY_BackSpace {
                    return FilterResult::Intercept(KeyAction::ClipboardBackspace);
                }
                if (0x20..=0x7e).contains(&raw_u32) {
                    if let Some(ch) = char::from_u32(raw_u32) {
                        return FilterResult::Intercept(KeyAction::ClipboardTypeChar(ch));
                    }
                }
            }

            // Escape closes active compositor overlays / popovers
            if raw_u32 == keysyms::KEY_Escape
                && (state.overlays.overview_open
                    || state.overlays.clipboard_open
                    || state.overlays.window_context_menu.is_some()
                    || state.overlays.context_menu_pos.is_some()
                    || state.overlays.icon_context_menu.is_some()
                    || state.overlays.properties_modal.is_some()
                    || state.overlays.control_center_open
                    || state.overlays.calendar_open
                    || state.overlays.notif_drawer_open
                    || state.panel.top_bar_menu.is_some()
                    || state.panel.pin_context_menu.is_some())
            {
                return FilterResult::Intercept(KeyAction::CloseAnyOverlay);
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
            state.overlays.overview_open = false;
            state.panel.launcher_open = !state.panel.launcher_open;
            if state.panel.launcher_open {
                state.panel.search_query.clear();
            }
        }
        KeyAction::ToggleOverview => {
            state.panel.launcher_open = false;
            state.overlays.overview_open = !state.overlays.overview_open;
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
            state.overlays.lock_error = false;
            state.overlays.overview_open = false;
            state.panel.launcher_open = false;
        }
        KeyAction::TakeScreenshot => {
            crate::overlays::take_screenshot(state);
        }
        KeyAction::AreaScreenshot => {
            crate::overlays::begin_area_screenshot(state);
        }
        KeyAction::CancelAreaScreenshot => {
            state.overlays.screenshot_selecting = false;
            state.overlays.screenshot_start = None;
            state.overlays.screenshot_current = None;
        }
        KeyAction::CloseFocused => state.close_focused_window(),
        KeyAction::ToggleFullscreen => state.toggle_fullscreen_focused(),
        KeyAction::ToggleTiling => state.toggle_tiling(),
        KeyAction::ToggleAlwaysOnTop => state.toggle_always_on_top_focused(),
        KeyAction::MinimizeFocused => state.minimize_focused_window(),
        KeyAction::RestoreMinimized => state.restore_last_minimized(),
        KeyAction::CenterFocused => state.center_focused_window(),
        KeyAction::FocusNextPrev(forward) => state.focus_next_prev(forward),
        KeyAction::SwapTiled(forward) => state.swap_tiled_window(forward),
        KeyAction::AdjustMasterRatio(delta_pct) => {
            state.adjust_master_ratio(delta_pct as f32 / 100.0);
        }
        KeyAction::SnapFocused(zone) => {
            if let Some(win) = state.focused_window() {
                state.snap_window(&win, zone);
            }
        }
        KeyAction::SwitchWorkspace(ws) => state.switch_workspace(ws),
        KeyAction::MoveToWorkspace(ws) => state.move_focused_to_workspace(ws),
        KeyAction::CycleWorkspaceDir(delta) => {
            let count = state.workspaces.count.max(1) as isize;
            let cur_zero = state.workspaces.active as isize - 1;
            let next_zero = (cur_zero + delta as isize).rem_euclid(count);
            state.switch_workspace((next_zero + 1) as usize);
        }
        KeyAction::CycleFocusDir(forward) => {
            state.cycle_focus_dir(forward);
            state.overlays.alttab_visible_until =
                Some(Instant::now() + Duration::from_millis(1400));
        }
        KeyAction::VolumeStep(delta) => {
            state.panel.set_volume(state.panel.volume_percent + delta);
            let vol = state.panel.volume_percent;
            state
                .overlays
                .show_osd("Volume", &format!("{}%", vol), Some(vol));
        }
        KeyAction::ToggleMute => {
            state.panel.toggle_mute();
            let val = if state.panel.volume_muted {
                "Muted".to_string()
            } else {
                format!("{}%", state.panel.volume_percent)
            };
            let prog = if state.panel.volume_muted {
                Some(0)
            } else {
                Some(state.panel.volume_percent)
            };
            state.overlays.show_osd("Volume", &val, prog);
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
            state.overlays.attempt_unlock();
        }
        KeyAction::LockBackspace => {
            state.overlays.lock_input.pop();
            state.overlays.lock_error = false;
        }
        KeyAction::LockTypeChar(ch) => {
            if state.overlays.lock_input.len() < 32 {
                state.overlays.lock_input.push(ch);
                state.overlays.lock_error = false;
            }
        }
        KeyAction::NewFolderTypeChar(ch) => {
            if ch != '\0' {
                if let Some(ref mut cf) = state.overlays.creating_folder {
                    if cf.name.chars().count() < 40 {
                        cf.name.push(ch);
                    }
                }
            }
        }
        KeyAction::NewFolderBackspace => {
            if let Some(ref mut cf) = state.overlays.creating_folder {
                cf.name.pop();
            }
        }
        KeyAction::ConfirmNewFolder => {
            state.overlays.confirm_creating_folder();
        }
        KeyAction::CancelNewFolder => {
            state.overlays.cancel_creating_folder();
        }
        KeyAction::BeginRenameSelectedIcon => {
            if let Some(&idx) = state.overlays.selected_icons.first() {
                state.overlays.begin_renaming_icon(idx);
            }
        }
        KeyAction::RenameIconTypeChar(ch) => {
            if ch != '\0' {
                if let Some((_, ref mut name)) = state.overlays.renaming_icon {
                    if name.chars().count() < 40 {
                        name.push(ch);
                    }
                }
            }
        }
        KeyAction::RenameIconBackspace => {
            if let Some((_, ref mut name)) = state.overlays.renaming_icon {
                name.pop();
            }
        }
        KeyAction::ConfirmRenameIcon => {
            state.overlays.confirm_renaming_icon();
        }
        KeyAction::CancelRenameIcon => {
            state.overlays.cancel_renaming_icon();
        }
        KeyAction::WifiPwTypeChar(ch) => {
            if ch != '\0' {
                if let Some((_, ref mut pw)) = state.settings.wifi_password_prompt {
                    if pw.chars().count() < 64 {
                        pw.push(ch);
                    }
                }
            }
        }
        KeyAction::WifiPwBackspace => {
            if let Some((_, ref mut pw)) = state.settings.wifi_password_prompt {
                pw.pop();
            }
        }
        KeyAction::ConfirmWifiPw => {
            state.settings.confirm_wifi_password();
        }
        KeyAction::CancelWifiPw => {
            state.settings.cancel_wifi_password();
        }
        KeyAction::AdShieldTypeChar(ch) => {
            if ch != '\0' && state.settings.adshield_domain_input.chars().count() < 128 {
                state.settings.adshield_domain_input.push(ch);
            }
        }
        KeyAction::AdShieldBackspace => {
            state.settings.adshield_domain_input.pop();
        }
        KeyAction::AdShieldUnfocus => {
            state.settings.adshield_input_focused = false;
        }
        KeyAction::ClipboardTypeChar(ch) => {
            if ch != '\0' && state.overlays.clipboard_search.chars().count() < 64 {
                state.overlays.clipboard_search.push(ch);
            }
        }
        KeyAction::ClipboardBackspace => {
            state.overlays.clipboard_search.pop();
        }
        KeyAction::CloseAnyOverlay => {
            state.overlays.overview_open = false;
            state.overlays.clipboard_open = false;
            state.overlays.clipboard_search.clear();
            state.overlays.sysmon_popup_open = false;
            state.overlays.color_picker_active = false;
            state.overlays.window_context_menu = None;
            state.overlays.context_menu_pos = None;
            state.overlays.icon_context_menu = None;
            state.overlays.properties_modal = None;
            state.overlays.control_center_open = false;
            state.overlays.calendar_open = false;
            state.overlays.notif_drawer_open = false;
            state.panel.top_bar_menu = None;
            state.panel.pin_context_menu = None;
        }
        KeyAction::ExitCompositor => {
            tracing::info!("Super+Escape pressed — shutting down OKay DE");
            state.running = false;
            state.loop_signal.stop();
        }
    }
}

fn handle_pointer_motion<I: InputBackend>(state: &mut OkayState, event: I::PointerMotionEvent) {
    let accel_delta = event.delta();
    let unaccel_delta = event.delta_unaccel();
    // High-polling (2000Hz+) or jitter-filtered mice can produce NaN/0 in libinput adaptive accel
    // when two HID reports share close timestamps; fall back to unaccelerated deltas cleanly.
    let raw_delta = if state.pointer_accel_disabled {
        if unaccel_delta.x.is_finite() && unaccel_delta.y.is_finite() {
            unaccel_delta
        } else {
            accel_delta
        }
    } else if accel_delta.x.is_finite()
        && accel_delta.y.is_finite()
        && (accel_delta.x != 0.0
            || accel_delta.y != 0.0
            || (unaccel_delta.x == 0.0 && unaccel_delta.y == 0.0))
    {
        accel_delta
    } else {
        unaccel_delta
    };

    let dx = if raw_delta.x.is_finite() { raw_delta.x } else { 0.0 };
    let dy = if raw_delta.y.is_finite() { raw_delta.y } else { 0.0 };
    let sens = if state.pointer_sensitivity.is_finite() {
        state.pointer_sensitivity.clamp(0.1, 5.0)
    } else {
        1.0
    };
    let base_loc = if state.pointer_location.x.is_finite() && state.pointer_location.y.is_finite() {
        state.pointer_location
    } else {
        Point::from((0.0, 0.0))
    };
    let scaled_delta = Point::from((dx * sens, dy * sens));
    let new_pos = clamp_pointer_to_outputs(state, base_loc + scaled_delta);
    move_pointer_to(state, new_pos, Event::time_msec(&event));
}

fn handle_pointer_motion_absolute<I: InputBackend>(
    state: &mut OkayState,
    event: I::PointerMotionAbsoluteEvent,
) {
    let output_geo = state
        .output_under_pointer()
        .or_else(|| state.space.outputs().next().cloned())
        .and_then(|o| state.space.output_geometry(&o))
        .unwrap_or_else(|| smithay::utils::Rectangle::from_size((1280, 720).into()));

    let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
    let safe_pos = if pos.x.is_finite() && pos.y.is_finite() {
        pos
    } else {
        Point::from((
            output_geo.loc.x as f64 + output_geo.size.w as f64 * 0.5,
            output_geo.loc.y as f64 + output_geo.size.h as f64 * 0.5,
        ))
    };
    let clamped = clamp_pointer_to_outputs(state, safe_pos);
    move_pointer_to(state, clamped, Event::time_msec(&event));
}

fn move_pointer_to(state: &mut OkayState, pos: Point<f64, Logical>, time_msec: u32) {
    state.pointer_location = pos;

    if crate::settings::handle_settings_motion(state, pos) {
        state.cursor_status = smithay::input::pointer::CursorImageStatus::default_named();
        return;
    }
    if crate::overlays::handle_overlays_motion(state, pos) {
        state.cursor_status = smithay::input::pointer::CursorImageStatus::default_named();
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
                if !window.alive() {
                    state.pointer_grab = None;
                    state.overlays.snap_preview = None;
                    return;
                }
                let dx = (pos.x - initial_pointer.x).round() as i32;
                let dy = (pos.y - initial_pointer.y).round() as i32;
                let mut new_loc = Point::from((initial_window_loc.x + dx, initial_window_loc.y + dy));

                // Prevent moving the window or its titlebar under the top bar
                if !state.workspaces.is_fullscreen(&window) {
                    let win_geo = window.geometry();
                    let top_bar_h = if state.panel.dock_mode || state.panel.panel_top {
                        state.panel.topbar_h()
                    } else {
                        0
                    };
                    let out_top = state
                        .output_under_pointer()
                        .and_then(|o| state.space.output_geometry(&o))
                        .map(|g| g.loc.y)
                        .unwrap_or(0);
                    let min_y = out_top + top_bar_h + crate::state::TITLEBAR_HEIGHT - win_geo.loc.y;
                    if new_loc.y < min_y {
                        new_loc.y = min_y;
                    }
                }

                let win_id = window.toplevel().map(|t| t.wl_surface().id());
                if let Some((_, tb_rect)) = state.window_titlebar_under(pos, win_id.as_ref()) {
                    state.overlays.snap_preview = Some(tb_rect);
                } else {
                    state.overlays.snap_preview = state.detect_snap_zone(pos).map(|(_, r)| r);
                }
                state.space.map_element(window, new_loc, false);
                return;
            }
            PointerGrabMode::Resize {
                window,
                initial_pointer,
                initial_window_loc,
                initial_size,
                edges,
            } => {
                if !window.alive() {
                    state.pointer_grab = None;
                    return;
                }
                let dx = (pos.x - initial_pointer.x).round() as i32;
                let dy = (pos.y - initial_pointer.y).round() as i32;
                let mut new_x = initial_window_loc.x;
                let mut new_y = initial_window_loc.y;
                let mut new_w = initial_size.w;
                let mut new_h = initial_size.h;

                if edges.right {
                    new_w = (initial_size.w + dx).max(180);
                } else if edges.left {
                    let clamped_w = (initial_size.w - dx).max(180);
                    new_x = initial_window_loc.x + (initial_size.w - clamped_w);
                    new_w = clamped_w;
                }

                if edges.bottom {
                    new_h = (initial_size.h + dy).max(120);
                } else if edges.top {
                    let mut clamped_h = (initial_size.h - dy).max(120);
                    let mut candidate_y = initial_window_loc.y + (initial_size.h - clamped_h);
                    if !state.workspaces.is_fullscreen(&window) {
                        let win_geo = window.geometry();
                        let top_bar_h = if state.panel.dock_mode || state.panel.panel_top {
                            state.panel.topbar_h()
                        } else {
                            0
                        };
                        let out_top = state
                            .output_under_pointer()
                            .and_then(|o| state.space.output_geometry(&o))
                            .map(|g| g.loc.y)
                            .unwrap_or(0);
                        let min_y = out_top + top_bar_h + crate::state::TITLEBAR_HEIGHT - win_geo.loc.y;
                        if candidate_y < min_y {
                            candidate_y = min_y;
                            clamped_h = (initial_size.h - (candidate_y - initial_window_loc.y)).max(120);
                        }
                    }
                    new_y = candidate_y;
                    new_h = clamped_h;
                }

                if edges.left || edges.top {
                    state.space.map_element(window.clone(), (new_x, new_y), false);
                }

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
    if !pos.x.is_finite() || !pos.y.is_finite() {
        if let Some(geo) = state
            .space
            .outputs()
            .next()
            .and_then(|o| state.space.output_geometry(o))
        {
            return Point::from((
                geo.loc.x as f64 + geo.size.w as f64 * 0.5,
                geo.loc.y as f64 + geo.size.h as f64 * 0.5,
            ));
        }
        return Point::from((0.0, 0.0));
    }
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

        let super_held = state.super_pressed
            || state
                .seat
                .get_keyboard()
                .map(|k| k.modifier_state().logo)
                .unwrap_or(false);

        // Color Picker mode takes priority on click
        if state.overlays.color_picker_active {
            if button == 0x110 {
                state.pending_color_pick = Some(Point::from((pos.x.round() as i32, pos.y.round() as i32)));
                state.overlays.color_picker_active = false;
                return;
            } else if button == 0x111 {
                state.overlays.color_picker_active = false;
                return;
            }
        }

        // Middle-click (0x112) Pro-User Topbar shortcuts:
        // - Volume icon -> toggle Mute
        // - Clock -> toggle Do Not Disturb (DND)
        if button == 0x112 {
            let px = pos.x.round() as i32;
            let py = pos.y.round() as i32;
            if let Some(vr) = state.panel.topbar_volume_rect {
                if crate::panel::rect_contains(vr, px, py) {
                    state.panel.toggle_mute();
                    let val = if state.panel.volume_muted {
                        "Muted".to_string()
                    } else {
                        format!("{}%", state.panel.volume_percent)
                    };
                    state.overlays.show_osd(
                        "Volume",
                        &val,
                        if state.panel.volume_muted { Some(0) } else { Some(state.panel.volume_percent) },
                    );
                    return;
                }
            }
            if let Some(cr) = state.panel.topbar_clock_rect {
                if crate::panel::rect_contains(cr, px, py) {
                    state.overlays.dnd_enabled = !state.overlays.dnd_enabled;
                    state.overlays.show_osd(
                        "Do Not Disturb",
                        if state.overlays.dnd_enabled { "Enabled" } else { "Disabled" },
                        None,
                    );
                    return;
                }
            }
        }

        // Interactive Area Screenshot mode takes priority over all clicks
        if state.overlays.screenshot_selecting {
            if button == 0x110 && crate::overlays::handle_overlays_click(state, pos) {
                return;
            }
            if button == 0x111 {
                state.overlays.screenshot_selecting = false;
                state.overlays.screenshot_start = None;
                state.overlays.screenshot_current = None;
                return;
            }
        }

        // Super + Left Click (0x110) -> Drag-to-move window from anywhere on the window
        // Super + Right Click (0x111) -> Directional quadrant resize from anywhere on the window
        if super_held
            && (button == 0x110 || button == 0x111)
            && !state.overlays.locked
            && !state.overlays.overview_open
        {
            if let Some(win) = state.window_under(pos) {
                state.focus_window(&win);
                state.unmaximize_if_needed(&win);
                if button == 0x110 {
                    if let Some(win_loc) = state.space.element_location(&win) {
                        state.pointer_grab = Some(PointerGrabMode::Move {
                            window: win,
                            initial_pointer: pos,
                            initial_window_loc: win_loc,
                        });
                        return;
                    }
                } else if let Some(win_loc) = state.space.element_location(&win) {
                    let geo = win.geometry();
                    let cx = (win_loc.x + geo.loc.x) as f64 + geo.size.w as f64 * 0.5;
                    let cy = (win_loc.y + geo.loc.y) as f64 + geo.size.h as f64 * 0.5;
                    let edges = ResizeEdges {
                        left: pos.x < cx,
                        right: pos.x >= cx,
                        top: pos.y < cy,
                        bottom: pos.y >= cy,
                    };
                    state.pointer_grab = Some(PointerGrabMode::Resize {
                        window: win,
                        initial_pointer: pos,
                        initial_window_loc: win_loc,
                        initial_size: geo.size,
                        edges,
                    });
                    return;
                }
            }
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

        // Right-click (0x111) on Dock / Launcher -> Pin/Unpin Context Menu
        if button == 0x111 && !super_held && crate::panel::handle_panel_right_click(state, pos) {
            return;
        }

        // Right/Middle-click on server-side titlebar -> Window Menu / Always-on-Top
        if (button == 0x111 || button == 0x112)
            && !super_held
            && !state.overlays.overview_open
            && !state.overlays.locked
            && state.handle_titlebar_alt_click(pos, button == 0x111)
        {
            return;
        }

        // Right-click (0x111) on bare empty desktop -> Open Desktop Context Menu (never over windows/surfaces)
        if button == 0x111
            && !super_held
            && state.window_under(pos).is_none()
            && state.surface_under(pos).is_none()
            && crate::overlays::handle_desktop_right_click(state, pos)
        {
            return;
        }

        // Window outer border/corner resize without Super
        if button == 0x110 && !state.overlays.overview_open && !state.overlays.locked {
            if let Some((win, edges)) = state.window_resize_edges_at(pos) {
                state.focus_window(&win);
                state.unmaximize_if_needed(&win);
                if let Some(win_loc) = state.space.element_location(&win) {
                    let initial_size = win.geometry().size;
                    state.pointer_grab = Some(PointerGrabMode::Resize {
                        window: win,
                        initial_pointer: pos,
                        initial_window_loc: win_loc,
                        initial_size,
                        edges,
                    });
                    return;
                }
            }
        }

        // Server-side titlebar click (buttons [-] [+] [X], double-click maximize, or drag-to-move)
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
                    state.overlays.snap_preview = None;
                    let win_id = window.toplevel().map(|t| t.wl_surface().id());
                    if let Some((target, _)) =
                        state.window_titlebar_under(state.pointer_location, win_id.as_ref())
                    {
                        state.stack_window_onto(&window, &target);
                    } else if let Some((zone, _)) = state.detect_snap_zone(state.pointer_location) {
                        state.snap_window(&window, zone);
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

    // Pro-User Scroll Gestures on Topbar & Dock
    let raw_v = event.amount(Axis::Vertical).unwrap_or_else(|| {
        event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.0
    });
    if raw_v.abs() > 0.01 {
        let step_sign = if raw_v < 0.0 { 1 } else { -1 };
        let px = state.pointer_location.x.round() as i32;
        let py = state.pointer_location.y.round() as i32;
        let ctrl_held = state
            .seat
            .get_keyboard()
            .map(|k| k.modifier_state().ctrl)
            .unwrap_or(false);

        if ctrl_held {
            if let Some(dr) = state.panel.dock_area_rect {
                if crate::panel::rect_contains(dr, px, py) {
                    state.panel.dock_icon_size = (state.panel.dock_icon_size + step_sign * 2).clamp(28, 64);
                    state.retile_workspace();
                    state.blur_dirty = true;
                    state.panel.save_full_config(
                        state.pointer_sensitivity,
                        state.natural_scroll,
                        state.animations.enabled,
                        state.animations.speed,
                        state.animations.minimize_effect,
                        &state.settings.xkb_layout,
                    );
                    state.overlays.show_osd(
                        "Dock Size",
                        &format!("{} px", state.panel.dock_icon_size),
                        None,
                    );
                    return;
                }
            }
        }

        if let Some(vr) = state.panel.topbar_volume_rect {
            if crate::panel::rect_contains(vr, px, py) {
                let next_v = (state.panel.volume_percent + step_sign * 5).clamp(0, 100);
                state.panel.set_volume(next_v);
                state.overlays.show_osd("Volume", &format!("{}%", next_v), Some(next_v));
                return;
            }
        }
        if let Some(br) = state.panel.topbar_brightness_rect {
            if crate::panel::rect_contains(br, px, py) {
                state.panel.adjust_brightness(step_sign * 5);
                state.overlays.show_osd(
                    "Brightness",
                    &format!("{}%", state.panel.brightness_percent),
                    Some(state.panel.brightness_percent),
                );
                return;
            }
        }
    }
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
