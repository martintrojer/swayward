use super::*;

pub(super) fn parse_border(args: &[&str]) -> Result<Border, String> {
    const SYNTAX: &str = "Expected 'border <none|normal|pixel|csd|toggle>' or 'border pixel <px>'";
    let Some(style) = args.first() else {
        return Err(SYNTAX.into());
    };
    let (style, mut width) = match style.to_ascii_lowercase().as_str() {
        "normal" => (BorderStyle::Normal, None),
        "none" => (BorderStyle::None, None),
        "pixel" => (BorderStyle::Pixel, None),
        "csd" => (BorderStyle::Csd, None),
        "toggle" => (BorderStyle::Toggle, None),
        _ => return Err(SYNTAX.into()),
    };
    match args {
        [_] => {}
        [_, value] if !matches!(style, BorderStyle::None | BorderStyle::Csd) => {
            width = Some(value.parse().map_err(|_| SYNTAX.to_owned())?);
        }
        _ => return Err(SYNTAX.into()),
    }
    Ok(Border { style, width })
}

pub(super) const FOCUS_USAGE: &str = "Expected 'focus <direction|next|prev|parent|child|mode_toggle|floating|tiling>' or 'focus output <direction|name>'";

pub(super) fn parse_focus(args: &[&str]) -> Result<Command, String> {
    if args.is_empty() {
        return Ok(Command::Focus);
    }
    if args
        .first()
        .is_some_and(|arg| arg.eq_ignore_ascii_case("output"))
    {
        return match args.get(1..).unwrap_or_default() {
            [] => Err("Expected 'focus output <direction|name>'.".into()),
            output => Ok(Command::FocusOutput(join_words(output))),
        };
    }
    if let [direction, sibling] = args {
        if sibling.eq_ignore_ascii_case("sibling") {
            return match direction.to_ascii_lowercase().as_str() {
                "next" => Ok(Command::FocusNextSibling),
                "prev" => Ok(Command::FocusPrevSibling),
                _ => Err("Expected 'focus next|prev [sibling]'".into()),
            };
        }
    }
    let arg = one(
        args,
        "focus <left|right|up|down|parent|child|next|prev|floating|tiling|mode_toggle>",
    )?;
    if let Some(direction) = parse_direction(arg) {
        return Ok(Command::FocusDirection(direction));
    }
    match arg.to_ascii_lowercase().as_str() {
        "parent" => Ok(Command::FocusParent),
        "child" => Ok(Command::FocusChild),
        "next" => Ok(Command::FocusNext),
        "prev" => Ok(Command::FocusPrev),
        "floating" => Ok(Command::FocusFloating),
        "tiling" => Ok(Command::FocusTiling),
        "mode_toggle" => Ok(Command::FocusModeToggle),
        "workspace" => Ok(Command::FocusWorkspace),
        _ => Err(FOCUS_USAGE.into()),
    }
}

pub(super) const MOVE_USAGE: &str = "Expected 'move left|right|up|down [<amount> [px]]' or 'move [--no-auto-back-and-forth] [window|container] [to] workspace  <name>|next|prev|next_on_output|prev_on_output|current|(number <num>)' or 'move [window|container] [to] output <name/id>|left|right|up|down' or 'move [window|container] [to] mark <mark>' or 'move [window|container] [to] scratchpad' or 'move workspace to [output] <name/id>|left|right|up|down' or 'move [window|container] [to] [absolute] position <x> [px] <y> [px]' or 'move [window|container] [to] [absolute] position center' or 'move [window|container] [to] position mouse|cursor|pointer'";

pub(super) fn parse_move(args: &[&str]) -> Result<Command, String> {
    let (no_auto_back_and_forth, args) = match args {
        [flag, rest @ ..] if flag.eq_ignore_ascii_case("--no-auto-back-and-forth") => (true, rest),
        args => (false, args),
    };
    let args = match args {
        [kind, rest @ ..]
            if kind.eq_ignore_ascii_case("window") || kind.eq_ignore_ascii_case("container") =>
        {
            rest
        }
        args => args,
    };
    let args = match args {
        [to, rest @ ..] if to.eq_ignore_ascii_case("to") => rest,
        args => args,
    };
    if no_auto_back_and_forth
        && !matches!(args.first(), Some(value) if value.eq_ignore_ascii_case("workspace"))
    {
        return Err("Expected 'move [--no-auto-back-and-forth] [window|container] [to] workspace <name|number>'".into());
    }
    if let [workspace, rest @ ..] = args {
        if workspace.eq_ignore_ascii_case("workspace")
            && matches!(rest.first(), Some(value) if value.eq_ignore_ascii_case("to") || value.eq_ignore_ascii_case("output"))
        {
            let target = rest
                .iter()
                .skip_while(|value| {
                    value.eq_ignore_ascii_case("to") || value.eq_ignore_ascii_case("output")
                })
                .copied()
                .collect::<Vec<_>>();
            return parse_output(&target).map(Command::MoveWorkspaceToOutput);
        }
        if workspace.eq_ignore_ascii_case("output") {
            return parse_output(rest).map(Command::MoveToOutput);
        }
        if workspace.eq_ignore_ascii_case("mark") {
            return one(rest, "move [window|container] [to] mark <mark>")
                .map(|mark| Command::MoveToMark(join_words(&[mark])));
        }
    }
    if matches!(args, [scratchpad] if scratchpad.eq_ignore_ascii_case("scratchpad"))
        || matches!(args, [to, scratchpad]
            if to.eq_ignore_ascii_case("to") && scratchpad.eq_ignore_ascii_case("scratchpad"))
    {
        return Ok(Command::MoveScratchpad);
    }
    if let Some(direction) = args.first().and_then(|arg| parse_direction(arg)) {
        let pixels = args
            .get(1)
            .map(|amount| parse_move_distance(amount))
            .transpose()?;
        return Ok(Command::MoveDirection { direction, pixels });
    }
    if args.first().is_some_and(|arg| {
        arg.eq_ignore_ascii_case("position") || arg.eq_ignore_ascii_case("absolute")
    }) {
        return parse_move_position(args).map(Command::MovePosition);
    }
    let target = match args {
        [workspace, rest @ ..] if workspace.eq_ignore_ascii_case("workspace") => {
            parse_workspace(rest)?
        }
        [to, workspace, rest @ ..]
            if to.eq_ignore_ascii_case("to") && workspace.eq_ignore_ascii_case("workspace") =>
        {
            parse_workspace(rest)?
        }
        _ => return Err(MOVE_USAGE.into()),
    };
    Ok(Command::MoveToWorkspace {
        target,
        auto_back_and_forth: !no_auto_back_and_forth,
    })
}

pub(super) fn parse_move_position(args: &[&str]) -> Result<MovePosition, String> {
    let (absolute, args) = match args {
        [absolute, rest @ ..] if absolute.eq_ignore_ascii_case("absolute") => (true, rest),
        args => (false, args),
    };
    let [position, args @ ..] = args else {
        return Err(move_position_usage());
    };
    if !position.eq_ignore_ascii_case("position") {
        return Err(move_position_usage());
    }
    if matches!(args, [value] if value.eq_ignore_ascii_case("center")) {
        return Ok(MovePosition::Center { absolute });
    }
    if matches!(args, [value] if value.eq_ignore_ascii_case("cursor") || value.eq_ignore_ascii_case("mouse") || value.eq_ignore_ascii_case("pointer"))
    {
        return (!absolute)
            .then_some(MovePosition::Pointer)
            .ok_or_else(move_position_usage);
    }
    if args.len() < 2 {
        return Err(move_position_usage());
    }
    let (x, consumed) = parse_resize_amount(args).map_err(|_| "Invalid x position specified")?;
    let args = args.get(consumed..).unwrap_or_default();
    if args.is_empty() {
        return Err(move_position_usage());
    }
    let (y, consumed) = parse_resize_amount(args).map_err(|_| "Invalid y position specified")?;
    if consumed != args.len() {
        return Err(move_position_usage());
    }
    Ok(MovePosition::Coordinates { x, y, absolute })
}

pub(super) fn move_position_usage() -> String {
    "Expected 'move [absolute] position <x> [px] <y> [px]' or 'move [absolute] position center' or 'move position cursor|mouse|pointer'".into()
}

pub(super) fn parse_output(args: &[&str]) -> Result<OutputTarget, String> {
    let Some(value) = args.first() else {
        return Err(
            "Expected 'move [window|container|workspace] [to] output <name|direction>'".into(),
        );
    };
    Ok(parse_direction(value).map_or_else(
        || OutputTarget::Name((*value).to_owned()),
        OutputTarget::Direction,
    ))
}

pub(super) const LAYOUT_USAGE: &str = "Expected 'layout default|tabbed|stacking|splitv|splith' or 'layout toggle [split|all]' or 'layout toggle [split|tabbed|stacking|splitv|splith] [split|tabbed|stacking|splitv|splith]...'";

pub(super) fn parse_layout(args: &[&str]) -> Result<Command, String> {
    let direct = |layout: &str| match layout.to_ascii_lowercase().as_str() {
        "splith" => Some(Layout::SplitH),
        "splitv" => Some(Layout::SplitV),
        "tabbed" => Some(Layout::Tabbed),
        "stacking" => Some(Layout::Stacked),
        _ => None,
    };
    if let [layout] = args {
        if let Some(layout) = direct(layout) {
            return Ok(Command::Layout(layout));
        }
        if layout.eq_ignore_ascii_case("default") {
            return Ok(Command::LayoutDefault);
        }
    }
    let [toggle, rest @ ..] = args else {
        return Err("Invalid layout command (expected at least 1 argument, got 0)".into());
    };
    if !toggle.eq_ignore_ascii_case("toggle") {
        return Err(LAYOUT_USAGE.into());
    }
    let toggle = match rest {
        [] => LayoutToggle::Default,
        ["split"] => LayoutToggle::Split,
        ["all"] => LayoutToggle::All,
        [_] => return Err("Expected 'layout toggle [split|all]' or a list of layouts".into()),
        entries => {
            let cycle = entries
                .iter()
                .filter_map(|entry| {
                    if entry.eq_ignore_ascii_case("split") {
                        Some(LayoutToggleEntry::Split)
                    } else {
                        direct(entry).map(LayoutToggleEntry::Layout)
                    }
                })
                .collect::<Vec<_>>();
            if cycle.is_empty() {
                return Err("Expected a valid layout in the toggle list".into());
            }
            LayoutToggle::Cycle(cycle)
        }
    };
    Ok(Command::LayoutToggle(toggle))
}

pub(super) fn parse_split(args: &[&str]) -> Result<Command, String> {
    let arg = one(args, "split <h|v|none|toggle>")?;
    let layout = match arg.to_ascii_lowercase().as_str() {
        "h" | "horizontal" => Some(Layout::SplitH),
        "v" | "vertical" => Some(Layout::SplitV),
        "t" | "toggle" => Some(Layout::ToggleSplit),
        "n" | "none" => None,
        _ => return Err("Invalid split command (expected either horizontal or vertical).".into()),
    };
    Ok(Command::Split(layout))
}

pub(super) fn parse_opacity(args: &[&str]) -> Result<Command, String> {
    let value = args
        .get(if args.len() == 1 { 0 } else { 1 })
        .ok_or_else(|| {
            format!(
                "Invalid opacity command (expected at least 1 argument, got {})",
                args.len()
            )
        })?
        .parse::<f32>()
        .map_err(|_| "opacity float invalid".to_owned())?;

    match args.first().map(|arg| arg.to_ascii_lowercase()).as_deref() {
        Some("plus") => Ok(Command::OpacityRelative(value)),
        Some("minus") => Ok(Command::OpacityRelative(-value)),
        Some("set") if args.len() > 1 => Ok(Command::Opacity(value)),
        Some(operation) if args.len() > 1 => {
            Err(format!("Expected: set|plus|minus <0..1>: {operation}"))
        }
        Some(_) => Ok(Command::Opacity(value)),
        None => unreachable!(),
    }
}

pub(super) fn parse_fullscreen(args: &[&str]) -> Result<Command, String> {
    let syntax = "Expected 'fullscreen [enable|disable|toggle] [global]'";
    let mode = |value: &str| {
        if value.eq_ignore_ascii_case("toggle") {
            Toggle::Toggle
        } else if parse_boolean(value, false) {
            Toggle::Enable
        } else {
            Toggle::Disable
        }
    };
    let (mode, global) = match args {
        [] => (Toggle::Toggle, false),
        [global] if global.eq_ignore_ascii_case("global") => (Toggle::Toggle, true),
        [value] => (mode(value), false),
        [value, global] => (mode(value), global.eq_ignore_ascii_case("global")),
        _ => return Err(syntax.into()),
    };
    Ok(Command::Fullscreen { mode, global })
}

pub(super) fn parse_resize(args: &[&str]) -> Result<Command, String> {
    let [operation, rest @ ..] = args else {
        return Err(resize_usage());
    };
    if operation.eq_ignore_ascii_case("set") {
        return parse_resize_set(rest);
    }
    let [axis, rest @ ..] = rest else {
        return Err(resize_usage());
    };
    let grow = if operation.eq_ignore_ascii_case("grow") {
        true
    } else if operation.eq_ignore_ascii_case("shrink") {
        false
    } else {
        return Err(resize_usage());
    };
    let axis = if axis.eq_ignore_ascii_case("width") || axis.eq_ignore_ascii_case("horizontal") {
        ResizeAxis::Width
    } else if axis.eq_ignore_ascii_case("height") || axis.eq_ignore_ascii_case("vertical") {
        ResizeAxis::Height
    } else if axis.eq_ignore_ascii_case("up") {
        ResizeAxis::Up
    } else if axis.eq_ignore_ascii_case("down") {
        ResizeAxis::Down
    } else if axis.eq_ignore_ascii_case("left") {
        ResizeAxis::Left
    } else if axis.eq_ignore_ascii_case("right") {
        ResizeAxis::Right
    } else {
        return Err(resize_usage());
    };

    let (first, consumed) = if rest.is_empty() {
        (
            ResizeAmount {
                amount: 10,
                unit: ResizeUnit::Default,
            },
            0,
        )
    } else {
        parse_resize_amount(rest)?
    };
    let rest = rest.get(consumed..).unwrap_or_default();
    let second = if rest.is_empty() {
        None
    } else {
        let Some(rest) = rest.strip_prefix(&["or"]) else {
            return Err(resize_usage());
        };
        let (amount, consumed) = parse_resize_amount(rest)?;
        if consumed != rest.len() {
            return Err(resize_usage());
        }
        Some(amount)
    };
    Ok(Command::Resize {
        grow,
        axis,
        first,
        second,
    })
}

pub(super) fn parse_resize_set(mut args: &[&str]) -> Result<Command, String> {
    let usage = || {
        "Expected 'resize set [width] <width> [px|ppt]' or 'resize set height <height> [px|ppt]' or 'resize set [width] <width> [px|ppt] [height] <height> [px|ppt]".to_owned()
    };
    if args.is_empty() {
        return Err(usage());
    }

    let mut width = None;
    if matches!(args, ["width", next, ..] if *next != "height") {
        args = args.get(1..).unwrap_or_default();
    }
    if args.first() != Some(&"height") {
        let (amount, consumed) = parse_resize_amount(args).map_err(|_| usage())?;
        width = Some(amount);
        args = args.get(consumed..).unwrap_or_default();
    }

    let mut height = None;
    if !args.is_empty() {
        if matches!(args, ["height", _, ..]) {
            args = args.get(1..).unwrap_or_default();
        }
        let (amount, consumed) = parse_resize_amount(args).map_err(|_| usage())?;
        if consumed != args.len() {
            return Err(usage());
        }
        height = Some(amount);
    }

    Ok(Command::ResizeSet { width, height })
}

pub(super) fn parse_move_distance(value: &str) -> Result<i32, String> {
    let bytes = value.as_bytes();
    let mut split = usize::from(
        matches!(bytes.first(), Some(b'+' | b'-')) && bytes.get(1).is_some_and(u8::is_ascii_digit),
    );
    while bytes.get(split).is_some_and(u8::is_ascii_digit) {
        split += 1;
    }
    let amount = if split == 0 {
        0
    } else {
        parse_i32(&value[..split], "move distance")?
    };
    let suffix = &value[split..];
    if suffix.is_empty() || suffix.eq_ignore_ascii_case("px") {
        Ok(amount)
    } else {
        Err("Invalid distance specified".into())
    }
}

pub(super) fn parse_resize_amount(args: &[&str]) -> Result<(ResizeAmount, usize), String> {
    let value = args.first().ok_or_else(resize_usage)?;
    let split = value
        .find(|character: char| !character.is_ascii_digit() && character != '-')
        .unwrap_or(value.len());
    let amount = parse_i32(&value[..split], "resize amount")?;
    let attached_unit = &value[split..];
    let (unit, consumed) = if attached_unit.eq_ignore_ascii_case("px") {
        (ResizeUnit::Pixels, 1)
    } else if attached_unit.eq_ignore_ascii_case("ppt") {
        (ResizeUnit::PercentagePoints, 1)
    } else if !attached_unit.is_empty() {
        return Err(resize_usage());
    } else if args
        .get(1)
        .is_some_and(|unit| unit.eq_ignore_ascii_case("px"))
    {
        (ResizeUnit::Pixels, 2)
    } else if args
        .get(1)
        .is_some_and(|unit| unit.eq_ignore_ascii_case("ppt"))
    {
        (ResizeUnit::PercentagePoints, 2)
    } else {
        (ResizeUnit::Default, 1)
    };
    Ok((ResizeAmount { amount, unit }, consumed))
}

pub(super) fn resize_usage() -> String {
    "Expected 'resize grow|shrink <direction> [<amount> px|ppt [or <amount> px|ppt]]'".into()
}

pub(super) fn parse_i32(value: &str, name: &str) -> Result<i32, String> {
    value
        .parse()
        .map_err(|_| format!("Invalid {name} '{value}'"))
}
