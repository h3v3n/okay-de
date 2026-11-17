use std::{
    ffi::OsString,
    fs,
    io::Read,
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
    },
    RefreshBluetooth,
    ToggleBluetoothPower {
        enable: bool,
    },
    ToggleBluetoothDevice {
        mac: String,
        connect: bool,
    },
    RefreshSystemInfo,
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
    },
    SystemInfoRefreshed(SystemInfoSnapshot),
    ClipboardText {
        text: String,
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
        WorkerResponse::BluetoothRefreshed { devices, status } => {
            state.settings.bt_devices = devices;
            state.settings.bt_status = status;
        }
        WorkerResponse::SystemInfoRefreshed(snap) => {
            state.settings.apply_system_info_snapshot(snap);
        }
        WorkerResponse::ClipboardText { text } => {
            state.overlays.push_clipboard_text(text);
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
                let (networks, status) = fetch_wifi(force_rescan);
                let _ = resp_tx.send(WorkerResponse::WifiRefreshed { networks, status });
            }
            WorkerRequest::ConnectWifi { ssid } => {
                let _ = Command::new("nmcli")
                    .args(["dev", "wifi", "connect", &ssid])
                    .status();
                let (networks, status) = fetch_wifi(false);
                let _ = resp_tx.send(WorkerResponse::WifiRefreshed { networks, status });
            }
            WorkerRequest::RefreshBluetooth => {
                let (devices, status) = fetch_bluetooth();
                let _ = resp_tx.send(WorkerResponse::BluetoothRefreshed { devices, status });
            }
            WorkerRequest::ToggleBluetoothPower { enable } => {
                let arg = if enable { "on" } else { "off" };
                let _ = Command::new("bluetoothctl")
                    .args(["power", arg])
                    .status();
                let (devices, status) = fetch_bluetooth();
                let _ = resp_tx.send(WorkerResponse::BluetoothRefreshed { devices, status });
            }
            WorkerRequest::ToggleBluetoothDevice { mac, connect } => {
                let sub = if connect { "connect" } else { "disconnect" };
                let _ = Command::new("bluetoothctl").args([sub, &mac]).status();
                let (devices, status) = fetch_bluetooth();
                let _ = resp_tx.send(WorkerResponse::BluetoothRefreshed { devices, status });
            }
            WorkerRequest::RefreshSystemInfo => {
                let snap = crate::settings::collect_system_info_snapshot();
                let _ = resp_tx.send(WorkerResponse::SystemInfoRefreshed(snap));
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
            WorkerRequest::RunCommand { program, args } => {
                let _ = Command::new(&program).args(&args).status();
            }
        }
    }
}

fn fetch_wifi(force_rescan: bool) -> (Vec<WifiNetwork>, String) {
    let rescan_arg = if force_rescan { "yes" } else { "no" };
    let output = Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "dev",
            "wifi",
            "list",
            "--rescan",
            rescan_arg,
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut nets = Vec::new();
            let mut active_ssid = None;
            for line in text.lines() {
                let parts: Vec<&str> = line.splitn(4, ':').collect();
                if parts.len() >= 4 {
                    let in_use = parts[0].trim() == "*";
                    let ssid = parts[1].trim();
                    if ssid.is_empty() {
                        continue;
                    }
                    if in_use {
                        active_ssid = Some(ssid.to_string());
                    }
                    if nets.iter().any(|n: &WifiNetwork| n.ssid == ssid) {
                        continue;
                    }
                    nets.push(WifiNetwork {
                        in_use,
                        ssid: ssid.to_string(),
                        signal: parts[2].trim().to_string(),
                        security: if parts[3].trim().is_empty() {
                            "Open".to_string()
                        } else {
                            parts[3].trim().to_string()
                        },
                    });
                    if nets.len() >= 6 {
                        break;
                    }
                }
            }
            let status = match active_ssid {
                Some(s) => format!("Connected to {}", s),
                None => format!("Found {} Wi-Fi networks", nets.len()),
            };
            (nets, status)
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            let msg = format!(
                "nmcli: {}",
                err.lines().next().unwrap_or("unavailable")
            );
            (Vec::new(), msg)
        }
        Err(_) => (
            Vec::new(),
            "NetworkManager (nmcli) not installed".to_string(),
        ),
    }
}

fn fetch_bluetooth() -> (Vec<BluetoothDevice>, String) {
    let out = Command::new("bluetoothctl").arg("devices").output();
    match out {
        Ok(res) if res.status.success() => {
            let text = String::from_utf8_lossy(&res.stdout);
            let mut devs = Vec::new();
            for line in text.lines() {
                let mut parts = line.splitn(3, ' ');
                if parts.next() == Some("Device") {
                    if let (Some(mac), Some(name)) = (parts.next(), parts.next()) {
                        devs.push(BluetoothDevice {
                            mac: mac.to_string(),
                            name: name.trim().to_string(),
                            connected: false,
                        });
                    }
                }
            }
            let status = format!("Found {} Bluetooth devices", devs.len());
            (devs, status)
        }
        _ => (
            Vec::new(),
            "bluetoothctl service not active or unavailable".to_string(),
        ),
    }
}
