//! swaymsg's output: json-c's JSON layout and the human summaries.
//!
//! The JSON printer works on the reply text rather than a parsed value, so
//! object keys keep the order the compositor sent them in, as json-c keeps
//! them. Strings are re-escaped the way json-c escapes them, and numbers are
//! copied as written.

use std::fmt::Write as _;

use serde_json::Value;
use swayward_ipc::MessageType;

/// How json-c lays out a value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `JSON_C_TO_STRING_PRETTY | JSON_C_TO_STRING_SPACED`: two-space
    /// indent, one member per line.
    Pretty,
    /// `JSON_C_TO_STRING_SPACED` alone: one line, `{ "a": 1 }`.
    Spaced,
}

/// Re-print JSON text in json-c's layout, or `None` if it is not JSON.
pub fn json(text: &str, layout: Layout) -> Option<String> {
    let mut tokens = Tokens { text, at: 0 };
    let mut out = String::new();
    write_value(&mut tokens, layout, 0, &mut out)?;
    tokens.skip_space();
    (tokens.at == text.len()).then_some(out)
}

struct Tokens<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Tokens<'a> {
    fn rest(&self) -> &'a str {
        self.text.get(self.at..).unwrap_or_default()
    }

    fn skip_space(&mut self) {
        let rest = self.rest();
        self.at += rest.len() - rest.trim_start().len();
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_space();
        self.rest().chars().next()
    }

    fn eat(&mut self, wanted: char) -> bool {
        let found = self.peek() == Some(wanted);
        if found {
            self.at += wanted.len_utf8();
        }
        found
    }

    /// A string token, decoded.
    fn string(&mut self) -> Option<String> {
        self.skip_space();
        let rest = self.rest();
        let body = rest.strip_prefix('"')?;
        let mut escaped = false;
        let end = body.char_indices().find_map(|(index, c)| {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                return Some(index);
            }
            None
        })?;
        let token = rest.get(..end + 2)?;
        self.at += token.len();
        serde_json::from_str(token).ok()
    }

    /// A number or literal, copied as written.
    fn scalar(&mut self) -> Option<&'a str> {
        self.skip_space();
        let rest = self.rest();
        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.')))
            .unwrap_or(rest.len());
        let token = rest.get(..len).filter(|token| !token.is_empty())?;
        serde_json::from_str::<Value>(token).ok()?;
        self.at += len;
        Some(token)
    }
}

fn write_value(tokens: &mut Tokens, layout: Layout, depth: usize, out: &mut String) -> Option<()> {
    match tokens.peek()? {
        '{' => write_container(tokens, layout, depth, out, '{', '}', true),
        '[' => write_container(tokens, layout, depth, out, '[', ']', false),
        '"' => {
            let value = tokens.string()?;
            write_string(&value, out);
            Some(())
        }
        _ => {
            out.push_str(tokens.scalar()?);
            Some(())
        }
    }
}

fn write_container(
    tokens: &mut Tokens,
    layout: Layout,
    depth: usize,
    out: &mut String,
    open: char,
    close: char,
    object: bool,
) -> Option<()> {
    tokens.eat(open).then_some(())?;
    out.push(open);
    if tokens.eat(close) {
        if layout == Layout::Spaced {
            out.push(' ');
        }
        out.push(close);
        return Some(());
    }
    let mut first = true;
    loop {
        if !first {
            out.push(',');
        }
        first = false;
        match layout {
            Layout::Pretty => {
                out.push('\n');
                indent(depth + 1, out);
            }
            Layout::Spaced => out.push(' '),
        }
        if object {
            let key = tokens.string()?;
            tokens.eat(':').then_some(())?;
            write_string(&key, out);
            out.push_str(": ");
        }
        write_value(tokens, layout, depth + 1, out)?;
        if tokens.eat(close) {
            break;
        }
        tokens.eat(',').then_some(())?;
    }
    match layout {
        Layout::Pretty => {
            out.push('\n');
            indent(depth, out);
        }
        Layout::Spaced => out.push(' '),
    }
    out.push(close);
    Some(())
}

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// json-c's string escaping: `/` is escaped, control characters other than
/// the named ones become `\u00XX`, and other characters are written as is.
fn write_string(value: &str, out: &mut String) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Whether a reply reports success, as swaymsg's `success()` decides it
/// (`sway/swaymsg/main.c:19-52`): an object without `success` succeeded, an
/// array succeeded if every member did, and anything else is `fallback`.
pub fn success(value: &Value, fallback: bool) -> bool {
    let object_success = |value: &Value| value.get("success").is_none_or(json_c_bool);
    match value {
        Value::Array(items) if items.is_empty() => fallback,
        Value::Array(items) => items.iter().all(object_success),
        Value::Object(_) => object_success(value),
        _ => fallback,
    }
}

/// The human summary swaymsg prints without `-r`, or `None` where it prints
/// the JSON instead (`sway/swaymsg/main.c:394-440`).
pub fn summary(msg_type: MessageType, value: &Value) -> Option<String> {
    let mut out = String::new();
    let items = || value.as_array().into_iter().flatten();
    match msg_type {
        MessageType::SendTick => {}
        MessageType::GetVersion => {
            let _ = writeln!(out, "sway version {}", text(value.get("human_readable")));
        }
        MessageType::GetConfig => {
            let _ = writeln!(out, "{}", text(value.get("config")));
        }
        MessageType::GetTree => tree(value, 0, &mut out),
        MessageType::RunCommand => items().for_each(|item| command(item, &mut out)),
        MessageType::GetWorkspaces => items().for_each(|item| workspace(item, &mut out)),
        MessageType::GetInputs => items().for_each(|item| input(item, &mut out)),
        MessageType::GetOutputs => items().for_each(|item| output(item, &mut out)),
        MessageType::GetSeats => items().for_each(|item| seat(item, &mut out)),
        _ => return None,
    }
    Some(out)
}

/// `json_object_get_string`: JSON text for non-strings, `(null)` for an
/// absent or null value, as glibc's printf shows a NULL `%s`.
fn text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "(null)".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(value) => value.to_string(),
    }
}

fn field(value: &Value, key: &str) -> String {
    text(value.get(key))
}

/// `json_object_get_boolean`.
fn json_c_bool(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Null => false,
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(json_c_bool)
}

/// `json_object_get_int`.
fn int(value: &Value, key: &str) -> i64 {
    match value.get(key) {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|number| number as i64))
            .unwrap_or_default(),
        Some(Value::Bool(value)) => i64::from(*value),
        Some(Value::String(text)) => text.trim().parse().unwrap_or_default(),
        _ => 0,
    }
}

/// `json_object_get_double`.
fn double(value: &Value, key: &str) -> f64 {
    match value.get(key) {
        Some(Value::Number(number)) => number.as_f64().unwrap_or_default(),
        Some(Value::Bool(value)) => f64::from(u8::from(*value)),
        Some(Value::String(text)) => text.trim().parse().unwrap_or_default(),
        _ => 0.0,
    }
}

/// A json-c pointer that is NULL for both an absent and a null member.
fn present<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|value| !value.is_null())
}

fn command(item: &Value, out: &mut String) {
    if success(item, true) || !item.is_object() {
        return;
    }
    // swaymsg prints no newline after the unknown-error text.
    match item.get("error") {
        Some(error) => {
            let _ = writeln!(out, "Error: {}", text(Some(error)));
        }
        None => out.push_str("An unknown error occurred"),
    }
}

fn workspace(item: &Value, out: &mut String) {
    let _ = write!(
        out,
        "Workspace {}{}{}{}\n  Output: {}\n  Layout: {}\n  Representation: {}\n\n",
        field(item, "name"),
        if flag(item, "focused") {
            " (focused)"
        } else {
            ""
        },
        if flag(item, "visible") {
            ""
        } else {
            " (off-screen)"
        },
        if flag(item, "urgent") {
            " (urgent)"
        } else {
            ""
        },
        field(item, "output"),
        field(item, "layout"),
        field(item, "representation"),
    );
}

fn input(item: &Value, out: &mut String) {
    let kind = field(item, "type");
    let kind = match kind.as_str() {
        "keyboard" => "Keyboard",
        "pointer" => "Pointer",
        "touchpad" => "Touchpad",
        "tablet_pad" => "Tablet pad",
        "tablet_tool" => "Tablet tool",
        "touch" => "Touch",
        "switch" => "Switch",
        other => other,
    };
    let _ = write!(
        out,
        "Input device: {}\n  Type: {kind}\n  Identifier: {}\n  Product ID: {}\n  Vendor ID: {}\n",
        field(item, "name"),
        field(item, "identifier"),
        int(item, "product") as i32,
        int(item, "vendor") as i32,
    );
    if let Some(layout) = item.get("xkb_active_layout_name") {
        let layout = present(item, "xkb_active_layout_name")
            .map_or("(unnamed)".to_owned(), |_| text(Some(layout)));
        let _ = writeln!(out, "  Active Keyboard Layout: {layout}");
    }
    if let Some(events) = present(item, "libinput").and_then(|libinput| libinput.get("send_events"))
    {
        let _ = writeln!(out, "  Libinput Send Events: {}", text(Some(events)));
    }
    out.push('\n');
}

fn seat(item: &Value, out: &mut String) {
    let _ = write!(
        out,
        "Seat: {}\n  Capabilities: {}\n",
        field(item, "name"),
        int(item, "capabilities") as i32,
    );
    let devices = item.get("devices").and_then(Value::as_array);
    if let Some(devices) = devices.filter(|devices| !devices.is_empty()) {
        out.push_str("  Devices:\n");
        for device in devices {
            let _ = writeln!(out, "    {}", field(device, "name"));
        }
    }
    out.push('\n');
}

fn output(item: &Value, out: &mut String) {
    let name = format!(
        "Output {} '{} {} {}'",
        field(item, "name"),
        field(item, "make"),
        field(item, "model"),
        field(item, "serial"),
    );
    if flag(item, "non_desktop") {
        let _ = writeln!(out, "{name} (non-desktop)");
    } else if flag(item, "active") {
        let null = Value::Null;
        let mode = item.get("current_mode").unwrap_or(&null);
        let rect = item.get("rect").unwrap_or(&null);
        let features = item.get("features").unwrap_or(&null);
        let _ = write!(
            out,
            "{name}{}\n  Current mode: {}x{} @ {:.3} Hz\n  Power: {}\n  Position: {},{}\n  \
             Scale factor: {:.6}\n  Scale filter: {}\n  Subpixel hinting: {}\n  Transform: {}\n  \
             Workspace: {}\n",
            if flag(item, "focused") {
                " (focused)"
            } else {
                ""
            },
            int(mode, "width") as i32,
            int(mode, "height") as i32,
            f64::from(int(mode, "refresh") as i32) / 1000.0,
            if flag(item, "power") { "on" } else { "off" },
            int(rect, "x") as i32,
            int(rect, "y") as i32,
            double(item, "scale"),
            field(item, "scale_filter"),
            field(item, "subpixel_hinting"),
            field(item, "transform"),
            field(item, "current_workspace"),
        );
        match int(item, "max_render_time") as i32 {
            0 => out.push_str("  Max render time: off\n"),
            time => {
                let _ = writeln!(out, "  Max render time: {time} ms");
            }
        }
        let adaptive_sync = if flag(features, "adaptive_sync") {
            field(item, "adaptive_sync_status")
        } else {
            "unsupported".to_owned()
        };
        let _ = writeln!(out, "  Adaptive sync: {adaptive_sync}");
        let tearing = if flag(item, "allow_tearing") {
            "yes"
        } else {
            "no"
        };
        let _ = writeln!(out, "  Allow tearing: {tearing}");
        let hdr = match (flag(features, "hdr"), flag(item, "hdr")) {
            (false, _) => "unsupported",
            (true, true) => "on",
            (true, false) => "off",
        };
        let _ = writeln!(out, "  HDR: {hdr}");
    } else {
        let _ = writeln!(out, "{name} (disabled)");
    }
    let modes = item.get("modes").and_then(Value::as_array);
    if let Some(modes) = modes.filter(|modes| !modes.is_empty()) {
        out.push_str("  Available modes:\n");
        for mode in modes {
            let _ = write!(
                out,
                "    {}x{} @ {:.3} Hz",
                int(mode, "width") as i32,
                int(mode, "height") as i32,
                f64::from(int(mode, "refresh") as i32) / 1000.0,
            );
            if let Some(ratio) = present(mode, "picture_aspect_ratio") {
                let ratio = text(Some(ratio));
                if ratio != "none" {
                    let _ = write!(out, " ({ratio})");
                }
            }
            out.push('\n');
        }
    }
    out.push('\n');
}

fn tree(node: &Value, depth: usize, out: &mut String) {
    indent(depth, out);
    let _ = write!(
        out,
        "#{}: {} \"{}\"",
        int(node, "id") as i32,
        field(node, "type"),
        field(node, "name"),
    );
    if let Some(shell) = present(node, "shell") {
        let _ = write!(
            out,
            " ({}, pid: {}",
            text(Some(shell)),
            int(node, "pid") as i32
        );
        let quoted = |out: &mut String, label: &str, value: Option<&Value>| {
            if let Some(value) = value.filter(|value| !value.is_null()) {
                let _ = write!(out, ", {label}: \"{}\"", text(Some(value)));
            }
        };
        let properties = present(node, "window_properties");
        quoted(out, "app_id", node.get("app_id"));
        quoted(out, "instance", properties.and_then(|p| p.get("instance")));
        quoted(out, "class", properties.and_then(|p| p.get("class")));
        let window = int(node, "window") as i32;
        if window != 0 {
            let _ = write!(out, ", X11 window: 0x{window:X}");
        }
        quoted(
            out,
            "foreign_toplevel_id",
            node.get("foreign_toplevel_identifier"),
        );
        quoted(out, "sandbox_engine", node.get("sandbox_engine"));
        quoted(out, "sandbox_app_id", node.get("sandbox_app_id"));
        quoted(out, "sandbox_instance_id", node.get("sandbox_instance_id"));
        out.push(')');
    }
    out.push('\n');
    for key in ["nodes", "floating_nodes"] {
        for child in node
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            tree(child, depth + 1, out);
        }
    }
}
