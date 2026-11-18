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

#[derive(Debug, Clone)]
pub struct TopProcessEntry {
    pub pid: i32,
    pub cpu: String,
    pub mem: String,
    pub name: String,
}

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
    CheckSystemUpdates,
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
    RefreshTopProcesses,
    KillProcess {
        pid: i32,
    },
    SetBrightness {
        percent: i32,
    },
    RebuildIconCache,
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
    UpdatesChecked(String),
    ClipboardText {
        text: String,
    },
    PanelStatusPolled {
        battery_percent: Option<i32>,
        volume_percent: Option<i32>,
        volume_muted: Option<bool>,
        active_wifi_ssid: Option<String>,
        cpu_pct: Option<u32>,
        ram_pct: Option<u32>,
        brightness_percent: Option<i32>,
    },
    TopProcessesRefreshed(Vec<TopProcessEntry>),
    IconCacheRebuilt {
        ok: bool,
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
        WorkerResponse::UpdatesChecked(status) => {
            state.settings.update_status = status;
        }
        WorkerResponse::ClipboardText { text } => {
            state.overlays.push_clipboard_text(text);
        }
        WorkerResponse::PanelStatusPolled {
            battery_percent,
            volume_percent,
            volume_muted,
            active_wifi_ssid,
            cpu_pct,
            ram_pct,
            brightness_percent,
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
            if let Some(cpu) = cpu_pct {
                state.panel.cpu_usage_pct = cpu;
            }
            if let Some(ram) = ram_pct {
                state.panel.ram_usage_pct = ram;
            }
            if let Some(br) = brightness_percent {
                state.panel.brightness_percent = br.clamp(5, 100);
            }
        }
        WorkerResponse::TopProcessesRefreshed(procs) => {
            state.overlays.top_processes = procs;
        }
        WorkerResponse::IconCacheRebuilt { ok } => {
            state.overlays.push_notification(
                "Icon Cache & DE",
                if ok {
                    "Rebuilt okay-icons cache & reloaded DE"
                } else {
                    "Reloaded DE (gtk-update-icon-cache not found)"
                },
            );
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
            WorkerRequest::CheckSystemUpdates => {
                let status = crate::system_backends::check_system_updates();
                let _ = resp_tx.send(WorkerResponse::UpdatesChecked(status));
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
                let (battery_percent, volume_percent, volume_muted, active_wifi_ssid, cpu_pct, ram_pct, brightness_percent) =
                    poll_panel_status_sync();
                let _ = resp_tx.send(WorkerResponse::PanelStatusPolled {
                    battery_percent,
                    volume_percent,
                    volume_muted,
                    active_wifi_ssid,
                    cpu_pct,
                    ram_pct,
                    brightness_percent,
                });
            }
            WorkerRequest::RefreshTopProcesses => {
                let procs = fetch_top_processes_sync();
                let _ = resp_tx.send(WorkerResponse::TopProcessesRefreshed(procs));
            }
            WorkerRequest::KillProcess { pid } => {
                if pid > 1 {
                    let _ = Command::new("kill")
                        .args(["-TERM", &pid.to_string()])
                        .status();
                }
                let procs = fetch_top_processes_sync();
                let _ = resp_tx.send(WorkerResponse::TopProcessesRefreshed(procs));
            }
            WorkerRequest::SetBrightness { percent } => {
                set_brightness_sync(percent);
            }
            WorkerRequest::RebuildIconCache => {
                let mut ok = false;
                if let Some(home) = std::env::var_os("HOME") {
                    let icon_dir = PathBuf::from(home).join(".local/share/icons/okay-icons");
                    if let Ok(st) = Command::new("gtk-update-icon-cache")
                        .args(["-f", "-t"])
                        .arg(&icon_dir)
                        .status()
                    {
                        ok = st.success();
                    }
                }
                let _ = resp_tx.send(WorkerResponse::IconCacheRebuilt { ok });
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

fn poll_panel_status_sync() -> (
    Option<i32>,
    Option<i32>,
    Option<bool>,
    Option<String>,
    Option<u32>,
    Option<u32>,
    Option<i32>,
) {
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

    let (cpu_pct, ram_pct) = poll_cpu_ram_sync();
    let brightness_percent = read_brightness_sync();
    (
        battery_percent,
        volume_percent,
        volume_muted,
        active_wifi_ssid,
        cpu_pct,
        ram_pct,
        brightness_percent,
    )
}

fn poll_cpu_ram_sync() -> (Option<u32>, Option<u32>) {
    static PREV_CPU: std::sync::Mutex<Option<(u64, u64)>> = std::sync::Mutex::new(None);
    let mut cpu_pct = None;
    if let Ok(stat) = fs::read_to_string("/proc/stat") {
        if let Some(line) = stat.lines().next() {
            let nums: Vec<u64> = line
                .split_whitespace()
                .skip(1)
                .filter_map(|s| s.parse::<u64>().ok())
                .collect();
            if nums.len() >= 4 {
                let idle = nums[3] + nums.get(4).copied().unwrap_or(0);
                let total: u64 = nums.iter().sum();
                if let Ok(mut prev) = PREV_CPU.lock() {
                    if let Some((pt, pi)) = *prev {
                        let dt = total.saturating_sub(pt);
                        let di = idle.saturating_sub(pi);
                        if dt > 0 {
                            cpu_pct = Some((((dt.saturating_sub(di)) * 100) / dt).min(100) as u32);
                        }
                    }
                    *prev = Some((total, idle));
                }
            }
        }
    }

    let mut ram_pct = None;
    if let Ok(mem) = fs::read_to_string("/proc/meminfo") {
        let mut total_kb = 0u64;
        let mut avail_kb = 0u64;
        for line in mem.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                total_kb = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                avail_kb = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
            }
        }
        if total_kb > 0 {
            let used = total_kb.saturating_sub(avail_kb);
            ram_pct = Some(((used * 100) / total_kb).min(100) as u32);
        }
    }

    (cpu_pct, ram_pct)
}

fn read_brightness_sync() -> Option<i32> {
    let entries = fs::read_dir("/sys/class/backlight").ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        let cur = fs::read_to_string(p.join("brightness"))
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok());
        let max = fs::read_to_string(p.join("max_brightness"))
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok());
        if let (Some(c), Some(m)) = (cur, max) {
            if m > 0.0 {
                return Some(((c / m) * 100.0).round() as i32);
            }
        }
    }
    None
}

fn set_brightness_sync(percent: i32) {
    let pct = percent.clamp(5, 100);
    let arg = format!("{}%", pct);
    if Command::new("brightnessctl")
        .args(["set", &arg])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return;
    }
    if let Ok(entries) = fs::read_dir("/sys/class/backlight") {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Some(max) = fs::read_to_string(p.join("max_brightness"))
                .ok()
                .and_then(|s| s.trim().parse::<f64>().ok())
            {
                let target = ((max * pct as f64) / 100.0).round().max(1.0) as u64;
                let _ = fs::write(p.join("brightness"), target.to_string());
            }
        }
    }
}

fn fetch_top_processes_sync() -> Vec<TopProcessEntry> {
    let Ok(out) = Command::new("ps")
        .args(["-eo", "pid,pcpu,pmem,comm", "--sort=-%cpu"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let txt = String::from_utf8_lossy(&out.stdout);
    let mut list = Vec::new();
    for line in txt.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            if let Ok(pid) = parts[0].parse::<i32>() {
                list.push(TopProcessEntry {
                    pid,
                    cpu: parts[1].to_string(),
                    mem: parts[2].to_string(),
                    name: parts[3..].join(" "),
                });
                if list.len() >= 5 {
                    break;
                }
            }
        }
    }
    list
}
