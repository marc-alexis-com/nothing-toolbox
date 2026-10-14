//! The single owner of the control channel.
//!
//! RFCOMM channel 15 accepts ONE connection at a time: whoever holds it locks everybody
//! else out. The hub owns it, reconnects when asked (or continuously in daemon mode),
//! fans pushed events out to subscribers, and reference-counts the head-tracking stream
//! so that it runs exactly while someone listens to it.

use crate::cmd::{self, Risk};
use crate::decode;
use crate::frame::Frame;
use crate::link::{Link, LinkEvent, Sink};
use crate::mac;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MODEL_TIMEOUT: Duration = Duration::from_millis(1500);
const STREAM_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, Debug, Default)]
pub struct Opts {
    /// The user explicitly allowed risky commands.
    pub dangerous: bool,
    /// Allow writes to a headset that does not report model B170.
    pub any_model: bool,
}

#[derive(Clone, Debug)]
pub enum Event {
    Frame(Frame),
    Connected { mac: String, model: Option<String> },
    Disconnected(String),
}

struct Sub {
    id: u64,
    tx: Sender<Event>,
    headtrack: bool,
}

pub struct Hub {
    mac_arg: Option<String>,
    link: Mutex<Option<Arc<Link>>>,
    model: Mutex<Option<String>>,
    subs: Mutex<Vec<Sub>>,
    next_id: AtomicU64,
    /// What the headset was last told about the head-tracking stream.
    stream_on: Mutex<bool>,
    stopping: AtomicBool,
    verbose: bool,
}

pub struct Subscription {
    hub: Arc<Hub>,
    id: u64,
    pub rx: Receiver<Event>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.hub.unsubscribe(self.id);
    }
}

impl Hub {
    pub fn new(mac_arg: Option<String>, verbose: bool) -> Arc<Hub> {
        Arc::new(Hub {
            mac_arg,
            link: Mutex::new(None),
            model: Mutex::new(None),
            subs: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            stream_on: Mutex::new(false),
            stopping: AtomicBool::new(false),
            verbose,
        })
    }

    fn log(&self, msg: &str) {
        if self.verbose {
            eprintln!("{msg}");
        }
    }

    pub fn status(&self) -> (bool, Option<String>, Option<String>) {
        let link = self.link.lock().unwrap();
        match link.as_ref().filter(|l| l.is_alive()) {
            Some(l) => (true, Some(l.mac().to_string()), self.model.lock().unwrap().clone()),
            None => (false, None, None),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.status().0
    }

    /// Returns the live link, connecting first if needed.
    pub fn ensure_link(self: &Arc<Self>) -> Result<Arc<Link>, String> {
        let mut guard = self.link.lock().unwrap();
        if let Some(l) = guard.as_ref() {
            if l.is_alive() {
                return Ok(l.clone());
            }
        }
        *guard = None;
        let mac = mac::resolve(self.mac_arg.as_deref())?;
        let weak: Weak<Hub> = Arc::downgrade(self);
        let sink: Sink = Arc::new(move |ev| {
            if let Some(hub) = weak.upgrade() {
                hub.on_link_event(ev);
            }
        });
        let link = Arc::new(
            Link::open(&mac, CONNECT_TIMEOUT, sink)
                .map_err(|e| format!("cannot open RFCOMM channel 15 on {mac}: {e}"))?,
        );
        let model =
            link.request(cmd::GET_MODEL, &[], MODEL_TIMEOUT).ok().and_then(|f| decode::model(&f.payload));
        *self.model.lock().unwrap() = model.clone();
        *guard = Some(link.clone());
        drop(guard);
        *self.stream_on.lock().unwrap() = false;
        self.log(&format!("connected to {mac} (model {})", model.as_deref().unwrap_or("unknown")));
        self.broadcast(Event::Connected { mac, model });
        // A reconnect must resume the head-tracking stream for its listeners.
        let _ = self.sync_stream();
        Ok(link)
    }

    fn on_link_event(self: &Arc<Self>, ev: LinkEvent) {
        match ev {
            LinkEvent::Frame(f) => self.broadcast(Event::Frame(f)),
            LinkEvent::Closed(reason) => {
                self.log(&format!("disconnected: {reason}"));
                self.broadcast(Event::Disconnected(reason));
            }
        }
    }

    fn broadcast(self: &Arc<Self>, ev: Event) {
        let mut subs = self.subs.lock().unwrap();
        let before = subs.iter().any(|s| s.headtrack);
        subs.retain(|s| s.tx.send(ev.clone()).is_ok());
        let after = subs.iter().any(|s| s.headtrack);
        drop(subs);
        if before && !after {
            // We are on the reader thread: a request from here would wait for itself.
            let hub = self.clone();
            std::thread::spawn(move || {
                let _ = hub.sync_stream();
            });
        }
    }

    /// One request, through the safety policy and the model check.
    pub fn request(
        self: &Arc<Self>,
        cmd_code: u16,
        payload: &[u8],
        timeout: Duration,
        opts: Opts,
    ) -> Result<Frame, String> {
        cmd::check(cmd_code, opts.dangerous)?;
        let link = self.ensure_link()?;
        if cmd::risk(cmd_code) != Risk::Read && !opts.any_model {
            let model = self.model.lock().unwrap().clone();
            if model.as_deref() != Some(cmd::SUPPORTED_MODEL) {
                return Err(format!(
                    "refusing to write: the headset reports model {}, only {} is supported \
                     (--any-model overrides)",
                    model.as_deref().unwrap_or("unknown"),
                    cmd::SUPPORTED_MODEL
                ));
            }
        }
        link.request(cmd_code, payload, timeout).map_err(|e| e.to_string())
    }

    pub fn subscribe(self: &Arc<Self>, headtrack: bool) -> Result<Subscription, String> {
        let (tx, rx) = mpsc::channel();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.subs.lock().unwrap().push(Sub { id, tx, headtrack });
        let sub = Subscription { hub: self.clone(), id, rx };
        self.ensure_link()?;
        if headtrack {
            self.sync_stream()?;
        }
        Ok(sub)
    }

    fn unsubscribe(self: &Arc<Self>, id: u64) {
        let mut subs = self.subs.lock().unwrap();
        subs.retain(|s| s.id != id);
        drop(subs);
        let _ = self.sync_stream();
    }

    /// Starts or stops the 0xE015 stream so that it runs iff a subscriber wants it.
    fn sync_stream(self: &Arc<Self>) -> Result<(), String> {
        let mut on = self.stream_on.lock().unwrap();
        let want =
            !self.stopping.load(Ordering::SeqCst) && self.subs.lock().unwrap().iter().any(|s| s.headtrack);
        if want == *on {
            return Ok(());
        }
        let link = {
            let guard = self.link.lock().unwrap();
            guard.as_ref().filter(|l| l.is_alive()).cloned()
        };
        let Some(link) = link else {
            // Nothing to stop on a dead link; a reconnect calls us again to start.
            *on = false;
            return if want { Err("headset not connected".into()) } else { Ok(()) };
        };
        // The B170 answers 0x4058; a few 0xE015 frames may still follow a stop.
        match link.request(cmd::HEADTRACK_STREAM, &[want as u8], STREAM_TIMEOUT) {
            Ok(_) => {
                *on = want;
                self.log(if want { "head-tracking stream started" } else { "head-tracking stream stopped" });
                Ok(())
            }
            Err(e) => {
                Err(format!("cannot {} the head-tracking stream: {e}", if want { "start" } else { "stop" }))
            }
        }
    }

    /// Daemon mode: keeps the channel open, reconnecting with backoff.
    pub fn keep_alive(self: &Arc<Self>) {
        let hub = self.clone();
        std::thread::Builder::new()
            .name("keep-alive".into())
            .spawn(move || {
                let mut backoff = Duration::from_secs(1);
                let mut next_try = Instant::now();
                let mut last_error = String::new();
                while !hub.stopping.load(Ordering::SeqCst) {
                    if !hub.is_connected() && Instant::now() >= next_try {
                        match hub.ensure_link() {
                            Ok(_) => {
                                backoff = Duration::from_secs(1);
                                last_error.clear();
                            }
                            Err(e) => {
                                if e != last_error {
                                    eprintln!("{e} (retrying)");
                                    last_error = e;
                                }
                                next_try = Instant::now() + backoff;
                                backoff = (backoff * 2).min(Duration::from_secs(30));
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            })
            .expect("spawn keep-alive thread");
    }

    /// Stops the head-tracking stream if it runs, then closes the channel.
    pub fn shutdown(self: &Arc<Self>) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.sync_stream();
        if let Some(l) = self.link.lock().unwrap().take() {
            l.close();
        }
    }
}
