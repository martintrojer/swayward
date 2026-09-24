//! Blocking client for sway's IPC socket.
//!
//! This is the client half of the protocol [`crate::wire`] frames and
//! `swayward`'s IPC server speaks. It talks to `$SWAYSOCK`, so it works
//! against sway and i3 as well as swayward.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::{env, fmt};

use crate::wire::{decode_header, decode_header_raw, encode, HEADER_SIZE};
use crate::MessageType;

/// Name of the environment variable holding the sway IPC socket path.
pub const SOCKET_PATH_ENV: &str = "SWAYSOCK";

/// A blocking connection to a sway-protocol IPC socket.
pub struct SwaySocket {
    stream: UnixStream,
}

/// Why a reply could not be read.
#[derive(Debug)]
pub enum SwayError {
    Io(io::Error),
    /// The server framed a reply we could not parse.
    Wire(crate::wire::WireError),
    /// The reply carried a different message type than the request.
    Mismatch {
        sent: u32,
        got: u32,
    },
}

impl fmt::Display for SwayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Wire(err) => write!(f, "{err}"),
            Self::Mismatch { sent, got } => {
                write!(f, "replied to message type {got}, expected {sent}")
            }
        }
    }
}

impl std::error::Error for SwayError {}

impl From<io::Error> for SwayError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl SwaySocket {
    /// Connects to the socket named by `$SWAYSOCK`.
    pub fn connect() -> Result<Self, SwayError> {
        // Treat an empty value like an unset one: an exported but empty
        // SWAYSOCK otherwise reaches connect() and fails with a bare
        // "Invalid argument", which says nothing useful.
        let path = env::var_os(SOCKET_PATH_ENV)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{SOCKET_PATH_ENV} is not set; is a compositor running?"),
                )
            })?;
        Self::connect_to(path)
    }

    /// Connects to a socket at an explicit path.
    pub fn connect_to(path: impl AsRef<Path>) -> Result<Self, SwayError> {
        Ok(Self {
            stream: UnixStream::connect(path)?,
        })
    }

    /// Sends one request and reads its reply payload.
    ///
    /// Returns the raw JSON text. Callers that want typed values deserialise
    /// it themselves, so this stays useful for message types whose schema this
    /// crate does not model.
    pub fn send(&mut self, msg_type: MessageType, payload: &str) -> Result<String, SwayError> {
        self.stream.write_all(&encode(msg_type, payload))?;
        self.stream.flush()?;

        let mut header = [0u8; HEADER_SIZE];
        self.stream.read_exact(&mut header)?;
        let (got, len) = decode_header(&header).map_err(SwayError::Wire)?;

        let sent = msg_type as u32;
        let got = got as u32;
        if got != sent {
            return Err(SwayError::Mismatch { sent, got });
        }

        let mut body = vec![0u8; len as usize];
        self.stream.read_exact(&mut body)?;
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    /// Reads one further reply, for a subscription that streams events.
    pub fn read_event(&mut self) -> Result<(u32, String), SwayError> {
        let mut header = [0u8; HEADER_SIZE];
        self.stream.read_exact(&mut header)?;
        let (msg_type, len) = decode_header_raw(&header).map_err(SwayError::Wire)?;
        let mut body = vec![0u8; len as usize];
        self.stream.read_exact(&mut body)?;
        Ok((msg_type, String::from_utf8_lossy(&body).into_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::encode_raw;

    /// A subscribed client must accept event frames. Sway numbers events
    /// separately with the high bit set (`sway/include/ipc.h:27-38`), so a
    /// reader that parses the type as a `MessageType` dies on the first event
    /// it asked for.
    #[test]
    fn read_event_accepts_high_bit_event_types() {
        let (mut server, client) = UnixStream::pair().unwrap();
        let mut sock = SwaySocket { stream: client };

        // IPC_EVENT_WINDOW, sway/include/ipc.h:31.
        let window_event = (1u32 << 31) | 3;
        server
            .write_all(&encode_raw(window_event, r#"{"change":"focus"}"#))
            .unwrap();
        server.flush().unwrap();

        assert_eq!(
            sock.read_event().unwrap(),
            (window_event, r#"{"change":"focus"}"#.to_owned())
        );
    }
}
