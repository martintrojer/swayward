use crate::utils::{Flag, MergeWith};

#[derive(Debug, PartialEq)]
pub struct Input {
    pub keyboard: Keyboard,
    pub touchpad: Touchpad,
    pub mouse: Mouse,
    pub trackpoint: Trackpoint,
    pub trackball: Trackball,
    pub tablet: Tablet,
    pub touch: Touch,
    pub disable_power_key_handling: bool,
    pub warp_mouse_to_focus: Option<WarpMouseToFocus>,
    pub focus_follows_mouse: Option<FocusFollowsMouse>,
    /// Sway's `mouse_warping` policy.
    ///
    /// Distinct from `warp_mouse_to_focus`, which is niri's centering
    /// control: that one says *how* to place the cursor, this one says *when*
    /// a focus change warps at all (`sway/sway/input/seat.c:1526-1547`).
    pub mouse_warping: MouseWarping,
    /// Sway's `floating_modifier`, when a command has set one.
    ///
    /// `None` means no policy was set and the compositor `mod_key` drives
    /// pointer drags, which is swayward's inherited behaviour. `Some` stores
    /// sway's two independent pieces of state: the modifier (which may be
    /// `ModKey::None`, sway's `floating_modifier none`) and the inverse bit
    /// (`sway/include/sway/config.h:509-510`).
    pub floating_modifier: Option<FloatingModifier>,
    pub workspace_auto_back_and_forth: bool,
    pub tiling_drag: bool,
    pub tiling_drag_threshold: u32,
    /// Resize windows by left-dragging their border without a modifier, as
    /// sway always does (`sway/sway/input/seatop_default.c:396-410,468-474`).
    /// Sway has no switch for this; swayward adds one because some users
    /// prefer the inherited modifier-only drag.
    pub border_resize: bool,
    /// Resize tiled windows by left-dragging the gap between them. Sway has
    /// no such handle; this serves borderless setups with `gaps` above zero.
    pub gap_resize: bool,
    pub mod_key: Option<ModKey>,
    pub mod_key_nested: Option<ModKey>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            keyboard: Default::default(),
            touchpad: Default::default(),
            mouse: Default::default(),
            trackpoint: Default::default(),
            trackball: Default::default(),
            tablet: Default::default(),
            touch: Default::default(),
            disable_power_key_handling: false,
            warp_mouse_to_focus: None,
            focus_follows_mouse: None,
            mouse_warping: Default::default(),
            floating_modifier: None,
            workspace_auto_back_and_forth: false,
            tiling_drag: true,
            tiling_drag_threshold: 9,
            border_resize: true,
            gap_resize: false,
            mod_key: None,
            mod_key_nested: None,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, PartialEq)]
pub struct InputPart {
    #[knuffel(child)]
    pub keyboard: Option<KeyboardPart>,
    #[knuffel(child)]
    pub touchpad: Option<Touchpad>,
    #[knuffel(child)]
    pub mouse: Option<Mouse>,
    #[knuffel(child)]
    pub trackpoint: Option<Trackpoint>,
    #[knuffel(child)]
    pub trackball: Option<Trackball>,
    #[knuffel(child)]
    pub tablet: Option<Tablet>,
    #[knuffel(child)]
    pub touch: Option<Touch>,
    #[knuffel(child)]
    pub disable_power_key_handling: Option<Flag>,
    #[knuffel(child)]
    pub warp_mouse_to_focus: Option<WarpMouseToFocus>,
    #[knuffel(child)]
    pub focus_follows_mouse: Option<FocusFollowsMousePart>,
    #[knuffel(child)]
    pub floating_modifier: Option<FloatingModifier>,
    #[knuffel(child)]
    pub workspace_auto_back_and_forth: Option<Flag>,
    #[knuffel(child)]
    pub tiling_drag: Option<Flag>,
    #[knuffel(child, unwrap(argument))]
    pub tiling_drag_threshold: Option<u32>,
    #[knuffel(child)]
    pub border_resize: Option<Flag>,
    #[knuffel(child)]
    pub gap_resize: Option<Flag>,
    #[knuffel(child, unwrap(argument, str))]
    pub mod_key: Option<ModKey>,
    #[knuffel(child, unwrap(argument, str))]
    pub mod_key_nested: Option<ModKey>,
}

impl MergeWith<InputPart> for Input {
    fn merge_with(&mut self, part: &InputPart) {
        merge!(
            (self, part),
            keyboard,
            disable_power_key_handling,
            workspace_auto_back_and_forth,
            tiling_drag,
            border_resize,
            gap_resize,
        );

        merge_clone!(
            (self, part),
            touchpad,
            mouse,
            trackpoint,
            trackball,
            tablet,
            touch,
            tiling_drag_threshold,
        );

        merge_clone_opt!(
            (self, part),
            warp_mouse_to_focus,
            floating_modifier,
            mod_key,
            mod_key_nested,
        );

        // The KDL node carries only the scroll threshold, so a config file
        // spells sway's `yes`. `always` has no KDL spelling and is reachable
        // only through the IPC command, which is why the runtime type carries
        // the mode and the decoded part does not.
        if let Some(part) = &part.focus_follows_mouse {
            self.focus_follows_mouse = Some(FocusFollowsMouse {
                mode: FocusFollowsMouseMode::Yes,
                max_scroll_amount: part.max_scroll_amount,
            });
        }
    }
}

mod keyboard;
mod libinput;
mod pointer_policy;
mod tablet;

pub use keyboard::{Keyboard, KeyboardPart, TrackLayout, Xkb};
pub use libinput::{
    AccelProfile, ClickMethod, Mouse, ScrollFactor, ScrollMethod, TapButtonMap, Touchpad,
    Trackball, Trackpoint,
};
use pointer_policy::FocusFollowsMousePart;
pub use pointer_policy::{
    FloatingModifier, FocusFollowsMouse, FocusFollowsMouseMode, ModKey, MouseWarping,
    WarpMouseToFocus, WarpMouseToFocusMode,
};
pub use tablet::{Tablet, Touch};

#[cfg(test)]
mod tests {
    use insta::assert_debug_snapshot;

    use super::*;
    use crate::binds::Modifiers;
    use crate::FloatOrInt;

    #[track_caller]
    fn do_parse(text: &str) -> Input {
        let part = knuffel::parse("test.kdl", text)
            .map_err(miette::Report::new)
            .unwrap();
        Input::from_part(&part)
    }

    #[test]
    fn none_disables_the_compositor_modifier() {
        let parsed = do_parse(r#"mod-key "None""#);

        assert_eq!(parsed.mod_key, Some(ModKey::None));
        assert!(!parsed.mod_key.unwrap().is_pressed(Modifiers::empty()));
        assert!(!parsed.mod_key.unwrap().is_pressed(Modifiers::SUPER));
    }

    #[test]
    fn parses_floating_modifier_independently_from_mod_key() {
        let parsed = do_parse(
            r#"
            mod-key "Super"
            floating-modifier "Alt" inverse=true
            "#,
        );

        assert_eq!(parsed.mod_key, Some(ModKey::Super));
        assert_eq!(
            parsed.floating_modifier,
            Some(FloatingModifier {
                modifier: ModKey::Alt,
                inverse: true,
            })
        );
    }

    #[test]
    fn parse_scroll_factor_combined() {
        // Test combined scroll-factor syntax
        let parsed = do_parse(
            r#"
            mouse {
                scroll-factor 2.0
            }
            touchpad {
                scroll-factor 1.5
            }
            "#,
        );

        assert_debug_snapshot!(parsed.mouse.scroll_factor, @r#"
        Some(
            ScrollFactor {
                base: Some(
                    FloatOrInt(
                        2.0,
                    ),
                ),
                horizontal: None,
                vertical: None,
            },
        )
        "#);
        assert_debug_snapshot!(parsed.touchpad.scroll_factor, @r#"
        Some(
            ScrollFactor {
                base: Some(
                    FloatOrInt(
                        1.5,
                    ),
                ),
                horizontal: None,
                vertical: None,
            },
        )
        "#);
    }

    #[test]
    fn parse_scroll_factor_split() {
        // Test split horizontal/vertical syntax
        let parsed = do_parse(
            r#"
            mouse {
                scroll-factor horizontal=2.0 vertical=-1.0
            }
            touchpad {
                scroll-factor horizontal=-1.5 vertical=0.5
            }
            "#,
        );

        assert_debug_snapshot!(parsed.mouse.scroll_factor, @r#"
        Some(
            ScrollFactor {
                base: None,
                horizontal: Some(
                    FloatOrInt(
                        2.0,
                    ),
                ),
                vertical: Some(
                    FloatOrInt(
                        -1.0,
                    ),
                ),
            },
        )
        "#);
        assert_debug_snapshot!(parsed.touchpad.scroll_factor, @r#"
        Some(
            ScrollFactor {
                base: None,
                horizontal: Some(
                    FloatOrInt(
                        -1.5,
                    ),
                ),
                vertical: Some(
                    FloatOrInt(
                        0.5,
                    ),
                ),
            },
        )
        "#);
    }

    #[test]
    fn parse_scroll_factor_partial() {
        // Test partial specification (only one axis)
        let parsed = do_parse(
            r#"
            mouse {
                scroll-factor horizontal=2.0
            }
            touchpad {
                scroll-factor vertical=-1.5
            }
            "#,
        );

        assert_debug_snapshot!(parsed.mouse.scroll_factor, @r#"
        Some(
            ScrollFactor {
                base: None,
                horizontal: Some(
                    FloatOrInt(
                        2.0,
                    ),
                ),
                vertical: None,
            },
        )
        "#);
        assert_debug_snapshot!(parsed.touchpad.scroll_factor, @r#"
        Some(
            ScrollFactor {
                base: None,
                horizontal: None,
                vertical: Some(
                    FloatOrInt(
                        -1.5,
                    ),
                ),
            },
        )
        "#);
    }

    #[test]
    fn parse_scroll_factor_mixed() {
        // Test mixed base + override syntax
        let parsed = do_parse(
            r#"
            mouse {
                scroll-factor 2 vertical=-1
            }
            touchpad {
                scroll-factor 1.5 horizontal=3
            }
            "#,
        );

        assert_debug_snapshot!(parsed.mouse.scroll_factor, @r#"
        Some(
            ScrollFactor {
                base: Some(
                    FloatOrInt(
                        2.0,
                    ),
                ),
                horizontal: None,
                vertical: Some(
                    FloatOrInt(
                        -1.0,
                    ),
                ),
            },
        )
        "#);
        assert_debug_snapshot!(parsed.touchpad.scroll_factor, @r#"
        Some(
            ScrollFactor {
                base: Some(
                    FloatOrInt(
                        1.5,
                    ),
                ),
                horizontal: Some(
                    FloatOrInt(
                        3.0,
                    ),
                ),
                vertical: None,
            },
        )
        "#);
    }

    #[test]
    fn scroll_factor_h_v_factors() {
        let sf = ScrollFactor {
            base: Some(FloatOrInt(2.0)),
            horizontal: None,
            vertical: None,
        };
        assert_debug_snapshot!(sf.h_v_factors(), @r#"
        (
            2.0,
            2.0,
        )
        "#);

        let sf = ScrollFactor {
            base: None,
            horizontal: Some(FloatOrInt(3.0)),
            vertical: Some(FloatOrInt(-1.0)),
        };
        assert_debug_snapshot!(sf.h_v_factors(), @r#"
        (
            3.0,
            -1.0,
        )
        "#);

        let sf = ScrollFactor {
            base: Some(FloatOrInt(2.0)),
            horizontal: Some(FloatOrInt(1.0)),
            vertical: None,
        };
        assert_debug_snapshot!(sf.h_v_factors(), @r"
        (
            1.0,
            2.0,
        )
        ");
    }
}
