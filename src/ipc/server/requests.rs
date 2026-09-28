use super::query_state::serialize_outcomes;
use super::*;

pub(super) async fn dispatch(ctx: &ClientCtx, msg_type: MessageType, payload: &[u8]) -> String {
    // Sway serialises each query from the live tree when the request arrives
    // (`sway/sway/ipc-server.c:815-823`). We cache, so refresh first: a
    // connection that stays open (any subscriber) would otherwise answer from
    // the snapshot taken when it connected.
    if matches!(
        msg_type,
        MessageType::GetTree
            | MessageType::GetWorkspaces
            | MessageType::GetOutputs
            | MessageType::GetMarks
            | MessageType::GetInputs
            | MessageType::GetSeats
            | MessageType::GetConfig
            | MessageType::GetBindingModes
            | MessageType::GetBindingState
            | MessageType::GetVersion
    ) {
        let (reply, receiver) = async_channel::bounded(1);
        if ctx
            .commands
            .send(CommandRequest {
                kind: RequestKind::RefreshQueryState,
                reply,
            })
            .is_ok()
        {
            let _ = receiver.recv().await;
        }
    }
    match msg_type {
        MessageType::GetVersion => serde_json::to_string(&Version {
            human_readable: format!("swayward {}", version()),
            variant: "swayward".into(),
            major: SWAYWARD_IPC_VERSION.0,
            minor: SWAYWARD_IPC_VERSION.1,
            patch: SWAYWARD_IPC_VERSION.2,
            loaded_config_file_name: ctx.query_state.borrow().loaded_config_file_name.clone(),
        })
        .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into()),
        MessageType::GetTree => ctx.query_state.borrow().tree.clone(),
        MessageType::GetWorkspaces => ctx.query_state.borrow().workspaces.clone(),
        MessageType::GetOutputs => ctx.query_state.borrow().outputs.clone(),
        MessageType::GetMarks => ctx.query_state.borrow().marks.clone(),
        MessageType::GetBindingModes => ctx.query_state.borrow().binding_modes.clone(),
        MessageType::GetBindingState => ctx.query_state.borrow().binding_state.clone(),
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
        MessageType::GetConfig => String::from(r#"{"success": false}"#),
        MessageType::GetInputs => ctx.query_state.borrow().inputs.clone(),
        MessageType::GetSeats => ctx.query_state.borrow().seats.clone(),
        MessageType::RunCommand => {
            let input = match String::from_utf8(payload.to_vec()) {
                Ok(input) => input,
                Err(_) => {
                    return serialize_outcomes(&[CommandOutcome {
                        success: false,
                        error: Some("command is not valid UTF-8".into()),
                        parse_error: Some(true),
                    }]);
                }
            };
            let (reply, receiver) = async_channel::bounded(1);
            if ctx
                .commands
                .send(CommandRequest {
                    kind: RequestKind::Command(input),
                    reply,
                })
                .is_err()
            {
                return serialize_outcomes(&[CommandOutcome {
                    success: false,
                    error: Some("command dispatcher is unavailable".into()),
                    parse_error: None,
                }]);
            }
            match receiver.recv().await {
                Ok(outcomes) => serialize_outcomes(&outcomes),
                Err(_) => serialize_outcomes(&[CommandOutcome {
                    success: false,
                    error: Some("command dispatcher stopped without replying".into()),
                    parse_error: None,
                }]),
            }
        }
        MessageType::GetBarConfig if payload.is_empty() => "[]".into(),
        // Byte-identical to sway, spaces included: it writes this as a C
        // string literal rather than serialising it
        // (`sway/ipc-server.c:870`).
        MessageType::GetBarConfig => {
            r#"{ "success": false, "error": "No bar with that ID" }"#.into()
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
            r#"{"success": true}"#.into()
        }
        _ => r#"{"success":false,"error":"not implemented"}"#.into(),
    }
}
