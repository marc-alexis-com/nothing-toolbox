//! `nothing`: Linux control of the Nothing Headphone (1) (model B170) over its RFCOMM
//! control channel. Other Nothing products are untested and unsupported.

mod backend;
mod cmd;
mod decode;
mod eqedit;
mod eqfile;
mod frame;
mod headtrack;
mod hub;
mod kitty;
mod link;
mod logs;
mod mac;
mod model;
mod rfcomm;
mod signals;
mod viz;
mod zlib;

use backend::{Backend, Events};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use decode::AncMode;
use hub::{Event, Opts};
use std::io::Write;
use std::process::ExitCode;
use std::time::{Duration, Instant};

type Res<T> = Result<T, String>;

const EXAMPLES: &str = "\
Examples:
  nothing status                         everything at a glance
  nothing anc toggle                     noise cancelling <-> transparency
  nothing anc set adaptive
  nothing eq set more-bass
  nothing controls set button hold spatial
  nothing watch --on-magic 'playerctl play-pause'
  nothing headtrack --viz
  nothing gestures --dry-run --debug
  nothing completions zsh > ~/.zfunc/_nothing

Failures are also logged to $XDG_RUNTIME_DIR/nothing.log.
Protocol notes: https://github.com/marc-alexis-com/nothing-toolbox/blob/main/docs/PROTOCOL.md";

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
                  other model unless --any-model is given.",
    after_long_help = EXAMPLES
)]
struct Cli {
    /// Headset address (default: $NOTHING_MAC, else the first connected Nothing headset)
    #[arg(long, global = true, value_name = "ADDRESS", help_heading = "Global options")]
    mac: Option<String>,
    /// Allow risky commands (codec change with reboot, untested raw codes)
    #[arg(long, global = true, help_heading = "Global options")]
    dangerous: bool,
    /// Allow writes to a headset that does not report model B170
    #[arg(long, global = true, help_heading = "Global options")]
    any_model: bool,
    /// How long to wait for each answer, in milliseconds
    #[arg(long, global = true, default_value_t = 2000, value_name = "MS", help_heading = "Global options")]
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
    /// Everything at a glance: model, firmware, battery, state, sound settings
    Status {
        /// Print one JSON object instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Battery level and charging state
    Battery {
        /// Print {"level":N,"charging":BOOL}
        #[arg(long)]
        json: bool,
    },
    /// Model, firmware, color, serial number, paired devices
    Info,
    /// Equaliser: presets, custom and advanced curves
    Eq {
        #[command(subcommand)]
        action: EqAction,
    },
    /// Bluetooth codec (changing it reboots the headset, so it needs --dangerous)
    Codec {
        #[command(subcommand)]
        action: CodecAction,
    },
    /// Bass boost
    Bass {
        #[command(subcommand)]
        action: BassAction,
    },
    /// Spatial audio, with or without head tracking
    Spatial {
        #[command(subcommand)]
        action: SpatialAction,
    },
    /// Low-latency mode, for games and video
    Latency(SwitchArgs),
    /// Connection to two devices at once
    Multipoint(SwitchArgs),
    /// Wear detection: pause when the headset is taken off (and ignore its buttons meanwhile)
    InEar(SwitchArgs),
    /// Audiodo personal sound profile
    Audiodo(SwitchArgs),
    /// What the button and the wheel do, and remapping them
    #[command(after_long_help = "Examples:\n  nothing controls\n  nothing controls actions\n  \
                                 nothing controls set button hold spatial\n  \
                                 nothing controls set wheel hold noise-cycle")]
    Controls {
        #[command(subcommand)]
        action: Option<ControlsAction>,
    },
    /// Print the events pushed by the headset (battery, noise mode, state, magic button...)
    Watch {
        /// Run CMD through `sh -c` on each magic-button event; $NOTHING_PAYLOAD holds its payload in hex
        #[arg(long, value_name = "CMD")]
        on_magic: Option<String>,
        /// Also print the head-tracking stream (about 9 lines per second)
        #[arg(long)]
        headtrack: bool,
        /// Print every frame, including answers to other clients
        #[arg(long)]
        raw: bool,
    },
    /// Head-tracking stream: one line per sample, or a live 3D view
    Headtrack {
        /// Terminal 3D view. Keys: arrows or hjkl orbit, 0 resets the view, r or space recenters, q quits
        #[arg(long)]
        viz: bool,
    },
    /// Media control by head gestures: nod = play/pause, shake = next track
    Gestures(GestureArgs),
    /// Diagnostic logs kept by the headset (read-only)
    Logs {
        #[command(subcommand)]
        action: LogsAction,
    },
    /// Own the control channel and serve other commands through the local socket
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
        /// Log connections, disconnections and head-tracking stream changes
        #[arg(long, short)]
        verbose: bool,
    },
    /// Send one raw request and print the answer (reads are free, writes are policed)
    #[command(after_long_help = "Examples:\n  nothing raw C007          battery\n  \
                                 nothing raw C01E 03       noise mode\n\n\
                                 See docs/PROTOCOL.md for the command codes.")]
    Raw {
        /// Command code in hex, e.g. C007
        cmd: String,
        /// Payload in hex, e.g. 03 or "01 07 00"
        payload: Option<String>,
    },
    /// Print a shell completion script
    #[command(after_long_help = "Examples:\n  nothing completions zsh > ~/.zfunc/_nothing\n  \
                                 nothing completions bash > ~/.local/share/bash-completion/completions/nothing\n  \
                                 nothing completions fish > ~/.config/fish/completions/nothing.fish")]
    Completions {
        /// Target shell
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
enum AncAction {
    /// Print the current mode
    Get,
    /// Set a mode
    Set {
        /// Noise mode to set
        mode: AncArg,
    },
    /// Noise cancelling <-> transparency; from any other mode, back to the last cancelling level
    Toggle,
}

#[derive(Clone, Copy, ValueEnum)]
enum AncArg {
    /// Strongest noise cancelling
    High,
    /// Medium noise cancelling
    Mid,
    /// Light noise cancelling
    Low,
    /// Noise cancelling that adapts to the surroundings
    Adaptive,
    /// No noise control
    Off,
    /// Let outside sound in
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
    /// Print the active preset, or `advanced` when the advanced EQ is on
    Get,
    /// Select a preset
    Set {
        /// Preset to select
        preset: EqPreset,
    },
    /// Print the custom 3-band curve
    Custom,
    /// Print the advanced curve (up to 8 bands)
    Advanced,
    /// Load an advanced curve from a file (native or AutoEQ ParametricEQ format) and turn it on
    #[command(after_long_help = "File formats:\n  \
        native (written by `nothing eq save`):   55  +3.0  1.00     (Hz, dB, Q per line)\n  \
        AutoEQ ParametricEQ.txt:                 Filter 1: ON PK Fc 105 Hz Gain -2.3 dB Q 0.70\n\n\
        Limits: 8 bands, -6..+6 dB in 0.5 dB steps, 20-20000 Hz, Q 0.1-10. Only peaking (PK)\n\
        filters. The previous curve is saved to $XDG_STATE_HOME/nothing/eq-previous.txt first.")]
    Load {
        /// Curve file
        file: std::path::PathBuf,
        /// Show what would be sent without sending it
        #[arg(long)]
        dry_run: bool,
    },
    /// Save the advanced curve to a file (or print it)
    Save {
        /// Destination file; standard output when omitted
        file: Option<std::path::PathBuf>,
    },
    /// Set one band of the advanced curve, adding it if no band is near that frequency
    Band {
        /// Centre frequency in Hz (20-20000)
        freq: f32,
        /// Gain in dB (-6..+6, 0.5 dB steps)
        #[arg(allow_negative_numbers = true)]
        gain: f32,
        /// Width (0.1-10); kept from the existing band, 1.0 for a new one
        #[arg(long)]
        q: Option<f32>,
    },
    /// Restore the vendor app's factory curve (three flat bands) and turn it on
    Reset,
    /// Edit the advanced curve interactively, hearing each change as it is made
    #[command(after_long_help = "Keys:\n  \
        up/down, k/j       gain of the selected band (0.5 dB steps)\n  \
        left/right, h/l    frequency (1/12 octave); H/L or shift+arrows: 1/3 octave\n  \
        [ ]                width (Q), in quarter-octave steps\n  \
        tab/shift+tab, 1-8 select a band\n  \
        a / x              add a flat band / delete the selected one\n  \
        0                  set the selected band flat\n  \
        r                  the vendor app's factory curve (three flat bands)\n  \
        u                  undo everything: the starting curve, and the preset if it was in use\n  \
        s                  save the curve to FILE\n  \
        q, Ctrl+C          quit, keeping the curve on the headset\n\n\
        Each change is sent 0.3 s after the last key and read back. The curve found at start is\n\
        saved to $XDG_STATE_HOME/nothing/eq-previous.txt before the first change. The drawn\n\
        response assumes the headset runs the filters the vendor app models (inferred).")]
    Edit {
        /// Where `s` saves the curve
        #[arg(default_value_os_t = state_dir().join("eq-edited.txt"))]
        file: std::path::PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum EqPreset {
    /// Neutral tuning
    Balanced,
    /// Stronger low end
    MoreBass,
    /// Brighter highs
    MoreTreble,
    /// Voices forward
    Voice,
    /// The custom 3-band curve
    Custom,
    /// The advanced 8-band curve
    Advanced,
}

#[derive(Subcommand)]
enum CodecAction {
    /// Print the current codec
    Get,
    /// Change the codec. The headset reboots (about 6 s). Needs --dangerous
    Set {
        /// Codec to use
        codec: CodecArg,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum CodecArg {
    /// AAC
    Aac,
    /// LDAC, higher bitrate; costs battery and range
    Ldac,
}

#[derive(Subcommand)]
enum BassAction {
    /// Print the bass boost state and level
    Get,
    /// Turn bass boost off (the level is kept)
    Off,
    /// Turn bass boost on
    On {
        /// Boost level, 1 to 5
        #[arg(default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=5))]
        level: u8,
    },
}

#[derive(Subcommand)]
enum SpatialAction {
    /// Print the spatial audio mode
    Get,
    /// Set the spatial audio mode
    Set {
        /// Spatial audio mode
        mode: SpatialArg,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum SpatialArg {
    /// Plain stereo
    Off,
    /// Fixed spatial audio
    On,
    /// Spatial audio that follows the head
    HeadTracking,
}

#[derive(Subcommand)]
enum ControlsAction {
    /// Print the current bindings (default)
    Get,
    /// Bind an action to a control gesture
    Set {
        /// Which control
        control: ControlArg,
        /// Which gesture
        gesture: GestureArg,
        /// Action to bind; `nothing controls actions` lists what each control accepts
        action: String,
    },
    /// List the actions each control accepts
    Actions,
}

#[derive(Clone, Copy, ValueEnum)]
enum ControlArg {
    /// The customisable button
    Button,
    /// The roller (its single press is fixed to play/pause)
    Wheel,
}

#[derive(Clone, Copy, ValueEnum)]
enum GestureArg {
    /// Single press
    Press,
    /// Press and hold
    Hold,
}

#[derive(Args)]
struct SwitchArgs {
    #[command(subcommand)]
    action: SwitchAction,
}

#[derive(Subcommand)]
enum SwitchAction {
    /// Print the current state
    Get,
    /// Turn it on
    On,
    /// Turn it off
    Off,
}

#[derive(Args)]
#[command(after_long_help = "A gesture is a quick back-and-forth on one axis that returns to neutral; \
                             slow movements are ignored. Use --dry-run --debug to see what is \
                             measured, then adjust the thresholds.")]
struct GestureArgs {
    /// Print detections without running anything
    #[arg(long)]
    dry_run: bool,
    /// Print the measured amplitude, speed and drift of every sample, to tune the thresholds
    #[arg(long)]
    debug: bool,
    /// Command run on a nod
    #[arg(long, default_value = "playerctl play-pause", value_name = "CMD")]
    nod_cmd: String,
    /// Command run on a shake
    #[arg(long, default_value = "playerctl next", value_name = "CMD")]
    shake_cmd: String,
    /// Minimum nod amplitude, degrees peak-to-peak [default: 10]
    #[arg(long, value_name = "DEG")]
    nod_amplitude: Option<f64>,
    /// Minimum nod speed, degrees per second, in both directions [default: 45]
    #[arg(long, value_name = "DEG_S")]
    nod_speed: Option<f64>,
    /// Minimum shake amplitude, degrees peak-to-peak [default: 15]
    #[arg(long, value_name = "DEG")]
    shake_amplitude: Option<f64>,
    /// Minimum shake speed, degrees per second, in both directions [default: 45]
    #[arg(long, value_name = "DEG_S")]
    shake_speed: Option<f64>,
}

#[derive(Subcommand)]
enum LogsAction {
    /// List the log files kept by the headset
    List,
    /// Whether a buried log is available, and its size
    Info,
    /// Logging state
    Debug,
    /// Download the buried log, packet by packet
    Dump {
        /// Write the data to FILE instead of printing a hex dump
        #[arg(long, short, value_name = "FILE")]
        output: Option<std::path::PathBuf>,
        /// Stop after this many packets
        #[arg(long, default_value_t = 2048, value_name = "N")]
        max_packets: u16,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Tell whether a daemon owns the channel, and to which headset it is connected
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
    // Behave like other Unix tools when the reader goes away (`nothing watch | head`):
    // die quietly from SIGPIPE instead of panicking on the failed write.
    // SAFETY: restoring the default disposition of one signal, before any thread exists.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
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
        Command::Eq { action: EqAction::Edit { file } } => {
            long_running(cli.mac, opts, timeout, |ctx| eq_edit(ctx, &file))
        }
        Command::Eq { action } => eq(&one_shot(), action),
        Command::Codec { action } => codec(&one_shot(), action),
        Command::Bass { action } => bass(&one_shot(), action),
        Command::Spatial { action } => spatial(&one_shot(), action),
        Command::Latency(a) => switch(&one_shot(), a.action, Switch::LATENCY),
        Command::Multipoint(a) => switch(&one_shot(), a.action, Switch::MULTIPOINT),
        Command::InEar(a) => switch(&one_shot(), a.action, Switch::IN_EAR),
        Command::Audiodo(a) => switch(&one_shot(), a.action, Switch::AUDIODO),
        Command::Controls { action } => controls(&one_shot(), action.unwrap_or(ControlsAction::Get)),
        Command::Raw { cmd, payload } => raw(&one_shot(), &cmd, payload.as_deref()),
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "nothing", &mut std::io::stdout());
            Ok(())
        }
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
        EqAction::Get => {
            // The preset byte keeps its value while the advanced EQ is on: ask the switch too.
            let preset = decode::eq_preset(&ctx.get(cmd::GET_EQ_MODE)?);
            if decode::switch(&ctx.get(cmd::GET_ADVANCED_EQ_MODE)?) == "on" {
                println!("advanced (preset underneath: {preset})");
            } else {
                println!("{preset}");
            }
        }
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
        EqAction::Load { file, dry_run } => {
            let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            write_curve(ctx, eqfile::parse(&text)?, dry_run)?;
        }
        EqAction::Save { file } => {
            let eq = read_curve(ctx)?;
            match file {
                Some(path) => std::fs::write(&path, eqfile::format(&eq))
                    .map_err(|e| format!("{}: {e}", path.display()))?,
                None => print!("{}", eqfile::format(&eq)),
            }
        }
        EqAction::Band { freq, gain, q } => {
            let mut bands = read_curve(ctx)?.bands;
            // Same band if within a sixth of an octave.
            match bands.iter_mut().find(|b| (b.freq / freq).log2().abs() < 1.0 / 6.0) {
                Some(b) => {
                    b.freq = freq;
                    b.gain = gain;
                    if let Some(q) = q {
                        b.q = q;
                    }
                }
                None => bands.push(decode::EqBand {
                    filter: eqfile::FILTER_PEAK,
                    gain,
                    freq,
                    q: q.unwrap_or(1.0),
                }),
            }
            write_curve(ctx, bands, false)?;
        }
        EqAction::Reset => write_curve(ctx, eqfile::factory(), false)?,
        EqAction::Edit { .. } => unreachable!("run() starts the editor as the channel owner"),
        EqAction::Advanced => {
            let eq = decode::advanced_eq(&ctx.request(cmd::GET_ADVANCED_EQ, &[0])?)
                .ok_or("malformed advanced EQ")?;
            println!("{eq}");
        }
    }
    Ok(())
}

fn read_curve(ctx: &Ctx) -> Res<decode::Eq> {
    decode::advanced_eq(&ctx.request(cmd::GET_ADVANCED_EQ, &[0])?)
        .ok_or_else(|| "malformed advanced EQ answer".into())
}

fn state_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("nothing")
}

/// Saves `eq` to $XDG_STATE_HOME/nothing/eq-previous.txt, before it is overwritten.
fn backup_curve(eq: &decode::Eq) -> Res<std::path::PathBuf> {
    let dir = state_dir();
    let backup = dir.join("eq-previous.txt");
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&backup, eqfile::format(eq)))
        .map_err(|e| format!("cannot save the current curve to {}: {e}", backup.display()))?;
    Ok(backup)
}

/// Validates and writes an advanced curve, after saving the current one so it can be restored.
/// Writing the bands also turns the advanced EQ on (observed on a B170).
fn write_curve(ctx: &Ctx, bands: Vec<decode::EqBand>, dry_run: bool) -> Res<()> {
    let firmware = ctx.get(cmd::GET_FIRMWARE).ok().map(|p| decode::text(&p));
    let (eq, notes) = eqfile::curve(bands, firmware.as_deref())?;
    for n in &notes {
        eprintln!("note: {n}");
    }
    if dry_run {
        println!("{eq}");
        println!("payload: [{}]", frame::hex(&eqfile::payload(&eq)));
        return Ok(());
    }
    let backup = backup_curve(&read_curve(ctx)?)?;
    ctx.request(cmd::SET_ADVANCED_EQ, &eqfile::payload(&eq))?;
    let now = read_curve(ctx)?;
    println!("{now}");
    if now.bands != eq.bands {
        return Err("the headset reports a different curve than the one sent".into());
    }
    eprintln!(
        "advanced EQ: {}; previous curve saved to {}",
        decode::switch(&ctx.get(cmd::GET_ADVANCED_EQ_MODE)?),
        backup.display()
    );
    Ok(())
}

/// Pause between the last key and sending the curve, so that a held key sends once.
const EDIT_SEND_DELAY: Duration = Duration::from_millis(300);
const EDIT_RETRY_DELAY: Duration = Duration::from_secs(2);

fn eq_edit(ctx: &Ctx, file: &std::path::Path) -> Res<()> {
    let term = viz::RawTerminal::enter();
    if !term.is_interactive() {
        return Err("eq edit needs a terminal; use eq band or eq load in scripts".into());
    }
    let firmware = ctx.get(cmd::GET_FIRMWARE).ok().map(|p| decode::text(&p));
    let start = read_curve(ctx)?;
    let was_on = decode::switch(&ctx.get(cmd::GET_ADVANCED_EQ_MODE)?) == "on";
    let mut ed = eqedit::Editor::new(start.bands.clone());
    let mut backed_up = false;
    let mut send_at: Option<Instant> = None;
    // Set by `u` when the preset was in use at start: switch the advanced EQ back off.
    let mut back_to_preset = false;
    let mut advanced_on = was_on;
    let mut headset = String::from("headset    as found");
    let mut status = String::new();
    let mut redraw = true;
    let mut size = (0, 0);

    let send = |bands: &[decode::EqBand], back_to_preset: bool, backed_up: &mut bool| -> Res<()> {
        if !*backed_up {
            backup_curve(&start)?;
            *backed_up = true;
        }
        let (eq, _) = eqfile::curve(bands.to_vec(), firmware.as_deref())?;
        ctx.request(cmd::SET_ADVANCED_EQ, &eqfile::payload(&eq))?;
        if read_curve(ctx)?.bands != eq.bands {
            return Err("the headset reports a different curve than the one sent".into());
        }
        if back_to_preset {
            ctx.request(cmd::SET_ADVANCED_EQ_MODE, &[0])?;
        }
        Ok(())
    };

    while !signals::stop_requested() {
        for key in eqedit::parse_keys(&term.keys()) {
            match ed.handle(key) {
                eqedit::Outcome::Nothing => {}
                eqedit::Outcome::Changed => {
                    send_at = Some(Instant::now() + EDIT_SEND_DELAY);
                    back_to_preset = false;
                }
                eqedit::Outcome::Reverted => {
                    send_at = Some(Instant::now() + EDIT_SEND_DELAY);
                    back_to_preset = !was_on;
                }
                eqedit::Outcome::Save => {
                    let eq = decode::Eq { profile: Some(0), total_gain: 0.0, bands: ed.bands.clone() };
                    status = match file
                        .parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .and_then(|()| std::fs::write(file, eqfile::format(&eq)))
                    {
                        Ok(()) => format!("saved to {}", file.display()),
                        Err(e) => format!("cannot save to {}: {e}", file.display()),
                    };
                }
                eqedit::Outcome::Quit => signals::request_stop(),
            }
            redraw = true;
        }
        if send_at.is_some_and(|t| Instant::now() >= t) {
            match send(&ed.bands, back_to_preset, &mut backed_up) {
                Ok(()) => {
                    send_at = None;
                    advanced_on = !back_to_preset;
                    headset = String::from("headset    up to date");
                    status.clear();
                }
                Err(e) => {
                    send_at = Some(Instant::now() + EDIT_RETRY_DELAY);
                    headset = String::from("headset    not updated, retrying");
                    status = e;
                }
            }
            redraw = true;
        }
        let now = viz::terminal_size();
        if redraw || now != size {
            (size, redraw) = (now, false);
            let info = [
                headset.clone(),
                format!("mode       {}", if advanced_on { "advanced EQ" } else { "preset" }),
            ];
            let total = eqfile::total_gain(&ed.bands, firmware.as_deref());
            print!("{}", eqedit::render(&ed, total, size.0, size.1, &info, &status));
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    drop(term);
    // Quitting right after a key: send what is on screen rather than drop it.
    if send_at.is_some() {
        send(&ed.bands, back_to_preset, &mut backed_up)?;
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
            // A B170 acknowledges (0x701C [00]) and then reboots, back after about 7 s. A link
            // closed by the reboot before the acknowledgement arrives is not an error either.
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

// --- Controls -----------------------------------------------------------------------------

fn print_bindings(list: &[decode::Binding]) {
    for b in list {
        println!(
            "{:<7} {:<6} {}",
            decode::control_name(b.button),
            decode::gesture_name(b.gesture),
            decode::action_name(b.operation)
        );
    }
}

fn controls(ctx: &Ctx, action: ControlsAction) -> Res<()> {
    match action {
        ControlsAction::Get => print_bindings(&decode::bindings(&ctx.get(cmd::GET_KEY_CONFIG)?)),
        ControlsAction::Actions => {
            for (button, gesture) in [
                (decode::BUTTON_CUSTOM, decode::GESTURE_PRESS),
                (decode::BUTTON_CUSTOM, decode::GESTURE_HOLD),
                (decode::BUTTON_WHEEL, decode::GESTURE_HOLD),
            ] {
                let names: Vec<String> = decode::allowed_actions(button, gesture)
                    .iter()
                    .map(|&a| decode::action_name(a))
                    .collect();
                println!(
                    "{:<7} {:<6} {}",
                    decode::control_name(button),
                    decode::gesture_name(gesture),
                    names.join(", ")
                );
            }
            println!("wheel   press  play-pause (fixed)");
        }
        ControlsAction::Set { control, gesture, action } => {
            let button = match control {
                ControlArg::Button => decode::BUTTON_CUSTOM,
                ControlArg::Wheel => decode::BUTTON_WHEEL,
            };
            let gesture = match gesture {
                GestureArg::Press => decode::GESTURE_PRESS,
                GestureArg::Hold => decode::GESTURE_HOLD,
            };
            let op = decode::action_code(&action).ok_or_else(|| format!("unknown action {action:?}"))?;
            if !decode::allowed_actions(button, gesture).contains(&op) {
                return Err(format!(
                    "{action} is not offered on {} {} (see `nothing controls actions`)",
                    decode::control_name(button),
                    decode::gesture_name(gesture)
                ));
            }
            // Read, change one binding, write the whole list back: the form tested on a B170.
            let mut list = decode::bindings(&ctx.get(cmd::GET_KEY_CONFIG)?);
            let entry = list
                .iter_mut()
                .find(|b| b.device == decode::DEVICE_HEADSET && b.button == button && b.gesture == gesture)
                .ok_or("the headset did not report this control")?;
            entry.operation = op;
            ctx.request(cmd::SET_KEY_CONFIG, &decode::bindings_payload(&list))?;
            print_bindings(&decode::bindings(&ctx.get(cmd::GET_KEY_CONFIG)?));
        }
    }
    Ok(())
}

// --- Raw ----------------------------------------------------------------------------------

fn raw(ctx: &Ctx, cmd_text: &str, payload: Option<&str>) -> Res<()> {
    let code = u16::from_str_radix(cmd_text.trim_start_matches("0x"), 16)
        .map_err(|e| format!("{cmd_text:?}: {e}"))?;
    let payload = payload.map(frame::parse_hex).transpose()?.unwrap_or_default();
    let f = ctx.backend.request(code, &payload, ctx.timeout, ctx.opts)?;
    println!(
        "{:04X} fsn={} ctl={:04X} [{}]{}",
        f.cmd,
        f.fsn,
        f.ctl,
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
        cmd::EV_MAGIC_BUTTON => match p.first() {
            Some(&a) => format!("action {}", decode::action_name(a)),
            None => String::new(),
        },
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

    // Draw the headset in its own colour (C00C: 0x01 black, 0x09 white).
    let edition = match ctx.get(cmd::GET_COLOR).ok().and_then(|p| p.first().copied()) {
        Some(0x01) => model::Edition::Black,
        _ => model::Edition::White,
    };
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
                // Recentre: this pose is straight ahead, seen from straight behind.
                b'r' | b'R' | b' ' => {
                    reference = Some(q);
                    (az, el) = (viz::DEFAULT_AZIMUTH, viz::DEFAULT_ELEVATION);
                }
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
            print!("{}", viz::render(shown, edition, cols, rows, az, el, &status));
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
                // On the B170 a nod turns the head about X (roll), a shake about Z (yaw).
                let (yaw, _, roll) = headtrack::euler_deg(&headtrack::mat_from_quat(q));
                let t = start.elapsed().as_secs_f64();
                let g = det.push(t, roll, yaw);
                if a.debug {
                    let d = det.diag;
                    eprintln!(
                        "{t:7.2} nod {roll:+6.1} shake {yaw:+7.1} | swing n {:5.1} s {:5.1} | speed n {:+5.0}/{:+5.0} s {:+5.0}/{:+5.0} | drift {:+5.1} {:+5.1} deg/s{}",
                        d.nod_swing,
                        d.shake_swing,
                        d.nod_speed.0,
                        d.nod_speed.1,
                        d.shake_speed.0,
                        d.shake_speed.1,
                        d.drift.0,
                        d.drift.1,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }

    /// Every command, argument and value must explain itself in --help.
    #[test]
    fn help_is_complete() {
        fn walk(cmd: &clap::Command, path: &str, missing: &mut Vec<String>) {
            for sub in cmd.get_subcommands() {
                let here = format!("{path} {}", sub.get_name());
                if sub.get_about().is_none() {
                    missing.push(here.clone());
                }
                walk(sub, &here, missing);
            }
            for arg in cmd.get_arguments() {
                let id = arg.get_id().as_str();
                if id == "help" || id == "version" {
                    continue;
                }
                if arg.get_help().is_none() {
                    missing.push(format!("{path} <{id}>"));
                }
                for v in arg.get_possible_values() {
                    if arg.get_action().takes_values() && v.get_help().is_none() && id != "shell" {
                        missing.push(format!("{path} <{id}={}>", v.get_name()));
                    }
                }
            }
        }
        let mut missing = Vec::new();
        walk(&Cli::command(), "nothing", &mut missing);
        assert!(missing.is_empty(), "no help text for: {missing:?}");
    }
}
