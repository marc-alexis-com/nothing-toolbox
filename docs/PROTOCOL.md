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
| The channel accepts **one connection at a time**. A second client gets connection errors while the first holds it. This is why `nothing` funnels everything through a single owner. | confirmed |
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
| event `E003` | same triplets as `401E` | | app |

Triplet type 1 = current mode, type 2 = last noise-cancelling level chosen.
Modes: `1` high, `2` mid, `3` low, `4` adaptive, `5` off, `7` transparency. Older models use
`0` for off; `0xFE` also reads as transparency. Setting a mode plays the headset's voice prompt.
**confirmed** (values 1, 5, 7 set and read back on the device; 2–4 from the app).

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
| EQ preset | `C01F` → `[p]` | `F010 [p]` | `0` balanced, `1` voice, `2` more treble, `3` more bass, `5` custom; reads `0x64` while the advanced EQ is on | confirmed |
| Advanced EQ switch | `C04C` → `[s]` | `F04F [s]` | `1` on, `0` off. Writing advanced bands (`F050`) also turns it on. | confirmed |
| Custom EQ (3 bands) | `C044` | `F041` | `[n, total f32, (type, gain f32, freq f32, q f32) × n]`; bytes past the bands in the answer are stale memory | confirmed |
| Advanced EQ (8 bands) | `C04D [profile]` | `F050` | `[profile, n, total f32, (type, gain f32, freq f32, q f32) × n]` | confirmed |
| Bass boost | `C04E` → `[on, level×2]` | `F051 [on, level×2]` | the device stores both bytes as written | confirmed |
| Spatial audio | `C04F` → `[mode, head]` | `F052 [mode, head]` | `00 00` off, `01 00` on, `01 01` head tracking | confirmed |
| Low latency | `C041` → `[s]` | `F040 [s]` | `1` on, `2` off | confirmed |
| Multipoint | `C027` → `[s]` | `F01A [s]` | `1` on, `0` off; no reboot | confirmed |
| Extra features | `C00E` → `[n, (id, value) × n]` | `F004 [01, id, value]` | id 1 = wear detection; other ids unknown | confirmed |
| Audiodo profile | `C05A` → `[s]` | `F05C [s]` | `1` on, `0` off; the device then pushes `E019 [profile, sdk_link]` | confirmed |
| Codec | `C029` → `[c]` | `F01C [c]` | `0` AAC, `2` LDAC (`1` LHDC, not on the B170). **Writing reboots the headset** (~6 s). | confirmed |

### 4.6 Head tracking

`C058 01` starts the `E015` stream, `C058 00` stops it (answer `4058`). Each `E015` payload is
a quaternion **x, y, z, w** as four int32 little-endian in **Q30** fixed point (divide by 2³⁰);
the norm is 1.0000 on every frame. Rate ~9–10 frames per second. A few frames may still
arrive right after the stop. **confirmed**

### 4.7 Pushed events

| Code | Event | Payload | Status |
|---|---|---|---|
| `E001` | battery changed | as `4007` | app |
| `E002` | device state changed | as `400A` | app |
| `E003` | noise control changed | as `401E` | app |
| `E009` | working status changed | 3 bytes, meaning unknown | inferred |
| `E00E` | connection changed | 8 bytes (address + state) | inferred |
| `E014` | **magic button pressed** | 2 bytes; the app reads byte 0 as an action code | app |
| `E015` | head tracking | quaternion, see 4.6 | confirmed |
| `E016` | Bluetooth / LE Audio status | 2 bytes | inferred |
| `E017` | rewind / fast forward | `[type, down]`; probably only sent to a Nothing phone | inferred |
| `E018` | Essential Space | probably only sent to a Nothing phone | inferred |
| `E019` | Audiodo state | `[profile, sdk_link]` | confirmed |

### 4.8 Diagnostic logs (debug class)

| Request | Answer | Status |
|---|---|---|
| `FC03` file list | `7C03`: names separated by `;` — `Ecode.bin;normal.log` on the B170 | confirmed |
| `FC21` log info | `7C21`: `[status, packet_size u32, file_size u32, crc32 u32, side]`, LE; status 1 = available. A unit with nothing buried answers status 2, 240-byte packets, size 0. | confirmed |
| `FC22 <packet u16 LE>` log packet | `7C22`: `[index u32 LE, data]` | app |
| `FC09` debug info | `7C09`: logging state, 3 bytes (`00 00 00` seen) | confirmed (raw) |

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
