# Contributing

Thanks for helping. This tool was worked out on one headset; it grows when people with other
Nothing devices or firmwares share what theirs does.

## Report your device

Open a
[device report](https://github.com/marc-alexis-com/nothing-toolbox/issues/new?template=report-a-device.yml)
with:

- `nothing info` — the model id and firmware;
- `nothing status` — every setting the tool reads;
- if something specific misbehaves, the relevant line from `$XDG_RUNTIME_DIR/nothing.log`.

Remove the serial number and any Bluetooth address before you post.

Everything above only reads the headset, or changes settings it reports back. Please don't paste
output from commands the tool refuses (see [docs/PROTOCOL.md](docs/PROTOCOL.md)) — they were left
out on purpose.

## What is and isn't supported

`nothing` was built for and tested on the Nothing Headphone (1) (`B170`). Other Nothing products
speak a related protocol over the same channel but are untested, and the tool refuses writes to a
device it does not recognise. A report from another model is how support for it would start.

## Changing the code or the protocol notes

- `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` all pass.
- In `docs/PROTOCOL.md`, keep each fact marked **confirmed** (seen on a device), **app** (from the
  vendor app) or **inferred**, and say which device and firmware it was seen on.
- The tool only sends commands confirmed on a real device, and reads them back. A change that
  sends something new should say how it was verified, and on what hardware.

By contributing you agree your contribution is licensed under the repository's MIT license.
