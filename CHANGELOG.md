# Changelog

## Unreleased

- **Advanced EQ editing**: `eq band <Hz> <dB>`, `eq load <file>` (native format or AutoEQ
  `ParametricEQ.txt`, with `--dry-run`), `eq save`, `eq reset`. Limits follow the vendor app,
  and the overall gain is computed exactly as the app computes it (including the formula of
  firmware older than 1.0.1.46); the previous curve is saved before every write.
- `eq get` reports the advanced EQ when it is on (the preset byte does not change with it).
- `raw` and the local socket now carry each frame's control word.
- Docs: the magic-button hook is described as untested; source map completed.
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
