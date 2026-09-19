//! `swaywardmsg` — send a message to swayward over sway's IPC socket.
//!
//! A drop-in equivalent of `swaymsg` for the message types swayward
//! implements. It exists so that installing swayward does not require
//! installing sway, which would mean shipping a second compositor to obtain
//! one IPC client.

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use swayward_ipc::sway_socket::SwaySocket;
use swayward_ipc::MessageType;

const USAGE: &str = "\
swaywardmsg — send a message to swayward over its sway-compatible IPC socket

USAGE:
    swaywardmsg [OPTIONS] [COMMAND]

OPTIONS:
    -t, --type <TYPE>   message type (default: run_command)
    -s, --socket <PATH> socket path (default: $SWAYSOCK)
    -r, --raw           print the raw JSON reply, never a summary
    -p, --pretty        pretty-print the JSON reply
    -m, --monitor       with -t subscribe, keep printing events
    -h, --help          show this message
    -v, --version       show the version

TYPES:
    run_command          get_workspaces      get_outputs
    get_tree             get_marks           get_bar_config
    get_version          get_binding_modes   get_config
    send_tick            get_binding_state   get_inputs
    get_seats            subscribe

EXAMPLES:
    swaywardmsg -t get_tree
    swaywardmsg -t get_workspaces -p
    swaywardmsg 'workspace 3'
    swaywardmsg -t subscribe -m '[\"window\"]'
";

fn message_type(name: &str) -> Option<MessageType> {
    Some(match name {
        "run_command" | "command" => MessageType::RunCommand,
        "get_workspaces" => MessageType::GetWorkspaces,
        "subscribe" => MessageType::Subscribe,
        "get_outputs" => MessageType::GetOutputs,
        "get_tree" => MessageType::GetTree,
        "get_marks" => MessageType::GetMarks,
        "get_bar_config" => MessageType::GetBarConfig,
        "get_version" => MessageType::GetVersion,
        "get_binding_modes" => MessageType::GetBindingModes,
        "get_config" => MessageType::GetConfig,
        "send_tick" => MessageType::SendTick,
        "get_binding_state" => MessageType::GetBindingState,
        "get_inputs" => MessageType::GetInputs,
        "get_seats" => MessageType::GetSeats,
        _ => return None,
    })
}

/// Reports whether a `run_command` reply says every command succeeded.
///
/// swaymsg prints nothing on success and the error text on failure, and
/// scripts depend on the exit status, so the summary is derived from the
/// reply rather than from the fact that a reply arrived at all.
fn command_failures(reply: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(reply) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter(|item| item.get("success").and_then(serde_json::Value::as_bool) == Some(false))
        .map(|item| {
            item.get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("command failed")
                .to_owned()
        })
        .collect()
}

fn render(reply: &str, pretty: bool) -> String {
    if !pretty {
        return reply.to_owned();
    }
    match serde_json::from_str::<serde_json::Value>(reply) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| reply.to_owned()),
        Err(_) => reply.to_owned(),
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut kind = "run_command".to_owned();
    let mut socket: Option<String> = None;
    let mut raw = false;
    let mut pretty = false;
    let mut monitor = false;
    let mut rest: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(ExitCode::SUCCESS);
            }
            "-v" | "--version" => {
                println!("swaywardmsg {}", env!("CARGO_PKG_VERSION"));
                return Ok(ExitCode::SUCCESS);
            }
            "-t" | "--type" => kind = args.next().ok_or("--type needs a value")?,
            "-s" | "--socket" => socket = Some(args.next().ok_or("--socket needs a value")?),
            "-r" | "--raw" => raw = true,
            "-p" | "--pretty" => pretty = true,
            "-m" | "--monitor" => monitor = true,
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option: {other}").into());
            }
            other => rest.push(other.to_owned()),
        }
    }

    let msg_type = message_type(&kind).ok_or_else(|| format!("unknown message type: {kind}"))?;
    let payload = rest.join(" ");

    let mut sock = match socket {
        Some(path) => SwaySocket::connect_to(path)?,
        None => SwaySocket::connect()?,
    };

    let reply = sock.send(msg_type, &payload)?;

    // Default to pretty output on a terminal, like swaymsg, but never when the
    // caller is piping us into something.
    let pretty = pretty || (!raw && io::stdout().is_terminal());

    if msg_type == MessageType::RunCommand && !raw {
        let failures = command_failures(&reply);
        if failures.is_empty() {
            return Ok(ExitCode::SUCCESS);
        }
        for error in &failures {
            eprintln!("{error}");
        }
        return Ok(ExitCode::FAILURE);
    }

    println!("{}", render(&reply, pretty));

    if monitor && msg_type == MessageType::Subscribe {
        let mut out = io::stdout().lock();
        loop {
            let (_, event) = sock.read_event()?;
            writeln!(out, "{}", render(&event, pretty))?;
            out.flush()?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("swaywardmsg: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_advertised_type_parses() {
        // The usage text is the contract; a name listed there must resolve.
        let listed: Vec<&str> = USAGE
            .lines()
            .skip_while(|line| !line.starts_with("TYPES:"))
            .take_while(|line| !line.starts_with("EXAMPLES:"))
            .flat_map(str::split_whitespace)
            .filter(|word| word.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
            .filter(|word| !word.is_empty())
            .collect();
        assert!(
            listed.len() >= 14,
            "expected the full type list, got {listed:?}"
        );
        for name in listed {
            assert!(
                message_type(name).is_some(),
                "usage lists unknown type {name}"
            );
        }
    }

    #[test]
    fn run_command_failures_are_reported() {
        assert!(command_failures(r#"[{"success":true}]"#).is_empty());
        assert_eq!(
            command_failures(r#"[{"success":false,"error":"no such workspace"}]"#),
            vec!["no such workspace".to_owned()]
        );
        // A failure without an error string still counts as a failure.
        assert_eq!(command_failures(r#"[{"success":false}]"#).len(), 1);
        // Replies that are not command arrays are not failures.
        assert!(command_failures(r#"{"human_readable":"1.11"}"#).is_empty());
        assert!(command_failures("not json").is_empty());
    }
}
