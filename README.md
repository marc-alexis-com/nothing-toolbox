# nothing-toolbox

`nothing` is a command-line tool to control the **Nothing Headphone (1)** from Linux, over
its Bluetooth control channel: noise cancelling, EQ, codec, battery and state, the events the
headset pushes (including the magic button), the head-tracking stream, and diagnostic logs.

> **Only the Nothing Headphone (1) (model B170, firmware 1.0.1.81) has been tested.**
> Other Nothing or CMF products speak a related protocol but are untested and unsupported:
> `nothing` reads from them, and refuses to write unless you pass `--any-model`.

This is an independent interoperability project. It is not affiliated with or endorsed by
Nothing Technology Limited. It contains no firmware, app code or other material from Nothing:
only a description of the wire protocol ([docs/PROTOCOL.md](docs/PROTOCOL.md)) and a client.

## Install

Needs Linux with BlueZ, and a Rust toolchain to build.

```sh
cargo install --path .
```

The headset must be paired and connected as usual (e.g. with `bluetoothctl`). `nothing` finds
it among the connected devices by its Bluetooth vendor id; `--mac` or `NOTHING_MAC` override.

## Usage

```sh
nothing status                  # everything at a glance (--json for scripts)
nothing anc toggle              # noise cancelling <-> transparency
nothing anc set adaptive        # high | mid | low | adaptive | off | transparency
nothing battery
nothing eq set more-bass        # balanced | more-bass | more-treble | voice | custom | advanced
nothing eq advanced             # print the 8-band curve
nothing spatial set on          # off | on | head-tracking
nothing latency on
nothing watch                   # print the events pushed by the headset
nothing watch --on-magic 'playerctl play-pause'
nothing headtrack --viz         # live 3D view of your head in the terminal
nothing gestures                # nod = play/pause, shake = next track (uses playerctl)
nothing logs list
nothing info                    # model, firmware, serial number, paired devices
```

`nothing <command> --help` documents every option.

### One channel, many commands

The headset accepts **one** connection on its control channel at a time. Whoever holds it
locks every other client out, which is a problem as soon as something long-running (an event
watcher, head tracking) is active and you press the key bound to `nothing anc toggle`.

`nothing` solves this with a single owner:

- `nothing daemon`, `watch`, `headtrack` and `gestures` **own** the channel and serve other
  invocations through a local socket, `$XDG_RUNTIME_DIR/nothing.sock`;
- every other command first looks for that owner and goes through it; when there is none, it
  opens the channel itself for the duration of the command (a few hundred milliseconds).

So a key binding on `nothing anc toggle` keeps working whatever else runs. Running the daemon
permanently is optional; note that while it holds the channel, other clients (such as the
vendor's phone app) cannot use it.

The head-tracking stream is reference-counted: the headset streams only while at least one
client listens to it.

### Gestures

`nothing gestures` detects a nod (pitch oscillation) and a shake (yaw oscillation) and runs a
command for each (`--nod-cmd`, `--shake-cmd`). A gesture must be a quick back-and-forth that
returns to neutral, on one dominant axis; slow head movements are ignored. Thresholds can be
tuned (`--nod-amplitude`, `--nod-speed`, `--shake-amplitude`, `--shake-speed`); `--debug`
prints the measured amplitude and speed of each sample to help, `--dry-run` runs nothing.

## Safety

The headset will accept commands that reboot it, reset it or forget paired devices. `nothing`
sorts every command code:

- **reads** are always allowed;
- **writes** that were tested on a B170 (and reverted) are allowed;
- everything else (codec change, which reboots the headset; factory reset; unpairing; test
  mode; any untested code sent with `nothing raw`) requires `--dangerous`;
- **firmware updates are never sent**, not even with `--dangerous`.

Failures are also appended to `$XDG_RUNTIME_DIR/nothing.log`, which helps when a key binding
swallows the error message.

## Development

```sh
cargo test                      # frame, decoders, policy, gesture detector
cargo clippy --all-targets
```

The protocol notes in [docs/PROTOCOL.md](docs/PROTOCOL.md) mark each fact as confirmed on the
device, taken from the vendor app's behaviour, or inferred.

## License

MIT, see [LICENSE](LICENSE). "Nothing" and "Nothing Headphone (1)" are trademarks of Nothing
Technology Limited, used here only to name the hardware this tool talks to.
