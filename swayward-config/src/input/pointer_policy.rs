use std::str::FromStr;

use miette::miette;

use crate::binds::Modifiers;
use crate::utils::Percent;

/// The KDL spelling of `focus-follows-mouse`.
///
/// Presence means enabled, which is sway's `yes`.
#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub struct FocusFollowsMousePart {
    #[knuffel(property, str)]
    pub max_scroll_amount: Option<Percent>,
}

/// Sway's `focus_follows_mouse` policy when it is enabled.
///
/// Sway stores three states, `FOLLOWS_NO`, `FOLLOWS_YES` and
/// `FOLLOWS_ALWAYS` (`sway/include/sway/config.h:458-462`). `FOLLOWS_NO` is
/// this type's absence, so the enum has two variants and cannot disagree with
/// the `Option` wrapping it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusFollowsMouse {
    pub mode: FocusFollowsMouseMode,
    pub max_scroll_amount: Option<Percent>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FocusFollowsMouseMode {
    /// Focus the hovered window only when the hovered window changed.
    #[default]
    Yes,
    /// Also focus it when focus moved away for another reason while the
    /// pointer stayed put (`sway/sway/input/seatop_default.c:590-598`).
    Always,
}

/// Sway's `mouse_warping` policy (`sway/include/sway/config.h:471-475`).
///
/// swayward defaults to `No` rather than sway's `Output`; see
/// `docs/KNOWN_DEVIATIONS.md`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MouseWarping {
    #[default]
    No,
    /// Warp only when the newly focused target is on an output that does not
    /// contain the pointer.
    Output,
    /// Warp to the focused container on every qualifying focus change.
    Container,
}

/// Sway's `floating_modifier` state.
#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingModifier {
    /// `ModKey::None` is sway's `floating_modifier none`: the drag is off.
    #[knuffel(argument, str)]
    pub modifier: ModKey,
    /// Swaps the move and resize buttons
    /// (`sway/sway/input/seatop_default.c:360-363`).
    #[knuffel(property, default)]
    pub inverse: bool,
}

#[derive(knuffel::Decode, Debug, PartialEq, Eq, Clone, Copy)]
pub struct WarpMouseToFocus {
    #[knuffel(property, str)]
    pub mode: Option<WarpMouseToFocusMode>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WarpMouseToFocusMode {
    CenterXy,
    CenterXyAlways,
}

impl FromStr for WarpMouseToFocusMode {
    type Err = miette::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "center-xy" => Ok(Self::CenterXy),
            "center-xy-always" => Ok(Self::CenterXyAlways),
            _ => Err(miette!(
                r#"invalid mode for warp-mouse-to-focus, can be "center-xy" or "center-xy-always" (or leave unset for separate centering)"#
            )),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ModKey {
    None,
    Ctrl,
    Shift,
    Alt,
    Super,
    IsoLevel3Shift,
    IsoLevel5Shift,
}

impl ModKey {
    pub fn is_pressed(self, modifiers: Modifiers) -> bool {
        self != Self::None && modifiers.contains(self.to_modifiers())
    }

    pub fn to_modifiers(&self) -> Modifiers {
        match self {
            ModKey::None => Modifiers::empty(),
            ModKey::Ctrl => Modifiers::CTRL,
            ModKey::Shift => Modifiers::SHIFT,
            ModKey::Alt => Modifiers::ALT,
            ModKey::Super => Modifiers::SUPER,
            ModKey::IsoLevel3Shift => Modifiers::ISO_LEVEL3_SHIFT,
            ModKey::IsoLevel5Shift => Modifiers::ISO_LEVEL5_SHIFT,
        }
    }
}

impl FromStr for ModKey {
    type Err = miette::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match &*s.to_ascii_lowercase() {
            "none" => Ok(Self::None),
            "ctrl" | "control" => Ok(Self::Ctrl),
            "shift" => Ok(Self::Shift),
            "alt" => Ok(Self::Alt),
            "super" | "win" => Ok(Self::Super),
            "iso_level3_shift" | "mod5" => Ok(Self::IsoLevel3Shift),
            "iso_level5_shift" | "mod3" => Ok(Self::IsoLevel5Shift),
            _ => Err(miette!("invalid Mod key: {s}")),
        }
    }
}
