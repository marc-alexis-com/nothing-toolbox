//! Minimal RFCOMM client socket on top of libc (BlueZ kernel sockets, no D-Bus).

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

const BTPROTO_RFCOMM: libc::c_int = 3;

/// `struct sockaddr_rc` from <bluetooth/rfcomm.h>.
#[repr(C)]
struct SockaddrRc {
    rc_family: libc::sa_family_t,
    /// Bluetooth address, least significant byte first.
    rc_bdaddr: [u8; 6],
    rc_channel: u8,
}

/// Parses `AA:BB:CC:DD:EE:FF` into the byte order the kernel expects (reversed).
pub fn parse_mac(mac: &str) -> Result<[u8; 6], String> {
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() != 6 {
        return Err(format!("invalid Bluetooth address: {mac:?}"));
    }
    let mut out = [0u8; 6];
    for (i, p) in parts.iter().enumerate() {
        out[5 - i] = u8::from_str_radix(p, 16).map_err(|_| format!("invalid Bluetooth address: {mac:?}"))?;
    }
    Ok(out)
}

pub struct Stream {
    fd: OwnedFd,
}

fn cvt(ret: libc::c_int) -> io::Result<libc::c_int> {
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

impl Stream {
    /// Connects to `mac` on `channel`, giving up after `timeout` (an absent headset
    /// otherwise keeps `connect` busy for the whole page timeout).
    pub fn connect(mac: &str, channel: u8, timeout: Duration) -> io::Result<Stream> {
        let bdaddr = parse_mac(mac).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        // SAFETY: plain socket(2) call; the returned descriptor is owned right away.
        let fd = unsafe {
            cvt(libc::socket(
                libc::AF_BLUETOOTH,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                BTPROTO_RFCOMM,
            ))?
        };
        // SAFETY: `fd` was just returned by socket(2) and is not owned elsewhere.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let addr = SockaddrRc {
            rc_family: libc::AF_BLUETOOTH as libc::sa_family_t,
            rc_bdaddr: bdaddr,
            rc_channel: channel,
        };
        // SAFETY: `addr` is a valid sockaddr_rc and its exact size is passed.
        let ret = unsafe {
            libc::connect(
                fd.as_raw_fd(),
                &addr as *const SockaddrRc as *const libc::sockaddr,
                std::mem::size_of::<SockaddrRc>() as libc::socklen_t,
            )
        };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::EINPROGRESS) {
                return Err(err);
            }
            let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
            let ms = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
            // SAFETY: one valid pollfd.
            let n = unsafe { cvt(libc::poll(&mut pfd, 1, ms))? };
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "RFCOMM connect timed out"));
            }
            let mut so_error: libc::c_int = 0;
            let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            // SAFETY: SO_ERROR writes one c_int into `so_error`.
            unsafe {
                cvt(libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    &mut so_error as *mut _ as *mut libc::c_void,
                    &mut len,
                ))?;
            }
            if so_error != 0 {
                return Err(io::Error::from_raw_os_error(so_error));
            }
        }
        // Back to blocking mode: a dedicated thread reads, writers block briefly.
        // SAFETY: fcntl on a descriptor we own.
        unsafe {
            let flags = cvt(libc::fcntl(fd.as_raw_fd(), libc::F_GETFL))?;
            cvt(libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK))?;
        }
        Ok(Stream { fd })
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            // SAFETY: `buf` is valid for `buf.len()` bytes.
            let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n >= 0 {
                return Ok(n as usize);
            }
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::Interrupted {
                return Err(err);
            }
        }
    }

    pub fn write_all(&self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            // SAFETY: `buf` is valid for `buf.len()` bytes.
            let n = unsafe { libc::write(self.fd.as_raw_fd(), buf.as_ptr().cast(), buf.len()) };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            buf = &buf[n as usize..];
        }
        Ok(())
    }

    /// Wakes up a thread blocked in `read` (it then returns 0 or an error).
    pub fn shutdown(&self) {
        // SAFETY: shutdown(2) on a descriptor we own; errors are irrelevant here.
        unsafe {
            libc::shutdown(self.fd.as_raw_fd(), libc::SHUT_RDWR);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_is_reversed_for_the_kernel() {
        assert_eq!(parse_mac("12:34:56:78:9A:BC").unwrap(), [0xBC, 0x9A, 0x78, 0x56, 0x34, 0x12]);
        assert!(parse_mac("12:34:56:78:9A").is_err());
        assert!(parse_mac("zz:34:56:78:9A:BC").is_err());
    }

    #[test]
    fn sockaddr_layout_matches_bluez() {
        // sa_family_t (2) + bdaddr (6) + channel (1), padded to 10 like the C struct.
        assert_eq!(std::mem::size_of::<SockaddrRc>(), 10);
    }
}
