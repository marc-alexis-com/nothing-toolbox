# Changelog

## Unreleased

- **`eq edit`**: an interactive editor for the advanced EQ. It draws the response curve in the
  terminal, highlights the selected band's own shape, and sends each change to the headset
  0.3 s after the last key, then reads it back. `u` restores the starting curve, and the preset
  if one was in use; `s` saves the curve to a file. It owns the channel while it runs, so other
  commands (the ANC key) keep working. Tested on a B170: a change sent and read back, read
  through the socket by a second command, undone with `u`, and sent when quitting right after a
  key.
- README: screenshots of the editor, of everyday commands and of the help; a status-bar recipe
  that updates as soon as the headset changes mode (`watch` piped into a Waybar signal).

## 0.2.0 — 2026-10-15

- **Advanced EQ editing**: `eq band <Hz> <dB>`, `eq load <file>` (native format or AutoEQ
  `ParametricEQ.txt`, with `--dry-run`), `eq save`, `eq reset`. Limits follow the vendor app,
  and the overall gain is computed exactly as the app computes it (including the formula of
  firmware older than 1.0.1.46); the previous curve is saved before every write.
- `eq get` reports the advanced EQ when it is on (the preset byte does not change with it).
- `raw` and the local socket now carry each frame's control word.
- Docs: the magic-button hook is described as untested; source map completed.
- `watch` names the action carried by a magic-button event; the README shows how to turn the
  headset's button into a Linux button (bind it to `channel-hop`, then `watch --on-magic`).
- A busy channel is retried for about a second before giving up.
- Verified on a B170 with the user: battery, state and magic-button events, codec change
  (AAC and back to LDAC), automatic reconnection after the headset drops the link.
- Verified on a B170: all six noise modes, bass boost levels 1-5, and every control binding
  the app offers, each written, read back and restored.

## 0.1.0 — 2026-10-15

First release. Tested on a Nothing Headphone (1), model B170, firmware 1.0.1.81.

- **Noise control**: `anc get`, `anc set <mode>`, and `anc toggle` for a keyboard shortcut.
- **Status**: `status` (also `--json`), `battery`, `info` (model, firmware, serial number,
  paired devices).
- **Sound**: EQ presets and curves (`eq`), `bass`, `spatial`, `latency`, `multipoint`,
  `in-ear` (wear detection), `audiodo`, and `codec` (changing it needs `--dangerous`).
- **Controls**: `controls` reads and remaps the button and the wheel.
- **Events**: `watch` prints what the headset pushes, with `--on-magic` to run a command.
- **Head tracking**: `headtrack` streams the orientation as text, or as a live 3D view
  (`--viz`). `gestures` turns nods and shakes into media commands; it tolerates the drift the
  headset's orientation sometimes shows.
- **Logs**: `logs` lists and downloads the headset's diagnostic logs, read-only.
- **One channel, shared**: long-running commands own the headset's single control connection
  and serve the others through a local socket.
- **Safety**: writes limited to commands tested on a B170, risky ones behind `--dangerous`,
  firmware updates never sent.
- Shell completions (`completions <shell>`), static binaries for x86_64 and aarch64, and a Nix
  flake.
