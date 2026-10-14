//! Finding the headset's Bluetooth address.

use std::process::Command;

/// Nothing's Bluetooth SIG vendor id, as it appears in BlueZ's Modalias (`v02B0`).
const NOTHING_VENDOR: &str = "v02B0";

/// `--mac`, then `$NOTHING_MAC`, then the first connected Nothing audio device.
pub fn resolve(explicit: Option<&str>) -> Result<String, String> {
    if let Some(m) = explicit {
        return Ok(m.to_uppercase());
    }
    if let Ok(m) = std::env::var("NOTHING_MAC") {
        if !m.is_empty() {
            return Ok(m.to_uppercase());
        }
    }
    discover().ok_or_else(|| "no connected Nothing headset found (pass --mac or set NOTHING_MAC)".to_string())
}

/// Asks BlueZ (through `bluetoothctl`) for a connected audio device made by Nothing.
/// Matching on the vendor id rather than on the name, which the user may have changed.
fn discover() -> Option<String> {
    let out = Command::new("bluetoothctl").args(["devices", "Connected"]).output().ok()?;
    let list = String::from_utf8_lossy(&out.stdout);
    for line in list.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("Device") {
            continue;
        }
        let Some(mac) = fields.next() else { continue };
        let Ok(info) = Command::new("bluetoothctl").args(["info", mac]).output() else {
            continue;
        };
        if is_nothing_audio(&String::from_utf8_lossy(&info.stdout)) {
            return Some(mac.to_uppercase());
        }
    }
    None
}

fn is_nothing_audio(info: &str) -> bool {
    info.contains(NOTHING_VENDOR) && info.contains("Icon: audio-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_on_vendor_and_audio_icon() {
        let headset = "Device 12:34:56:78:9A:BC (public)\n\tName: Nothing Headphone (1)\n\
                       \tIcon: audio-headset\n\tModalias: bluetooth:v02B0p1234d0001\n";
        assert!(is_nothing_audio(headset));
        let phone = "\tIcon: phone\n\tModalias: bluetooth:v02B0p0001d0001\n";
        assert!(!is_nothing_audio(phone));
        let other = "\tIcon: audio-headset\n\tModalias: bluetooth:v004Cp2014d0001\n";
        assert!(!is_nothing_audio(other));
    }
}
