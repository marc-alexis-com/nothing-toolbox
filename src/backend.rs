//! How a command reaches the headset: through a running owner of the channel (daemon, or
//! a long-running `watch`/`headtrack`/`gestures`), or by opening the channel itself.
//!
//! Local socket protocol (`$XDG_RUNTIME_DIR/nothing.sock`), one text line per message:
//!
//! ```text
//! -> PING                                   <- PONG <up|down> <mac|-> <model|->
//! -> REQ <cmd> <payload|-> <timeout_ms> <flags|->
//!                                           <- RSP <cmd> <fsn> <payload|->  |  ERR <message>
//! -> SUB [headtrack]                        <- OK, then a stream of:
//!                                              EVT <cmd> <fsn> <payload|->
//!                                              CON <mac> <model|->  |  DIS <reason>
//! ```
//! Numbers are hex, payloads are contiguous hex. Flags: `d` = dangerous, `m` = any model.

use crate::frame::{self, Frame};
use crate::hub::{Event, Hub, Opts, Subscription};
use crate::signals;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    dir.join("nothing.sock")
}

fn payload_hex(p: &[u8]) -> String {
    if p.is_empty() {
        "-".into()
    } else {
        p.iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn parse_payload(s: &str) -> Result<Vec<u8>, String> {
    if s == "-" {
        Ok(Vec::new())
    } else {
        frame::parse_hex(s)
    }
}

fn frame_line(f: &Frame) -> String {
    format!("EVT {:04x} {:02x} {}", f.cmd, f.fsn, payload_hex(&f.payload))
}

// --- Server -------------------------------------------------------------------------------

/// Serves the hub on the local socket from a background thread. Fails if another live
/// owner already listens there.
pub fn serve(hub: Arc<Hub>, path: &Path) -> Result<(), String> {
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(format!("another instance already owns the channel ({})", path.display()));
        }
        // Stale socket left by a crashed instance.
        let _ = std::fs::remove_file(path);
    }
    let listener =
        UnixListener::bind(path).map_err(|e| format!("cannot listen on {}: {e}", path.display()))?;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    std::thread::Builder::new()
        .name("socket-server".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let hub = hub.clone();
                std::thread::spawn(move || {
                    let _ = handle_client(&hub, conn);
                });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn handle_client(hub: &Arc<Hub>, conn: UnixStream) -> std::io::Result<()> {
    let mut out = conn.try_clone()?;
    let mut lines = BufReader::new(conn).lines();
    while let Some(line) = lines.next() {
        let line = line?;
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["PING"] => {
                let (up, mac, model) = hub.status();
                writeln!(
                    out,
                    "PONG {} {} {}",
                    if up { "up" } else { "down" },
                    mac.as_deref().unwrap_or("-"),
                    model.as_deref().unwrap_or("-")
                )?;
            }
            ["REQ", cmd, payload, timeout, flags] => {
                let reply = (|| {
                    let cmd = u16::from_str_radix(cmd, 16).map_err(|e| e.to_string())?;
                    let payload = parse_payload(payload)?;
                    let timeout = Duration::from_millis(timeout.parse().map_err(|_| "bad timeout")?);
                    let opts = Opts { dangerous: flags.contains('d'), any_model: flags.contains('m') };
                    hub.request(cmd, &payload, timeout, opts)
                })();
                match reply {
                    Ok(f) => writeln!(out, "RSP {:04x} {:02x} {}", f.cmd, f.fsn, payload_hex(&f.payload))?,
                    Err(e) => writeln!(out, "ERR {e}")?,
                }
            }
            ["SUB", rest @ ..] => {
                let sub = match hub.subscribe(rest.contains(&"headtrack")) {
                    Ok(s) => s,
                    Err(e) => {
                        writeln!(out, "ERR {e}")?;
                        continue;
                    }
                };
                writeln!(out, "OK")?;
                // From here on this connection only streams events. A second thread
                // watches for the client hanging up so that the subscription (and maybe
                // the head-tracking stream) ends right away, not at the next event.
                let sub = Arc::new(std::sync::Mutex::new(Some(sub)));
                let watcher = sub.clone();
                std::thread::spawn(move || {
                    for _ in lines.by_ref() {}
                    watcher.lock().unwrap().take();
                });
                loop {
                    let ev = {
                        let guard = sub.lock().unwrap();
                        let Some(s) = guard.as_ref() else { return Ok(()) };
                        match s.rx.recv_timeout(Duration::from_millis(200)) {
                            Ok(ev) => ev,
                            Err(RecvTimeoutError::Timeout) => continue,
                            Err(RecvTimeoutError::Disconnected) => return Ok(()),
                        }
                    };
                    let line = match ev {
                        Event::Frame(f) => frame_line(&f),
                        Event::Connected { mac, model } => {
                            format!("CON {mac} {}", model.as_deref().unwrap_or("-"))
                        }
                        Event::Disconnected(r) => format!("DIS {r}"),
                    };
                    if writeln!(out, "{line}").is_err() {
                        sub.lock().unwrap().take();
                        return Ok(());
                    }
                }
            }
            _ => writeln!(out, "ERR unknown request")?,
        }
    }
    Ok(())
}

// --- Client -------------------------------------------------------------------------------

pub struct DaemonClient {
    path: PathBuf,
}

impl DaemonClient {
    /// Connects only if a live owner answers PING.
    pub fn probe(path: &Path) -> Option<DaemonClient> {
        let c = DaemonClient { path: path.to_path_buf() };
        c.ping().ok()?;
        Some(c)
    }

    fn exchange(&self, line: &str) -> Result<String, String> {
        let mut s = UnixStream::connect(&self.path).map_err(|e| format!("daemon socket: {e}"))?;
        writeln!(s, "{line}").map_err(|e| e.to_string())?;
        let mut reply = String::new();
        BufReader::new(s).read_line(&mut reply).map_err(|e| e.to_string())?;
        Ok(reply.trim_end().to_string())
    }

    pub fn ping(&self) -> Result<String, String> {
        let r = self.exchange("PING")?;
        if r.starts_with("PONG ") {
            Ok(r)
        } else {
            Err(format!("unexpected answer: {r}"))
        }
    }

    fn request(&self, cmd: u16, payload: &[u8], timeout: Duration, opts: Opts) -> Result<Frame, String> {
        let flags =
            format!("{}{}", if opts.dangerous { "d" } else { "" }, if opts.any_model { "m" } else { "" });
        let reply = self.exchange(&format!(
            "REQ {cmd:04x} {} {} {}",
            payload_hex(payload),
            timeout.as_millis(),
            if flags.is_empty() { "-" } else { &flags }
        ))?;
        let w: Vec<&str> = reply.splitn(4, ' ').collect();
        match w.as_slice() {
            ["RSP", c, fsn, p] => Ok(Frame {
                ctl: frame::CTL_CLIENT,
                cmd: u16::from_str_radix(c, 16).map_err(|e| e.to_string())?,
                fsn: u8::from_str_radix(fsn, 16).map_err(|e| e.to_string())?,
                payload: parse_payload(p)?,
            }),
            ["ERR", ..] => Err(reply[4..].to_string()),
            _ => Err(format!("unexpected answer from the daemon: {reply}")),
        }
    }

    fn subscribe(&self, headtrack: bool) -> Result<Receiver<Event>, String> {
        let mut s = UnixStream::connect(&self.path).map_err(|e| format!("daemon socket: {e}"))?;
        writeln!(s, "SUB{}", if headtrack { " headtrack" } else { "" }).map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(s);
        let mut first = String::new();
        reader.read_line(&mut first).map_err(|e| e.to_string())?;
        if first.trim_end() != "OK" {
            return Err(first.trim_end().trim_start_matches("ERR ").to_string());
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in reader.lines() {
                let Ok(line) = line else { break };
                let Some(ev) = parse_event(&line) else { continue };
                if tx.send(ev).is_err() {
                    break; // dropping the reader closes the socket: the daemon unsubscribes
                }
            }
        });
        Ok(rx)
    }
}

fn parse_event(line: &str) -> Option<Event> {
    let w: Vec<&str> = line.splitn(4, ' ').collect();
    match w.as_slice() {
        ["EVT", c, fsn, p] => Some(Event::Frame(Frame {
            ctl: frame::CTL_CLIENT,
            cmd: u16::from_str_radix(c, 16).ok()?,
            fsn: u8::from_str_radix(fsn, 16).ok()?,
            payload: parse_payload(p).ok()?,
        })),
        ["CON", mac, model] => {
            Some(Event::Connected { mac: mac.to_string(), model: (*model != "-").then(|| model.to_string()) })
        }
        ["DIS", ..] => Some(Event::Disconnected(line[4..].to_string())),
        _ => None,
    }
}

// --- Backend ------------------------------------------------------------------------------

pub enum Backend {
    /// This process owns the channel.
    Local(Arc<Hub>),
    /// Another process owns it; we talk to it over the socket.
    Daemon(DaemonClient),
}

pub enum Events {
    Local(Subscription),
    Remote(Receiver<Event>),
}

impl Events {
    /// Waits for the next event; returns None on timeout, Err when the source is gone.
    pub fn next(&self, timeout: Duration) -> Result<Option<Event>, ()> {
        let rx = match self {
            Events::Local(s) => &s.rx,
            Events::Remote(r) => r,
        };
        match rx.recv_timeout(timeout) {
            Ok(ev) => Ok(Some(ev)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(()),
        }
    }
}

impl Backend {
    /// One-shot commands: use the owner if there is one, else open the channel briefly.
    pub fn one_shot(mac: Option<String>) -> Backend {
        match DaemonClient::probe(&socket_path()) {
            Some(c) => Backend::Daemon(c),
            None => Backend::Local(Hub::new(mac, false)),
        }
    }

    /// Long-running commands: use the owner if there is one, else BECOME the owner, so
    /// that one-shot commands (the ANC key) keep working while we run.
    pub fn long_running(mac: Option<String>, verbose: bool) -> Backend {
        let path = socket_path();
        if let Some(c) = DaemonClient::probe(&path) {
            return Backend::Daemon(c);
        }
        let hub = Hub::new(mac, verbose);
        match serve(hub.clone(), &path) {
            Ok(()) => {
                signals::remove_on_exit(path);
                hub.keep_alive();
                Backend::Local(hub)
            }
            Err(e) => match DaemonClient::probe(&path) {
                // Lost a race against another instance that just started: use it.
                Some(c) => Backend::Daemon(c),
                None => {
                    eprintln!("warning: {e}; other commands will not reach the headset meanwhile");
                    hub.keep_alive();
                    Backend::Local(hub)
                }
            },
        }
    }

    pub fn request(&self, cmd: u16, payload: &[u8], timeout: Duration, opts: Opts) -> Result<Frame, String> {
        match self {
            Backend::Local(h) => h.request(cmd, payload, timeout, opts),
            Backend::Daemon(c) => c.request(cmd, payload, timeout, opts),
        }
    }

    pub fn subscribe(&self, headtrack: bool) -> Result<Events, String> {
        match self {
            Backend::Local(h) => h.subscribe(headtrack).map(Events::Local),
            Backend::Daemon(c) => c.subscribe(headtrack).map(Events::Remote),
        }
    }

    pub fn shutdown(&self) {
        if let Backend::Local(h) = self {
            h.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_lines_round_trip() {
        let f = Frame { ctl: frame::CTL_CLIENT, cmd: 0xE014, fsn: 0, payload: vec![0x20, 0x01] };
        match parse_event(&frame_line(&f)) {
            Some(Event::Frame(g)) => assert_eq!(g, f),
            _ => panic!("frame line not parsed"),
        }
        let empty = Frame { payload: vec![], ..f.clone() };
        match parse_event(&frame_line(&empty)) {
            Some(Event::Frame(g)) => assert!(g.payload.is_empty()),
            _ => panic!("empty payload not parsed"),
        }
        assert!(
            matches!(parse_event("DIS closed by the headset"), Some(Event::Disconnected(r)) if r == "closed by the headset")
        );
        assert!(
            matches!(parse_event("CON 12:34:56:78:9A:BC B170"), Some(Event::Connected { model: Some(m), .. }) if m == "B170")
        );
    }
}
