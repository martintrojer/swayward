use super::*;

pub(super) fn parse_input_command(args: &[&str]) -> Result<Command, String> {
    let [identifier, subcommand, values @ ..] = args else {
        return Err(
            "only input <identifier> xkb_switch_layout <next|prev|index> is supported".into(),
        );
    };
    if *subcommand != "xkb_switch_layout" {
        return Err(
            "only input <identifier> xkb_switch_layout <next|prev|index> is supported".into(),
        );
    }
    let [target] = values else {
        return Err(format!(
            "Invalid xkb_switch_layout command (expected 1 argument, got {})",
            values.len()
        ));
    };
    let target = match *target {
        "next" => XkbLayoutTarget::Next,
        "prev" => XkbLayoutTarget::Prev,
        value => {
            let index: i64 = value.parse().map_err(|_| "Invalid argument.")?;
            if index < 0 {
                return Err("Invalid layout index.".into());
            }
            XkbLayoutTarget::Index(index.try_into().map_err(|_| "Invalid argument.")?)
        }
    };
    Ok(Command::InputSwitchLayout {
        identifier: (*identifier).to_owned(),
        target,
    })
}

/// One output subcommand parsed from the arguments after its name, with the
/// number of arguments it consumed.
type OutputSubcommand = fn(&str, &[&str]) -> Result<(crate::OutputAction, usize), String>;

/// Sway's output subcommands (`sway/sway/commands/output.c:9-30`), matched
/// case-insensitively.
const OUTPUT_SUBCOMMANDS: &[(&[&str], OutputSubcommand)] = &[
    (&["enable"], |_, _| Ok((crate::OutputAction::On, 0))),
    (&["disable"], |_, _| Ok((crate::OutputAction::Off, 0))),
    (&["mode", "res", "resolution"], parse_output_mode),
    (&["scale"], parse_output_scale),
    (&["transform"], parse_output_transform),
    (&["position", "pos"], parse_output_position),
    (&["adaptive_sync"], parse_output_adaptive_sync),
    (&["render_bit_depth"], parse_output_bit_depth),
    (&["modeline"], parse_output_modeline),
    (&["power", "dpms"], parse_output_power),
];

pub(super) fn parse_output_command(args: &[&str]) -> Result<Command, String> {
    let Some((target, mut args)) = args.split_first() else {
        return Err("Invalid output command (expected at least 1 argument, got 0)".into());
    };
    let mut actions = Vec::new();
    while let Some((name, rest)) = args.split_first() {
        let lowercase = name.to_ascii_lowercase();
        let Some((_, parse)) = OUTPUT_SUBCOMMANDS
            .iter()
            .find(|(names, _)| names.contains(&lowercase.as_str()))
        else {
            return Err(format!("Invalid output subcommand: {name}."));
        };
        let (action, consumed) = parse(target, rest)?;
        action.validate()?;
        actions.push(action);
        args = rest
            .get(consumed..)
            .ok_or_else(|| format!("Invalid output subcommand: {name}."))?;
    }

    Ok(Command::Output {
        target: (*target).to_owned(),
        actions,
    })
}

fn parse_output_mode(_: &str, rest: &[&str]) -> Result<(crate::OutputAction, usize), String> {
    let (custom, rest) = match rest {
        [flag, rest @ ..] if *flag == "--custom" => (true, rest),
        rest => (false, rest),
    };
    let Some(value) = rest.first() else {
        return Err("Missing mode argument.".into());
    };
    let (mode, consumed) = if value.contains('x') {
        let value = ["Hz", "HZ", "hz", "hZ"]
            .into_iter()
            .find_map(|suffix| value.strip_suffix(suffix))
            .unwrap_or(value);
        (
            value
                .parse::<crate::ConfiguredMode>()
                .map_err(str::to_owned)?,
            1,
        )
    } else {
        let Some(height) = rest.get(1) else {
            return Err("Missing mode argument (height).".into());
        };
        (
            crate::ConfiguredMode {
                width: value.parse().map_err(|_| "Invalid mode width.")?,
                height: height.parse().map_err(|_| "Invalid mode height.")?,
                refresh: None,
            },
            2,
        )
    };
    let action = if custom {
        crate::OutputAction::CustomMode { mode }
    } else {
        crate::OutputAction::Mode {
            mode: crate::ModeToSet::Specific(mode),
        }
    };
    Ok((action, usize::from(custom) + consumed))
}

fn parse_output_scale(_: &str, rest: &[&str]) -> Result<(crate::OutputAction, usize), String> {
    let Some(value) = rest.first() else {
        return Err("Missing scale argument.".into());
    };
    let scale = value
        .parse::<f64>()
        .map_err(|_| "Invalid scale.".to_owned())?;
    if !scale.is_finite() || scale <= 0. {
        return Err("Invalid scale.".into());
    }
    Ok((
        crate::OutputAction::Scale {
            scale: crate::ScaleToSet::Specific(scale),
        },
        1,
    ))
}

fn parse_output_transform(_: &str, rest: &[&str]) -> Result<(crate::OutputAction, usize), String> {
    let Some(value) = rest.first() else {
        return Err("Missing transform argument.".into());
    };
    let transform = if *value == "0" {
        crate::Transform::Normal
    } else {
        value.parse().map_err(|_| "Invalid output transform.")?
    };
    let transform = match transform {
        crate::Transform::_90 => crate::Transform::_270,
        crate::Transform::_270 => crate::Transform::_90,
        crate::Transform::Flipped90 => crate::Transform::Flipped270,
        crate::Transform::Flipped270 => crate::Transform::Flipped90,
        transform => transform,
    };
    Ok((crate::OutputAction::Transform { transform }, 1))
}

fn parse_output_position(_: &str, rest: &[&str]) -> Result<(crate::OutputAction, usize), String> {
    let Some(value) = rest.first() else {
        return Err("Missing position argument.".into());
    };
    let (x, y, consumed) = if let Some((x, y)) = value.split_once(',') {
        (x, y, 1)
    } else {
        let Some(y) = rest.get(1) else {
            return Err("Missing position argument (y).".into());
        };
        (*value, *y, 2)
    };
    let position = crate::ConfiguredPosition {
        x: x.parse().map_err(|_| "Invalid position x.")?,
        y: y.parse().map_err(|_| "Invalid position y.")?,
    };
    Ok((
        crate::OutputAction::Position {
            position: crate::PositionToSet::Specific(position),
        },
        consumed,
    ))
}

fn parse_output_adaptive_sync(
    target: &str,
    rest: &[&str],
) -> Result<(crate::OutputAction, usize), String> {
    let Some(value) = rest.first() else {
        return Err("Missing adaptive_sync argument".into());
    };
    if value.eq_ignore_ascii_case("toggle") {
        return Err(if target == "*" {
            "Cannot apply toggle to all outputs"
        } else {
            "adaptive_sync toggle is not implemented"
        }
        .into());
    }
    Ok((
        crate::OutputAction::Vrr {
            vrr: crate::VrrToSet {
                vrr: parse_boolean(value, true),
                on_demand: false,
            },
        },
        1,
    ))
}

fn parse_output_bit_depth(_: &str, rest: &[&str]) -> Result<(crate::OutputAction, usize), String> {
    let Some(value) = rest.first() else {
        return Err("Missing bit depth argument.".into());
    };
    let max_bpc = match *value {
        "6" => crate::MaxBpc::_6,
        "8" => crate::MaxBpc::_8,
        "10" => crate::MaxBpc::_10,
        _ => return Err("Invalid bit depth. Must be a value in (6|8|10).".into()),
    };
    Ok((crate::OutputAction::MaxBpc { max_bpc }, 1))
}

fn parse_output_modeline(_: &str, rest: &[&str]) -> Result<(crate::OutputAction, usize), String> {
    const INVALID: &str = "Invalid modeline";
    let [clock, timings @ .., hsync_polarity, vsync_polarity] = rest.get(..11).ok_or(INVALID)?
    else {
        return Err(INVALID.into());
    };
    let timings = timings
        .iter()
        .map(|value| value.parse::<u16>().map_err(|_| INVALID))
        .collect::<Result<Vec<_>, _>>()?;
    let [hdisplay, hsync_start, hsync_end, htotal, vdisplay, vsync_start, vsync_end, vtotal] =
        timings[..]
    else {
        return Err(INVALID.into());
    };
    let action = crate::OutputAction::Modeline {
        clock: clock.parse().map_err(|_| INVALID)?,
        hdisplay,
        hsync_start,
        hsync_end,
        htotal,
        vdisplay,
        vsync_start,
        vsync_end,
        vtotal,
        hsync_polarity: parse_polarity(hsync_polarity)?,
        vsync_polarity: parse_polarity(vsync_polarity)?,
    };
    Ok((action, 11))
}

fn parse_polarity<T: std::str::FromStr<Err = &'static str>>(value: &str) -> Result<T, String> {
    value.to_ascii_lowercase().parse().map_err(str::to_owned)
}

fn parse_output_power(target: &str, rest: &[&str]) -> Result<(crate::OutputAction, usize), String> {
    let Some(value) = rest.first() else {
        return Err("Missing power argument".into());
    };
    let power = parse_boolean_toggle(value);
    if target == "*" && power == Toggle::Toggle {
        return Err("Cannot apply toggle to all outputs".into());
    }
    Ok((crate::OutputAction::Power { power }, 1))
}
