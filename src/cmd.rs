//! Command codes and the safety policy that gates what may be sent to the headset.
//!
//! Classes: GET 0xC0xx, SET 0xF0xx, response 0x40xx/0x70xx, pushed event 0xE0xx,
//! debug 0xFCxx, OTA 0xF8xx. Names follow the B170 firmware log banners (typos fixed).

// --- GET (read, no side effect) ----------------------------------------------------------
pub const GET_WHERE_AM_I: u16 = 0xC002;
pub const GET_SERIAL: u16 = 0xC006;
pub const GET_BATTERY: u16 = 0xC007;
pub const GET_DEVICE_STATUS: u16 = 0xC00A;
pub const GET_COLOR: u16 = 0xC00C;
pub const GET_FEATURES: u16 = 0xC00D;
pub const GET_EXTRA_FEATURES: u16 = 0xC00E;
pub const GET_KEY_CONFIG: u16 = 0xC018;
pub const GET_MODEL: u16 = 0xC01C;
pub const GET_ANC: u16 = 0xC01E;
pub const GET_EQ_MODE: u16 = 0xC01F;
pub const GET_MULTIPOINT: u16 = 0xC027;
pub const GET_PAIRED_LIST: u16 = 0xC028;
pub const GET_CODEC: u16 = 0xC029;
pub const GET_LATENCY: u16 = 0xC041;
pub const GET_FIRMWARE: u16 = 0xC042;
pub const GET_CUSTOM_EQ: u16 = 0xC044;
pub const GET_ADVANCED_EQ_MODE: u16 = 0xC04C;
pub const GET_ADVANCED_EQ: u16 = 0xC04D;
pub const GET_BASS_BOOST: u16 = 0xC04E;
pub const GET_SPATIAL: u16 = 0xC04F;
/// Has the GET prefix but carries a payload: [01] starts the 0xE015 stream, [00] stops it.
pub const HEADTRACK_STREAM: u16 = 0xC058;
pub const GET_AUDIODO: u16 = 0xC05A;
pub const GET_AUDIODO_LINK: u16 = 0xC05B;

// --- SET (reversible, tested on a B170 and restored) -----------------------------------
pub const SET_KEY_CONFIG: u16 = 0xF003;
pub const SET_EXTRA_FEATURE: u16 = 0xF004;
pub const SET_ANC: u16 = 0xF00F;
pub const SET_EQ_MODE: u16 = 0xF010;
pub const SET_MULTIPOINT: u16 = 0xF01A;
pub const SET_LATENCY: u16 = 0xF040;
pub const SET_CUSTOM_EQ: u16 = 0xF041;
pub const SET_ADVANCED_EQ_MODE: u16 = 0xF04F;
pub const SET_ADVANCED_EQ: u16 = 0xF050;
pub const SET_BASS_BOOST: u16 = 0xF051;
pub const SET_SPATIAL: u16 = 0xF052;
pub const SET_AUDIODO: u16 = 0xF05C;

// --- SET with side effects beyond a setting --------------------------------------------
/// Changes the codec AND reboots the headset (~6 s).
pub const SET_CODEC: u16 = 0xF01C;

// --- Debug class, read-only subset ------------------------------------------------------
pub const DBG_FILE_LIST: u16 = 0xFC03;
pub const DBG_DEBUG_INFO: u16 = 0xFC09;
pub const DBG_LOG_INFO: u16 = 0xFC21;
pub const DBG_LOG_REQUEST: u16 = 0xFC22;

// --- Pushed events -----------------------------------------------------------------------
pub const EV_BATTERY: u16 = 0xE001;
pub const EV_DEVICE_STATUS: u16 = 0xE002;
pub const EV_ANC: u16 = 0xE003;
pub const EV_WORKING_STATUS: u16 = 0xE009;
pub const EV_CONNECTION: u16 = 0xE00E;
pub const EV_MAGIC_BUTTON: u16 = 0xE014;
pub const EV_HEADTRACK: u16 = 0xE015;
pub const EV_BT_LE_STATUS: u16 = 0xE016;
pub const EV_REWIND_FORWARD: u16 = 0xE017;
pub const EV_ESSENTIAL_SPACE: u16 = 0xE018;
pub const EV_AUDIODO: u16 = 0xE019;

/// The only model this tool has been tested against.
pub const SUPPORTED_MODEL: &str = "B170";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    /// GET 0xC0xx and the read-only debug subset: no side effect.
    Read,
    /// SET commands that were written, read back and restored on a real B170.
    Reversible,
    /// Anything that may reboot, unpair, reset, enter test mode, or that is untested.
    Dangerous,
    /// Never sent, not even with --dangerous (firmware update: brick risk).
    Forbidden,
}

/// Commands whose effect is outside the reach of `--dangerous`'s "I know what I am doing":
/// the OTA class. Flashing is deliberately not implemented.
fn is_ota(cmd: u16) -> bool {
    cmd & 0xFF00 == 0xF800
}

pub fn risk(cmd: u16) -> Risk {
    if is_ota(cmd) {
        return Risk::Forbidden;
    }
    match cmd {
        HEADTRACK_STREAM => Risk::Reversible,
        c if c & 0xFF00 == 0xC000 => Risk::Read,
        DBG_FILE_LIST | DBG_DEBUG_INFO | DBG_LOG_INFO | DBG_LOG_REQUEST => Risk::Read,
        SET_KEY_CONFIG | SET_EXTRA_FEATURE | SET_ANC | SET_EQ_MODE | SET_MULTIPOINT | SET_LATENCY
        | SET_CUSTOM_EQ | SET_ADVANCED_EQ_MODE | SET_ADVANCED_EQ | SET_BASS_BOOST | SET_SPATIAL
        | SET_AUDIODO => Risk::Reversible,
        // Responses and events are not requests: sending one makes no sense.
        c if c & 0x8000 == 0 || c & 0xFF00 == 0xE000 => Risk::Forbidden,
        // Everything else: codec+reboot (F01C), factory reset (F03D), unpair (F066),
        // connect/disconnect another device (F01B), ring (F002), test mode (FC01/FC07),
        // log level (FC08), and every untested code.
        _ => Risk::Dangerous,
    }
}

/// Checks a command against the policy. `dangerous` is the user's explicit opt-in.
pub fn check(cmd: u16, dangerous: bool) -> Result<(), String> {
    match risk(cmd) {
        Risk::Read | Risk::Reversible => Ok(()),
        Risk::Dangerous if dangerous => Ok(()),
        Risk::Dangerous => Err(format!(
            "0x{cmd:04X} is a risky command (reboot, unpair, reset, test mode or untested); \
             refusing without --dangerous"
        )),
        Risk::Forbidden if is_ota(cmd) => {
            Err(format!("0x{cmd:04X} belongs to the firmware update class; flashing is not supported"))
        }
        Risk::Forbidden => Err(format!("0x{cmd:04X} is not a request code")),
    }
}

/// Firmware banner name, when known (for display only).
pub fn name(cmd: u16) -> Option<&'static str> {
    Some(match cmd {
        0xC001 => "GET_PROTOCOL_VERSION",
        GET_WHERE_AM_I => "GET_WHERE_AM_I",
        GET_SERIAL => "GET_REMOTE_CONFIGURATION",
        GET_BATTERY => "GET_REMOTE_BATTERY_LEVEL",
        GET_DEVICE_STATUS => "GET_EARPHONE_STATUS",
        0xC00B => "GET_REMOTE_EXTRA_VERSION_CODE",
        GET_COLOR => "GET_REMOTE_COLOR_ID",
        GET_FEATURES => "GET_SUPPORT_FEATURE",
        GET_EXTRA_FEATURES => "GET_EXTRA_FEATURE_STATUS",
        0xC017 => "GET_BOX_LED_COLOR",
        GET_KEY_CONFIG => "GET_KEY_CONFIGURATION",
        GET_MODEL => "GET_DEVICE_MODEL",
        0xC01D => "GET_NOISE_REDUCTION_CONFIGURATION",
        GET_ANC => "GET_CURRENT_NOISE_REDUCTION",
        GET_EQ_MODE => "GET_EQ_MODE",
        GET_MULTIPOINT => "GET_ONE_BRING_TWO_ENABLE",
        GET_PAIRED_LIST => "GET_PAIRED_DEVICE_LIST",
        GET_CODEC => "GET_CODED_FORMAT",
        0xC03F => "GET_HOST_UTC_TIME",
        GET_LATENCY => "GET_LATENCY_MODE",
        GET_FIRMWARE => "GET_FW_VERSION",
        GET_CUSTOM_EQ => "GET_CUSTOM_EQ",
        GET_ADVANCED_EQ_MODE => "GET_ADVANCE_CUSTOM_EQ_MODE",
        GET_ADVANCED_EQ => "GET_ADVANCE_CUSTOM_EQ",
        GET_BASS_BOOST => "GET_AUDIO_DYNAMIC_BOOST_MODE",
        GET_SPATIAL => "GET_A2DP_SPATIAL_AUDIO_MODE",
        0xC055 => "GET_ASSIST_REAL_FF_FIR_LMS_MODE",
        HEADTRACK_STREAM => "GET_HEADTRACKING_QUATERNIONS",
        0xC059 => "GET_LE_AUDIO_CONNECT_MODE",
        GET_AUDIODO => "GET_AUDIODO_ENABLE",
        GET_AUDIODO_LINK => "GET_AUDIODO_CONNECTED_MODE",
        0xF001 => "SET_PROTOCOL_ACTIVATED",
        0xF002 => "SET_WHERE_AM_I",
        SET_KEY_CONFIG => "SET_KEY_CONFIGURATION",
        SET_EXTRA_FEATURE => "SET_EXTRA_FEATURE_ENABLE_DISABLE",
        0xF00A => "SYNC_UTC_TIME",
        SET_ANC => "SET_CURRENT_NOISE_REDUCTION",
        SET_EQ_MODE => "SET_EQ_MODE",
        SET_MULTIPOINT => "SET_ONE_BRING_TWO_ENABLE",
        0xF01B => "SET_DEVICE_CONNECT",
        SET_CODEC => "SET_CODED_FORMAT",
        0xF03D => "RESTORE_FACTORY_SETTING",
        SET_LATENCY => "SET_LATENCY_MODE",
        SET_CUSTOM_EQ => "SET_CUSTOM_EQ",
        SET_ADVANCED_EQ_MODE => "SET_ADVANCE_CUSTOM_EQ_MODE",
        SET_ADVANCED_EQ => "SET_ADVANCE_CUSTOM_EQ_VALUE",
        SET_BASS_BOOST => "SET_AUDIO_DYNAMIC_BOOST_MODE",
        SET_SPATIAL => "SET_A2DP_SPATIAL_AUDIO_MODE",
        0xF059 => "SET_ASSIST_REAL_FF_FIR_LMS_MODE",
        SET_AUDIODO => "SET_AUDIODO_ENABLE",
        0xF05D => "SET_AUDIODO_CONNECTED_MODE",
        0xFC01 => "ENTER_TEST_MODE",
        0xFC07 => "EXIT_TEST_MODE",
        0xFC08 => "CHANGE_LEVEL",
        DBG_FILE_LIST => "GET_FILE_LIST",
        DBG_DEBUG_INFO => "GET_DEBUG_INFO",
        DBG_LOG_INFO => "LOG_INFO",
        DBG_LOG_REQUEST => "LOG_REQUEST",
        EV_BATTERY => "BATTERY_CHANGED",
        EV_DEVICE_STATUS => "DEVICE_STATUS_CHANGED",
        EV_ANC => "NOISE_REDUCTION_LEVEL_CHANGED",
        EV_WORKING_STATUS => "DEVICE_WORKING_STATUS_CHANGE",
        EV_CONNECTION => "DEVICE_CONNECTED_STATUS_CHANGE",
        EV_MAGIC_BUTTON => "MAGIC_BUTTON_EVENT",
        EV_HEADTRACK => "HEADTRACKING_QUATERNIONS",
        EV_BT_LE_STATUS => "BT_CONNECT_STATUS",
        EV_REWIND_FORWARD => "AUDIO_REWIND_FORWARD",
        EV_ESSENTIAL_SPACE => "ESSENTIAL_SPACE_EVENT",
        EV_AUDIODO => "AUDIODO_STATUS_CHANGE",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_are_free() {
        for c in [GET_BATTERY, GET_ANC, GET_FIRMWARE, 0xC0FF, DBG_FILE_LIST, DBG_LOG_REQUEST] {
            assert_eq!(risk(c), Risk::Read, "{c:04X}");
            assert!(check(c, false).is_ok());
        }
    }

    #[test]
    fn tested_writes_are_reversible() {
        for c in [SET_ANC, SET_EQ_MODE, SET_LATENCY, SET_SPATIAL, HEADTRACK_STREAM] {
            assert_eq!(risk(c), Risk::Reversible, "{c:04X}");
        }
    }

    #[test]
    fn risky_commands_need_the_flag() {
        for c in [SET_CODEC, 0xF03D, 0xF066, 0xF01B, 0xF002, 0xFC01, 0xFC07, 0xFC08, 0xF0AB] {
            assert_eq!(risk(c), Risk::Dangerous, "{c:04X}");
            assert!(check(c, false).is_err());
            assert!(check(c, true).is_ok());
        }
    }

    #[test]
    fn ota_and_non_requests_are_always_refused() {
        for c in [0xF801, 0xF856, 0xF8A1, 0x401E, 0x700F, 0xE014] {
            assert_eq!(risk(c), Risk::Forbidden, "{c:04X}");
            assert!(check(c, true).is_err());
        }
    }
}
