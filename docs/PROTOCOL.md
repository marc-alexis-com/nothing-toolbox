# Nothing Headphone (1) control protocol

Wire-level description of the control channel of the **Nothing Headphone (1)** (model id
`B170`), as implemented by `nothing`. It was worked out for interoperability, from the
behaviour of the vendor's Android app and of the device itself, on a unit owned by the author.

**Scope.** Everything below was checked against a B170 running firmware `1.0.1.81`. Other
Nothing products (Ear, CMF, Headphone (1) Pro…) share parts of this protocol but were
**not tested**; values that differ between models are pointed out where known.

Each statement carries a status:

- **confirmed**: observed on the device (request sent, answer read, effect read back);
- **app**: how the vendor app builds or parses it, not exercised on the device yet;
- **inferred**: deduced, with the reasoning given.

## 1. Transport

| Fact | Status |
|---|---|
| Classic Bluetooth, RFCOMM **channel 15**, no SDP lookup needed. | confirmed |
| The channel accepts **one connection at a time**: while a client holds it, a second one fails to connect with `EBUSY`. This is why `nothing` funnels everything through a single owner. | confirmed |
| No pairing, authentication or session setup beyond the RFCOMM connection itself. | confirmed |
| A single `read` may carry several frames, and a frame may be split across reads. | confirmed |

## 2. Frame

```
offset  size  field
0       1     0x55 header
1       2     control, little-endian. Clients send 0x0160.
3       2     command, little-endian
5       2     payload length, little-endian
7       1     fsn (frame sequence number)
8       n     payload
8+n     2     CRC-16/MODBUS, little-endian, over bytes 0..8+n — present iff control & 0x20
```

| Fact | Status |
|---|---|
| CRC-16/MODBUS: init `0xFFFF`, reflected polynomial `0xA001`, no final XOR. Check value of `"123456789"` is `0x4B37`. | confirmed |
| The headset answers with control `0x0160` and a valid CRC. | confirmed |
| Clients number frames 1…255 then wrap to 1; the answer **echoes the request's fsn**. | confirmed |
| Pushed events carry **fsn 0**. | confirmed |
| Some spontaneous reports reuse a response code with **fsn 0xFF** (EQ preset changed on the device). | app |
| Debug-class answers may use control `0x0300` **without CRC** (FC22 packets). | app |

Example, ANC read: request `55 60 01 1e c0 01 00 01 03 9c 89`, answer
`55 60 01 1e 40 06 00 01 01 01 00 02 01 00 47 ea`. **confirmed**

## 3. Command classes

| Class | Meaning |
|---|---|
| `0xC0xx` | GET (read). One exception: `0xC058` starts/stops a stream. |
| `0xF0xx` | SET (write). |
| `0x40xx` / `0x70xx` | Answer: the request code with **bit 15 cleared**. |
| `0xE0xx` | Event pushed by the headset, **without subscription** (`0xF03E` "register notification" does nothing on the B170). |
| `0xFCxx` | Debug and log transfer. |
| `0xF8xx` | Firmware update. **Not implemented by this tool.** |

Most write acknowledgements are empty; seen exceptions: `0x700F [00]`, `0x7051 [02]` or `[03]`,
`0x701A [00]`. **confirmed**

## 4. Commands

Payload notation: bytes in hex; `f32` = IEEE-754 single precision, little-endian.

### 4.1 Noise control

| Request | Payload | Answer | Status |
|---|---|---|---|
| `C01E` | `03` | `401E`: `(type, value, 00)` triplets | confirmed |
| `F00F` | `01 <mode> 00` | `700F` | confirmed |
| event `E003` | same triplets as `401E` | | confirmed |

Triplet type 1 = current mode, type 2 = last noise-cancelling level chosen.
Modes: `1` high, `2` mid, `3` low, `4` adaptive, `5` off, `7` transparency. Older models use
`0` for off; `0xFE` also reads as transparency. Setting a mode plays the headset's voice prompt.
**confirmed**: all six values set and read back on the device; each noise-cancelling level
also becomes the "last level" (triplet type 2).

### 4.2 Battery

`C007` (empty) → `4007`: `[n, (id, value) × n]`; value bit 7 = charging, bits 0–6 = percent.
The B170 sends one entry with **id 6** (single device): `01 06 55` = 85 %, not charging.
Other ids (earbuds and case): 2 left, 3 right, 4 case. Event `E001` has the same format.
**confirmed** (read); **app** (event)

### 4.3 Identity

| Request | Answer | Status |
|---|---|---|
| `C01C` model | little-endian id: `70 b1` → `B170` | confirmed |
| `C042` firmware | UTF-8 text, e.g. `1.0.1.81` | confirmed |
| `C00C` color | 1 byte; `09` = white (`01` = black per the app) | confirmed (white) |
| `C006` serial | `[n]` then UTF-8 lines `a,type,value`; type 4 = serial number, type 2 = firmware, type 6 = headset address | confirmed |
| `C00D` features | 32-bit little-endian bit mask; `0x0002EB43` on the test unit | confirmed |
| `C028 <packet>` paired devices | `[packets, index, count, (flags, address ×6 LE, len, name UTF-8) × count]`; flags low nibble = connected, high nibble = the querying host; len bit 7 = name truncated | confirmed |

### 4.4 Device state

`C00A` (empty) → `400A`: `[n, (key, mask) × n]`, key 6 = single device. Mask bits:
0 in, 1 out, 2 worn, 3 music, 4 call, 5 firmware update, 6 fit test, 7 Bluetooth connected.
`01 06 8c` = worn, playing, connected. Event `E002`: same format. **confirmed** (read)

### 4.5 Audio settings

| Setting | Read | Write | Values | Status |
|---|---|---|---|---|
| EQ preset | `C01F` → `[p]` | `F010 [p]` | `0` balanced, `1` voice, `2` more treble, `3` more bass, `5` custom. The byte keeps its value while the advanced EQ is on (the vendor app also understands `0x64` as "advanced", never seen from a B170) | confirmed |
| Advanced EQ switch | `C04C` → `[s]` | `F04F [s]` | `1` on, `0` off. Writing advanced bands (`F050`) also turns it on. | confirmed |
| Custom EQ (3 bands) | `C044` | `F041` | `[n, total f32, (type, gain f32, freq f32, q f32) × n]`; bytes past the bands in the answer are stale memory | confirmed |
| Advanced EQ (up to 8 bands) | `C04D [profile]` | `F050` | `[profile, n, total f32, (type, gain f32, freq f32, q f32) × n]`; profile `0`, type `1` (peaking). The headset stores any band count from 1 to 8 and the total gain as written. | confirmed |
| Bass boost | `C04E` → `[on, level×2]` | `F051 [on, level×2]` | levels 1–5 (bytes `02`–`0A`) written and read back; the device stores both bytes as written. Audible difference between levels not measured | confirmed |
| Spatial audio | `C04F` → `[mode, head]` | `F052 [mode, head]` | `00 00` off, `01 00` on, `01 01` head tracking | confirmed |
| Low latency | `C041` → `[s]` | `F040 [s]` | `1` on, `2` off | confirmed |
| Multipoint | `C027` → `[s]` | `F01A [s]` | `1` on, `0` off; no reboot | confirmed |
| Extra features | `C00E` → `[n, (id, value) × n]` | `F004 [01, id, value]` | id 1 = wear detection; other ids unknown | confirmed |
| Audiodo profile | `C05A` → `[s]` | `F05C [s]` | `1` on, `0` off; the device then pushes `E019 [profile, sdk_link]` | confirmed |
| Codec | `C029` → `[c]` | `F01C [c]` | `0` AAC, `2` LDAC (`1` LHDC, not on the B170). **Writing reboots the headset** (~6 s). | confirmed |

Advanced EQ limits applied by the vendor app (**app**): at most 8 bands, gain −6..+6 dB in
0.5 dB steps, 20–20 000 Hz, Q 0.1–10.

The total gain sent with a curve is computed by the app, not by the headset (**app**, read from
`EQAlgorithm.getCoordinate` and its callers):

- `peak` is the highest point, floored at 0 dB, of the curve's response, evaluated as follows:
  frequencies and gains are rounded to whole numbers (halves away from zero); each band is an
  RBJ peaking biquad at 44 100 Hz (`A = 10^(gain/40)`, `alpha = sin(w0) / 2Q`); the bands are
  cascaded as complex products; the grid is 300 points spaced 0.01 decade from 20 Hz, whose
  first N points are replaced by the N band centres.
- Firmware 1.0.1.46 and later: `total = min(0, 6 − peak)`, headroom only once the curve rises
  above +6 dB. Older firmware: `total = −(trunc(peak) + 1)`.

`nothing` implements the same computation; its result matches a transcription of the app's
code to the last digit on the test curves. The headset stores the total gain as written: a
3-band curve sent with −3.2 dB was read back unchanged. **confirmed**

### 4.6 Controls (button and wheel)

`C018` (empty) → `4018`: `[n, (device, button, gesture, action) × n]`; `F003` writes the same
layout and the headset stores it as written. **confirmed** (read, write and restore).

| Field | Values on the B170 |
|---|---|
| device | `06` the headset (earbuds use `02` left, `03` right) |
| button | `01` wheel, `0A` customisable button |
| gesture | `01` single press, `07` press and hold |
| action | `01` none, `02` play/pause, `0A` noise control off/transparency/ANC, `14` off/ANC, `15` off/transparency, `16` ANC/transparency, `0B` voice assistant, `1B` spatial audio, `1D` mute the microphone in calls, `1F` news, `20` "channel hop", `21` "Essential Space", `22` next EQ preset |

The B170 reports three bindings: wheel hold, button press, button hold. The wheel's single press
(play/pause) and its rotation (volume) are fixed and not part of the list. Which actions the
vendor app offers for each binding: **app** (see `nothing controls actions`). Every offered
action was written to the wheel's and the button's press-and-hold and read back unchanged
(17 writes), and to the button's single press for `20` and `16`. **confirmed** (stored; what
each action does when pressed was not tested one by one). Actions `1F`,
`20` and `21` drive features of Nothing phones; whether binding `20` makes the headset send the
magic-button event (`E014`) to a Linux client is **untested**.

When the headset believes it is not worn (state bit `out`, see 4.4) and wear detection is on,
presses on the button and the wheel seem to be ignored while the volume still works: both were
observed together once, with one ear cup off the ear. **inferred**

### 4.7 Head tracking

`C058 01` starts the `E015` stream, `C058 00` stops it (answer `4058`). Each `E015` payload is
a quaternion **x, y, z, w** as four int32 little-endian in **Q30** fixed point (divide by 2³⁰);
the norm is 1.0000 on every frame. Rate ~9–10 frames per second. A few frames may still
arrive right after the stop. **confirmed**

Axes, from a labelled recording of a person wearing the headset: nodding rotates about **X**
(±30–50°), shaking the head about **Z**; slow tilts towards a shoulder barely register on any
axis. **confirmed** (one unit, one wearer). The orientation can drift: once, after a long
streaming session, it turned steadily at about −21°/s around Z with the head still; a power
cycle of the headset removed it. **confirmed** (seen once, cause unknown)

### 4.8 Pushed events

| Code | Event | Payload | Status |
|---|---|---|---|
| `E001` | battery changed | as `4007` | app |
| `E002` | device state changed | as `400A` | app |
| `E003` | noise control changed | as `401E`: `01 07 00` transparency, `01 01 00` high, sent when the mode is changed on the headset (not after `F00F`) | confirmed |
| `E009` | working status changed | 3 bytes, meaning unknown | inferred |
| `E00E` | connection changed | 8 bytes (address + state) | inferred |
| `E014` | magic button pressed | 2 bytes; the app reads byte 0 as an action code. Not seen from a B170 yet: with the button bound to noise control (`16`), a press sends `E003` instead | app |
| `E015` | head tracking | quaternion, see 4.7 | confirmed |
| `E016` | Bluetooth / LE Audio status | 2 bytes | inferred |
| `E017` | rewind / fast forward | `[type, down]`; probably only sent to a Nothing phone | inferred |
| `E018` | Essential Space | probably only sent to a Nothing phone | inferred |
| `E019` | Audiodo state | `[profile, sdk_link]` | confirmed |

### 4.9 Diagnostic logs (debug class)

| Request | Answer | Status |
|---|---|---|
| `FC03` file list | `7C03`: names separated by `;` — `Ecode.bin;normal.log` on the B170 | confirmed |
| `FC21` log info | `7C21`: `[status, packet_size u32, file_size u32, crc32 u32, side]`, LE; status 1 = available. A unit with nothing buried answers status 2, 240-byte packets, size 0. | confirmed |
| `FC22 <packet u16 LE>` log packet | `7C22`: `[index u32 LE, data]` | app |
| `FC09` debug info | `7C09`: 3 bytes, `00 00 00` seen. Meaning (logging on/off and level) **inferred** | confirmed (raw) |

Other debug codes (`FC01` test mode, `FC07`, `FC08` log level, file transfer `FC04`–`FC06`)
are not used by this tool.

## 5. Commands this tool refuses

| Code | Why |
|---|---|
| `F8xx` | Firmware update: brick risk. Never sent. |
| `F01C` | Changes the codec and reboots the headset: needs `--dangerous`. |
| `F03D` | Factory reset: needs `--dangerous`. |
| `F066`, `F01B` | Forget / connect / disconnect another paired device: needs `--dangerous`. |
| `F002` | Makes the headset ring: needs `--dangerous`. |
| `FC01`, `FC07`, `FC08` | Factory test mode and log level: needs `--dangerous`. |
| any other untested code | needs `--dangerous`. |
