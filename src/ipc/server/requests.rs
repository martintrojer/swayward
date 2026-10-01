use super::query_state::serialize_outcomes;
use super::*;

const SERIALIZATION_FAILED: &[u8] = br#"{"success":false,"error":"serialization failed"}"#;

/// Ask the event loop for a reply computed from live compositor state.
async fn ask_event_loop(ctx: &ClientCtx, kind: RequestKind) -> Option<Vec<u8>> {
    let (reply, receiver) = async_channel::bounded(1);
    ctx.commands.send(CommandRequest { kind, reply }).ok()?;
    receiver.recv().await.ok()
}

pub(super) async fn dispatch(ctx: &ClientCtx, msg_type: MessageType, payload: &[u8]) -> Vec<u8> {
    match msg_type {
        MessageType::GetVersion => serde_json::to_vec(&Version {
            human_readable: format!("swayward {}", version()),
            variant: "swayward".into(),
            major: SWAYWARD_IPC_VERSION.0,
            minor: SWAYWARD_IPC_VERSION.1,
            patch: SWAYWARD_IPC_VERSION.2,
            loaded_config_file_name: ctx.query_state.borrow().loaded_config_file_name.clone(),
        })
        .unwrap_or_else(|_| SERIALIZATION_FAILED.to_vec()),
        // Sway serialises each query from the live tree when the request
        // arrives (`sway/sway/ipc-server.c:815-823`), so compute exactly the
        // one requested reply on the event loop.
        MessageType::GetTree
        | MessageType::GetWorkspaces
        | MessageType::GetOutputs
        | MessageType::GetMarks
        | MessageType::GetInputs
        | MessageType::GetSeats
        | MessageType::GetBindingModes
        | MessageType::GetBindingState => ask_event_loop(ctx, RequestKind::Query(msg_type))
            .await
            .unwrap_or_else(|| {
                br#"{"success":false,"error":"compositor is unavailable"}"#.to_vec()
            }),
        // GET_CONFIG is not implemented, and must not be faked.
        //
        // Sway's contract is the verbatim text of the sway config file:
        // `config->current_config` is the file read byte for byte into a
        // buffer (`sway/sway/config.c:734-773`) and returned unaltered
        // (`sway/sway/ipc-server.c:908-917`). A client receiving it expects
        // sway syntax it can parse, diff or re-serve.
        //
        // swayward's config is KDL. Returning it in sway's single-field
        // envelope would be well-formed and wrong: the shape says "sway
        // config" and the bytes are not one, so a client that parses the
        // reply breaks in a way no error surfaces. A wire deviation is 100%
        // compliant or not implemented; there is no third option.
        //
        // Sway itself sets the precedent for the honest answer: IPC_SYNC
        // returns `{"success": false}` rather than inventing a reply
        // (`sway/sway/ipc-server.c:919-925`).
        MessageType::GetConfig => br#"{"success": false}"#.to_vec(),
        MessageType::RunCommand => {
            let input = match String::from_utf8(payload.to_vec()) {
                Ok(input) => input,
                Err(_) => {
                    let mut reply =
                        br#"[ { "success": false, "parse_error": true, "error": "Unknown\/invalid command '"#
                            .to_vec();
                    reply.extend_from_slice(payload);
                    reply.extend_from_slice(br#"'" } ]"#);
                    return reply;
                }
            };
            let input = split_payload_lines(&input);
            ask_event_loop(ctx, RequestKind::Command(input))
                .await
                .unwrap_or_else(|| {
                    serialize_outcomes(&[CommandOutcome {
                        success: false,
                        error: Some("command dispatcher is unavailable".into()),
                        parse_error: None,
                    }])
                    .into_bytes()
                })
        }
        MessageType::GetBarConfig if payload.is_empty() => b"[]".to_vec(),
        // Byte-identical to sway, spaces included: it writes this as a C
        // string literal rather than serialising it
        // (`sway/ipc-server.c:870`).
        MessageType::GetBarConfig => {
            br#"{ "success": false, "error": "No bar with that ID" }"#.to_vec()
        }
        MessageType::SendTick => {
            let payload = String::from_utf8_lossy(payload).into_owned();
            for stream in ctx.event_streams.borrow_mut().iter_mut() {
                let _ = stream.events.try_send(Event::Tick {
                    payload: payload.clone(),
                    first: false,
                });
            }
            // Sway writes this literal with a space, and the two other
            // success replies in this file already match it
            // (`sway/ipc-server.c`, IPC_SEND_TICK).
            br#"{"success": true}"#.to_vec()
        }
        _ => br#"{"success":false,"error":"not implemented"}"#.to_vec(),
    }
}

/// Apply sway's RUN_COMMAND line rewrite before parsing.
///
/// Sway strtoks the payload on `\n` and overwrites the terminator of every
/// token with `;`, stopping only at a final token that has no terminator
/// (`sway/sway/ipc-server.c:640-648`). strtok skips empty tokens, so exactly
/// the newlines that directly follow another byte become separators. This
/// ignores quoting, as sway does; the command splitter still treats a `;`
/// inside quotes as data.
fn split_payload_lines(input: &str) -> String {
    let mut previous = None;
    input
        .chars()
        .map(|character| {
            let rewritten = if character == '\n' && previous.is_some_and(|p| p != '\n') {
                ';'
            } else {
                character
            };
            previous = Some(character);
            rewritten
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::split_payload_lines;

    #[test]
    fn payload_lines_follow_sways_strtok_rewrite() {
        for (input, expected) in [
            ("a\nb", "a;b"),
            ("a\n", "a;"),
            ("a\n\nb", "a;\nb"),
            ("\na\nb\n", "\na;b;"),
            ("a\nb\n\n", "a;b;\n"),
            ("\"x\ny\"", "\"x;y\""),
            ("", ""),
            ("\n\n", "\n\n"),
            ("λ\nμ", "λ;μ"),
        ] {
            assert_eq!(split_payload_lines(input), expected, "{input:?}");
        }
    }
}
