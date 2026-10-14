//! Wire frame of the Nothing control channel (RFCOMM channel 15). See docs/PROTOCOL.md.
//!
//! ```text
//! 0x55 | control u16 LE | command u16 LE | length u16 LE | fsn u8 | payload | CRC u16 LE
//! ```
//! The CRC-16/MODBUS covers everything before it, 0x55 included, and is present only when
//! bit 0x20 of the control word is set. The client always sends control 0x0160 (CRC on).

pub const HEADER: u8 = 0x55;
pub const CTL_CLIENT: u16 = 0x0160;
pub const CTL_CRC: u16 = 0x0020;
const HEAD_LEN: usize = 8;
/// Longest plausible payload. The largest ones seen on the B170 are ~250 bytes; anything
/// longer is a false start (a stray 0x55 inside data) that we skip, instead of waiting for
/// kilobytes that will never come.
const MAX_PAYLOAD: usize = 4096;

/// CRC-16/MODBUS: init 0xFFFF, reflected polynomial 0xA001, no final XOR.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

/// Builds a client frame, CRC included.
pub fn encode(cmd: u16, payload: &[u8], fsn: u8) -> Vec<u8> {
    let len = u16::try_from(payload.len()).expect("payload longer than 65535 bytes");
    let mut v = Vec::with_capacity(HEAD_LEN + payload.len() + 2);
    v.push(HEADER);
    v.extend_from_slice(&CTL_CLIENT.to_le_bytes());
    v.extend_from_slice(&cmd.to_le_bytes());
    v.extend_from_slice(&len.to_le_bytes());
    v.push(fsn);
    v.extend_from_slice(payload);
    let crc = crc16(&v);
    v.extend_from_slice(&crc.to_le_bytes());
    v
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub ctl: u16,
    pub cmd: u16,
    pub fsn: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    /// Event pushed by the headset (class 0xE0xx).
    pub fn is_notification(&self) -> bool {
        self.cmd & 0xFF00 == 0xE000
    }
}

/// Response code expected for a request: bit 15 cleared (0xC01E -> 0x401E).
pub fn response_code(cmd: u16) -> u16 {
    cmd & 0x7FFF
}

/// Splits an RFCOMM byte stream into frames. One `read` may carry several frames, and one
/// frame may arrive in several pieces: leftovers are kept for the next call.
#[derive(Default)]
pub struct Reader {
    buf: Vec<u8>,
    /// Frames dropped because of a bad CRC (diagnostics).
    pub bad_crc: u64,
}

impl Reader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, data: &[u8]) -> Vec<Frame> {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            let Some(start) = self.buf.iter().position(|&b| b == HEADER) else {
                self.buf.clear();
                break;
            };
            if start > 0 {
                self.buf.drain(..start);
            }
            if self.buf.len() < HEAD_LEN {
                break;
            }
            let ctl = u16::from_le_bytes([self.buf[1], self.buf[2]]);
            let cmd = u16::from_le_bytes([self.buf[3], self.buf[4]]);
            let len = u16::from_le_bytes([self.buf[5], self.buf[6]]) as usize;
            if len > MAX_PAYLOAD {
                self.buf.drain(..1);
                continue;
            }
            let has_crc = ctl & CTL_CRC != 0;
            let total = HEAD_LEN + len + if has_crc { 2 } else { 0 };
            if self.buf.len() < total {
                break;
            }
            if has_crc {
                let got = u16::from_le_bytes([self.buf[total - 2], self.buf[total - 1]]);
                if crc16(&self.buf[..total - 2]) != got {
                    // False start or damaged frame: move one byte forward and resync.
                    self.bad_crc += 1;
                    self.buf.drain(..1);
                    continue;
                }
            }
            out.push(Frame {
                ctl,
                cmd,
                fsn: self.buf[7],
                payload: self.buf[HEAD_LEN..HEAD_LEN + len].to_vec(),
            });
            self.buf.drain(..total);
        }
        out
    }
}

pub fn hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 3);
    for (i, b) in data.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Parses hex, tolerating whitespace, `:`, `-` and `0x`.
pub fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    let cleaned = s.replace("0x", "");
    let digits: String = cleaned.chars().filter(|c| !c.is_whitespace() && *c != ':' && *c != '-').collect();
    if !digits.len().is_multiple_of(2) {
        return Err(format!("odd number of hex digits: {s:?}"));
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).map_err(|e| format!("{s:?}: {e}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Vec<u8> {
        parse_hex(s).unwrap()
    }

    #[test]
    fn crc_modbus_check_value() {
        assert_eq!(crc16(b"123456789"), 0x4B37);
    }

    #[test]
    fn encode_matches_frames_sent_to_the_headset() {
        // Read requests, all answered by a real B170.
        assert_eq!(encode(0xC01E, &[0x03], 1), h("55 60 01 1e c0 01 00 01 03 9c 89"));
        assert_eq!(encode(0xC007, &[], 1), h("55 60 01 07 c0 00 00 01 ac df"));
        assert_eq!(encode(0xC042, &[], 1), h("55 60 01 42 c0 00 00 01 61 10"));
        assert_eq!(encode(0xC04D, &[0x00], 1), h("55 60 01 4d c0 01 00 01 00 d0 eb"));
        // Codec write (AAC / LDAC), accepted by a real B170.
        assert_eq!(encode(0xF01C, &[0x00], 1), h("5560011cf0010001009d6e"));
        assert_eq!(encode(0xF01C, &[0x02], 1), h("5560011cf0010001021caf"));
    }

    #[test]
    fn decode_real_anc_response() {
        let mut r = Reader::new();
        let f = r.push(&h("55 60 01 1e 40 06 00 01 01 01 00 02 01 00 47 ea"));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].cmd, 0x401E);
        assert_eq!(f[0].fsn, 1);
        assert_eq!(f[0].payload, h("01 01 00 02 01 00"));
        assert_eq!(r.bad_crc, 0);
    }

    #[test]
    fn split_and_concatenated_frames() {
        let a = encode(0x4007, &[0x01, 0x06, 0x55], 1);
        let b = encode(0xE003, &[0x01, 0x07, 0x00], 0);
        let mut stream = a.clone();
        stream.extend_from_slice(&b);
        // Byte by byte: nothing may come out before a frame is complete.
        let mut r = Reader::new();
        let mut out = Vec::new();
        for byte in &stream {
            out.extend(r.push(std::slice::from_ref(byte)));
        }
        assert_eq!(out.iter().map(|f| f.cmd).collect::<Vec<_>>(), vec![0x4007, 0xE003]);
        // In one block, with garbage in front.
        let mut noisy = vec![0x00, 0x13, 0x55, 0x99];
        noisy.extend_from_slice(&stream);
        let out = Reader::new().push(&noisy);
        assert_eq!(out.iter().map(|f| f.cmd).collect::<Vec<_>>(), vec![0x4007, 0xE003]);
    }

    #[test]
    fn frame_without_crc_bit() {
        // Shape of FC22 responses as built by the vendor app: control 0x0300, no CRC.
        let mut t = vec![0x55, 0x00, 0x03, 0x22, 0x7c, 0x07, 0x00, 0x00];
        t.extend_from_slice(&[0, 0, 0, 0, 0xaa, 0xbb, 0xcc]);
        let f = Reader::new().push(&t);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].cmd, 0x7C22);
        assert_eq!(f[0].payload.len(), 7);
    }

    #[test]
    fn bad_crc_is_skipped() {
        let mut t = encode(0x4007, &[0x01, 0x06, 0x55], 1);
        let n = t.len();
        t[n - 1] ^= 0xFF;
        t.extend_from_slice(&encode(0x401F, &[0x00], 1));
        let mut r = Reader::new();
        let f = r.push(&t);
        assert_eq!(f.iter().map(|f| f.cmd).collect::<Vec<_>>(), vec![0x401F]);
        assert_eq!(r.bad_crc, 1);
    }

    #[test]
    fn response_code_clears_bit_15() {
        assert_eq!(response_code(0xC01E), 0x401E);
        assert_eq!(response_code(0xF00F), 0x700F);
        assert_eq!(response_code(0xFC22), 0x7C22);
    }
}
