//! Blocking client for sway's IPC socket.
//!
//! This is the client half of the protocol [`crate::wire`] frames and
//! `swayward`'s IPC server speaks. It talks to `$SWAYSOCK`, so it works
//! against sway and i3 as well as swayward.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::{env, fmt};

use crate::wire::{
    checked_encode, decode_header, decode_header_raw, WireError, HEADER_SIZE, MAX_PAYLOAD_SIZE,
};
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
    /// The socket could not be opened, read, or written.
    Io(io::Error),
    /// The server framed a reply we could not parse.
    Wire(crate::wire::WireError),
    /// The reply body was not valid UTF-8.
    InvalidUtf8(std::string::FromUtf8Error),
    /// The reply body was not valid JSON.
    Json(serde_json::Error),
    /// The frame carried an event type this crate does not model.
    UnknownEventType(u32),
    /// The reply carried a different message type than the request.
    Mismatch {
        /// Numeric type sent in the request.
        sent: u32,
        /// Numeric type received in the reply.
        got: u32,
    },
}

impl fmt::Display for SwayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Wire(err) => write!(f, "{err}"),
            Self::InvalidUtf8(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::UnknownEventType(value) => write!(f, "unknown IPC event type {value}"),
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
        self.stream
            .write_all(&checked_encode(msg_type, payload).map_err(SwayError::Wire)?)?;
        self.stream.flush()?;

        let mut header = [0u8; HEADER_SIZE];
        self.stream.read_exact(&mut header)?;
        let (got, len) = decode_header(&header).map_err(SwayError::Wire)?;
        validate_payload_length(len)?;

        let sent = msg_type as u32;
        let got = got as u32;
        if got != sent {
            return Err(SwayError::Mismatch { sent, got });
        }

        let mut body = vec![0u8; len as usize];
        self.stream.read_exact(&mut body)?;
        String::from_utf8(body).map_err(SwayError::InvalidUtf8)
    }

    /// Reads one event from a subscription.
    ///
    /// The first tuple item is the raw event ID, including sway's high event bit. The second item
    /// is the JSON payload. This method also validates the frame and enforces
    /// [`crate::wire::MAX_PAYLOAD_SIZE`].
    pub fn read_event(&mut self) -> Result<(u32, String), SwayError> {
        let mut header = [0u8; HEADER_SIZE];
        self.stream.read_exact(&mut header)?;
        let (msg_type, len) = decode_header_raw(&header).map_err(SwayError::Wire)?;
        validate_payload_length(len)?;
        let mut body = vec![0u8; len as usize];
        self.stream.read_exact(&mut body)?;
        Ok((
            msg_type,
            String::from_utf8(body).map_err(SwayError::InvalidUtf8)?,
        ))
    }

    /// Reads and classifies one event using its wire type.
    pub fn read_typed_event(&mut self) -> Result<crate::Event, SwayError> {
        let (msg_type, body) = self.read_event()?;
        let payload = serde_json::from_str(&body).map_err(SwayError::Json)?;
        match msg_type & !(1 << 31) {
            0 => Ok(crate::Event::Workspace(crate::WorkspaceEvent(payload))),
            1 => Ok(crate::Event::Output(crate::OutputEvent(payload))),
            2 => Ok(crate::Event::Mode(crate::ModeEvent(payload))),
            3 => Ok(crate::Event::Window(crate::WindowEvent(payload))),
            4 => Ok(crate::Event::BarconfigUpdate(crate::BarconfigUpdateEvent(
                payload,
            ))),
            5 => Ok(crate::Event::Binding(crate::BindingEvent(payload))),
            6 => Ok(crate::Event::Shutdown(crate::ShutdownEvent(payload))),
            7 => Ok(crate::Event::Tick(crate::TickEvent(payload))),
            20 => Ok(crate::Event::BarStateUpdate(crate::BarStateUpdateEvent(
                payload,
            ))),
            21 => Ok(crate::Event::Input(crate::InputEvent(payload))),
            _ => Err(SwayError::UnknownEventType(msg_type)),
        }
    }
}

fn validate_payload_length(length: u32) -> Result<(), SwayError> {
    if length > MAX_PAYLOAD_SIZE {
        return Err(SwayError::Wire(WireError::FrameTooLarge {
            length,
            maximum: MAX_PAYLOAD_SIZE,
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{encode_raw, MAX_PAYLOAD_SIZE};

    fn oversized_header(msg_type: u32) -> [u8; HEADER_SIZE] {
        let mut header: [u8; HEADER_SIZE] =
            encode_raw(msg_type, "")[..HEADER_SIZE].try_into().unwrap();
        header[6..10].copy_from_slice(&(MAX_PAYLOAD_SIZE + 1).to_ne_bytes());
        header
    }

    #[test]
    fn send_rejects_an_oversized_reply_before_reading_its_body() {
        let (mut server, client) = UnixStream::pair().unwrap();
        let mut sock = SwaySocket { stream: client };
        server
            .write_all(&oversized_header(MessageType::GetTree as u32))
            .unwrap();

        assert!(matches!(
            sock.send(MessageType::GetTree, ""),
            Err(SwayError::Wire(crate::wire::WireError::FrameTooLarge {
                length,
                maximum: MAX_PAYLOAD_SIZE,
            })) if length == MAX_PAYLOAD_SIZE + 1
        ));
    }

    #[test]
    fn read_event_rejects_an_oversized_frame_before_reading_its_body() {
        let (mut server, client) = UnixStream::pair().unwrap();
        let mut sock = SwaySocket { stream: client };
        server.write_all(&oversized_header(1 << 31)).unwrap();

        assert!(matches!(
            sock.read_event(),
            Err(SwayError::Wire(crate::wire::WireError::FrameTooLarge {
                length,
                maximum: MAX_PAYLOAD_SIZE,
            })) if length == MAX_PAYLOAD_SIZE + 1
        ));
    }

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

    #[test]
    fn invalid_utf8_in_replies_and_events_is_reported() {
        for event in [false, true] {
            let (mut server, client) = UnixStream::pair().unwrap();
            let mut sock = SwaySocket { stream: client };
            let msg_type = if event {
                (1u32 << 31) | 7
            } else {
                MessageType::GetVersion as u32
            };
            let mut frame = encode_raw(msg_type, "x");
            *frame.last_mut().unwrap() = 0xff;
            server.write_all(&frame).unwrap();
            server.flush().unwrap();

            let error = if event {
                sock.read_event().unwrap_err()
            } else {
                sock.send(MessageType::GetVersion, "").unwrap_err()
            };
            assert!(matches!(error, SwayError::InvalidUtf8(_)));
        }
    }

    #[test]
    fn typed_events_use_the_frame_type_instead_of_payload_shape() {
        let (mut server, client) = UnixStream::pair().unwrap();
        let mut sock = SwaySocket { stream: client };
        for event_type in [0, 1, 2, 3, 4, 5, 6, 7, 20, 21] {
            server
                .write_all(&encode_raw((1u32 << 31) | event_type, "{}"))
                .unwrap();
        }
        server.flush().unwrap();

        assert!(matches!(
            sock.read_typed_event().unwrap(),
            crate::Event::Workspace(_)
        ));
        assert!(matches!(
            sock.read_typed_event().unwrap(),
            crate::Event::Output(_)
        ));
        assert!(matches!(
            sock.read_typed_event().unwrap(),
            crate::Event::Mode(_)
        ));
        assert!(matches!(
            sock.read_typed_event().unwrap(),
            crate::Event::Window(_)
        ));
        assert!(matches!(
            sock.read_typed_event().unwrap(),
            crate::Event::BarconfigUpdate(_)
        ));
        assert!(matches!(
            sock.read_typed_event().unwrap(),
            crate::Event::Binding(_)
        ));
        assert!(matches!(
            sock.read_typed_event().unwrap(),
            crate::Event::Shutdown(_)
        ));
        assert!(matches!(
            sock.read_typed_event().unwrap(),
            crate::Event::Tick(_)
        ));
        assert!(matches!(
            sock.read_typed_event().unwrap(),
            crate::Event::BarStateUpdate(_)
        ));
        assert!(matches!(
            sock.read_typed_event().unwrap(),
            crate::Event::Input(_)
        ));
    }
}
