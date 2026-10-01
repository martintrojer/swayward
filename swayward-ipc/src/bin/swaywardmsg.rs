//! `swaywardmsg` — send a message to swayward over sway's IPC socket.
//!
//! A drop-in equivalent of `swaymsg` (`sway/swaymsg/main.c` at 1.12): the
//! same options, case-insensitive message types, the human summaries it
//! prints on a terminal or with `-p`, json-c's JSON layout otherwise, one
//! event after `-t subscribe` (all of them with `-m`), the three-second reply
//! timeout and the exit codes 0, 1 (no usable reply) and 2 (unsuccessful).
//! It also accepts `run_command` for `command`. It exists so that installing
//! swayward does not require installing sway, which would mean shipping a
//! second compositor to obtain one IPC client.

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use swayward_ipc::sway_socket::SwaySocket;
use swayward_ipc::MessageType;

#[path = "swaywardmsg/summary.rs"]
mod summary;

const REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

const USAGE: &str = "\
swaywardmsg — send a message to swayward over its sway-compatible IPC socket

USAGE:
    swaywardmsg [OPTIONS] [COMMAND]

OPTIONS:
    -t, --type <TYPE>   message type, any case (default: command)
    -s, --socket <PATH> socket path (default: $SWAYSOCK)
    -r, --raw           print JSON even on a terminal
    -p, --pretty        print swaymsg's summary even when piped
    -q, --quiet         suppress output
    -m, --monitor       with -t subscribe, print every event, not just one
    -h, --help          show this message
    -v, --version       show the version

TYPES:
    command              get_workspaces      get_outputs
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

/// Sway's message type names, matched case-insensitively as swaymsg does
/// with strcasecmp (`sway/swaymsg/main.c:528-563`). swaymsg calls
/// RUN_COMMAND `command`; `run_command` is accepted as well.
fn message_type(name: &str) -> Option<MessageType> {
    Some(match name.to_ascii_lowercase().as_str() {
        "command" | "run_command" => MessageType::RunCommand,
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
            // The later of -r and -p wins, as in swaymsg's getopt loop
            // (`sway/swaymsg/main.c:484-492`).
            "-r" | "--raw" => (raw, pretty) = (true, false),
            "-p" | "--pretty" => (raw, pretty) = (false, true),
            "-q" | "--quiet" => quiet = true,
            "-m" | "--monitor" => monitor = true,
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option: {other}"));
            }
            other => rest.push(other.to_owned()),
        }
    }

    let msg_type = message_type(&kind).ok_or_else(|| format!("unknown message type: {kind}"))?;
    if monitor && msg_type != MessageType::Subscribe {
        // `sway/swaymsg/main.c:566-571`.
        return Err("monitor can only be used with -t subscribe".into());
    }
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
    // swaymsg gives up on a reply after three seconds
    // (`sway/swaymsg/main.c:582-583`), so a wedged compositor cannot hang a
    // script.
    sock.set_read_timeout(Some(REPLY_TIMEOUT))?;
    let reply = sock.send(options.msg_type, &options.payload)?;

    // swaymsg is raw when stdout is not a terminal, -r forces raw and -p
    // forces the human summary (`sway/swaymsg/main.c:470,485-492`).
    let raw = options.raw || (!options.pretty && !io::stdout().is_terminal());
    let (value, failed) = match serde_json::from_str::<serde_json::Value>(&reply) {
        Ok(value) => {
            let failed = !summary::success(&value, true);
            (value, failed)
        }
        Err(error) => {
            if !options.quiet {
                eprintln!("swaywardmsg: failed to parse payload as json: {error}");
            }
            return Ok((sock, ExitCode::FAILURE, raw));
        }
    };

    // A successful subscribe reply is not printed; the event is
    // (`sway/swaymsg/main.c:609`).
    if !options.quiet && (options.msg_type != MessageType::Subscribe || failed) {
        let summary = (!raw)
            .then(|| summary::summary(options.msg_type, &value))
            .flatten();
        match summary {
            Some(summary) => print!("{summary}"),
            None => println!("{}", pretty_json(&reply)),
        }
    }

    let code = if failed {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    };
    Ok((sock, code, raw))
}

/// The reply in json-c's pretty layout, keeping the compositor's key order.
fn pretty_json(reply: &str) -> String {
    summary::json(reply, summary::Layout::Pretty).unwrap_or_else(|| reply.to_owned())
}

/// Print subscription events: one, or with `-m` every event until the
/// connection ends (`sway/swaymsg/main.c:613-665`). The reply timeout no
/// longer applies once subscribed.
fn print_events(
    sock: &mut SwaySocket,
    quiet: bool,
    raw: bool,
    monitor: bool,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    sock.set_read_timeout(None)?;
    let mut out = io::stdout().lock();
    loop {
        let (_, event) = sock.read_event()?;
        let value = match serde_json::from_str::<serde_json::Value>(&event) {
            Ok(value) => value,
            Err(error) => {
                if !quiet {
                    eprintln!("swaywardmsg: failed to parse payload as json: {error}");
                }
                return Ok(ExitCode::FAILURE);
            }
        };
        if !quiet {
            // Raw events are compact, pretty ones indented
            // (`sway/swaymsg/main.c:650-656`).
            let layout = if raw {
                summary::Layout::Spaced
            } else {
                summary::Layout::Pretty
            };
            let text = summary::json(&event, layout).unwrap_or_else(|| value.to_string());
            writeln!(out, "{text}")?;
            out.flush()?;
        }
        if !monitor {
            return Ok(ExitCode::SUCCESS);
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

    let (mut sock, code, raw) = execute_once(&options)?;
    if code != ExitCode::SUCCESS {
        return Ok(code);
    }
    if options.msg_type == MessageType::Subscribe {
        return print_events(&mut sock, options.quiet, raw, options.monitor);
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
    fn message_types_match_swaymsg_case_insensitively() {
        assert_eq!(message_type("GET_TREE"), Some(MessageType::GetTree));
        assert_eq!(message_type("Command"), Some(MessageType::RunCommand));
        assert_eq!(message_type("run_command"), Some(MessageType::RunCommand));
        assert_eq!(message_type("get_nothing"), None);
    }

    #[test]
    fn monitor_needs_subscribe_and_the_later_output_flag_wins() {
        assert_eq!(
            parse_args(["-m", "-t", "get_tree"].map(str::to_owned)).unwrap_err(),
            "monitor can only be used with -t subscribe"
        );
        let Invocation::Execute(options) = parse_args(["-r", "-p"].map(str::to_owned)).unwrap()
        else {
            panic!("expected executable options");
        };
        assert!(options.pretty && !options.raw);
    }
}
