//! Diagnostic logs (debug class 0xFC00), read-only subset.
//!
//! - FC03 GET_FILE_LIST: file names separated by ';'.
//! - FC21 LOG_INFO: `[status, packet size u32, file size u32, crc32 u32, side u8]`, all LE;
//!   status 1 means a buried log is available.
//! - FC22 LOG_REQUEST: payload = packet number as u16 LE, answer = u32 LE index + data.
//! - FC09 GET_DEBUG_INFO: logging state (raw).

use crate::frame::hex;
use std::fmt;

pub fn file_list(p: &[u8]) -> Vec<String> {
    crate::decode::text(p).split(';').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogInfo {
    pub status: u8,
    pub packet_size: u32,
    pub file_size: u32,
    pub crc32: u32,
    pub side: u8,
}

impl LogInfo {
    pub fn available(&self) -> bool {
        self.status == 1 && self.file_size > 0
    }
}

fn u32_at(p: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(p.get(i..i + 4)?.try_into().ok()?))
}

pub fn log_info(p: &[u8]) -> Option<LogInfo> {
    Some(LogInfo {
        status: *p.first()?,
        packet_size: u32_at(p, 1)?,
        file_size: u32_at(p, 5)?,
        crc32: u32_at(p, 9)?,
        side: p.get(13).copied().unwrap_or(0),
    })
}

impl fmt::Display for LogInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "status {} ({}), file size {} bytes, packet size {} bytes, crc32 0x{:08x}, side {}",
            self.status,
            if self.status == 1 { "available" } else { "not available" },
            self.file_size,
            self.packet_size,
            self.crc32,
            self.side
        )
    }
}

pub fn log_request(packet: u16) -> [u8; 2] {
    packet.to_le_bytes()
}

/// FC22 answer: (packet index, data).
pub fn log_packet(p: &[u8]) -> Option<(u32, &[u8])> {
    Some((u32_at(p, 0)?, &p[4..]))
}

pub fn hexdump(data: &[u8]) -> String {
    let mut out = String::new();
    for (i, chunk) in data.chunks(16).enumerate() {
        let ascii: String =
            chunk.iter().map(|&b| if (32..127).contains(&b) { b as char } else { '.' }).collect();
        out.push_str(&format!("{:08x}  {:<48}  {}\n", i * 16, hex(chunk), ascii));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::parse_hex;

    #[test]
    fn file_list_from_b170() {
        assert_eq!(file_list(b"Ecode.bin;normal.log"), vec!["Ecode.bin", "normal.log"]);
    }

    #[test]
    fn log_info_from_b170() {
        // Answer of a B170 with nothing buried: status 2, 240-byte packets, empty file.
        let info = log_info(&parse_hex("02 f0 00 00 00 00 00 00 00 00 00 00 00 06").unwrap()).unwrap();
        assert_eq!(info, LogInfo { status: 2, packet_size: 240, file_size: 0, crc32: 0, side: 6 });
        assert!(!info.available());
    }

    #[test]
    fn packets() {
        assert_eq!(log_request(3), [3, 0]);
        let mut p = 3u32.to_le_bytes().to_vec();
        p.extend_from_slice(b"XYZ");
        assert_eq!(log_packet(&p), Some((3, &b"XYZ"[..])));
        assert!(hexdump(b"hello").starts_with("00000000  68 65 6c 6c 6f"));
    }
}
