use super::*;

pub(super) fn parse_switch_bind_command(args: &[&str], unbind: bool) -> Result<Command, String> {
    let name = if unbind { "unbindswitch" } else { "bindswitch" };
    let minimum = if unbind { 1 } else { 2 };
    if args.len() < minimum {
        return Err(format!(
            "Invalid {name} command (expected at least {minimum} arguments, got {})",
            args.len()
        ));
    }
    let mut locked = false;
    let mut index = 0;
    while let Some(option) = args.get(index).filter(|arg| arg.starts_with("--")) {
        match *option {
            "--locked" => locked = true,
            "--no-warn" => {}
            // `--reload` is not part of switch-binding identity, so sway's
            // unbind accepts and ignores it. Adding such a binding would
            // require replaying tracked switch state during reload.
            "--reload" if unbind => {}
            "--reload" => {
                return Err(
                    "bindswitch --reload requires tracked per-device switch state during reload"
                        .into(),
                )
            }
            option => return Err(format!("unsupported {name} option {option}")),
        }
        index += 1;
    }
    let remaining = args.get(index..).unwrap_or_default();
    if remaining.len() < minimum {
        return Err(format!(
            "Invalid {name} command (expected at least {minimum} non-option arguments, got {})",
            remaining.len()
        ));
    }
    let Some((combo, command)) = remaining.split_first() else {
        return Err(format!("Invalid {name} command"));
    };
    let combo = join_words(&[combo]);
    let Some((switch, state)) = combo.split_once(':') else {
        return if unbind {
            Ok(Command::SwitchBind {
                switch: combo,
                command: None,
                locked,
            })
        } else {
            Err(format!(
                "Invalid {name} command (expected binding with the form <switch>:<state>)"
            ))
        };
    };
    if !matches!(switch, "lid" | "tablet") {
        return Err(format!(
            "Invalid {name} command (expected switch binding: unknown switch {switch})"
        ));
    }
    if !matches!(state, "on" | "off" | "toggle") {
        return Err(format!(
            "Invalid {name} command (expected switch state: unknown state {state})"
        ));
    }
    Ok(Command::SwitchBind {
        switch: combo,
        command: (!unbind).then(|| join_words(command)),
        locked,
    })
}

pub(super) fn parse_bind_command(
    args: &[&str],
    keycode: bool,
    unbind: bool,
) -> Result<Command, String> {
    let name = match (keycode, unbind) {
        (false, false) => "bindsym",
        (false, true) => "unbindsym",
        (true, false) => "bindcode",
        (true, true) => "unbindcode",
    };
    let minimum = if unbind { 1 } else { 2 };
    if args.len() < minimum {
        return Err(format!(
            "Invalid {name} command (expected at least {minimum} arguments, got {})",
            args.len()
        ));
    }

    let mut release = false;
    let mut locked = false;
    let mut inhibited = false;
    let mut no_repeat = false;
    let mut input_device = "*".to_owned();
    let mut index = 0;
    while let Some(option) = args.get(index).filter(|arg| arg.starts_with("--")) {
        match *option {
            "--release" => release = true,
            "--locked" => locked = true,
            "--inhibited" => inhibited = true,
            "--no-repeat" => no_repeat = true,
            "--no-warn" => {}
            option if option.starts_with("--input-device=") => {
                input_device = option
                    .strip_prefix("--input-device=")
                    .map(unquote)
                    .unwrap_or_default()
                    .to_owned();
            }
            // These sway forms target mouse regions or translate keysyms via
            // the live XKB keymap. The runtime command remains fail-loud until
            // those exact semantics have a backing path.
            option => return Err(format!("unsupported {name} option {option}")),
        }
        index += 1;
    }
    let remaining = args.get(index..).unwrap_or_default();
    if remaining.len() < minimum {
        return Err(format!(
            "Invalid {name} command (expected at least {minimum} non-option arguments, got {})",
            remaining.len()
        ));
    }
    let Some((key, command)) = remaining.split_first() else {
        return Err(format!("Invalid {name} command"));
    };
    Ok(Command::Bind {
        key: join_words(&[key]),
        command: (!unbind).then(|| join_words(command)),
        keycode,
        release,
        locked,
        inhibited,
        no_repeat,
        input_device,
    })
}

pub(super) fn parse_mode(args: &[&str]) -> Result<Command, String> {
    let (pango_markup, args) = match args {
        [flag, rest @ ..] if *flag == "--pango_markup" => (true, rest),
        _ => (false, args),
    };
    let Some((name, subcommand)) = args.split_first() else {
        return Err(if pango_markup {
            "Mode name is missing"
        } else {
            "Expected 'mode <name>'"
        }
        .into());
    };
    let name = join_words(&[name]);
    let subcommand = match subcommand {
        [] => None,
        [name, rest @ ..] if name.eq_ignore_ascii_case("set") => Some(Box::new(parse_set(rest)?)),
        // Sway dispatches the nested word through the same handlers as the
        // top level, with `config->current_mode` pointed at this mode for the
        // duration (`sway/sway/commands/mode.c:11-21,80-84`).
        [name, rest @ ..]
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "bindcode"
                    | "bindgesture"
                    | "bindswitch"
                    | "bindsym"
                    | "unbindcode"
                    | "unbindgesture"
                    | "unbindswitch"
                    | "unbindsym"
            ) =>
        {
            Some(Box::new(match name.to_ascii_lowercase().as_str() {
                "bindsym" => parse_bind_command(rest, false, false)?,
                "unbindsym" => parse_bind_command(rest, false, true)?,
                "bindcode" => parse_bind_command(rest, true, false)?,
                "unbindcode" => parse_bind_command(rest, true, true)?,
                "bindswitch" => parse_switch_bind_command(rest, false)?,
                "unbindswitch" => parse_switch_bind_command(rest, true)?,
                // Same refusal as the top-level form: swayward has no gesture
                // command-binding table to insert into.
                _ => return Err("gesture events have no sway command-binding model".into()),
            }))
        }
        [name, ..] => return Err(format!("Unknown/invalid command '{name}'")),
    };
    Ok(Command::Mode {
        name,
        pango_markup,
        subcommand,
    })
}

pub(super) fn parse_set(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err(format!(
            "Invalid set command (expected at least 2 arguments, got {})",
            args.len()
        ));
    }
    let [name, value @ ..] = args else {
        return Err("Invalid set command (expected at least 2 arguments, got 0)".into());
    };
    if !name.starts_with('$') {
        return Err(format!("variable '{name}' must start with $"));
    }
    Ok(Command::Set {
        name: (*name).to_owned(),
        value: join_words(value),
    })
}
