use std::process::Command;

use crate::settings::{BluetoothDevice, WifiNetwork};

#[derive(Debug, Clone)]
pub struct AudioSink {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Splits a `nmcli -t` colon-delimited line while respecting `\\:` escaped colons.
fn split_nmcli_escaped(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if next == ':' || next == '\\' {
                    cur.push(next);
                    chars.next();
                    continue;
                }
            }
            cur.push(c);
        } else if c == ':' {
            fields.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    fields.push(cur);
    fields
}

/// Scans Wi-Fi networks via `nmcli` (runs on background worker thread).
pub fn scan_wifi(force_rescan: bool) -> (Vec<WifiNetwork>, String) {
    if force_rescan {
        let _ = Command::new("nmcli").args(["dev", "wifi", "rescan"]).output();
    }

    let Ok(out) = Command::new("nmcli")
        .args(["-t", "-f", "SSID,SIGNAL,SECURITY,ACTIVE", "dev", "wifi", "list"])
        .output()
    else {
        return (Vec::new(), "NetworkManager (nmcli) not available".to_string());
    };

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return (
            Vec::new(),
            if err.is_empty() {
                "nmcli wifi scan failed".to_string()
            } else {
                err
            },
        );
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut nets: Vec<WifiNetwork> = Vec::new();
    for line in text.lines() {
        let parts = split_nmcli_escaped(line);
        if parts.len() < 4 {
            continue;
        }
        let ssid = parts[0].trim().to_string();
        if ssid.is_empty() {
            continue;
        }
        let signal = parts[1].trim().to_string();
        let security = if parts[2].trim().is_empty() {
            "Open".to_string()
        } else {
            parts[2].trim().to_string()
        };
        let in_use = parts[3].trim().eq_ignore_ascii_case("yes") || parts[3].trim() == "*";

        if let Some(existing) = nets.iter_mut().find(|n| n.ssid == ssid) {
            if in_use {
                existing.in_use = true;
            }
            let new_sig = signal.parse::<i32>().unwrap_or(0);
            let old_sig = existing.signal.parse::<i32>().unwrap_or(0);
            if new_sig > old_sig {
                existing.signal = signal;
            }
            continue;
        }

        nets.push(WifiNetwork {
            in_use,
            ssid,
            signal,
            security,
        });
    }

    nets.sort_by(|a, b| {
        b.in_use.cmp(&a.in_use).then_with(|| {
            b.signal
                .parse::<i32>()
                .unwrap_or(0)
                .cmp(&a.signal.parse::<i32>().unwrap_or(0))
        })
    });

    let connected = nets.iter().find(|n| n.in_use).map(|n| n.ssid.as_str());
    let status = match connected {
        Some(ssid) => format!("Connected to {} ({} networks)", ssid, nets.len()),
        None => format!("Found {} Wi-Fi networks", nets.len()),
    };
    (nets, status)
}

/// Connects to a Wi-Fi network (with optional password) via `nmcli`, then refreshes list.
pub fn connect_wifi(ssid: &str, password: Option<&str>) -> (Vec<WifiNetwork>, String) {
    let res = if let Some(pw) = password.filter(|p| !p.is_empty()) {
        Command::new("nmcli")
            .args(["dev", "wifi", "connect", ssid, "password", pw])
            .output()
    } else {
        let up = Command::new("nmcli")
            .args(["connection", "up", "id", ssid])
            .output();
        match up {
            Ok(o) if o.status.success() => Ok(o),
            _ => Command::new("nmcli")
                .args(["dev", "wifi", "connect", ssid])
                .output(),
        }
    };

    let (nets, _) = scan_wifi(false);
    let status = match res {
        Ok(o) if o.status.success() => format!("Connected to {}", ssid),
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            if err.is_empty() {
                format!("Failed to connect to {}", ssid)
            } else {
                err
            }
        }
        Err(_) => "nmcli not available".to_string(),
    };
    (nets, status)
}

/// Disconnects an active Wi-Fi connection via `nmcli`, then refreshes list.
pub fn disconnect_wifi(ssid: &str) -> (Vec<WifiNetwork>, String) {
    let res = Command::new("nmcli")
        .args(["connection", "down", "id", ssid])
        .output();
    let (nets, _) = scan_wifi(false);
    let status = match res {
        Ok(o) if o.status.success() => format!("Disconnected from {}", ssid),
        _ => format!("Could not disconnect {}", ssid),
    };
    (nets, status)
}

fn query_bt_powered() -> bool {
    let Ok(out) = Command::new("bluetoothctl").arg("show").output() else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .any(|l| l.trim().eq_ignore_ascii_case("Powered: yes"))
}

fn list_bt_devices() -> Vec<BluetoothDevice> {
    let Ok(out) = Command::new("bluetoothctl").arg("devices").output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut devices = Vec::new();

    for line in text.lines().take(14) {
        let mut parts = line.splitn(3, ' ');
        if parts.next() != Some("Device") {
            continue;
        }
        let Some(mac) = parts.next() else {
            continue;
        };
        let name = parts.next().unwrap_or(mac).trim().to_string();
        let connected = Command::new("bluetoothctl")
            .args(["info", mac])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.trim().eq_ignore_ascii_case("Connected: yes"))
            })
            .unwrap_or(false);

        devices.push(BluetoothDevice {
            mac: mac.to_string(),
            name,
            connected,
        });
    }
    devices.sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.cmp(&b.name)));
    devices
}

/// Scans Bluetooth devices via `bluetoothctl` and reads adapter power state.
pub fn scan_bluetooth() -> (Vec<BluetoothDevice>, String, bool) {
    let powered = query_bt_powered();
    if powered {
        let _ = Command::new("bluetoothctl")
            .args(["--timeout", "4", "scan", "on"])
            .output();
    }
    let devices = list_bt_devices();
    let status = if !powered {
        "Bluetooth adapter is powered off".to_string()
    } else if devices.is_empty() {
        "No Bluetooth devices found".to_string()
    } else {
        format!("Found {} Bluetooth device(s)", devices.len())
    };
    (devices, status, powered)
}

/// Enables or disables the Bluetooth adapter via `bluetoothctl power on/off`.
pub fn toggle_bluetooth_power(enable: bool) -> (Vec<BluetoothDevice>, String, bool) {
    let arg = if enable { "on" } else { "off" };
    let _ = Command::new("bluetoothctl").args(["power", arg]).output();
    let powered = query_bt_powered();
    let devices = if powered {
        list_bt_devices()
    } else {
        Vec::new()
    };
    let status = if powered {
        format!("Bluetooth On ({} device(s))", devices.len())
    } else {
        "Bluetooth adapter powered off".to_string()
    };
    (devices, status, powered)
}

/// Pairs/trusts/connects or disconnects a Bluetooth device via `bluetoothctl`.
pub fn connect_or_pair_bluetooth(mac: &str, connect: bool) -> (Vec<BluetoothDevice>, String, bool) {
    if connect {
        let _ = Command::new("bluetoothctl").args(["pair", mac]).output();
        let _ = Command::new("bluetoothctl").args(["trust", mac]).output();
        let res = Command::new("bluetoothctl").args(["connect", mac]).output();
        let powered = query_bt_powered();
        let devices = list_bt_devices();
        let status = match res {
            Ok(o) if o.status.success() => format!("Connected {}", mac),
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if err.is_empty() {
                    format!("Connection failed for {}", mac)
                } else {
                    err.lines().last().unwrap_or("Connection failed").to_string()
                }
            }
            Err(_) => "bluetoothctl not available".to_string(),
        };
        (devices, status, powered)
    } else {
        let _ = Command::new("bluetoothctl").args(["disconnect", mac]).output();
        let powered = query_bt_powered();
        let devices = list_bt_devices();
        (devices, format!("Disconnected {}", mac), powered)
    }
}

fn parse_wpctl_sinks() -> Vec<AudioSink> {
    let Ok(out) = Command::new("wpctl").arg("status").output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut in_audio = false;
    let mut in_sinks = false;
    let mut sinks = Vec::new();

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed == "Audio" {
            in_audio = true;
            continue;
        }
        if in_audio && (trimmed == "Video" || trimmed == "Settings") {
            break;
        }
        if !in_audio {
            continue;
        }
        if trimmed.contains("Sinks:") {
            in_sinks = true;
            continue;
        }
        if in_sinks {
            if trimmed.contains("Sources:")
                || trimmed.contains("Filters:")
                || trimmed.contains("Streams:")
                || trimmed.is_empty()
            {
                if trimmed.contains("Sources:") || trimmed.contains("Filters:") || trimmed.contains("Streams:") {
                    in_sinks = false;
                }
                continue;
            }
            let cleaned = trimmed.trim_start_matches(|c: char| !c.is_ascii_digit() && c != '*');
            let is_default = cleaned.starts_with('*');
            let rest = cleaned.trim_start_matches('*').trim();
            if let Some((id_part, name_part)) = rest.split_once('.') {
                let id = id_part.trim().to_string();
                let name = name_part
                    .split("[vol:")
                    .next()
                    .unwrap_or(name_part)
                    .trim()
                    .to_string();
                if !id.is_empty() && !name.is_empty() {
                    sinks.push(AudioSink {
                        id,
                        name,
                        is_default,
                    });
                }
            }
        }
    }
    sinks
}

fn parse_pactl_sinks() -> Vec<AudioSink> {
    let default_name = Command::new("pactl")
        .arg("get-default-sink")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let Ok(out) = Command::new("pactl").args(["list", "short", "sinks"]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut sinks = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 2 {
            let name = cols[1].trim().to_string();
            sinks.push(AudioSink {
                id: name.clone(),
                is_default: name == default_name,
                name,
            });
        }
    }
    sinks
}

/// Polls current volume, mute state, and available audio output sinks via `wpctl` / `pactl`.
pub fn poll_audio_state() -> (Option<i32>, Option<bool>, Vec<AudioSink>) {
    let mut vol_pct = None;
    let mut muted = None;

    if let Ok(out) = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
    {
        if out.status.success() {
            let txt = String::from_utf8_lossy(&out.stdout);
            if let Some(after) = txt.split("Volume:").nth(1) {
                if let Some(num_str) = after.split_whitespace().next() {
                    if let Ok(v) = num_str.parse::<f32>() {
                        vol_pct = Some((v * 100.0).round().clamp(0.0, 100.0) as i32);
                    }
                }
            }
            muted = Some(txt.contains("[MUTED]"));
        }
    }

    if vol_pct.is_none() {
        if let Ok(out) = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
        {
            let txt = String::from_utf8_lossy(&out.stdout);
            if let Some(pct_tok) = txt.split_whitespace().find(|t| t.ends_with('%')) {
                if let Ok(v) = pct_tok.trim_end_matches('%').parse::<i32>() {
                    vol_pct = Some(v.clamp(0, 100));
                }
            }
        }
    }

    let mut sinks = parse_wpctl_sinks();
    if sinks.is_empty() {
        sinks = parse_pactl_sinks();
    }
    (vol_pct, muted, sinks)
}

pub fn set_audio_volume(pct: i32) -> (Option<i32>, Option<bool>, Vec<AudioSink>) {
    let arg = format!("{}%", pct.clamp(0, 100));
    if Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &arg])
        .output()
        .is_err()
    {
        let _ = Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &arg])
            .output();
    }
    poll_audio_state()
}

pub fn toggle_audio_mute() -> (Option<i32>, Option<bool>, Vec<AudioSink>) {
    if Command::new("wpctl")
        .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
        .output()
        .is_err()
    {
        let _ = Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
            .output();
    }
    poll_audio_state()
}

pub fn set_default_audio_sink(sink_id: &str) -> (Option<i32>, Option<bool>, Vec<AudioSink>) {
    if sink_id.chars().all(|c| c.is_ascii_digit()) {
        let _ = Command::new("wpctl")
            .args(["set-default", sink_id])
            .output();
    } else {
        let _ = Command::new("pactl")
            .args(["set-default-sink", sink_id])
            .output();
    }
    poll_audio_state()
}
