//! Framing for sway's IPC protocol.
//!
//! A frame contains the six-byte [`MAGIC`], a native-endian `u32` payload length, a native-endian
//! `u32` message type, and the payload bytes. This matches sway's `ipc_header` layout
//! (`sway/include/ipc.h:47-51`). Event types set the high bit, so use [`decode_header_raw`] for an
//! event stream.

use std::error::Error;
use std::fmt;

use crate::MessageType;

/// Six-byte marker at the start of every sway IPC frame.
pub const MAGIC: &[u8; 6] = b"i3-ipc";
/// Size in bytes of the sway IPC header.
pub const HEADER_SIZE: usize = 14;
/// Largest IPC payload accepted from a peer.
pub const MAX_PAYLOAD_SIZE: u32 = 16 * 1024 * 1024;
/// Internal frame-sized marker used to wake and close a server connection.
///
/// This marker is not part of sway's IPC protocol and [`decode_header`] rejects it.
pub const CLOSE_SENTINEL: &[u8; HEADER_SIZE] = b"close-sway-ipc";

/// Error returned when an IPC header is invalid or unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    /// The frame does not start with [`MAGIC`].
    BadMagic,
    /// The numeric request type is not represented by [`MessageType`].
    UnknownMessageType(u32),
    /// The declared payload exceeds the configured allocation limit.
    FrameTooLarge {
        /// Declared payload length in bytes.
        length: u32,
        /// Maximum accepted payload length in bytes.
        maximum: u32,
    },
    /// An outgoing payload whose length does not fit the u32 length field.
    PayloadTooLong(usize),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => f.write_str("invalid IPC magic"),
            Self::UnknownMessageType(value) => write!(f, "unknown IPC message type {value}"),
            Self::FrameTooLarge { length, maximum } => {
                write!(f, "IPC payload is {length} bytes, maximum is {maximum}")
            }
            Self::PayloadTooLong(length) => write!(f, "IPC payload is too large: {length} bytes"),
        }
    }
}

impl Error for WireError {}

/// Encodes a request frame for a known [`MessageType`].
pub fn encode(msg_type: MessageType, payload: &str) -> Vec<u8> {
    encode_raw(msg_type as u32, payload)
}

/// Encodes a frame with an uninterpreted numeric message or event type.
pub fn encode_raw(msg_type: u32, payload: &str) -> Vec<u8> {
    encode_raw_bytes(msg_type, payload.as_bytes())
}

pub fn encode_raw_bytes(msg_type: u32, payload: &[u8]) -> Vec<u8> {
    encode_raw_with_length(msg_type, payload, payload.len() as u32)
}

/// Encodes an externally supplied payload, rejecting one whose length the u32
/// wire field cannot represent instead of truncating it.
pub fn checked_encode(msg_type: MessageType, payload: &str) -> Result<Vec<u8>, WireError> {
    let len = checked_payload_length(payload.len())?;
    Ok(encode_raw_with_length(
        msg_type as u32,
        payload.as_bytes(),
        len,
    ))
}

fn checked_payload_length(len: usize) -> Result<u32, WireError> {
    len.try_into().map_err(|_| WireError::PayloadTooLong(len))
}

fn encode_raw_with_length(msg_type: u32, payload: &[u8], len: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(HEADER_SIZE + payload.len());
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&len.to_ne_bytes());
    buf.extend_from_slice(&msg_type.to_ne_bytes());
    buf.extend_from_slice(payload);
    buf
}

/// Decodes a header without interpreting the type.
///
/// Events are not `MessageType`s: sway sets the high bit and numbers them
/// separately (`sway/include/ipc.h:27-38`), so a subscribed client must read
/// the raw value or it rejects every event it asked for.
pub fn decode_header_raw(buf: &[u8; HEADER_SIZE]) -> Result<(u32, u32), WireError> {
    let (magic, fields) = buf.split_at(MAGIC.len());
    if magic != MAGIC {
        return Err(WireError::BadMagic);
    }

    let (len, raw_type) = fields.split_at(size_of::<u32>());
    let len = u32::from_ne_bytes(len.try_into().map_err(|_| WireError::BadMagic)?);
    let raw_type = u32::from_ne_bytes(raw_type.try_into().map_err(|_| WireError::BadMagic)?);
    Ok((raw_type, len))
}

/// Decodes a request or reply header into its message type and payload length.
///
/// Use [`decode_header_raw`] for event frames because event IDs are not [`MessageType`] values.
pub fn decode_header(buf: &[u8; HEADER_SIZE]) -> Result<(MessageType, u32), WireError> {
    let (raw_type, len) = decode_header_raw(buf)?;
    let msg_type = MessageType::try_from(raw_type).map_err(WireError::UnknownMessageType)?;
    Ok((msg_type, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_header_raw_accepts_event_types() {
        // IPC_EVENT_WORKSPACE, sway/include/ipc.h:27. decode_header rejects
        // it because events are not MessageType values, which made every
        // subscribed client fail on the first event it received.
        let event_type = 1u32 << 31;
        let mut hdr = [0u8; HEADER_SIZE];
        hdr[..MAGIC.len()].copy_from_slice(MAGIC);
        hdr[6..10].copy_from_slice(&7u32.to_ne_bytes());
        hdr[10..14].copy_from_slice(&event_type.to_ne_bytes());

        assert_eq!(decode_header_raw(&hdr), Ok((event_type, 7)));
        assert!(decode_header(&hdr).is_err());
    }

    #[test]
    fn checked_payload_length_rejects_values_above_the_wire_limit() {
        assert_eq!(checked_payload_length(u32::MAX as usize), Ok(u32::MAX));
        assert_eq!(
            checked_payload_length(u32::MAX as usize + 1),
            Err(WireError::PayloadTooLong(u32::MAX as usize + 1))
        );
    }

    #[test]
    fn magic_is_six_bytes_no_nul() {
        assert_eq!(MAGIC.len(), 6);
        assert_eq!(MAGIC, b"i3-ipc");
    }

    #[test]
    fn header_round_trips() {
        let payload = r#"{"success":true}"#;
        let buf = encode(MessageType::RunCommand, payload);
        assert_eq!(&buf[..6], b"i3-ipc");
        assert_eq!(&buf[6..10], &(payload.len() as u32).to_ne_bytes());
        assert_eq!(
            &buf[10..14],
            &(MessageType::RunCommand as u32).to_ne_bytes()
        );
        assert_eq!(buf.len(), 14 + payload.len());
        let hdr: [u8; 14] = buf[..14].try_into().unwrap();
        let (ty, len) = decode_header(&hdr).unwrap();
        assert_eq!(ty, MessageType::RunCommand);
        assert_eq!(len as usize, payload.len());
    }

    #[test]
    fn event_header_sets_high_bit_and_keeps_byte_length() {
        let payload = r#"{"change":"reload"}"#;
        let buf = encode_raw(1 << 31, payload);
        assert_eq!(&buf[..6], MAGIC);
        assert_eq!(&buf[6..10], &(payload.len() as u32).to_ne_bytes());
        assert_eq!(&buf[10..14], &(1u32 << 31).to_ne_bytes());
    }

    #[test]
    fn waybar_close_sentinel_is_not_an_ipc_header() {
        assert_eq!(CLOSE_SENTINEL, b"close-sway-ipc");
        assert_eq!(CLOSE_SENTINEL.len(), HEADER_SIZE);
        assert_eq!(decode_header(CLOSE_SENTINEL), Err(WireError::BadMagic));
    }

    #[test]
    fn rejects_bad_magic() {
        let mut hdr = [0u8; 14];
        hdr[..6].copy_from_slice(b"XXXXXX");
        assert!(decode_header(&hdr).is_err());
    }

    #[test]
    fn unknown_message_type_is_an_error_not_a_panic() {
        let mut hdr = [0u8; 14];
        hdr[..6].copy_from_slice(b"i3-ipc");
        hdr[10..14].copy_from_slice(&9999u32.to_ne_bytes());
        assert!(decode_header(&hdr).is_err());
    }
}
