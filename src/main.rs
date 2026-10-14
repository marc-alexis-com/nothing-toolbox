//! `nothing`: Linux control of the Nothing Headphone (1) (model B170) over its RFCOMM
//! control channel. Other Nothing products are untested and unsupported.

mod backend;
mod cmd;
mod decode;
mod frame;
mod headtrack;
mod hub;
mod link;
mod logs;
mod mac;
mod rfcomm;
mod signals;
mod viz;

use backend::{Backend, Events};
use clap::{Args, Parser, Subcommand, ValueEnum};
use decode::AncMode;
use hub::{Event, Opts};
use std::io::Write;
use std::process::ExitCode;
use std::time::{Duration, Instant};

type Res<T> = Result<T, String>;

#[derive(Parser)]
#[command(
    name = "nothing",
    version,
    about = "Control a Nothing Headphone (1) (B170) from Linux",
    long_about = "Control a Nothing Headphone (1) (model B170) from Linux, over its RFCOMM \
                  control channel (15).\n\nThe channel accepts a single connection: long-running \
                  commands (daemon, watch, headtrack, gestures) own it and serve the others \
                  through $XDG_RUNTIME_DIR/nothing.sock, so `nothing anc toggle` keeps working \
                  while they run.\n\nOnly the B170 has been tested. Writes are refused on any \
                  other model unless --any-model is given."
)]
struct Cli {
    /// Headset address (default: $NOTHING_MAC, else the first connected Nothing headset)
    #[arg(long, global = true)]
    mac: Option<String>,
    /// Allow risky commands (codec change with reboot, raw untested codes)
    #[arg(long, global = true)]
    dangerous: bool,
    /// Allow writes to a headset that does not report model B170
    #[arg(long, global = true)]
    any_model: bool,
    /// Response timeout in milliseconds
    #[arg(long, global = true, default_value_t = 2000)]
    timeout: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Noise control: active noise cancelling, transparency, off
    Anc {
        #[command(subcommand)]
        action: AncAction,
    },
    /// Everything at a glance
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Battery level
    Battery {
        #[arg(long)]
        json: bool,
    },
    /// Model, firmware, color, serial number, paired devices
    Info,
    /// Equaliser preset, custom and advanced curves
    Eq {
        #[command(subcommand)]
        action: EqAction,
    },
    /// Bluetooth codec (changing it reboots the headset: needs --dangerous)
    Codec {
        #[command(subcommand)]
        action: CodecAction,
    },
    /// Bass boost
    Bass {
        #[command(subcommand)]
        action: BassAction,
    },
    /// Spatial audio
    Spatial {
        #[command(subcommand)]
        action: SpatialAction,
    },
    /// Low-latency mode
    Latency(SwitchArgs),
    /// Connection to two devices at once
    Multipoint(SwitchArgs),
    /// Wear detection
    InEar(SwitchArgs),
    /// Audiodo personal sound profile
    Audiodo(SwitchArgs),
    /// Print events pushed by the headset (battery, ANC, magic button...)
    Watch {
        /// Shell command to run when the magic button is pressed ($NOTHING_PAYLOAD holds the hex payload)
        #[arg(long, value_name = "CMD")]
        on_magic: Option<String>,
        /// Also print the head-tracking stream
        #[arg(long)]
        headtrack: bool,
        /// Print every frame, including responses to other clients
        #[arg(long)]
        raw: bool,
    },
    /// Head-tracking stream (quaternions), as text or as a 3D view
    Headtrack {
        /// Terminal 3D view (arrows/hjkl: orbit, 0: reset view, r/space: recenter, q: quit)
        #[arg(long)]
        viz: bool,
    },
    /// Media control by head gestures: nod = play/pause, shake = next track
    Gestures(GestureArgs),
    /// Diagnostic logs (read-only)
    Logs {
        #[command(subcommand)]
        action: LogsAction,
    },
    /// Own the control channel and serve other commands through the local socket
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
        /// Log connections and stream changes
        #[arg(long, short)]
        verbose: bool,
    },
    /// Send one raw request and print the answer (reads are free, writes are policed)
    Raw {
        /// Command code, hex (e.g. C007)
        cmd: String,
        /// Payload, hex
        payload: Option<String>,
    },
}

#[derive(Subcommand)]
enum AncAction {
    /// Print the current mode
    Get,
    /// Set a mode
    Set { mode: AncArg },
    /// Noise cancelling <-> transparency (from any other mode: back to the last ANC level)
    Toggle,
}

#[derive(Clone, Copy, ValueEnum)]
enum AncArg {
    High,
    Mid,
    Low,
    Adaptive,
    Off,
    Transparency,
}

impl From<AncArg> for AncMode {
    fn from(a: AncArg) -> AncMode {
        match a {
            AncArg::High => AncMode::High,
            AncArg::Mid => AncMode::Mid,
            AncArg::Low => AncMode::Low,
            AncArg::Adaptive => AncMode::Adaptive,
            AncArg::Off => AncMode::Off,
            AncArg::Transparency => AncMode::Transparency,
        }
    }
}

#[derive(Subcommand)]
enum EqAction {
    /// Print the active preset
    Get,
    /// Select a preset
    Set { preset: EqPreset },
    /// Print the custom (3-band) curve
    Custom,
    /// Print the advanced (8-band) curve
    Advanced,
}

#[derive(Clone, Copy, ValueEnum)]
enum EqPreset {
    Balanced,
    MoreBass,
    MoreTreble,
    Voice,
    Custom,
    Advanced,
}

#[derive(Subcommand)]
enum CodecAction {
    Get,
    /// Change the codec. The headset reboots (~6 s). Needs --dangerous.
    Set {
        codec: CodecArg,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum CodecArg {
    Aac,
    Ldac,
}

#[derive(Subcommand)]
enum BassAction {
    Get,
    Off,
    /// Turn on, at LEVEL (1-5; 1 and 2 were tested)
    On {
        #[arg(default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=5))]
        level: u8,
    },
}

#[derive(Subcommand)]
enum SpatialAction {
    Get,
    Set { mode: SpatialArg },
}

#[derive(Clone, Copy, ValueEnum)]
enum SpatialArg {
    Off,
    On,
    HeadTracking,
}

#[derive(Args)]
struct SwitchArgs {
    #[command(subcommand)]
    action: SwitchAction,
}

#[derive(Subcommand)]
enum SwitchAction {
    Get,
    On,
    Off,
}

#[derive(Args)]
struct GestureArgs {
    /// Print detections without running anything
    #[arg(long)]
    dry_run: bool,
    /// Print per-sample swing and speed, to tune the thresholds
    #[arg(long)]
    debug: bool,
    #[arg(long, default_value = "playerctl play-pause", value_name = "CMD")]
    nod_cmd: String,
    #[arg(long, default_value = "playerctl next", value_name = "CMD")]
    shake_cmd: String,
    /// Minimum nod amplitude, degrees peak-to-peak
    #[arg(long)]
    nod_amplitude: Option<f64>,
    /// Minimum nod speed, degrees per second, both ways
    #[arg(long)]
    nod_speed: Option<f64>,
    #[arg(long)]
    shake_amplitude: Option<f64>,
    #[arg(long)]
    shake_speed: Option<f64>,
}

#[derive(Subcommand)]
enum LogsAction {
    /// List the log files kept by the headset (FC03)
    List,
    /// Whether a buried log is available, and its size (FC21)
    Info,
    /// Logging state (FC09)
    Debug,
    /// Download the buried log, packet by packet (FC21 then FC22)
    Dump {
        /// Write the data to FILE instead of printing a hex dump
        #[arg(long, short)]
        output: Option<std::path::PathBuf>,
        /// Stop after this many packets
        #[arg(long, default_value_t = 2048)]
        max_packets: u16,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Tell whether a daemon owns the channel
    Status,
}

struct Ctx {
    backend: Backend,
    opts: Opts,
    timeout: Duration,
}

impl Ctx {
    fn request(&self, cmd: u16, payload: &[u8]) -> Res<Vec<u8>> {
        self.backend.request(cmd, payload, self.timeout, self.opts).map(|f| f.payload)
    }

    fn get(&self, cmd: u16) -> Res<Vec<u8>> {
        self.request(cmd, &[])
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let summary: Vec<String> = std::env::args().skip(1).collect();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nothing: {e}");
            log_failure(&summary.join(" "), &e);
            ExitCode::FAILURE
        }
    }
}

/// Failures also go to $XDG_RUNTIME_DIR/nothing.log: when a key binding runs us, stderr
/// is thrown away and a failed key press would otherwise leave no trace.
fn log_failure(args: &str, err: &str) {
    let path = backend::socket_path().with_file_name("nothing.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {args}: {err}", clock());
    }
}

fn clock() -> String {
    // SAFETY: time/localtime_r with valid pointers.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
    }
}

fn run(cli: Cli) -> Res<()> {
    let opts = Opts { dangerous: cli.dangerous, any_model: cli.any_model };
    let timeout = Duration::from_millis(cli.timeout);
    let one_shot = || Ctx { backend: Backend::one_shot(cli.mac.clone()), opts, timeout };
    match cli.command {
        Command::Anc { action } => anc(&one_shot(), action),
        Command::Status { json } => status(&one_shot(), json),
        Command::Battery { json } => battery(&one_shot(), json),
        Command::Info => info(&one_shot()),
        Command::Eq { action } => eq(&one_shot(), action),
        Command::Codec { action } => codec(&one_shot(), action),
        Command::Bass { action } => bass(&one_shot(), action),
        Command::Spatial { action } => spatial(&one_shot(), action),
        Command::Latency(a) => switch(&one_shot(), a.action, Switch::LATENCY),
        Command::Multipoint(a) => switch(&one_shot(), a.action, Switch::MULTIPOINT),
        Command::InEar(a) => switch(&one_shot(), a.action, Switch::IN_EAR),
        Command::Audiodo(a) => switch(&one_shot(), a.action, Switch::AUDIODO),
        Command::Raw { cmd, payload } => raw(&one_shot(), &cmd, payload.as_deref()),
        Command::Logs { action } => logs(&one_shot(), action),
        Command::Daemon { action: Some(DaemonAction::Status), .. } => daemon_status(),
        Command::Daemon { action: None, verbose } => daemon(cli.mac, verbose),
        Command::Watch { on_magic, headtrack, raw } => {
            long_running(cli.mac, opts, timeout, |ctx| watch(ctx, on_magic.as_deref(), headtrack, raw))
        }
        Command::Headtrack { viz } => long_running(cli.mac, opts, timeout, |ctx| headtrack_cmd(ctx, viz)),
        Command::Gestures(args) => long_running(cli.mac, opts, timeout, |ctx| gestures(ctx, &args)),
    }
}

/// Runs `f` as the channel owner (or as a client of the current owner), with Ctrl+C
/// handled so that the head-tracking stream is stopped and the socket removed.
fn long_running(
    mac: Option<String>,
    opts: Opts,
    timeout: Duration,
    f: impl FnOnce(&Ctx) -> Res<()>,
) -> Res<()> {
    signals::install();
    let ctx = Ctx { backend: Backend::long_running(mac, false), opts, timeout };
    let result = f(&ctx);
    ctx.backend.shutdown();
    signals::cleanup();
    result
}

// --- ANC ----------------------------------------------------------------------------------

fn read_anc(ctx: &Ctx) -> Res<decode::AncState> {
    let st = decode::anc(&ctx.request(cmd::GET_ANC, &[0x03])?);
    if st.mode.is_none() {
        return Err("the headset did not report its noise control mode".into());
    }
    Ok(st)
}

fn write_anc(ctx: &Ctx, mode: AncMode) -> Res<()> {
    // The acknowledgement (0x700F) is awaited so that two quick key presses do not overlap.
    ctx.request(cmd::SET_ANC, &[0x01, mode as u8, 0x00]).map(|_| ())
}

fn anc(ctx: &Ctx, action: AncAction) -> Res<()> {
    match action {
        AncAction::Get => println!("{}", decode::anc_name(read_anc(ctx)?.mode.unwrap())),
        AncAction::Set { mode } => {
            let mode = AncMode::from(mode);
            write_anc(ctx, mode)?;
            println!("{}", mode.name());
        }
        AncAction::Toggle => {
            // Read instead of keeping state: the phone app or the headset button may have
            // changed the mode behind our back.
            let st = read_anc(ctx)?;
            let current = st.mode.and_then(AncMode::from_byte);
            let next = if current.is_some_and(AncMode::is_anc) {
                AncMode::Transparency
            } else {
                st.last_level.and_then(AncMode::from_byte).filter(|m| m.is_anc()).unwrap_or(AncMode::High)
            };
            write_anc(ctx, next)?;
            println!("{}", next.name());
        }
    }
    Ok(())
}

// --- Status -------------------------------------------------------------------------------

struct Field {
    key: &'static str,
    label: &'static str,
    value: Res<String>,
}

fn status_fields(ctx: &Ctx) -> Vec<Field> {
    let f = |key, label, cmd: u16, payload: &[u8], dec: fn(&[u8]) -> String| Field {
        key,
        label,
        value: ctx.request(cmd, payload).map(|p| dec(&p)),
    };
    vec![
        f("model", "Model", cmd::GET_MODEL, &[], |p| decode::model(p).unwrap_or_else(|| "?".into())),
        f("firmware", "Firmware", cmd::GET_FIRMWARE, &[], decode::text),
        f("battery", "Battery", cmd::GET_BATTERY, &[], |p| {
            let b = decode::battery(p);
            if b.is_empty() {
                "?".into()
            } else {
                b.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", ")
            }
        }),
        f("state", "State", cmd::GET_DEVICE_STATUS, &[], decode::device_status_text),
        f("anc", "Noise control", cmd::GET_ANC, &[0x03], |p| decode::anc(p).to_string()),
        f("codec", "Codec", cmd::GET_CODEC, &[], decode::codec),
        f("eq", "EQ preset", cmd::GET_EQ_MODE, &[], decode::eq_preset),
        f("advanced_eq", "Advanced EQ", cmd::GET_ADVANCED_EQ_MODE, &[], decode::switch),
        f("bass_boost", "Bass boost", cmd::GET_BASS_BOOST, &[], |p| decode::bass_boost(p).to_string()),
        f("spatial", "Spatial audio", cmd::GET_SPATIAL, &[], decode::spatial),
        f("latency", "Low latency", cmd::GET_LATENCY, &[], decode::latency),
        f("multipoint", "Multipoint", cmd::GET_MULTIPOINT, &[], decode::switch),
        f("in_ear", "In-ear detection", cmd::GET_EXTRA_FEATURES, &[], |p| {
            match decode::in_ear_detection(p) {
                Some(true) => "on".into(),
                Some(false) => "off".into(),
                None => "?".into(),
            }
        }),
        f("audiodo", "Audiodo profile", cmd::GET_AUDIODO, &[], decode::switch),
    ]
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn status(ctx: &Ctx, json: bool) -> Res<()> {
    let fields = status_fields(ctx);
    if fields.iter().all(|f| f.value.is_err()) {
        return Err(fields[0].value.clone().unwrap_err());
    }
    if json {
        let body: Vec<String> = fields
            .iter()
            .map(|f| {
                format!(
                    "{}:{}",
                    json_str(f.key),
                    f.value.as_ref().map(|v| json_str(v)).unwrap_or("null".into())
                )
            })
            .collect();
        println!("{{{}}}", body.join(","));
    } else {
        for f in &fields {
            match &f.value {
                Ok(v) => println!("{:<17} {v}", f.label),
                Err(e) => println!("{:<17} ? ({e})", f.label),
            }
        }
    }
    Ok(())
}

fn battery(ctx: &Ctx, json: bool) -> Res<()> {
    let list = decode::battery(&ctx.get(cmd::GET_BATTERY)?);
    let b = list.first().ok_or("empty battery report")?;
    if json {
        println!("{{\"level\":{},\"charging\":{}}}", b.level, b.charging);
    } else {
        println!("{}%{}", b.level, if b.charging { " (charging)" } else { "" });
    }
    Ok(())
}

fn info(ctx: &Ctx) -> Res<()> {
    let model = ctx.get(cmd::GET_MODEL)?;
    println!("Model      {}", decode::model(&model).unwrap_or_else(|| "?".into()));
    let line = |label: &str, r: Res<String>| match r {
        Ok(v) => println!("{label:<10} {v}"),
        Err(e) => println!("{label:<10} ? ({e})"),
    };
    line("Firmware", ctx.get(cmd::GET_FIRMWARE).map(|p| decode::text(&p)));
    line("Color", ctx.get(cmd::GET_COLOR).map(|p| decode::color(&p)));
    line("Serial", ctx.get(cmd::GET_SERIAL).map(|p| decode::serial(&p).unwrap_or_else(|| "?".into())));
    line(
        "Features",
        ctx.get(cmd::GET_FEATURES).map(|p| {
            let mut w = [0u8; 4];
            for (i, b) in p.iter().take(4).enumerate() {
                w[i] = *b;
            }
            format!("0x{:08X}", u32::from_le_bytes(w))
        }),
    );
    println!("Paired devices:");
    let mut packet = 0u8;
    loop {
        let (total, list) = decode::paired_list(&ctx.request(cmd::GET_PAIRED_LIST, &[packet])?);
        for d in list {
            println!(
                "  {}  {}{}{}{}",
                d.mac,
                d.name,
                if d.name_truncated { "..." } else { "" },
                if d.connected { "  [connected]" } else { "" },
                if d.current { "  [this computer]" } else { "" }
            );
        }
        packet += 1;
        if packet >= total || packet == u8::MAX {
            break;
        }
    }
    Ok(())
}

// --- Audio settings -----------------------------------------------------------------------

fn eq(ctx: &Ctx, action: EqAction) -> Res<()> {
    match action {
        EqAction::Get => println!("{}", decode::eq_preset(&ctx.get(cmd::GET_EQ_MODE)?)),
        EqAction::Set { preset } => {
            if let EqPreset::Advanced = preset {
                ctx.request(cmd::SET_ADVANCED_EQ_MODE, &[1])?;
            } else {
                let name = preset.to_possible_value().unwrap().get_name().to_string();
                // Same sequence as the vendor app: leave the advanced EQ first.
                if decode::switch(&ctx.get(cmd::GET_ADVANCED_EQ_MODE)?) == "on" {
                    ctx.request(cmd::SET_ADVANCED_EQ_MODE, &[0])?;
                }
                ctx.request(cmd::SET_EQ_MODE, &[decode::eq_preset_byte(&name).unwrap()])?;
            }
            println!("{}", decode::eq_preset(&ctx.get(cmd::GET_EQ_MODE)?));
        }
        EqAction::Custom => {
            let eq = decode::custom_eq(&ctx.get(cmd::GET_CUSTOM_EQ)?).ok_or("malformed custom EQ")?;
            println!("{eq}");
        }
        EqAction::Advanced => {
            let eq = decode::advanced_eq(&ctx.request(cmd::GET_ADVANCED_EQ, &[0])?)
                .ok_or("malformed advanced EQ")?;
            println!("{eq}");
        }
    }
    Ok(())
}

fn codec(ctx: &Ctx, action: CodecAction) -> Res<()> {
    match action {
        CodecAction::Get => println!("{}", decode::codec(&ctx.get(cmd::GET_CODEC)?)),
        CodecAction::Set { codec } => {
            let byte = match codec {
                CodecArg::Aac => 0,
                CodecArg::Ldac => 2,
            };
            cmd::check(cmd::SET_CODEC, ctx.opts.dangerous)?;
            // The headset reboots instead of acknowledging: a timeout is the normal outcome.
            match ctx.request(cmd::SET_CODEC, &[byte]) {
                Ok(_) => {}
                Err(e) if e.contains("no response") || e.contains("closed") => {}
                Err(e) => return Err(e),
            }
            println!("codec change sent; the headset reboots (about 6 s)");
        }
    }
    Ok(())
}

fn bass(ctx: &Ctx, action: BassAction) -> Res<()> {
    match action {
        BassAction::Get => {}
        BassAction::Off => {
            let current = decode::bass_boost(&ctx.get(cmd::GET_BASS_BOOST)?);
            ctx.request(cmd::SET_BASS_BOOST, &[0, current.level * 2])?;
        }
        BassAction::On { level } => {
            ctx.request(cmd::SET_BASS_BOOST, &[1, level * 2])?;
        }
    }
    println!("{}", decode::bass_boost(&ctx.get(cmd::GET_BASS_BOOST)?));
    Ok(())
}

fn spatial(ctx: &Ctx, action: SpatialAction) -> Res<()> {
    if let SpatialAction::Set { mode } = action {
        let name = mode.to_possible_value().unwrap().get_name().to_string();
        ctx.request(cmd::SET_SPATIAL, &decode::spatial_payload(&name).unwrap())?;
    }
    println!("{}", decode::spatial(&ctx.get(cmd::GET_SPATIAL)?));
    Ok(())
}

struct Switch {
    get: u16,
    get_payload: &'static [u8],
    set: u16,
    on: &'static [u8],
    off: &'static [u8],
    show: fn(&[u8]) -> String,
}

impl Switch {
    const LATENCY: Switch = Switch {
        get: cmd::GET_LATENCY,
        get_payload: &[],
        set: cmd::SET_LATENCY,
        on: &[1],
        off: &[2],
        show: decode::latency,
    };
    const MULTIPOINT: Switch = Switch {
        get: cmd::GET_MULTIPOINT,
        get_payload: &[],
        set: cmd::SET_MULTIPOINT,
        on: &[1],
        off: &[0],
        show: decode::switch,
    };
    const AUDIODO: Switch = Switch {
        get: cmd::GET_AUDIODO,
        get_payload: &[],
        set: cmd::SET_AUDIODO,
        on: &[1],
        off: &[0],
        show: decode::switch,
    };
    /// Extra feature id 1 (in-ear detection) of 0xF004 `[count, id, value]`.
    const IN_EAR: Switch = Switch {
        get: cmd::GET_EXTRA_FEATURES,
        get_payload: &[],
        set: cmd::SET_EXTRA_FEATURE,
        on: &[1, 1, 1],
        off: &[1, 1, 0],
        show: |p| match decode::in_ear_detection(p) {
            Some(true) => "on".into(),
            Some(false) => "off".into(),
            None => "?".into(),
        },
    };
}

fn switch(ctx: &Ctx, action: SwitchAction, s: Switch) -> Res<()> {
    match action {
        SwitchAction::Get => {}
        SwitchAction::On => {
            ctx.request(s.set, s.on)?;
        }
        SwitchAction::Off => {
            ctx.request(s.set, s.off)?;
        }
    }
    println!("{}", (s.show)(&ctx.request(s.get, s.get_payload)?));
    Ok(())
}

// --- Raw ----------------------------------------------------------------------------------

fn raw(ctx: &Ctx, cmd_text: &str, payload: Option<&str>) -> Res<()> {
    let code = u16::from_str_radix(cmd_text.trim_start_matches("0x"), 16)
        .map_err(|e| format!("{cmd_text:?}: {e}"))?;
    let payload = payload.map(frame::parse_hex).transpose()?.unwrap_or_default();
    let f = ctx.backend.request(code, &payload, ctx.timeout, ctx.opts)?;
    println!(
        "{:04X} fsn={} [{}]{}",
        f.cmd,
        f.fsn,
        frame::hex(&f.payload),
        cmd::name(code).map(|n| format!("  {n}")).unwrap_or_default()
    );
    Ok(())
}

// --- Logs ---------------------------------------------------------------------------------

fn logs(ctx: &Ctx, action: LogsAction) -> Res<()> {
    match action {
        LogsAction::List => {
            for f in logs::file_list(&ctx.get(cmd::DBG_FILE_LIST)?) {
                println!("{f}");
            }
        }
        LogsAction::Info => {
            let p = ctx.get(cmd::DBG_LOG_INFO)?;
            match logs::log_info(&p) {
                Some(info) => println!("{info}"),
                None => println!("unexpected answer: [{}]", frame::hex(&p)),
            }
        }
        LogsAction::Debug => {
            let p = ctx.get(cmd::DBG_DEBUG_INFO)?;
            println!("[{}]", frame::hex(&p));
        }
        LogsAction::Dump { output, max_packets } => {
            let p = ctx.get(cmd::DBG_LOG_INFO)?;
            let info = logs::log_info(&p)
                .ok_or_else(|| format!("unexpected LOG_INFO answer: [{}]", frame::hex(&p)))?;
            eprintln!("{info}");
            if !info.available() {
                // Same rule as the vendor app: no LOG_REQUEST when nothing is announced.
                return Err("no buried log to download".into());
            }
            let mut data = Vec::new();
            for n in 0..max_packets {
                let p = ctx.request(cmd::DBG_LOG_REQUEST, &logs::log_request(n))?;
                let Some((index, chunk)) = logs::log_packet(&p) else {
                    eprintln!("packet {n}: short answer [{}], stopping", frame::hex(&p));
                    break;
                };
                if index != n as u32 {
                    eprintln!("packet {n}: headset answered index {index}");
                }
                if chunk.is_empty() {
                    break;
                }
                data.extend_from_slice(chunk);
                if data.len() as u64 >= info.file_size as u64 {
                    break;
                }
            }
            eprintln!("{} of {} bytes received", data.len(), info.file_size);
            match output {
                Some(path) => std::fs::write(&path, &data).map_err(|e| format!("{}: {e}", path.display()))?,
                None => print!("{}", logs::hexdump(&data)),
            }
        }
    }
    Ok(())
}

// --- Daemon -------------------------------------------------------------------------------

fn daemon(mac: Option<String>, verbose: bool) -> Res<()> {
    signals::install();
    let path = backend::socket_path();
    let hub = hub::Hub::new(mac, true);
    backend::serve(hub.clone(), &path)?;
    signals::remove_on_exit(path.clone());
    if verbose {
        eprintln!("listening on {}", path.display());
    }
    hub.keep_alive();
    while !signals::stop_requested() {
        std::thread::sleep(Duration::from_millis(200));
    }
    hub.shutdown();
    signals::cleanup();
    Ok(())
}

fn daemon_status() -> Res<()> {
    let path = backend::socket_path();
    let c = backend::DaemonClient::probe(&path).ok_or("no daemon owns the channel")?;
    let pong = c.ping()?;
    let w: Vec<&str> = pong.split(' ').collect();
    match w.as_slice() {
        ["PONG", "up", mac, model] => println!("daemon running, connected to {mac} (model {model})"),
        _ => println!("daemon running, headset not connected"),
    }
    Ok(())
}

// --- Events -------------------------------------------------------------------------------

/// Waits for the next event, polling the stop flag. None = stop.
fn next_event(events: &Events) -> Res<Option<Event>> {
    loop {
        if signals::stop_requested() {
            return Ok(None);
        }
        match events.next(Duration::from_millis(100)) {
            Ok(Some(ev)) => return Ok(Some(ev)),
            Ok(None) => continue,
            Err(()) => return Err("the channel owner went away".into()),
        }
    }
}

fn spawn_shell(command: &str, payload: &[u8]) {
    match std::process::Command::new("sh")
        .args(["-c", command])
        .env("NOTHING_PAYLOAD", frame::hex(payload).replace(' ', ""))
        .stdin(std::process::Stdio::null())
        .spawn()
    {
        // Reap it from a thread so that it does not linger as a zombie.
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("cannot run {command:?}: {e}"),
    }
}

fn describe_event(f: &frame::Frame) -> Option<String> {
    let p = &f.payload;
    Some(match f.cmd {
        cmd::EV_BATTERY => decode::battery(p).iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", "),
        cmd::EV_DEVICE_STATUS => decode::device_status_text(p),
        cmd::EV_ANC => decode::anc(p).to_string(),
        cmd::EV_AUDIODO => format!(
            "profile {}, SDK link {}",
            if p.first() == Some(&1) { "on" } else { "off" },
            if p.get(1) == Some(&1) { "up" } else { "down" }
        ),
        cmd::EV_HEADTRACK => {
            let q = decode::quaternion(p)?;
            format!("q=({:+.3}, {:+.3}, {:+.3}, {:+.3})", q[0], q[1], q[2], q[3])
        }
        // Spontaneous reports reuse response codes with fsn 0xFF (on-device EQ change).
        0x401F if f.fsn == 0xFF => format!("EQ preset: {}", decode::eq_preset(p)),
        0x404C if f.fsn == 0xFF => format!("advanced EQ: {}", decode::switch(p)),
        c if c & 0xFF00 == 0xE000 => String::new(),
        _ => return None,
    })
}

fn watch(ctx: &Ctx, on_magic: Option<&str>, headtrack: bool, raw: bool) -> Res<()> {
    let events = ctx.backend.subscribe(headtrack)?;
    eprintln!("watching (Ctrl+C to stop)");
    while let Some(ev) = next_event(&events)? {
        match ev {
            Event::Connected { mac, model } => {
                println!("{} connected to {mac} (model {})", clock(), model.as_deref().unwrap_or("unknown"))
            }
            Event::Disconnected(reason) => println!("{} disconnected: {reason}", clock()),
            Event::Frame(f) => {
                if f.cmd == cmd::EV_HEADTRACK && !headtrack {
                    continue;
                }
                let text = match describe_event(&f) {
                    Some(t) => t,
                    None if raw => String::new(),
                    None => continue,
                };
                println!(
                    "{} {:04X} {:<31} {:<40} [{}]",
                    clock(),
                    f.cmd,
                    cmd::name(f.cmd | if f.is_notification() { 0 } else { 0x8000 }).unwrap_or("?"),
                    text,
                    frame::hex(&f.payload)
                );
                if f.cmd == cmd::EV_MAGIC_BUTTON {
                    if let Some(c) = on_magic {
                        spawn_shell(c, &f.payload);
                    }
                }
            }
        }
        let _ = std::io::stdout().flush();
    }
    Ok(())
}

// --- Head tracking ------------------------------------------------------------------------

fn headtrack_cmd(ctx: &Ctx, viz: bool) -> Res<()> {
    let events = ctx.backend.subscribe(true)?;
    if !viz {
        println!("# t_s yaw pitch roll x y z w");
        let start = Instant::now();
        while let Some(ev) = next_event(&events)? {
            if let Event::Frame(f) = ev {
                if f.cmd != cmd::EV_HEADTRACK {
                    continue;
                }
                let Some(q) = decode::quaternion(&f.payload) else { continue };
                let (yaw, pitch, roll) = headtrack::euler_deg(&headtrack::mat_from_quat(q));
                println!(
                    "{:.3} {yaw:+.1} {pitch:+.1} {roll:+.1} {:+.4} {:+.4} {:+.4} {:+.4}",
                    start.elapsed().as_secs_f64(),
                    q[0],
                    q[1],
                    q[2],
                    q[3]
                );
            }
        }
        return Ok(());
    }

    let term = viz::RawTerminal::enter();
    let (mut az, mut el) = (viz::DEFAULT_AZIMUTH, viz::DEFAULT_ELEVATION);
    let mut q = [0.0, 0.0, 0.0, 1.0];
    let mut reference: Option<headtrack::Quat> = None;
    let mut stamps: std::collections::VecDeque<Instant> = Default::default();
    let mut count = 0u64;
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    loop {
        if signals::stop_requested() {
            break;
        }
        match events.next(Duration::from_millis(20)) {
            Ok(Some(Event::Frame(f))) if f.cmd == cmd::EV_HEADTRACK => {
                if let Some(nq) = decode::quaternion(&f.payload) {
                    q = nq;
                    // The first pose received is "straight ahead".
                    reference.get_or_insert(q);
                    stamps.push_back(Instant::now());
                    if stamps.len() > 40 {
                        stamps.pop_front();
                    }
                    count += 1;
                }
            }
            Ok(_) => {}
            Err(()) => return Err("the channel owner went away".into()),
        }
        let keys = term.keys();
        let mut i = 0;
        while i < keys.len() {
            match keys[i] {
                0x1b if keys.get(i + 1) == Some(&b'[') => {
                    match keys.get(i + 2) {
                        Some(b'A') => el = (el + 8.0).min(85.0),
                        Some(b'B') => el = (el - 8.0).max(-85.0),
                        Some(b'C') => az += 8.0,
                        Some(b'D') => az -= 8.0,
                        _ => {}
                    }
                    i += 2;
                }
                b'q' | b'Q' => signals::request_stop(),
                b'r' | b'R' | b' ' => reference = Some(q),
                b'l' => az += 8.0,
                b'h' => az -= 8.0,
                b'k' => el = (el + 8.0).min(85.0),
                b'j' => el = (el - 8.0).max(-85.0),
                b'0' => (az, el) = (viz::DEFAULT_AZIMUTH, viz::DEFAULT_ELEVATION),
                _ => {}
            }
            i += 1;
        }
        if last_draw.elapsed() >= Duration::from_millis(40) {
            last_draw = Instant::now();
            let shown = reference.map(|r| headtrack::recenter(r, q)).unwrap_or(q);
            let hz = match (stamps.front(), stamps.back()) {
                (Some(a), Some(b)) if stamps.len() >= 2 && b > a => {
                    (stamps.len() - 1) as f64 / (*b - *a).as_secs_f64()
                }
                _ => 0.0,
            };
            let status = format!(
                "\x1b[93m{hz:4.1} Hz\x1b[0m |q| {:.4} frames {count}  view {:.0}/{:.0}  \
                 arrows/hjkl orbit  0 reset view  r recenter  q quit",
                headtrack::norm(q),
                az.rem_euclid(360.0),
                el
            );
            let (cols, rows) = viz::terminal_size();
            print!("{}", viz::render(shown, cols, rows, az, el, &status));
            let _ = std::io::stdout().flush();
        }
    }
    drop(term);
    Ok(())
}

fn gestures(ctx: &Ctx, a: &GestureArgs) -> Res<()> {
    let mut tuning = headtrack::Tuning::default();
    if let Some(v) = a.nod_amplitude {
        tuning.nod.amplitude = v;
    }
    if let Some(v) = a.nod_speed {
        tuning.nod.speed = v;
    }
    if let Some(v) = a.shake_amplitude {
        tuning.shake.amplitude = v;
    }
    if let Some(v) = a.shake_speed {
        tuning.shake.speed = v;
    }
    let mut det = headtrack::Detector::new(tuning);
    let events = ctx.backend.subscribe(true)?;
    eprintln!(
        "gestures: nod >= {:.0} deg / {:.0} deg/s -> {:?}, shake >= {:.0} deg / {:.0} deg/s -> {:?}{}",
        tuning.nod.amplitude,
        tuning.nod.speed,
        a.nod_cmd,
        tuning.shake.amplitude,
        tuning.shake.speed,
        a.shake_cmd,
        if a.dry_run { " [dry run]" } else { "" }
    );
    let start = Instant::now();
    while let Some(ev) = next_event(&events)? {
        match ev {
            Event::Connected { .. } => det.reset(),
            Event::Frame(f) if f.cmd == cmd::EV_HEADTRACK => {
                let Some(q) = decode::quaternion(&f.payload) else { continue };
                let (yaw, pitch, _) = headtrack::euler_deg(&headtrack::mat_from_quat(q));
                let t = start.elapsed().as_secs_f64();
                let g = det.push(t, pitch, yaw);
                if a.debug {
                    let d = det.diag;
                    eprintln!(
                        "{t:7.2} pitch {pitch:+6.1} yaw {yaw:+6.1} | swing p {:5.1} y {:5.1} | speed p {:+6.0}/{:+6.0} y {:+6.0}/{:+6.0}{}",
                        d.pitch_swing,
                        d.yaw_swing,
                        d.pitch_speed.0,
                        d.pitch_speed.1,
                        d.yaw_speed.0,
                        d.yaw_speed.1,
                        if d.armed { "" } else { "  (disarmed)" }
                    );
                }
                if let Some(g) = g {
                    let (label, command) = match g {
                        headtrack::Gesture::Nod => ("nod", &a.nod_cmd),
                        headtrack::Gesture::Shake => ("shake", &a.shake_cmd),
                    };
                    println!("{} {label}", clock());
                    let _ = std::io::stdout().flush();
                    if !a.dry_run {
                        spawn_shell(command, &[]);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
