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

pub(super) fn parse_output_command(args: &[&str]) -> Result<Command, String> {
    let Some((target, mut args)) = args.split_first() else {
        return Err("Invalid output command (expected at least 1 argument, got 0)".into());
    };
    if args.is_empty() {
        return Err("Expected 'output <name> <subcommand>'".into());
    }

    let mut actions = Vec::new();
    while let Some((name, rest)) = args.split_first() {
        let (action, consumed) = match name.to_ascii_lowercase().as_str() {
            "enable" => (crate::OutputAction::On, 0),
            "disable" => (crate::OutputAction::Off, 0),
            "mode" | "res" | "resolution" => {
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
                (action, usize::from(custom) + consumed)
            }
            "scale" => {
                let Some(value) = rest.first() else {
                    return Err("Missing scale argument.".into());
                };
                let scale = value
                    .parse::<f64>()
                    .map_err(|_| "Invalid scale.".to_owned())?;
                if !scale.is_finite() || scale <= 0. {
                    return Err("Invalid scale.".into());
                }
                (
                    crate::OutputAction::Scale {
                        scale: crate::ScaleToSet::Specific(scale),
                    },
                    1,
                )
            }
            "transform" => {
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
                (crate::OutputAction::Transform { transform }, 1)
            }
            "position" | "pos" => {
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
                (
                    crate::OutputAction::Position {
                        position: crate::PositionToSet::Specific(position),
                    },
                    consumed,
                )
            }
            "adaptive_sync" => {
                let Some(value) = rest.first() else {
                    return Err("Missing adaptive_sync argument".into());
                };
                if value.eq_ignore_ascii_case("toggle") {
                    return Err(if *target == "*" {
                        "Cannot apply toggle to all outputs"
                    } else {
                        "adaptive_sync toggle is not implemented"
                    }
                    .into());
                }
                (
                    crate::OutputAction::Vrr {
                        vrr: crate::VrrToSet {
                            vrr: parse_boolean(value, true),
                            on_demand: false,
                        },
                    },
                    1,
                )
            }
            "render_bit_depth" => {
                let Some(value) = rest.first() else {
                    return Err("Missing bit depth argument.".into());
                };
                let max_bpc = match *value {
                    "6" => crate::MaxBpc::_6,
                    "8" => crate::MaxBpc::_8,
                    "10" => crate::MaxBpc::_10,
                    _ => return Err("Invalid bit depth. Must be a value in (6|8|10).".into()),
                };
                (crate::OutputAction::MaxBpc { max_bpc }, 1)
            }
            "modeline" => {
                let [clock, hdisplay, hsync_start, hsync_end, htotal, vdisplay, vsync_start, vsync_end, vtotal, hsync_polarity, vsync_polarity, ..] =
                    rest
                else {
                    return Err("Invalid modeline".into());
                };
                let action = crate::OutputAction::Modeline {
                    clock: clock.parse().map_err(|_| "Invalid modeline")?,
                    hdisplay: hdisplay.parse().map_err(|_| "Invalid modeline")?,
                    hsync_start: hsync_start.parse().map_err(|_| "Invalid modeline")?,
                    hsync_end: hsync_end.parse().map_err(|_| "Invalid modeline")?,
                    htotal: htotal.parse().map_err(|_| "Invalid modeline")?,
                    vdisplay: vdisplay.parse().map_err(|_| "Invalid modeline")?,
                    vsync_start: vsync_start.parse().map_err(|_| "Invalid modeline")?,
                    vsync_end: vsync_end.parse().map_err(|_| "Invalid modeline")?,
                    vtotal: vtotal.parse().map_err(|_| "Invalid modeline")?,
                    hsync_polarity: hsync_polarity
                        .to_ascii_lowercase()
                        .parse()
                        .map_err(str::to_owned)?,
                    vsync_polarity: vsync_polarity
                        .to_ascii_lowercase()
                        .parse()
                        .map_err(str::to_owned)?,
                };
                (action, 11)
            }
            "power" | "dpms" => {
                let Some(value) = rest.first() else {
                    return Err("Missing power argument".into());
                };
                let power = parse_boolean_toggle(value);
                if *target == "*" && power == Toggle::Toggle {
                    return Err("Cannot apply toggle to all outputs".into());
                }
                (crate::OutputAction::Power { power }, 1)
            }
            _ => return Err(format!("Invalid output subcommand: {name}.")),
        };
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
