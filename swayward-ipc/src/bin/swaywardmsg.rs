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
    -q, --quiet         suppress output
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

fn reply_status(reply: &str) -> serde_json::Result<bool> {
    let value = serde_json::from_str::<serde_json::Value>(reply)?;
    let failed = match value {
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| item.get("success").and_then(serde_json::Value::as_bool) == Some(false)),
        serde_json::Value::Object(object) => {
            object.get("success").and_then(serde_json::Value::as_bool) == Some(false)
        }
        _ => false,
    };
    Ok(failed)
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

#[derive(Debug, PartialEq)]
struct Options {
    msg_type: MessageType,
    socket: Option<String>,
    raw: bool,
    pretty: bool,
    quiet: bool,
    monitor: bool,
    payload: String,
}

#[derive(Debug, PartialEq)]
enum Invocation {
    Help,
    Version,
    Execute(Options),
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Invocation, String> {
    let mut kind = "run_command".to_owned();
    let mut socket = None;
    let mut raw = false;
    let mut pretty = false;
    let mut quiet = false;
    let mut monitor = false;
    let mut rest = Vec::new();

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Invocation::Help),
            "-v" | "--version" => return Ok(Invocation::Version),
            "-t" | "--type" => kind = args.next().ok_or("--type needs a value")?,
            "-s" | "--socket" => socket = Some(args.next().ok_or("--socket needs a value")?),
            "-r" | "--raw" => raw = true,
            "-p" | "--pretty" => pretty = true,
            "-q" | "--quiet" => quiet = true,
            "-m" | "--monitor" => monitor = true,
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option: {other}"));
            }
            other => rest.push(other.to_owned()),
        }
    }

    let msg_type = message_type(&kind).ok_or_else(|| format!("unknown message type: {kind}"))?;
    Ok(Invocation::Execute(Options {
        msg_type,
        socket,
        raw,
        pretty,
        quiet,
        monitor,
        payload: rest.join(" "),
    }))
}

fn execute_once(
    options: &Options,
) -> Result<(SwaySocket, ExitCode, bool), Box<dyn std::error::Error>> {
    let mut sock = match &options.socket {
        Some(path) => SwaySocket::connect_to(path)?,
        None => SwaySocket::connect()?,
    };
    let reply = sock.send(options.msg_type, &options.payload)?;

    // Default to pretty output on a terminal, like swaymsg, but never when the
    // caller is piping us into something.
    let pretty = options.pretty || (!options.raw && io::stdout().is_terminal());
    let failed = match reply_status(&reply) {
        Ok(failed) => failed,
        Err(error) => {
            if !options.quiet {
                eprintln!("swaywardmsg: failed to parse payload as JSON: {error}");
            }
            return Ok((sock, ExitCode::FAILURE, pretty));
        }
    };

    if options.msg_type == MessageType::RunCommand && !options.raw {
        if failed && !options.quiet {
            for error in command_failures(&reply) {
                eprintln!("{error}");
            }
        }
    } else if !options.quiet {
        println!("{}", render(&reply, pretty));
    }

    let code = if failed {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    };
    Ok((sock, code, pretty))
}

fn monitor_events(
    sock: &mut SwaySocket,
    quiet: bool,
    pretty: bool,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut out = io::stdout().lock();
    loop {
        let (_, event) = sock.read_event()?;
        if !quiet {
            writeln!(out, "{}", render(&event, pretty))?;
            out.flush()?;
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let invocation = parse_args(std::env::args().skip(1))?;
    let options = match invocation {
        Invocation::Help => {
            print!("{USAGE}");
            return Ok(ExitCode::SUCCESS);
        }
        Invocation::Version => {
            println!("swaywardmsg {}", env!("CARGO_PKG_VERSION"));
            return Ok(ExitCode::SUCCESS);
        }
        Invocation::Execute(options) => options,
    };

    let (mut sock, code, pretty) = execute_once(&options)?;
    if code != ExitCode::SUCCESS {
        return Ok(code);
    }
    if options.monitor && options.msg_type == MessageType::Subscribe {
        return monitor_events(&mut sock, options.quiet, pretty);
    }
    Ok(code)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            if !std::env::args().any(|arg| matches!(arg.as_str(), "-q" | "--quiet")) {
                eprintln!("swaywardmsg: {err}");
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_preserves_command_joining_and_flags() {
        let parsed = parse_args(
            [
                "-t",
                "get_tree",
                "--socket",
                "/socket",
                "-p",
                "workspace",
                "3",
            ]
            .map(str::to_owned),
        )
        .unwrap();
        let Invocation::Execute(options) = parsed else {
            panic!("expected executable options");
        };

        assert_eq!(options.msg_type, MessageType::GetTree);
        assert_eq!(options.socket.as_deref(), Some("/socket"));
        assert!(options.pretty);
        assert_eq!(options.payload, "workspace 3");
    }

    #[test]
    fn parse_args_preserves_early_actions_and_errors() {
        assert!(matches!(
            parse_args(["--help"].map(str::to_owned)).unwrap(),
            Invocation::Help
        ));
        assert!(matches!(
            parse_args(["--version"].map(str::to_owned)).unwrap(),
            Invocation::Version
        ));
        assert_eq!(
            parse_args(["--type"].map(str::to_owned)).unwrap_err(),
            "--type needs a value"
        );
        assert_eq!(
            parse_args(["--unknown"].map(str::to_owned)).unwrap_err(),
            "unknown option: --unknown"
        );
    }

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
    fn reply_status_distinguishes_invalid_json_and_failure() {
        assert!(!reply_status(r#"{"success":true}"#).unwrap());
        assert!(reply_status(r#"{"success":false}"#).unwrap());
        assert!(!reply_status(r#"[{"success":true}]"#).unwrap());
        assert!(reply_status(r#"[{"success":false}]"#).unwrap());
        assert!(reply_status("not json").is_err());
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
