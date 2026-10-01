//! Runs the built swaywardmsg against a one-connection fake sway IPC server.
//!
//! Each expected output was captured from pinned swaymsg
//! (sway 1.12-88869399, `sway/swaymsg/main.c`) against the same replies.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// A private socket path, never `$SWAYSOCK`. Like the compositor's test
/// sockets it lives in /var/tmp, on disk and short enough for `sun_path`.
fn socket_path() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    std::env::var_os("SWAYWARD_TEST_TMPDIR")
        .map_or_else(|| PathBuf::from("/var/tmp"), PathBuf::from)
        .join(format!(
            "swaywardmsg-test-{}.{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
}

fn frame(msg_type: u32, payload: &str) -> Vec<u8> {
    let mut frame = b"i3-ipc".to_vec();
    frame.extend(u32::try_from(payload.len()).unwrap().to_ne_bytes());
    frame.extend(msg_type.to_ne_bytes());
    frame.extend(payload.as_bytes());
    frame
}

/// How long the fake server waits for the client, and the client may run.
/// A client that never connects or never exits fails the test instead of
/// hanging it.
const DEADLINE: Duration = Duration::from_secs(10);

/// Answer one request with `reply` (or never, if `None`), then send
/// `events`, then close.
fn run(args: &[&str], reply: Option<&str>, events: &[(u32, &str)]) -> (Output, Duration) {
    let path = socket_path();
    let listener = UnixListener::bind(&path).unwrap();
    let reply = reply.map(str::to_owned);
    let events = events
        .iter()
        .map(|(kind, payload)| frame(*kind, payload))
        .collect::<Vec<_>>();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let started = Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if started.elapsed() > DEADLINE {
                        return;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        let mut header = [0u8; 14];
        stream.read_exact(&mut header).unwrap();
        let len = u32::from_ne_bytes(header[6..10].try_into().unwrap());
        let msg_type = u32::from_ne_bytes(header[10..14].try_into().unwrap());
        let mut body = vec![0u8; len as usize];
        stream.read_exact(&mut body).unwrap();
        let Some(reply) = reply else {
            thread::sleep(Duration::from_secs(5));
            return;
        };
        stream.write_all(&frame(msg_type, &reply)).unwrap();
        for event in events {
            thread::sleep(Duration::from_millis(20));
            let _ = stream.write_all(&event);
        }
        thread::sleep(Duration::from_millis(100));
    });
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_swaywardmsg"))
        .arg("-s")
        .arg(&path)
        .args(args)
        .env_remove("SWAYSOCK")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > DEADLINE {
            let _ = child.kill();
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    let elapsed = started.elapsed();
    server.join().unwrap();
    let _ = std::fs::remove_file(&path);
    (output, elapsed)
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap()
}

const VERSION: &str =
    r#"{"human_readable": "1.12-oracle", "major": 1, "loaded_config_file_name": "/c"}"#;

#[test]
fn message_types_are_case_insensitive() {
    let (output, _) = run(&["-r", "-t", "GET_VERSION"], Some(VERSION), &[]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        "{\n  \"human_readable\": \"1.12-oracle\",\n  \"major\": 1,\n  \
         \"loaded_config_file_name\": \"\\/c\"\n}\n"
    );
}

#[test]
fn subscribe_prints_one_event_and_not_the_reply() {
    let events = [
        (0x8000_0007, r#"{"first": true, "payload": ""}"#),
        (0x8000_0007, r#"{"first": false, "payload": "two"}"#),
    ];
    let (output, _) = run(
        &["-r", "-t", "subscribe", r#"["tick"]"#],
        Some(r#"{"success": true}"#),
        &events,
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "{ \"first\": true, \"payload\": \"\" }\n");

    let (output, _) = run(
        &["-r", "-m", "-t", "subscribe", r#"["tick"]"#],
        Some(r#"{"success": true}"#),
        &events,
    );
    // swaymsg -m exits 1 when the compositor closes the connection.
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout(&output),
        "{ \"first\": true, \"payload\": \"\" }\n{ \"first\": false, \"payload\": \"two\" }\n"
    );
}

#[test]
fn a_silent_compositor_times_out_after_three_seconds() {
    let (output, elapsed) = run(&["-t", "get_version"], None, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        (Duration::from_millis(2500)..Duration::from_millis(4500)).contains(&elapsed),
        "{elapsed:?}"
    );
}

#[test]
fn pretty_output_is_swaymsgs_summary() {
    let reply =
        r#"[{"success": true}, {"success": false, "error": "bad / thing"}, {"success": false}]"#;
    let (output, _) = run(&["-p", "nop"], Some(reply), &[]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stdout(&output),
        "Error: bad / thing\nAn unknown error occurred"
    );

    let workspaces = r#"[{"name": "1", "focused": true, "visible": true, "urgent": false, "output": "HEADLESS-1", "layout": "splith", "representation": "H[foot]"}, {"name": "2", "focused": false, "visible": false, "urgent": true, "output": "HEADLESS-1", "layout": "tabbed", "representation": null}]"#;
    let (output, _) = run(&["-p", "-t", "get_workspaces"], Some(workspaces), &[]);
    assert_eq!(
        stdout(&output),
        "Workspace 1 (focused)\n  Output: HEADLESS-1\n  Layout: splith\n  Representation: H[foot]\n\n\
         Workspace 2 (off-screen) (urgent)\n  Output: HEADLESS-1\n  Layout: tabbed\n  Representation: (null)\n\n"
    );

    let tree = r#"{"id": 1, "name": "root", "type": "root", "nodes": [{"id": 4, "name": "t", "type": "con", "shell": "xdg_shell", "pid": 42, "app_id": "foot", "nodes": []}], "floating_nodes": [{"id": 6, "name": "f", "type": "floating_con", "shell": "xdg_shell", "pid": 7, "app_id": null, "window_properties": {"class": "C", "instance": "I"}, "window": 4660, "nodes": []}]}"#;
    let (output, _) = run(&["-p", "-t", "get_tree"], Some(tree), &[]);
    assert_eq!(
        stdout(&output),
        "#1: root \"root\"\n  #4: con \"t\" (xdg_shell, pid: 42, app_id: \"foot\")\n  \
         #6: floating_con \"f\" (xdg_shell, pid: 7, instance: \"I\", class: \"C\", X11 window: 0x1234)\n"
    );

    let (output, _) = run(&["-p", "-t", "get_version"], Some(VERSION), &[]);
    assert_eq!(stdout(&output), "sway version 1.12-oracle\n");
}
