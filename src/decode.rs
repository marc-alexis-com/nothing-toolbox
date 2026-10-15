//! Payload decoders. Test vectors are payloads captured from a real B170 unless noted.

use std::fmt;

// --- ANC ----------------------------------------------------------------------------------

/// Noise control modes as sent in 0xF00F `[0x01, mode, 0x00]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AncMode {
    High = 1,
    Mid = 2,
    Low = 3,
    Adaptive = 4,
    Off = 5,
    Transparency = 7,
}

impl AncMode {
    pub fn from_byte(b: u8) -> Option<AncMode> {
        Some(match b {
            1 => AncMode::High,
            2 => AncMode::Mid,
            3 => AncMode::Low,
            4 => AncMode::Adaptive,
            // 0 is "off" on older models; the B170 reports 5.
            0 | 5 => AncMode::Off,
            7 | 0xFE => AncMode::Transparency,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            AncMode::High => "high",
            AncMode::Mid => "mid",
            AncMode::Low => "low",
            AncMode::Adaptive => "adaptive",
            AncMode::Off => "off",
            AncMode::Transparency => "transparency",
        }
    }

    pub fn is_anc(self) -> bool {
        matches!(self, AncMode::High | AncMode::Mid | AncMode::Low | AncMode::Adaptive)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AncState {
    /// Current mode (triplet type 1). Raw byte kept for unknown values.
    pub mode: Option<u8>,
    /// Last noise-cancelling level chosen (triplet type 2).
    pub last_level: Option<u8>,
}

/// 0x401E response and 0xE003 event: `(type, value, reserved)` triplets.
pub fn anc(p: &[u8]) -> AncState {
    let mut st = AncState::default();
    for t in p.as_chunks::<3>().0 {
        match t[0] {
            1 => st.mode = Some(t[1]),
            2 => st.last_level = Some(t[1]),
            _ => {}
        }
    }
    st
}

pub fn anc_name(b: u8) -> String {
    AncMode::from_byte(b).map(|m| m.name().to_string()).unwrap_or_else(|| format!("unknown(0x{b:02x})"))
}

impl fmt::Display for AncState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.mode {
            Some(m) => write!(f, "{}", anc_name(m))?,
            None => write!(f, "?")?,
        }
        if let Some(l) = self.last_level {
            write!(f, " (last ANC level: {})", anc_name(l))?;
        }
        Ok(())
    }
}

// --- Battery ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub id: u8,
    pub level: u8,
    pub charging: bool,
}

impl Battery {
    pub fn part(&self) -> &'static str {
        match self.id {
            2 => "left",
            3 => "right",
            4 => "case",
            6 | 7 => "headset",
            _ => "unknown",
        }
    }
}

/// 0x4007 response and 0xE001 event: `[n, (id, value) × n]`, value = bit 7 charging,
/// bits 0-6 percent. The B170 reports a single entry with id 6.
pub fn battery(p: &[u8]) -> Vec<Battery> {
    p.get(1..)
        .unwrap_or(&[])
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| Battery { id: c[0], level: c[1] & 0x7F, charging: c[1] & 0x80 != 0 })
        .collect()
}

impl fmt::Display for Battery {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}%{}", self.part(), self.level, if self.charging { " (charging)" } else { "" })
    }
}

// --- Identity ----------------------------------------------------------------------------

/// 0x401C: model id, little-endian (`70 b1` -> "B170").
pub fn model(p: &[u8]) -> Option<String> {
    if p.is_empty() {
        return None;
    }
    Some(p.iter().rev().map(|b| format!("{b:02X}")).collect())
}

/// 0x4042: firmware version as UTF-8 text.
pub fn text(p: &[u8]) -> String {
    let end = p.iter().position(|&b| b == 0).unwrap_or(p.len());
    String::from_utf8_lossy(&p[..end]).into_owned()
}

/// 0x400C: color id. Names from the vendor app's device list (0x09 seen on a white unit).
pub fn color(p: &[u8]) -> String {
    match p.first() {
        Some(0x01) => "black".into(),
        Some(0x09) => "white".into(),
        Some(b) => format!("0x{b:02x}"),
        None => "?".into(),
    }
}

/// 0x4006: `[n, text]`, lines split by '\n', fields by ','; type 4 is the serial number.
pub fn serial(p: &[u8]) -> Option<String> {
    let txt = String::from_utf8_lossy(p.get(1..).unwrap_or(&[])).into_owned();
    txt.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            (f.len() >= 3 && f[1] == "4" && !f[2].is_empty()).then(|| f[2].trim_end_matches('\0').to_string())
        })
        .next_back()
}

// --- Audio settings ----------------------------------------------------------------------

pub const EQ_PRESETS: [(u8, &str); 5] =
    [(0, "balanced"), (3, "more-bass"), (2, "more-treble"), (1, "voice"), (5, "custom")];

/// 0x401F: preset byte; 0x64 means the advanced EQ is active.
pub fn eq_preset(p: &[u8]) -> String {
    match p.first() {
        Some(0x64) => "advanced".into(),
        Some(b) => EQ_PRESETS
            .iter()
            .find(|(v, _)| v == b)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| format!("unknown(0x{b:02x})")),
        None => "?".into(),
    }
}

pub fn eq_preset_byte(name: &str) -> Option<u8> {
    EQ_PRESETS.iter().find(|(_, n)| *n == name).map(|(v, _)| *v)
}

/// 0x4029: 0 AAC, 1 LHDC (not on the B170), 2 LDAC.
pub fn codec(p: &[u8]) -> String {
    match p.first() {
        Some(0) => "aac".into(),
        Some(1) => "lhdc".into(),
        Some(2) => "ldac".into(),
        Some(b) => format!("unknown(0x{b:02x})"),
        None => "?".into(),
    }
}

/// 0x4041: 1 enabled, 2 disabled.
pub fn latency(p: &[u8]) -> String {
    match p.first() {
        Some(1) => "on".into(),
        Some(2) => "off".into(),
        Some(b) => format!("unknown(0x{b:02x})"),
        None => "?".into(),
    }
}

/// One-byte switches (multipoint 0x4027, advanced EQ 0x404C, Audiodo 0x405A): 1 = on.
pub fn switch(p: &[u8]) -> String {
    match p.first() {
        Some(1) => "on".into(),
        Some(0) => "off".into(),
        Some(b) => format!("unknown(0x{b:02x})"),
        None => "?".into(),
    }
}

/// 0x404F `[mode, head]`.
pub fn spatial(p: &[u8]) -> String {
    match (p.first(), p.get(1)) {
        (_, Some(1)) => "head-tracking".into(),
        (Some(0), _) => "off".into(),
        (Some(1), _) => "on".into(),
        (Some(2), _) => "concert".into(),
        (Some(3), _) => "movie".into(),
        _ => format!("unknown({})", crate::frame::hex(p)),
    }
}

pub fn spatial_payload(name: &str) -> Option<[u8; 2]> {
    Some(match name {
        "off" => [0, 0],
        "on" => [1, 0],
        "head-tracking" => [1, 1],
        _ => return None,
    })
}

/// 0x404E `[switch, level × 2]` (encoding "V1", the one the vendor app uses for the B170).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BassBoost {
    pub on: bool,
    pub level: u8,
}

pub fn bass_boost(p: &[u8]) -> BassBoost {
    let level = p.get(1).copied().unwrap_or(0) / 2;
    BassBoost { on: p.first() == Some(&1), level: if level == 0 { 5 } else { level } }
}

impl fmt::Display for BassBoost {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.on {
            write!(f, "on, level {}", self.level)
        } else {
            write!(f, "off (stored level {})", self.level)
        }
    }
}

/// 0x400E `[n, (id, value) × n]`. Only id 1 (in-ear detection) has a known meaning.
pub fn extra_features(p: &[u8]) -> Vec<(u8, u8)> {
    p.get(1..).unwrap_or(&[]).as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).collect()
}

pub fn in_ear_detection(p: &[u8]) -> Option<bool> {
    extra_features(p).into_iter().find(|(id, _)| *id == 1).map(|(_, v)| v != 0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqBand {
    pub filter: u8,
    pub gain: f32,
    pub freq: f32,
    pub q: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Eq {
    pub profile: Option<u8>,
    pub total_gain: f32,
    pub bands: Vec<EqBand>,
}

fn f32_at(p: &[u8], i: usize) -> Option<f32> {
    Some(f32::from_le_bytes(p.get(i..i + 4)?.try_into().ok()?))
}

fn eq_bands(p: &[u8], n: usize, start: usize) -> Vec<EqBand> {
    (0..n)
        .map_while(|i| {
            let o = start + 13 * i;
            Some(EqBand {
                filter: *p.get(o)?,
                gain: f32_at(p, o + 1)?,
                freq: f32_at(p, o + 5)?,
                q: f32_at(p, o + 9)?,
            })
        })
        .collect()
}

/// 0x4044 custom EQ: `[n, total f32, (type, gain, freq, q) × n]`. Bytes past the bands are
/// uninitialised firmware memory and must be ignored.
pub fn custom_eq(p: &[u8]) -> Option<Eq> {
    let n = *p.first()? as usize;
    Some(Eq { profile: None, total_gain: f32_at(p, 1)?, bands: eq_bands(p, n, 5) })
}

/// 0x404D advanced EQ: `[profile, n, total f32, (type, gain, freq, q) × n]`.
pub fn advanced_eq(p: &[u8]) -> Option<Eq> {
    let n = *p.get(1)? as usize;
    Some(Eq { profile: Some(p[0]), total_gain: f32_at(p, 2)?, bands: eq_bands(p, n, 6) })
}

impl fmt::Display for Eq {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "total gain {:+.1} dB", self.total_gain)?;
        for b in &self.bands {
            write!(f, "\n    {:>7.0} Hz  {:+5.1} dB  Q {:.2}  (filter {})", b.freq, b.gain, b.q, b.filter)?;
        }
        Ok(())
    }
}

// --- Device status -----------------------------------------------------------------------

const STATUS_BITS: [&str; 8] = ["in", "out", "worn", "music", "call", "ota", "fit-test", "bt-connected"];

/// 0x400A response / 0xE002 event: `[n, (key, mask) × n]`. Key 6 = single device.
pub fn device_status(p: &[u8]) -> Vec<(u8, Vec<&'static str>)> {
    p.get(1..)
        .unwrap_or(&[])
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| (c[0], (0..8).filter(|i| c[1] & (1 << i) != 0).map(|i| STATUS_BITS[i]).collect()))
        .collect()
}

pub fn device_status_text(p: &[u8]) -> String {
    let parts: Vec<String> = device_status(p)
        .into_iter()
        .map(|(k, flags)| {
            let flags = if flags.is_empty() { "-".to_string() } else { flags.join(",") };
            if k == 6 {
                flags
            } else {
                format!("key {k}: {flags}")
            }
        })
        .collect();
    if parts.is_empty() {
        "?".into()
    } else {
        parts.join("; ")
    }
}

// --- Paired devices ----------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedDevice {
    pub mac: String,
    pub name: String,
    pub connected: bool,
    pub current: bool,
    pub name_truncated: bool,
}

/// 0x4028 packet: `[packets, index, count, (flags, mac LE ×6, len, name) × count]`.
/// Returns (total packets, devices in this packet).
pub fn paired_list(p: &[u8]) -> (u8, Vec<PairedDevice>) {
    let total = p.first().copied().unwrap_or(0);
    let count = p.get(2).copied().unwrap_or(0) as usize;
    let mut out = Vec::new();
    let mut i = 3;
    for _ in 0..count {
        let Some(head) = p.get(i..i + 8) else { break };
        let name_len = (head[7] & 0x7F) as usize;
        let Some(name) = p.get(i + 8..i + 8 + name_len) else { break };
        out.push(PairedDevice {
            mac: head[1..7].iter().rev().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":"),
            name: String::from_utf8_lossy(name).into_owned(),
            connected: head[0] & 0x0F != 0,
            current: head[0] & 0xF0 != 0,
            name_truncated: head[7] & 0x80 != 0,
        });
        i += 8 + name_len;
    }
    (total, out)
}

// --- Controls (buttons and wheel) ----------------------------------------------------------

/// One control binding of 0x4018 / 0xF003: `(device, button, gesture, operation)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub device: u8,
    pub button: u8,
    pub gesture: u8,
    pub operation: u8,
}

/// The single device of a headphone (as opposed to left/right earbuds).
pub const DEVICE_HEADSET: u8 = 6;
pub const BUTTON_WHEEL: u8 = 1;
pub const BUTTON_CUSTOM: u8 = 10;
pub const GESTURE_PRESS: u8 = 1;
pub const GESTURE_HOLD: u8 = 7;

/// Actions the vendor app offers on the B170, with their wire code.
pub const ACTIONS: [(u8, &str); 13] = [
    (1, "none"),
    (2, "play-pause"),
    (10, "noise-cycle"),
    (20, "noise-anc-off"),
    (21, "noise-transparency-off"),
    (22, "noise-anc-transparency"),
    (11, "assistant"),
    (27, "spatial"),
    (29, "mic-mute"),
    (31, "news"),
    (32, "channel-hop"),
    (33, "essential-space"),
    (34, "eq-preset"),
];

pub fn action_name(op: u8) -> String {
    ACTIONS
        .iter()
        .find(|(v, _)| *v == op)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| format!("unknown({op})"))
}

pub fn action_code(name: &str) -> Option<u8> {
    ACTIONS.iter().find(|(_, n)| *n == name).map(|(v, _)| *v)
}

/// Which actions the vendor app lets the user pick for (button, gesture) on the B170.
/// The wheel's single press is fixed to play/pause.
pub fn allowed_actions(button: u8, gesture: u8) -> &'static [u8] {
    match (button, gesture) {
        (BUTTON_WHEEL, GESTURE_HOLD) => &[1, 10, 20, 21, 22],
        (BUTTON_CUSTOM, GESTURE_PRESS) => &[1, 10, 20, 21, 22, 11, 27, 29, 31, 32, 34],
        (BUTTON_CUSTOM, GESTURE_HOLD) => &[1, 10, 20, 21, 22, 11, 27, 29, 31, 32, 33, 34],
        _ => &[],
    }
}

pub fn control_name(button: u8) -> String {
    match button {
        BUTTON_WHEEL => "wheel".into(),
        BUTTON_CUSTOM => "button".into(),
        b => format!("control {b}"),
    }
}

pub fn gesture_name(gesture: u8) -> String {
    match gesture {
        GESTURE_PRESS => "press".into(),
        GESTURE_HOLD => "hold".into(),
        g => format!("gesture {g}"),
    }
}

/// 0x4018: `[n, (device, button, gesture, operation) × n]`.
pub fn bindings(p: &[u8]) -> Vec<Binding> {
    let n = p.first().copied().unwrap_or(0) as usize;
    p.get(1..)
        .unwrap_or(&[])
        .as_chunks::<4>()
        .0
        .iter()
        .take(n)
        .map(|c| Binding { device: c[0], button: c[1], gesture: c[2], operation: c[3] })
        .collect()
}

/// 0xF003 payload: same layout as the 0x4018 answer.
pub fn bindings_payload(list: &[Binding]) -> Vec<u8> {
    let mut out = vec![list.len() as u8];
    for b in list {
        out.extend_from_slice(&[b.device, b.button, b.gesture, b.operation]);
    }
    out
}

// --- Head tracking -----------------------------------------------------------------------

/// 0xE015: quaternion x, y, z, w as four int32 LE in Q30 fixed point (unit norm).
pub fn quaternion(p: &[u8]) -> Option<[f64; 4]> {
    let mut q = [0.0; 4];
    for (i, v) in q.iter_mut().enumerate() {
        let raw = i32::from_le_bytes(p.get(4 * i..4 * i + 4)?.try_into().ok()?);
        *v = raw as f64 / (1u32 << 30) as f64;
    }
    Some(q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::parse_hex;

    fn h(s: &str) -> Vec<u8> {
        parse_hex(s).unwrap()
    }

    #[test]
    fn anc_triplets() {
        let st = anc(&h("01 01 00 02 01 00"));
        assert_eq!(st, AncState { mode: Some(1), last_level: Some(1) });
        assert_eq!(st.to_string(), "high (last ANC level: high)");
        assert_eq!(anc(&h("01 07 00")).mode, Some(7));
        assert_eq!(AncMode::from_byte(5), Some(AncMode::Off));
        assert_eq!(AncMode::from_byte(0xFE), Some(AncMode::Transparency));
    }

    #[test]
    fn battery_single_entry() {
        assert_eq!(battery(&h("01 06 55")), vec![Battery { id: 6, level: 85, charging: false }]);
        assert_eq!(battery(&h("01 06 c8"))[0].to_string(), "headset 72% (charging)");
        assert!(battery(&[]).is_empty());
    }

    #[test]
    fn identity() {
        assert_eq!(model(&h("70 b1")).as_deref(), Some("B170"));
        assert_eq!(text(&h("31 2e 30 2e 31 2e 38 31")), "1.0.1.81");
        assert_eq!(color(&h("09")), "white");
        // Synthetic serial (shape of a real answer, value made up).
        let p = [&[4u8][..], b"6,1,\n6,2,1.0.1.81\n6,4,SN0000TEST\n6,6,BC9A78563412"].concat();
        assert_eq!(serial(&p).as_deref(), Some("SN0000TEST"));
    }

    #[test]
    fn audio_settings() {
        assert_eq!(eq_preset(&h("00")), "balanced");
        assert_eq!(eq_preset(&h("03")), "more-bass");
        assert_eq!(eq_preset(&h("64")), "advanced");
        assert_eq!(eq_preset_byte("custom"), Some(5));
        assert_eq!(codec(&h("02")), "ldac");
        assert_eq!(latency(&h("02")), "off");
        assert_eq!(switch(&h("00")), "off");
        assert_eq!(spatial(&h("00 00")), "off");
        assert_eq!(spatial(&h("01 01")), "head-tracking");
        assert_eq!(bass_boost(&h("00 0a")), BassBoost { on: false, level: 5 });
        assert_eq!(bass_boost(&h("01 04")), BassBoost { on: true, level: 2 });
    }

    #[test]
    fn extra_feature_pairs() {
        let p = h("09 01 01 02 01 07 01 09 01 0a 01 0b 00 0e 01 12 01 15 01");
        assert_eq!(extra_features(&p).len(), 9);
        assert_eq!(in_ear_detection(&p), Some(true));
        assert_eq!(in_ear_detection(&h("01 01 00")), Some(false));
    }

    #[test]
    fn custom_eq_ignores_trailing_garbage() {
        let mut p = h("03 0000c0c0 00 0000c040 00000c43 cdcc4c3f 01 0000c0c0 00007544 3333333f 02 0000c0c0 00a0d745 0000803f");
        p.extend_from_slice(&[0xAA; 65]);
        let eq = custom_eq(&p).unwrap();
        assert_eq!(eq.total_gain, -6.0);
        assert_eq!(eq.bands.len(), 3);
        assert_eq!((eq.bands[0].filter, eq.bands[0].gain, eq.bands[0].freq), (0, 6.0, 140.0));
        assert_eq!(eq.bands[2].freq, 6900.0);
    }

    #[test]
    fn advanced_eq_flat_curve() {
        let p = h("00 08 00000000 \
            01 00000000 00005c42 0000803f 01 00000000 0000dc42 0000803f \
            01 00000000 00005c43 0000803f 01 00000000 0000dc43 0000803f \
            01 00000000 0000a544 0000803f 01 00000000 00404e45 0000803f \
            01 00000000 0040ce45 0000803f 01 00000000 00404e46 0000803f");
        let eq = advanced_eq(&p).unwrap();
        assert_eq!(eq.profile, Some(0));
        let freqs: Vec<f32> = eq.bands.iter().map(|b| b.freq).collect();
        assert_eq!(freqs, vec![55.0, 110.0, 220.0, 440.0, 1320.0, 3300.0, 6600.0, 13200.0]);
    }

    #[test]
    fn device_status_mask() {
        assert_eq!(device_status_text(&h("01 06 8c")), "worn,music,bt-connected");
    }

    #[test]
    fn paired_list_packet() {
        // Synthetic packet: one current+connected device, one remembered device.
        let p =
            [&h("01 00 02 11 bc 9a 78 56 34 12 04")[..], b"Desk", &h("00 01 02 03 04 05 06 83")[..], b"abc"]
                .concat();
        let (total, list) = paired_list(&p);
        assert_eq!(total, 1);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].mac, "12:34:56:78:9A:BC");
        assert!(list[0].current && list[0].connected);
        assert_eq!(list[1].name, "abc");
        assert!(list[1].name_truncated && !list[1].connected);
    }

    #[test]
    fn control_bindings_round_trip() {
        // Answer of a B170: wheel hold -> none, button press -> ANC/transparency,
        // button hold -> assistant.
        let p = h("03 06 01 07 01 06 0a 01 16 06 0a 07 0b");
        let list = bindings(&p);
        assert_eq!(list.len(), 3);
        assert_eq!(list[1], Binding { device: 6, button: 10, gesture: 1, operation: 22 });
        assert_eq!(action_name(list[1].operation), "noise-anc-transparency");
        assert_eq!(bindings_payload(&list), p);
        assert!(allowed_actions(BUTTON_CUSTOM, GESTURE_PRESS).contains(&32));
        assert!(!allowed_actions(BUTTON_WHEEL, GESTURE_PRESS).contains(&1));
        assert!(!allowed_actions(BUTTON_CUSTOM, GESTURE_PRESS).contains(&33));
    }

    #[test]
    fn quaternion_q30() {
        let mut p = Vec::new();
        for v in [0i32, 0, 0, 1 << 30] {
            p.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(quaternion(&p), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(quaternion(&p[..15]), None);
    }
}
