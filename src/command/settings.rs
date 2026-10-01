use swayward_ipc::CommandOutcome;

use super::{failure, parse_boolean, success};
use crate::swayward::State;

#[cfg(test)]
std::thread_local! {
    static GLOBAL_SETTING_EXECUTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_global_setting_executions() {
    GLOBAL_SETTING_EXECUTIONS.set(0);
}

#[cfg(test)]
pub(crate) fn global_setting_executions() -> usize {
    GLOBAL_SETTING_EXECUTIONS.get()
}

pub(super) fn execute_global_setting(
    state: &mut State,
    option: &swayward_ipc::command::LayoutOption,
) -> CommandOutcome {
    // Sway invokes the handler once per criteria match (`sway/commands.c:288-330`).
    // Reading toggle state here preserves that repeat-per-match behavior.
    #[cfg(test)]
    GLOBAL_SETTING_EXECUTIONS.set(GLOBAL_SETTING_EXECUTIONS.get() + 1);

    use swayward_ipc::command::LayoutOption;

    if let LayoutOption::TitlebarFont { font, .. } = option {
        let description = pangocairo::pango::FontDescription::from_string(font);
        if description.family().is_none() {
            return failure("Invalid font family.");
        }
        if description.size() == 0 {
            return failure("Invalid font size.");
        }
    }

    {
        let mut config = state.swayward.config.borrow_mut();
        let layout = &mut config.layout;
        let parsed = match option {
            LayoutOption::FocusWrapping(value) => {
                use swayward_config::FocusWrapping;
                use swayward_ipc::command::FocusWrappingArg;

                layout.focus_wrapping = match value {
                    FocusWrappingArg::Force => FocusWrapping::Force,
                    FocusWrappingArg::Workspace => FocusWrapping::Workspace,
                    FocusWrappingArg::Yes => FocusWrapping::Yes,
                    FocusWrappingArg::No => FocusWrapping::No,
                    FocusWrappingArg::Toggle if layout.focus_wrapping == FocusWrapping::Yes => {
                        FocusWrapping::No
                    }
                    FocusWrappingArg::Toggle => FocusWrapping::Yes,
                };
                Ok(())
            }
            LayoutOption::ForceFocusWrapping(value) => {
                let enabled = parse_boolean(
                    value,
                    layout.focus_wrapping == swayward_config::FocusWrapping::Force,
                );
                layout.focus_wrapping = if enabled {
                    swayward_config::FocusWrapping::Force
                } else {
                    swayward_config::FocusWrapping::Yes
                };
                Ok(())
            }
            LayoutOption::HideEdgeBorders(value) => {
                value.parse().map(|value| layout.hide_edge_borders = value)
            }
            LayoutOption::HideEdgeBordersSmart(value) => value.parse().map(|value| {
                layout.hide_edge_borders = swayward_config::HideEdgeBorders::None;
                layout.smart_borders = value;
            }),
            // The parser already resolves `toggle` to off, as sway's
            // parse_boolean(value, true) does (`sway/sway/commands/smart_borders.c:16`).
            LayoutOption::SmartBorders(value) => {
                value.parse().map(|value| layout.smart_borders = value)
            }
            LayoutOption::SmartGaps(value) => {
                use swayward_config::SmartGaps;
                use swayward_ipc::command::SmartGapsArg;

                layout.smart_gaps = match value {
                    SmartGapsArg::On => SmartGaps::On,
                    SmartGapsArg::Off => SmartGaps::Off,
                    SmartGapsArg::InverseOuter => SmartGaps::InverseOuter,
                    SmartGapsArg::Toggle if layout.smart_gaps == SmartGaps::Off => SmartGaps::On,
                    SmartGapsArg::Toggle => SmartGaps::Off,
                };
                Ok(())
            }
            LayoutOption::ShowMarks(value) => {
                layout.titlebar.show_marks = parse_boolean(value, layout.titlebar.show_marks);
                Ok(())
            }
            LayoutOption::TitleAlignment(value) => {
                value.parse().map(|value| layout.titlebar.alignment = value)
            }
            LayoutOption::TilingDrag(value) => {
                config.input.tiling_drag = parse_boolean(value, config.input.tiling_drag);
                Ok(())
            }
            LayoutOption::TilingDragThreshold(value) => {
                config.input.tiling_drag_threshold = *value;
                Ok(())
            }
            LayoutOption::ForceDisplayUrgencyHint(value) => {
                config.urgent_timeout_ms = *value;
                Ok(())
            }
            LayoutOption::FocusOnWindowActivation(value) => value
                .parse()
                .map(|value| config.focus_on_window_activation = value),
            LayoutOption::FocusFollowsMouse(mode) => {
                use swayward_config::input::{FocusFollowsMouse, FocusFollowsMouseMode};
                use swayward_ipc::command::FocusFollowsMouse as Requested;

                // Sway's FOLLOWS_NO is this field's absence. The KDL
                // form carries an optional max-scroll-amount that
                // sway's command has no argument for, so a mode change
                // keeps whatever threshold is already configured.
                let mode = match mode {
                    Requested::No => None,
                    Requested::Yes => Some(FocusFollowsMouseMode::Yes),
                    Requested::Always => Some(FocusFollowsMouseMode::Always),
                };
                config.input.focus_follows_mouse = mode.map(|mode| FocusFollowsMouse {
                    mode,
                    max_scroll_amount: config
                        .input
                        .focus_follows_mouse
                        .and_then(|ffm| ffm.max_scroll_amount),
                });
                Ok(())
            }
            LayoutOption::WorkspaceAutoBackAndForth(value) => {
                config.input.workspace_auto_back_and_forth =
                    parse_boolean(value, config.input.workspace_auto_back_and_forth);
                Ok(())
            }
            LayoutOption::FloatingMinimumSize(width, height) => {
                layout.floating_minimum_size = swayward_config::layout::FloatingSize {
                    width: *width,
                    height: *height,
                };
                Ok(())
            }
            LayoutOption::FloatingMaximumSize(width, height) => {
                layout.floating_maximum_size = swayward_config::layout::FloatingSize {
                    width: *width,
                    height: *height,
                };
                Ok(())
            }
            LayoutOption::TitlebarFont { font, pango_markup } => {
                layout.titlebar.font = font.clone();
                layout.titlebar.pango_markup = *pango_markup;
                Ok(())
            }
            LayoutOption::TitlebarPadding {
                horizontal,
                vertical,
            } => {
                if f64::from((*horizontal).min(*vertical))
                    < f64::from(layout.titlebar.border_thickness)
                {
                    return failure("Invalid size specified");
                }
                layout.titlebar.horizontal_padding = f64::from(*horizontal);
                layout.titlebar.vertical_padding = f64::from(*vertical);
                Ok(())
            }
            LayoutOption::TitlebarBorderThickness(thickness) => {
                if f64::from(*thickness) > layout.titlebar.vertical_padding {
                    return failure("Invalid size specified");
                }
                layout.titlebar.border_thickness = *thickness;
                Ok(())
            }
            LayoutOption::DefaultBorder {
                floating,
                style,
                width,
            } => {
                use swayward_config::layout::{SwayBorderDefault, SwayBorderStyle};
                let style = match style.as_str() {
                    "none" => SwayBorderStyle::None,
                    "pixel" => SwayBorderStyle::Pixel,
                    _ => SwayBorderStyle::Normal,
                };
                let slot = if *floating {
                    &mut layout.default_floating_border
                } else {
                    &mut layout.default_border
                };
                *slot = SwayBorderDefault {
                    style,
                    width: width.or(slot.width),
                };
                Ok(())
            }
            LayoutOption::PopupDuringFullscreen(mode) => {
                use swayward_config::misc::PopupDuringFullscreen;
                use swayward_ipc::command::PopupDuringFullscreen as Requested;

                config.popup_during_fullscreen = match mode {
                    Requested::Smart => PopupDuringFullscreen::Smart,
                    Requested::Ignore => PopupDuringFullscreen::Ignore,
                    Requested::LeaveFullscreen => PopupDuringFullscreen::LeaveFullscreen,
                };
                Ok(())
            }
            LayoutOption::FloatingModifier { modifier, inverse } => {
                use swayward_config::input::{FloatingModifier, ModKey};

                // Sway keeps the modifier and the inverse bit as two
                // independent fields, and `none` is a value rather than a key
                // name. This is its own setting: mutating `mod_key` would move
                // every compositor binding as collateral.
                let modifier = match modifier {
                    None => Ok(ModKey::None),
                    Some(name) => name.parse(),
                };
                modifier.map(|modifier| {
                    config.input.floating_modifier = Some(FloatingModifier {
                        modifier,
                        inverse: *inverse,
                    });
                })
            }
            LayoutOption::MouseWarping(mode) => {
                use swayward_config::input::MouseWarping;
                use swayward_ipc::command::MouseWarping as Requested;

                config.input.mouse_warping = match mode {
                    Requested::No => MouseWarping::No,
                    Requested::Output => MouseWarping::Output,
                    Requested::Container => MouseWarping::Container,
                };
                Ok(())
            }
        };
        if let Err(error) = parsed {
            return failure(error.to_string());
        }
    }

    let config = state.swayward.config.clone();
    state.swayward.layout.update_config(&config.borrow());
    state.swayward.queue_redraw_all();
    success()
}
