use knuffel::errors::DecodeError;

use crate::utils::MergeWith;
use crate::FloatOrInt;

#[derive(Debug, Clone, PartialEq)]
pub struct Animations {
    pub off: bool,
    pub slowdown: f64,
    pub workspace_switch: WorkspaceSwitchAnim,
    pub window_open: WindowOpenAnim,
    pub window_close: WindowCloseAnim,
    pub window_movement: WindowMovementAnim,
    pub window_resize: WindowResizeAnim,
    pub config_notification_open_close: ConfigNotificationOpenCloseAnim,
    pub exit_confirmation_open_close: ExitConfirmationOpenCloseAnim,
    pub screenshot_ui_open: ScreenshotUiOpenAnim,
    pub overview_open_close: OverviewOpenCloseAnim,
    pub recent_windows_close: RecentWindowsCloseAnim,
}

impl Default for Animations {
    fn default() -> Self {
        Self {
            off: false,
            slowdown: 1.,
            workspace_switch: Default::default(),
            window_movement: Default::default(),
            window_open: Default::default(),
            window_close: Default::default(),
            window_resize: Default::default(),
            config_notification_open_close: Default::default(),
            exit_confirmation_open_close: Default::default(),
            screenshot_ui_open: Default::default(),
            overview_open_close: Default::default(),
            recent_windows_close: Default::default(),
        }
    }
}

#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub struct AnimationsPart {
    #[knuffel(child)]
    pub off: bool,
    #[knuffel(child)]
    pub on: bool,
    #[knuffel(child, unwrap(argument))]
    pub slowdown: Option<FloatOrInt<0, { i32::MAX }>>,
    #[knuffel(child)]
    pub workspace_switch: Option<WorkspaceSwitchAnim>,
    #[knuffel(child)]
    pub window_open: Option<WindowOpenAnim>,
    #[knuffel(child)]
    pub window_close: Option<WindowCloseAnim>,
    #[knuffel(child)]
    horizontal_view_movement: Option<RetiredHorizontalViewMovement>,
    #[knuffel(child)]
    pub window_movement: Option<WindowMovementAnim>,
    #[knuffel(child)]
    pub window_resize: Option<WindowResizeAnim>,
    #[knuffel(child)]
    pub config_notification_open_close: Option<ConfigNotificationOpenCloseAnim>,
    #[knuffel(child)]
    pub exit_confirmation_open_close: Option<ExitConfirmationOpenCloseAnim>,
    #[knuffel(child)]
    pub screenshot_ui_open: Option<ScreenshotUiOpenAnim>,
    #[knuffel(child)]
    pub overview_open_close: Option<OverviewOpenCloseAnim>,
    #[knuffel(child)]
    pub recent_windows_close: Option<RecentWindowsCloseAnim>,
}

impl MergeWith<AnimationsPart> for Animations {
    fn merge_with(&mut self, part: &AnimationsPart) {
        self.off |= part.off;
        if part.on {
            self.off = false;
        }

        merge!((self, part), slowdown);

        // Animation properties are fairly tied together, except maybe `off`. So let's just save
        // ourselves the work and not merge within individual animations.
        merge_clone!(
            (self, part),
            workspace_switch,
            window_open,
            window_close,
            window_movement,
            window_resize,
            config_notification_open_close,
            exit_confirmation_open_close,
            screenshot_ui_open,
            overview_open_close,
            recent_windows_close,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Animation {
    pub off: bool,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Easing(EasingParams),
    Spring(SpringParams),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EasingParams {
    pub duration_ms: u32,
    pub curve: Curve,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve {
    Linear,
    EaseOutQuad,
    EaseOutCubic,
    EaseOutExpo,
    CubicBezier(f64, f64, f64, f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringParams {
    pub damping_ratio: f64,
    pub stiffness: u32,
    pub epsilon: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkspaceSwitchAnim(pub Animation);

impl Default for WorkspaceSwitchAnim {
    fn default() -> Self {
        Self(Animation {
            off: false,
            kind: Kind::Spring(SpringParams {
                damping_ratio: 1.,
                stiffness: 1000,
                epsilon: 0.0001,
            }),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowOpenAnim {
    pub anim: Animation,
    pub custom_shader: Option<String>,
}

impl Default for WindowOpenAnim {
    fn default() -> Self {
        Self {
            anim: Animation {
                off: false,
                kind: Kind::Easing(EasingParams {
                    duration_ms: 150,
                    curve: Curve::EaseOutExpo,
                }),
            },
            custom_shader: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowCloseAnim {
    pub anim: Animation,
    pub custom_shader: Option<String>,
}

impl Default for WindowCloseAnim {
    fn default() -> Self {
        Self {
            anim: Animation {
                off: false,
                kind: Kind::Easing(EasingParams {
                    duration_ms: 150,
                    curve: Curve::EaseOutQuad,
                }),
            },
            custom_shader: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct RetiredHorizontalViewMovement;

impl<S> knuffel::Decode<S> for RetiredHorizontalViewMovement
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        ctx.emit_error(DecodeError::unexpected(
            node,
            "node",
            "horizontal-view-movement was retired with the scrolling layout engine",
        ));
        Ok(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowMovementAnim(pub Animation);

impl Default for WindowMovementAnim {
    fn default() -> Self {
        Self(Animation {
            off: false,
            kind: Kind::Spring(SpringParams {
                damping_ratio: 1.,
                stiffness: 800,
                epsilon: 0.0001,
            }),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowResizeAnim {
    pub anim: Animation,
    pub custom_shader: Option<String>,
}

impl Default for WindowResizeAnim {
    fn default() -> Self {
        Self {
            anim: Animation {
                off: false,
                kind: Kind::Spring(SpringParams {
                    damping_ratio: 1.,
                    stiffness: 800,
                    epsilon: 0.0001,
                }),
            },
            custom_shader: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConfigNotificationOpenCloseAnim(pub Animation);

impl Default for ConfigNotificationOpenCloseAnim {
    fn default() -> Self {
        Self(Animation {
            off: false,
            kind: Kind::Spring(SpringParams {
                damping_ratio: 0.6,
                stiffness: 1000,
                epsilon: 0.001,
            }),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExitConfirmationOpenCloseAnim(pub Animation);

impl Default for ExitConfirmationOpenCloseAnim {
    fn default() -> Self {
        Self(Animation {
            off: false,
            kind: Kind::Spring(SpringParams {
                damping_ratio: 0.6,
                stiffness: 500,
                epsilon: 0.01,
            }),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenshotUiOpenAnim(pub Animation);

impl Default for ScreenshotUiOpenAnim {
    fn default() -> Self {
        Self(Animation {
            off: false,
            kind: Kind::Easing(EasingParams {
                duration_ms: 200,
                curve: Curve::EaseOutQuad,
            }),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverviewOpenCloseAnim(pub Animation);

impl Default for OverviewOpenCloseAnim {
    fn default() -> Self {
        Self(Animation {
            off: false,
            kind: Kind::Spring(SpringParams {
                damping_ratio: 1.,
                stiffness: 800,
                epsilon: 0.0001,
            }),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecentWindowsCloseAnim(pub Animation);

impl Default for RecentWindowsCloseAnim {
    fn default() -> Self {
        Self(Animation {
            off: false,
            kind: Kind::Spring(SpringParams {
                damping_ratio: 1.,
                stiffness: 800,
                epsilon: 0.001,
            }),
        })
    }
}
