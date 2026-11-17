use std::{
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Read},
    os::fd::OwnedFd,
    path::PathBuf,
    process::Command,
    sync::{mpsc, OnceLock},
    thread,
};

use calloop::channel::Sender as CalloopSender;

use crate::{
    icons,
    settings::{BluetoothDevice, SystemInfoSnapshot, WifiNetwork},
    state::OkayState,
};

#[derive(Debug)]
pub enum WorkerRequest {
    LoadIcon {
        name: String,
        target_size: i32,
    },
    RefreshWifi {
        force_rescan: bool,
    },
    ConnectWifi {
        ssid: String,
        password: Option<String>,
    },
    DisconnectWifi {
        ssid: String,
    },
    RefreshBluetooth,
    ToggleBluetoothPower {
        enable: bool,
    },
    ToggleBluetoothDevice {
        mac: String,
        connect: bool,
    },
    RefreshAudio,
    SetAudioVolume {
        percent: i32,
    },
    ToggleAudioMute,
    SetDefaultAudioSink {
        sink_id: String,
    },
    RefreshSystemInfo,
    ScanDesktopApps,
    PollExternalClipboard,
    SetExternalClipboard {
        text: String,
    },
    ReadClipboardPipe {
        fd: OwnedFd,
    },
    TakeScreenshot {
        path: PathBuf,
        socket_name: OsString,
        fallback_png: Option<Vec<u8>>,
    },
    SaveRgbaPng {
        path: PathBuf,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    PollPanelStatus,
    RunCommand {
        program: String,
        args: Vec<String>,
    },
}

#[derive(Debug)]
pub enum WorkerResponse {
    IconLoaded,
    WifiRefreshed {
        networks: Vec<WifiNetwork>,
        status: String,
    },
    BluetoothRefreshed {
        devices: Vec<BluetoothDevice>,
        status: String,
        powered: bool,
    },
    AudioRefreshed {
        volume_percent: Option<i32>,
        volume_muted: Option<bool>,
        sinks: Vec<crate::system_backends::AudioSink>,
    },
    SystemInfoRefreshed(SystemInfoSnapshot),
    DesktopAppsScanned(Vec<crate::panel::DesktopAppEntry>),
    ClipboardText {
        text: String,
    },
    PanelStatusPolled {
        battery_percent: Option<i32>,
        volume_percent: Option<i32>,
        volume_muted: Option<bool>,
        active_wifi_ssid: Option<String>,
    },
    ScreenshotDone {
        path: PathBuf,
        ok: bool,
        via_grim: bool,
    },
}

static WORKER_TX: OnceLock<mpsc::Sender<WorkerRequest>> = OnceLock::new();

/// Spawns the background worker thread and registers its request sender globally.
pub fn init_worker(resp_tx: CalloopSender<WorkerResponse>) {
    let (req_tx, req_rx) = mpsc::channel::<WorkerRequest>();
    let _ = WORKER_TX.set(req_tx);

    thread::Builder::new()
        .name("okay-worker".to_string())
        .spawn(move || worker_loop(req_rx, resp_tx))
        .expect("Failed to spawn okay-worker thread");
}

/// Dispatches a non-blocking request to the background worker thread.
pub fn dispatch(req: WorkerRequest) {
    if let Some(tx) = WORKER_TX.get() {
        let _ = tx.send(req);
    }
}

/// Handles responses from the worker thread inside the main `calloop` event loop.
pub fn handle_response(state: &mut OkayState, resp: WorkerResponse) {
    match resp {
        WorkerResponse::IconLoaded => {
            // ICON_CACHE is already populated by the worker; next frame picks it up.
        }
        WorkerResponse::WifiRefreshed { networks, status } => {
            state.settings.wifi_networks = networks;
            state.settings.wifi_status = status;
        }
        WorkerResponse::BluetoothRefreshed {
            devices,
            status,
            powered,
        } => {
            state.settings.bt_devices = devices;
            state.settings.bt_status = status;
            state.settings.bt_powered = powered;
        }
        WorkerResponse::AudioRefreshed {
            volume_percent,
            volume_muted,
            sinks,
        } => {
            if let Some(v) = volume_percent {
                if !state.overlays.cc_dragging_volume && !state.settings.is_dragging_slider() {
                    state.panel.volume_percent = v.clamp(0, 100);
                }
            }
            if let Some(m) = volume_muted {
                state.panel.volume_muted = m;
            }
            if let Some(def) = sinks.iter().find(|s| s.is_default) {
                state.settings.audio_sink_name = def.name.clone();
            }
            state.settings.audio_sinks = sinks;
        }
        WorkerResponse::SystemInfoRefreshed(snap) => {
            state.settings.apply_system_info_snapshot(snap);
        }
        WorkerResponse::DesktopAppsScanned(apps) => {
            state.panel.desktop_apps = apps;
        }
        WorkerResponse::ClipboardText { text } => {
            state.overlays.push_clipboard_text(text);
        }
        WorkerResponse::PanelStatusPolled {
            battery_percent,
            volume_percent,
            volume_muted,
            active_wifi_ssid,
        } => {
            state.panel.battery_percent = battery_percent;
            if let Some(v) = volume_percent {
                if !state.overlays.cc_dragging_volume {
                    state.panel.volume_percent = v.clamp(0, 100);
                }
            }
            if let Some(m) = volume_muted {
                state.panel.volume_muted = m;
            }
            if let Some(ssid) = active_wifi_ssid {
                state.settings.wifi_status = format!("Connected to {}", ssid);
            }
        }
        WorkerResponse::ScreenshotDone { path, ok, via_grim } => {
            if ok {
                let method = if via_grim { "grim" } else { "compositor" };
                state.overlays.push_notification(
                    "Screenshot Saved",
                    &format!("Saved ({}) to {}", method, path.display()),
                );
            } else {
                state.overlays.push_notification(
                    "Screenshot Failed",
                    &format!("Could not write {}", path.display()),
                );
            }
        }
    }
}

fn worker_loop(req_rx: mpsc::Receiver<WorkerRequest>, resp_tx: CalloopSender<WorkerResponse>) {
    while let Ok(req) = req_rx.recv() {
        match req {
            WorkerRequest::LoadIcon { name, target_size } => {
                icons::load_system_icon_sync(&name, target_size);
                let _ = resp_tx.send(WorkerResponse::IconLoaded);
            }
            WorkerRequest::RefreshWifi { force_rescan } => {
                let (networks, status) = crate::system_backends::scan_wifi(force_rescan);
                let _ = resp_tx.send(WorkerResponse::WifiRefreshed { networks, status });
            }
            WorkerRequest::ConnectWifi { ssid, password } => {
                let (networks, status) =
                    crate::system_backends::connect_wifi(&ssid, password.as_deref());
                let _ = resp_tx.send(WorkerResponse::WifiRefreshed { networks, status });
            }
            WorkerRequest::DisconnectWifi { ssid } => {
                let (networks, status) = crate::system_backends::disconnect_wifi(&ssid);
                let _ = resp_tx.send(WorkerResponse::WifiRefreshed { networks, status });
            }
            WorkerRequest::RefreshBluetooth => {
                let (devices, status, powered) = crate::system_backends::scan_bluetooth();
                let _ = resp_tx.send(WorkerResponse::BluetoothRefreshed {
                    devices,
                    status,
                    powered,
                });
            }
            WorkerRequest::ToggleBluetoothPower { enable } => {
                let (devices, status, powered) =
                    crate::system_backends::toggle_bluetooth_power(enable);
                let _ = resp_tx.send(WorkerResponse::BluetoothRefreshed {
                    devices,
                    status,
                    powered,
                });
            }
            WorkerRequest::ToggleBluetoothDevice { mac, connect } => {
                let (devices, status, powered) =
                    crate::system_backends::connect_or_pair_bluetooth(&mac, connect);
                let _ = resp_tx.send(WorkerResponse::BluetoothRefreshed {
                    devices,
                    status,
                    powered,
                });
            }
            WorkerRequest::RefreshAudio => {
                let (volume_percent, volume_muted, sinks) =
                    crate::system_backends::poll_audio_state();
                let _ = resp_tx.send(WorkerResponse::AudioRefreshed {
                    volume_percent,
                    volume_muted,
                    sinks,
                });
            }
            WorkerRequest::SetAudioVolume { percent } => {
                let (volume_percent, volume_muted, sinks) =
                    crate::system_backends::set_audio_volume(percent);
                let _ = resp_tx.send(WorkerResponse::AudioRefreshed {
                    volume_percent,
                    volume_muted,
                    sinks,
                });
            }
            WorkerRequest::ToggleAudioMute => {
                let (volume_percent, volume_muted, sinks) =
                    crate::system_backends::toggle_audio_mute();
                let _ = resp_tx.send(WorkerResponse::AudioRefreshed {
                    volume_percent,
                    volume_muted,
                    sinks,
                });
            }
            WorkerRequest::SetDefaultAudioSink { sink_id } => {
                let (volume_percent, volume_muted, sinks) =
                    crate::system_backends::set_default_audio_sink(&sink_id);
                let _ = resp_tx.send(WorkerResponse::AudioRefreshed {
                    volume_percent,
                    volume_muted,
                    sinks,
                });
            }
            WorkerRequest::RefreshSystemInfo => {
                let snap = crate::settings::collect_system_info_snapshot();
                let _ = resp_tx.send(WorkerResponse::SystemInfoRefreshed(snap));
            }
            WorkerRequest::ScanDesktopApps => {
                let apps = crate::panel::scan_desktop_applications();
                let _ = resp_tx.send(WorkerResponse::DesktopAppsScanned(apps));
            }
            WorkerRequest::PollExternalClipboard => {
                if let Ok(out) = Command::new("wl-paste").arg("--no-newline").output() {
                    if out.status.success() {
                        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        if !text.is_empty() {
                            let _ = resp_tx.send(WorkerResponse::ClipboardText { text });
                        }
                    }
                }
            }
            WorkerRequest::SetExternalClipboard { text } => {
                let _ = Command::new("wl-copy").arg(&text).status();
            }
            WorkerRequest::ReadClipboardPipe { fd } => {
                let mut file = fs::File::from(fd);
                let mut buf = Vec::new();
                // Cap clipboard history read to 256 KiB to avoid unbounded memory allocation.
                let mut chunk = [0u8; 8192];
                while buf.len() < 256 * 1024 {
                    match file.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&buf).trim().to_string();
                if !text.is_empty() {
                    let _ = resp_tx.send(WorkerResponse::ClipboardText { text });
                }
            }
            WorkerRequest::TakeScreenshot {
                path,
                socket_name,
                fallback_png,
            } => {
                let grim_ok = Command::new("grim")
                    .env("WAYLAND_DISPLAY", &socket_name)
                    .arg(&path)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);

                if grim_ok {
                    let _ = resp_tx.send(WorkerResponse::ScreenshotDone {
                        path,
                        ok: true,
                        via_grim: true,
                    });
                } else if let Some(png_bytes) = fallback_png {
                    let ok = fs::write(&path, png_bytes).is_ok();
                    let _ = resp_tx.send(WorkerResponse::ScreenshotDone {
                        path,
                        ok,
                        via_grim: false,
                    });
                } else {
                    let _ = resp_tx.send(WorkerResponse::ScreenshotDone {
                        path,
                        ok: false,
                        via_grim: false,
                    });
                }
            }
            WorkerRequest::SaveRgbaPng {
                path,
                width,
                height,
                rgba,
            } => {
                let ok = save_rgba_png(&path, width, height, &rgba);
                let _ = resp_tx.send(WorkerResponse::ScreenshotDone {
                    path,
                    ok,
                    via_grim: false,
                });
            }
            WorkerRequest::PollPanelStatus => {
                let (battery_percent, volume_percent, volume_muted, active_wifi_ssid) =
                    poll_panel_status_sync();
                let _ = resp_tx.send(WorkerResponse::PanelStatusPolled {
                    battery_percent,
                    volume_percent,
                    volume_muted,
                    active_wifi_ssid,
                });
            }
            WorkerRequest::RunCommand { program, args } => {
                let _ = Command::new(&program).args(&args).status();
            }
        }
    }
}

fn save_rgba_png(path: &PathBuf, width: u32, height: u32, rgba: &[u8]) -> bool {
    let Ok(file) = File::create(path) else {
        return false;
    };
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let Ok(mut writer) = encoder.write_header() else {
        return false;
    };
    writer.write_image_data(rgba).is_ok()
}

fn poll_panel_status_sync() -> (Option<i32>, Option<i32>, Option<bool>, Option<String>) {
    let battery_percent = fs::read_dir("/sys/class/power_supply").ok().and_then(|entries| {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("BAT") {
                if let Ok(cap) = fs::read_to_string(entry.path().join("capacity")) {
                    if let Ok(v) = cap.trim().parse::<i32>() {
                        return Some(v);
                    }
                }
            }
        }
        None
    });

    let mut volume_percent = None;
    let mut volume_muted = None;
    if let Ok(out) = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
    {
        if out.status.success() {
            let txt = String::from_utf8_lossy(&out.stdout);
            // Format: `Volume: 0.75 [MUTED]`
            if let Some(val_str) = txt.split_whitespace().nth(1) {
                if let Ok(v) = val_str.parse::<f32>() {
                    volume_percent = Some((v * 100.0).round() as i32);
                }
            }
            volume_muted = Some(txt.contains("MUTED"));
        }
    }

    let mut active_wifi_ssid = None;
    if let Ok(out) = Command::new("nmcli")
        .args(["-t", "-f", "ACTIVE,SSID", "dev", "wifi"])
        .output()
    {
        if out.status.success() {
            let txt = String::from_utf8_lossy(&out.stdout);
            for line in txt.lines() {
                if let Some(ssid) = line.strip_prefix("yes:") {
                    if !ssid.trim().is_empty() {
                        active_wifi_ssid = Some(ssid.trim().to_string());
                        break;
                    }
                }
            }
        }
    }

    (battery_percent, volume_percent, volume_muted, active_wifi_ssid)
}
