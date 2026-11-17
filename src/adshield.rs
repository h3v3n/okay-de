use std::{
    collections::{HashSet, VecDeque},
    fs,
    net::UdpSocket,
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Starter built-in blocklist of common ad, analytics, and telemetry domains.
const STARTER_BLOCKLIST: &[&str] = &[
    "doubleclick.net",
    "adservice.google.com",
    "pagead2.googlesyndication.com",
    "googleadservices.com",
    "www.googleadservices.com",
    "googlesyndication.com",
    "analytics.google.com",
    "www.google-analytics.com",
    "google-analytics.com",
    "ssl.google-analytics.com",
    "googletagmanager.com",
    "www.googletagmanager.com",
    "connect.facebook.net",
    "pixel.facebook.com",
    "an.facebook.com",
    "graph.instagram.com",
    "ads-api.tiktok.com",
    "analytics.tiktok.com",
    "log.tiktokv.com",
    "mon.tiktokv.com",
    "sentry.io",
    "browser.sentry-cdn.com",
    "api.mixpanel.com",
    "api.amplitude.com",
    "api2.amplitude.com",
    "script.hotjar.com",
    "static.hotjar.com",
    "sb.scorecardresearch.com",
    "b.scorecardresearch.com",
    "cdn.taboola.com",
    "trc.taboola.com",
    "widgets.outbrain.com",
    "log.outbrain.com",
    "static.criteo.net",
    "bidder.criteo.com",
    "d.adroll.com",
    "s.adroll.com",
    "aax.amazon-adsystem.com",
    "c.amazon-adsystem.com",
    "ib.adnxs.com",
    "secure.adnxs.com",
    "fastlane.rubiconproject.com",
    "hbopenbid.pubmatic.com",
    "ads.pubmatic.com",
    "clarity.ms",
    "www.clarity.ms",
    "bat.bing.com",
    "ads-twitter.com",
    "static.ads-twitter.com",
    "sc-static.net",
    "tr.snapchat.com",
    "ct.pinterest.com",
    "ads.yahoo.com",
    "telemetry.mozilla.org",
    "incoming.telemetry.mozilla.org",
    "metrics.icloud.com",
    "app-measurement.com",
    "firebaselogging.googleapis.com",
];

const UPSTREAM_DNS: &str = "1.1.1.1:53";
const LOCAL_DNS_BIND: &str = "127.0.0.1:5353";
const BLOCKLIST_URL: &str = "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts";

#[derive(Debug, Clone)]
pub enum AdShieldCommand {
    SetEnabled(bool),
    UpdateBlocklist,
    SetCustomLists {
        whitelist: Vec<String>,
        blacklist: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum AdShieldEvent {
    Stats {
        blocked_count: u64,
        rules_count: usize,
        recent_blocked: Vec<(String, String)>,
    },
    BlockedQuery {
        domain: String,
        time_str: String,
        total_blocked: u64,
    },
    UpdateFinished {
        ok: bool,
        rules_count: usize,
        message: String,
    },
    SystemSyncStatus(String),
}

pub struct AdShieldManager {
    pub enabled: bool,
    pub blocked_count: u64,
    pub rules_count: usize,
    pub recent_blocked: Vec<(String, String)>,
    pub whitelist: Vec<String>,
    pub blacklist: Vec<String>,
    pub updating: bool,
    pub status_msg: String,
    pub dns_port: u16,
    cmd_tx: Option<Sender<AdShieldCommand>>,
    evt_rx: Option<Receiver<AdShieldEvent>>,
}

impl AdShieldManager {
    pub fn new(enabled: bool, whitelist: Vec<String>, blacklist: Vec<String>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<AdShieldCommand>();
        let (evt_tx, evt_rx) = mpsc::channel::<AdShieldEvent>();

        let init_enabled = enabled;
        let init_wl = whitelist.clone();
        let init_bl = blacklist.clone();

        if let Err(e) = std::thread::Builder::new()
            .name("okay-adshield".to_string())
            .spawn(move || {
                run_adshield_worker(init_enabled, init_wl, init_bl, cmd_rx, evt_tx);
            })
        {
            tracing::warn!("Failed to spawn AdShield background thread: {}", e);
        }

        Self {
            enabled,
            blocked_count: 0,
            rules_count: STARTER_BLOCKLIST.len(),
            recent_blocked: Vec::new(),
            whitelist,
            blacklist,
            updating: false,
            status_msg: if enabled {
                "Active (DNS :5353 + Hosts)".to_string()
            } else {
                "Disabled".to_string()
            },
            dns_port: 5353,
            cmd_tx: Some(cmd_tx),
            evt_rx: Some(evt_rx),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.status_msg = if enabled {
            "Activating system-wide shield...".to_string()
        } else {
            "Disabled".to_string()
        };
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(AdShieldCommand::SetEnabled(enabled));
        }
    }

    pub fn toggle(&mut self) -> bool {
        let next = !self.enabled;
        self.set_enabled(next);
        next
    }

    pub fn trigger_update_blocklist(&mut self) {
        if self.updating {
            return;
        }
        self.updating = true;
        self.status_msg = "Downloading blocklist...".to_string();
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(AdShieldCommand::UpdateBlocklist);
        }
    }

    pub fn add_whitelist_domain(&mut self, raw: &str) -> bool {
        let Some(dom) = normalize_domain(raw) else {
            return false;
        };
        self.blacklist.retain(|d| d != &dom);
        if !self.whitelist.iter().any(|d| d == &dom) {
            self.whitelist.push(dom);
        }
        self.sync_custom_lists();
        true
    }

    pub fn add_blacklist_domain(&mut self, raw: &str) -> bool {
        let Some(dom) = normalize_domain(raw) else {
            return false;
        };
        self.whitelist.retain(|d| d != &dom);
        if !self.blacklist.iter().any(|d| d == &dom) {
            self.blacklist.push(dom);
        }
        self.sync_custom_lists();
        true
    }

    pub fn remove_whitelist_domain(&mut self, dom: &str) {
        self.whitelist.retain(|d| d != dom);
        self.sync_custom_lists();
    }

    pub fn remove_blacklist_domain(&mut self, dom: &str) {
        self.blacklist.retain(|d| d != dom);
        self.sync_custom_lists();
    }

    fn sync_custom_lists(&self) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(AdShieldCommand::SetCustomLists {
                whitelist: self.whitelist.clone(),
                blacklist: self.blacklist.clone(),
            });
        }
    }

    /// Non-blocking poll of events from the AdShield background thread.
    pub fn poll_updates(&mut self) {
        let Some(rx) = &self.evt_rx else {
            return;
        };
        while let Ok(evt) = rx.try_recv() {
            match evt {
                AdShieldEvent::Stats {
                    blocked_count,
                    rules_count,
                    recent_blocked,
                } => {
                    self.blocked_count = blocked_count;
                    self.rules_count = rules_count;
                    self.recent_blocked = recent_blocked;
                }
                AdShieldEvent::BlockedQuery {
                    domain,
                    time_str,
                    total_blocked,
                } => {
                    self.blocked_count = total_blocked;
                    self.recent_blocked.insert(0, (domain, time_str));
                    if self.recent_blocked.len() > 20 {
                        self.recent_blocked.truncate(20);
                    }
                }
                AdShieldEvent::UpdateFinished {
                    ok: _,
                    rules_count,
                    message,
                } => {
                    self.updating = false;
                    self.rules_count = rules_count;
                    self.status_msg = message;
                }
                AdShieldEvent::SystemSyncStatus(msg) => {
                    self.status_msg = msg;
                }
            }
        }
    }
}

pub fn normalize_domain(raw: &str) -> Option<String> {
    let mut s = raw.trim().to_ascii_lowercase();
    if let Some(rest) = s.strip_prefix("http://") {
        s = rest.to_string();
    } else if let Some(rest) = s.strip_prefix("https://") {
        s = rest.to_string();
    }
    if let Some((host, _)) = s.split_once('/') {
        s = host.to_string();
    }
    let s = s.trim_matches('.').trim();
    if s.is_empty() || !s.contains('.') || s.contains(char::is_whitespace) || s.len() > 253 {
        return None;
    }
    Some(s.to_string())
}

fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".config/okay-de")
}

fn blocklist_path() -> PathBuf {
    config_dir().join("adshield_blocklist.txt")
}

fn load_blocklist_into_set() -> HashSet<String> {
    let mut set: HashSet<String> = STARTER_BLOCKLIST.iter().map(|s| (*s).to_string()).collect();
    let path = blocklist_path();
    if let Ok(content) = fs::read_to_string(&path) {
        for line in content.lines() {
            if let Some(dom) = parse_hosts_or_domain_line(line) {
                set.insert(dom);
            }
        }
    } else {
        // Seed starter blocklist on first run
        let _ = fs::create_dir_all(config_dir());
        let starter_text = STARTER_BLOCKLIST.join("\n");
        let _ = fs::write(&path, starter_text);
    }
    set
}

fn parse_hosts_or_domain_line(line: &str) -> Option<String> {
    let trimmed = line.split('#').next().unwrap_or("").trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let first = parts.next()?;
    let candidate = if first == "0.0.0.0" || first == "127.0.0.1" || first == "::" || first == "::1" {
        parts.next()?
    } else {
        first
    };
    if matches!(
        candidate,
        "localhost" | "localhost.localdomain" | "local" | "broadcasthost" | "ip6-localhost" | "ip6-loopback"
    ) {
        return None;
    }
    normalize_domain(candidate)
}

fn is_domain_blocked(
    domain: &str,
    blocklist: &HashSet<String>,
    whitelist: &HashSet<String>,
    blacklist: &HashSet<String>,
) -> bool {
    if whitelist.contains(domain) {
        return false;
    }
    if blacklist.contains(domain) || blocklist.contains(domain) {
        return true;
    }
    // Check parent subdomains (e.g. `sub.doubleclick.net` -> `doubleclick.net`)
    let mut rest = domain;
    while let Some((_, parent)) = rest.split_once('.') {
        if whitelist.contains(parent) {
            return false;
        }
        if blacklist.contains(parent) || blocklist.contains(parent) {
            return true;
        }
        rest = parent;
    }
    false
}

fn current_hms() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let local_secs = secs % 86_400;
    let h = local_secs / 3600;
    let m = (local_secs % 3600) / 60;
    let s = local_secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

fn run_adshield_worker(
    mut enabled: bool,
    init_wl: Vec<String>,
    init_bl: Vec<String>,
    cmd_rx: Receiver<AdShieldCommand>,
    evt_tx: Sender<AdShieldEvent>,
) {
    let mut blocklist = load_blocklist_into_set();
    let mut whitelist: HashSet<String> = init_wl.into_iter().filter_map(|d| normalize_domain(&d)).collect();
    let mut blacklist: HashSet<String> = init_bl.into_iter().filter_map(|d| normalize_domain(&d)).collect();
    let mut blocked_count: u64 = 0;
    let mut recent_blocked: VecDeque<(String, String)> = VecDeque::with_capacity(20);

    let _ = evt_tx.send(AdShieldEvent::Stats {
        blocked_count,
        rules_count: blocklist.len() + blacklist.len(),
        recent_blocked: Vec::new(),
    });

    if enabled {
        let msg = apply_fedora_system_hosts(true, &blocklist, &whitelist, &blacklist);
        let _ = evt_tx.send(AdShieldEvent::SystemSyncStatus(msg));
    }

    let socket = match UdpSocket::bind(LOCAL_DNS_BIND) {
        Ok(sock) => {
            let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
            Some(sock)
        }
        Err(e) => {
            tracing::warn!("AdShield DNS proxy bind on {} failed: {}", LOCAL_DNS_BIND, e);
            None
        }
    };

    let mut buf = [0u8; 1500];
    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                AdShieldCommand::SetEnabled(next) => {
                    enabled = next;
                    let msg = apply_fedora_system_hosts(enabled, &blocklist, &whitelist, &blacklist);
                    let _ = evt_tx.send(AdShieldEvent::SystemSyncStatus(msg));
                }
                AdShieldCommand::SetCustomLists {
                    whitelist: wl,
                    blacklist: bl,
                } => {
                    whitelist = wl.into_iter().filter_map(|d| normalize_domain(&d)).collect();
                    blacklist = bl.into_iter().filter_map(|d| normalize_domain(&d)).collect();
                    let rules_count = blocklist.len() + blacklist.len();
                    let _ = evt_tx.send(AdShieldEvent::Stats {
                        blocked_count,
                        rules_count,
                        recent_blocked: recent_blocked.iter().cloned().collect(),
                    });
                    if enabled {
                        let msg = apply_fedora_system_hosts(true, &blocklist, &whitelist, &blacklist);
                        let _ = evt_tx.send(AdShieldEvent::SystemSyncStatus(msg));
                    }
                }
                AdShieldCommand::UpdateBlocklist => {
                    match download_and_save_blocklist() {
                        Ok(new_set) => {
                            blocklist = new_set;
                            let rules_count = blocklist.len() + blacklist.len();
                            if enabled {
                                let _ = apply_fedora_system_hosts(true, &blocklist, &whitelist, &blacklist);
                            }
                            let _ = evt_tx.send(AdShieldEvent::UpdateFinished {
                                ok: true,
                                rules_count,
                                message: format!("Updated: {} rules", rules_count),
                            });
                        }
                        Err(err) => {
                            let rules_count = blocklist.len() + blacklist.len();
                            let _ = evt_tx.send(AdShieldEvent::UpdateFinished {
                                ok: false,
                                rules_count,
                                message: format!("Update failed: {}", err),
                            });
                        }
                    }
                }
            }
        }

        if let Some(ref sock) = socket {
            if let Ok((len, peer)) = sock.recv_from(&mut buf) {
                let pkt = &buf[..len];
                if enabled {
                    if let Some((qname, qtype, qend)) = parse_dns_question(pkt) {
                        if is_domain_blocked(&qname, &blocklist, &whitelist, &blacklist) {
                            blocked_count += 1;
                            let ts = current_hms();
                            recent_blocked.push_front((qname.clone(), ts.clone()));
                            if recent_blocked.len() > 20 {
                                recent_blocked.pop_back();
                            }
                            let _ = evt_tx.send(AdShieldEvent::BlockedQuery {
                                domain: qname,
                                time_str: ts,
                                total_blocked: blocked_count,
                            });
                            if let Some(resp) = build_dns_block_response(pkt, qtype, qend) {
                                let _ = sock.send_to(&resp, peer);
                                continue;
                            }
                        }
                    }
                }
                if let Some(resp) = forward_dns_upstream(pkt) {
                    let _ = sock.send_to(&resp, peer);
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

fn download_and_save_blocklist() -> Result<HashSet<String>, String> {
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", "20", BLOCKLIST_URL])
        .output()
        .map_err(|e| format!("curl error: {}", e))?;
    if !out.status.success() {
        return Err(format!("HTTP/curl exit {:?}", out.status.code()));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut set: HashSet<String> = STARTER_BLOCKLIST.iter().map(|s| (*s).to_string()).collect();
    let mut saved_lines = Vec::new();
    for line in text.lines() {
        if let Some(dom) = parse_hosts_or_domain_line(line) {
            if set.insert(dom.clone()) {
                saved_lines.push(dom);
            }
        }
    }
    let _ = fs::create_dir_all(config_dir());
    let path = blocklist_path();
    let mut all_domains: Vec<&str> = STARTER_BLOCKLIST.to_vec();
    for d in &saved_lines {
        all_domains.push(d.as_str());
    }
    let _ = fs::write(&path, all_domains.join("\n"));
    Ok(set)
}

/// Updates `/etc/hosts` section `# BEGIN OKAY-ADSHIELD` .. `# END OKAY-ADSHIELD` via `pkexec` on Fedora.
fn apply_fedora_system_hosts(
    enabled: bool,
    blocklist: &HashSet<String>,
    whitelist: &HashSet<String>,
    blacklist: &HashSet<String>,
) -> String {
    let tmp_hosts = std::env::temp_dir().join("okay-adshield-hosts.txt");
    if enabled {
        let mut lines = Vec::new();
        lines.push("# BEGIN OKAY-ADSHIELD".to_string());
        for dom in blocklist.iter().chain(blacklist.iter()) {
            if !whitelist.contains(dom) {
                lines.push(format!("0.0.0.0 {}", dom));
            }
        }
        lines.push("# END OKAY-ADSHIELD".to_string());
        if let Err(e) = fs::write(&tmp_hosts, lines.join("\n")) {
            tracing::warn!("AdShield temp hosts write failed: {}", e);
            return "Active (DNS :5353)".to_string();
        }
    } else {
        let _ = fs::write(&tmp_hosts, "");
    }

    let tmp_path_str = tmp_hosts.to_string_lossy().to_string();
    let script = if enabled {
        format!(
            "sed -i '/^# BEGIN OKAY-ADSHIELD$/,/^# END OKAY-ADSHIELD$/d' /etc/hosts && cat '{}' >> /etc/hosts",
            tmp_path_str.replace('\'', "")
        )
    } else {
        "sed -i '/^# BEGIN OKAY-ADSHIELD$/,/^# END OKAY-ADSHIELD$/d' /etc/hosts".to_string()
    };

    match Command::new("pkexec").args(["sh", "-c", &script]).output() {
        Ok(out) if out.status.success() => {
            let _ = fs::remove_file(&tmp_hosts);
            if enabled {
                "Active (System /etc/hosts + DNS :5353)".to_string()
            } else {
                "Disabled".to_string()
            }
        }
        Ok(_) => {
            let _ = fs::remove_file(&tmp_hosts);
            if enabled {
                "Active (DNS :5353; /etc/hosts auth skipped)".to_string()
            } else {
                "Disabled (DNS :5353; /etc/hosts unchanged)".to_string()
            }
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp_hosts);
            tracing::warn!("AdShield pkexec unavailable: {}", e);
            if enabled {
                "Active (DNS :5353)".to_string()
            } else {
                "Disabled".to_string()
            }
        }
    }
}

/// Parses QNAME, QTYPE, and end offset of the first question from a DNS packet.
fn parse_dns_question(pkt: &[u8]) -> Option<(String, u16, usize)> {
    if pkt.len() < 12 {
        return None;
    }
    let qdcount = u16::from_be_bytes([pkt[4], pkt[5]]);
    if qdcount == 0 {
        return None;
    }
    let mut pos = 12usize;
    let mut labels = Vec::new();
    while pos < pkt.len() {
        let len = *pkt.get(pos)? as usize;
        pos += 1;
        if len == 0 {
            break;
        }
        if len >= 64 || pos + len > pkt.len() {
            return None;
        }
        let label = std::str::from_utf8(pkt.get(pos..pos + len)?).ok()?;
        labels.push(label.to_ascii_lowercase());
        pos += len;
    }
    if labels.is_empty() || pos + 4 > pkt.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([*pkt.get(pos)?, *pkt.get(pos + 1)?]);
    let qend = pos + 4;
    Some((labels.join("."), qtype, qend))
}

/// Builds a fast `0.0.0.0` (A) or `::` (AAAA) or NXDOMAIN DNS reply for a blocked query.
fn build_dns_block_response(req: &[u8], qtype: u16, qend: usize) -> Option<Vec<u8>> {
    if req.len() < 12 || qend > req.len() {
        return None;
    }
    let mut resp = Vec::with_capacity(qend + 32);
    // Header
    resp.extend_from_slice(&req[0..2]); // ID
    let rd = req[2] & 0x01;
    resp.push(0x81 | rd); // QR=1, RD copied
    resp.push(0x80); // RA=1, RCODE=0
    resp.extend_from_slice(&[0x00, 0x01]); // QDCOUNT=1
    if qtype == 1 || qtype == 28 {
        resp.extend_from_slice(&[0x00, 0x01]); // ANCOUNT=1
    } else {
        resp[3] = 0x83; // NXDOMAIN for other record types
        resp.extend_from_slice(&[0x00, 0x00]);
    }
    resp.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // NSCOUNT=0, ARCOUNT=0
    resp.extend_from_slice(req.get(12..qend)?);

    if qtype == 1 {
        // A -> 0.0.0.0
        resp.extend_from_slice(&[0xC0, 0x0C, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x3C, 0x00, 0x04]);
        resp.extend_from_slice(&[0, 0, 0, 0]);
    } else if qtype == 28 {
        // AAAA -> ::
        resp.extend_from_slice(&[0xC0, 0x0C, 0x00, 0x1C, 0x00, 0x01, 0x00, 0x00, 0x00, 0x3C, 0x00, 0x10]);
        resp.extend_from_slice(&[0u8; 16]);
    }
    Some(resp)
}

fn forward_dns_upstream(pkt: &[u8]) -> Option<Vec<u8>> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(1200))).ok()?;
    sock.send_to(pkt, UPSTREAM_DNS).ok()?;
    let mut rbuf = [0u8; 1500];
    let (n, _) = sock.recv_from(&mut rbuf).ok()?;
    Some(rbuf[..n].to_vec())
}
