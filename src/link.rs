//! One open RFCOMM connection to the headset: a reader thread, request/response matching,
//! and a sink that receives every frame (responses and pushed events alike).

use crate::frame::{self, Frame};
use crate::rfcomm::Stream;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub const CHANNEL: u8 = 15;

pub enum LinkEvent {
    Frame(Frame),
    Closed(String),
}

pub type Sink = Arc<dyn Fn(LinkEvent) + Send + Sync>;

struct Pending {
    response: u16,
    fsn: u8,
    tx: SyncSender<Frame>,
}

pub struct Link {
    mac: String,
    stream: Arc<Stream>,
    alive: Arc<AtomicBool>,
    pending: Arc<Mutex<Option<Pending>>>,
    /// Serialises requests: the headset answers one at a time anyway.
    request_lock: Mutex<u8>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl Link {
    pub fn open(mac: &str, timeout: Duration, sink: Sink) -> io::Result<Link> {
        let stream = Arc::new(Stream::connect(mac, CHANNEL, timeout)?);
        let alive = Arc::new(AtomicBool::new(true));
        let pending: Arc<Mutex<Option<Pending>>> = Arc::new(Mutex::new(None));
        let reader = {
            let (stream, alive, pending) = (stream.clone(), alive.clone(), pending.clone());
            std::thread::Builder::new()
                .name("rfcomm-reader".into())
                .spawn(move || read_loop(&stream, &alive, &pending, &sink))?
        };
        Ok(Link {
            mac: mac.to_string(),
            stream,
            alive,
            pending,
            request_lock: Mutex::new(0),
            reader: Mutex::new(Some(reader)),
        })
    }

    pub fn mac(&self) -> &str {
        &self.mac
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Sends one request and waits for its response (`cmd & 0x7FFF`, same fsn).
    pub fn request(&self, cmd: u16, payload: &[u8], timeout: Duration) -> io::Result<Frame> {
        let mut fsn_counter = self.request_lock.lock().unwrap();
        if !self.is_alive() {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "link closed"));
        }
        // Same sequence as the vendor app: 1..=255, then back to 1 (0 is never sent;
        // the headset uses fsn 0 for its own pushed events).
        *fsn_counter = if *fsn_counter == 255 { 1 } else { *fsn_counter + 1 };
        let fsn = *fsn_counter;
        let (tx, rx) = mpsc::sync_channel(1);
        *self.pending.lock().unwrap() = Some(Pending { response: frame::response_code(cmd), fsn, tx });
        let result = self.stream.write_all(&frame::encode(cmd, payload, fsn)).and_then(|()| {
            rx.recv_timeout(timeout).map_err(|e| match e {
                mpsc::RecvTimeoutError::Timeout => io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("no response to 0x{cmd:04X} within {} ms", timeout.as_millis()),
                ),
                mpsc::RecvTimeoutError::Disconnected => {
                    io::Error::new(io::ErrorKind::NotConnected, "link closed")
                }
            })
        });
        *self.pending.lock().unwrap() = None;
        if let Err(e) = &result {
            if e.kind() != io::ErrorKind::TimedOut {
                self.close();
            }
        }
        result
    }

    pub fn close(&self) {
        self.alive.store(false, Ordering::SeqCst);
        self.stream.shutdown();
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.close();
        if let Some(h) = self.reader.lock().unwrap().take() {
            // The last owner may be dropped from the reader thread itself (through the
            // sink): joining would then wait forever.
            if h.thread().id() != std::thread::current().id() {
                let _ = h.join();
            }
        }
    }
}

fn read_loop(stream: &Stream, alive: &AtomicBool, pending: &Mutex<Option<Pending>>, sink: &Sink) {
    let mut reader = frame::Reader::new();
    let mut buf = [0u8; 4096];
    let reason = loop {
        match stream.read(&mut buf) {
            Ok(0) => break "closed by the headset".to_string(),
            Ok(n) => {
                for f in reader.push(&buf[..n]) {
                    if let Some(p) = pending.lock().unwrap().as_ref() {
                        // Pushed reports reuse the response code with fsn 0 or 0xFF
                        // (e.g. 0x401F after an on-device EQ change): same data, accept them.
                        if f.cmd == p.response && (f.fsn == p.fsn || f.fsn == 0 || f.fsn == 0xFF) {
                            let _ = p.tx.try_send(f.clone());
                        }
                    }
                    sink(LinkEvent::Frame(f));
                }
            }
            Err(e) => break e.to_string(),
        }
    };
    let was_alive = alive.swap(false, Ordering::SeqCst);
    // Wake up a request waiting for its response.
    pending.lock().unwrap().take();
    if was_alive {
        sink(LinkEvent::Closed(reason));
    }
}
