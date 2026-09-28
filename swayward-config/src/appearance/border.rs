use super::{Color, Gradient};
use crate::utils::MergeWith;
use crate::FloatOrInt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusRing {
    pub off: bool,
    pub width: f64,
    pub active_color: Color,
    pub inactive_color: Color,
    pub urgent_color: Color,
    pub active_gradient: Option<Gradient>,
    pub inactive_gradient: Option<Gradient>,
    pub urgent_gradient: Option<Gradient>,
}

impl Default for FocusRing {
    fn default() -> Self {
        Self {
            off: false,
            width: 4.,
            active_color: Color::from_rgba8_unpremul(127, 200, 255, 255),
            inactive_color: Color::from_rgba8_unpremul(80, 80, 80, 255),
            urgent_color: Color::from_rgba8_unpremul(155, 0, 0, 255),
            active_gradient: None,
            inactive_gradient: None,
            urgent_gradient: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    pub off: bool,
    pub width: f64,
    pub active_color: Color,
    pub inactive_color: Color,
    pub urgent_color: Color,
    pub active_gradient: Option<Gradient>,
    pub inactive_gradient: Option<Gradient>,
    pub urgent_gradient: Option<Gradient>,
}

impl Default for Border {
    fn default() -> Self {
        Self {
            off: true,
            width: 4.,
            active_color: Color::from_rgba8_unpremul(255, 200, 127, 255),
            inactive_color: Color::from_rgba8_unpremul(80, 80, 80, 255),
            urgent_color: Color::from_rgba8_unpremul(155, 0, 0, 255),
            active_gradient: None,
            inactive_gradient: None,
            urgent_gradient: None,
        }
    }
}

impl From<Border> for FocusRing {
    fn from(value: Border) -> Self {
        Self {
            off: value.off,
            width: value.width,
            active_color: value.active_color,
            inactive_color: value.inactive_color,
            urgent_color: value.urgent_color,
            active_gradient: value.active_gradient,
            inactive_gradient: value.inactive_gradient,
            urgent_gradient: value.urgent_gradient,
        }
    }
}

impl From<FocusRing> for Border {
    fn from(value: FocusRing) -> Self {
        Self {
            off: value.off,
            width: value.width,
            active_color: value.active_color,
            inactive_color: value.inactive_color,
            urgent_color: value.urgent_color,
            active_gradient: value.active_gradient,
            inactive_gradient: value.inactive_gradient,
            urgent_gradient: value.urgent_gradient,
        }
    }
}

impl MergeWith<BorderRule> for Border {
    fn merge_with(&mut self, part: &BorderRule) {
        self.off |= part.off;
        if part.on {
            self.off = false;
        }

        merge!((self, part), width);

        merge_color_gradient!(
            (self, part),
            (active_color, active_gradient),
            (inactive_color, inactive_gradient),
            (urgent_color, urgent_gradient),
        );
    }
}

impl MergeWith<BorderRule> for FocusRing {
    fn merge_with(&mut self, part: &BorderRule) {
        let mut x = Border::from(*self);
        x.merge_with(part);
        *self = FocusRing::from(x);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub on: bool,
    pub offset: ShadowOffset,
    pub softness: f64,
    pub spread: f64,
    pub draw_behind_window: bool,
    pub color: Color,
    pub inactive_color: Option<Color>,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            on: false,
            offset: ShadowOffset {
                x: FloatOrInt(0.),
                y: FloatOrInt(5.),
            },
            softness: 30.,
            spread: 5.,
            draw_behind_window: false,
            color: Color::from_rgba8_unpremul(0, 0, 0, 0x77),
            inactive_color: None,
        }
    }
}

impl MergeWith<ShadowRule> for Shadow {
    fn merge_with(&mut self, part: &ShadowRule) {
        self.on |= part.on;
        if part.off {
            self.on = false;
        }

        merge!((self, part), softness, spread);

        merge_clone!((self, part), offset, draw_behind_window, color);

        merge_clone_opt!((self, part), inactive_color);
    }
}

#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub struct ShadowOffset {
    #[knuffel(property, default)]
    pub x: FloatOrInt<-65535, 65535>,
    #[knuffel(property, default)]
    pub y: FloatOrInt<-65535, 65535>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkspaceShadow {
    pub off: bool,
    pub offset: ShadowOffset,
    pub softness: f64,
    pub spread: f64,
    pub color: Color,
}

impl Default for WorkspaceShadow {
    fn default() -> Self {
        Self {
            off: false,
            offset: ShadowOffset {
                x: FloatOrInt(0.),
                y: FloatOrInt(10.),
            },
            softness: 40.,
            spread: 10.,
            color: Color::from_rgba8_unpremul(0, 0, 0, 0x50),
        }
    }
}

impl From<WorkspaceShadow> for Shadow {
    fn from(value: WorkspaceShadow) -> Self {
        Self {
            on: !value.off,
            offset: value.offset,
            softness: value.softness,
            spread: value.spread,
            draw_behind_window: false,
            color: value.color,
            inactive_color: None,
        }
    }
}

#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub struct WorkspaceShadowPart {
    #[knuffel(child)]
    pub off: bool,
    #[knuffel(child)]
    pub on: bool,
    #[knuffel(child)]
    pub offset: Option<ShadowOffset>,
    #[knuffel(child, unwrap(argument))]
    pub softness: Option<FloatOrInt<0, 1024>>,
    #[knuffel(child, unwrap(argument))]
    pub spread: Option<FloatOrInt<-1024, 1024>>,
    #[knuffel(child)]
    pub color: Option<Color>,
}

impl MergeWith<WorkspaceShadowPart> for WorkspaceShadow {
    fn merge_with(&mut self, part: &WorkspaceShadowPart) {
        self.off |= part.off;
        if part.on {
            self.off = false;
        }

        merge_clone!((self, part), offset, color);
        merge!((self, part), softness, spread);
    }
}

#[derive(knuffel::DecodeScalar, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockOutFrom {
    Screencast,
    ScreenCapture,
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct BorderRule {
    #[knuffel(child)]
    pub off: bool,
    #[knuffel(child)]
    pub on: bool,
    #[knuffel(child, unwrap(argument))]
    pub width: Option<FloatOrInt<0, 65535>>,
    #[knuffel(child)]
    pub active_color: Option<Color>,
    #[knuffel(child)]
    pub inactive_color: Option<Color>,
    #[knuffel(child)]
    pub urgent_color: Option<Color>,
    #[knuffel(child)]
    pub active_gradient: Option<Gradient>,
    #[knuffel(child)]
    pub inactive_gradient: Option<Gradient>,
    #[knuffel(child)]
    pub urgent_gradient: Option<Gradient>,
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct ShadowRule {
    #[knuffel(child)]
    pub off: bool,
    #[knuffel(child)]
    pub on: bool,
    #[knuffel(child)]
    pub offset: Option<ShadowOffset>,
    #[knuffel(child, unwrap(argument))]
    pub softness: Option<FloatOrInt<0, 1024>>,
    #[knuffel(child, unwrap(argument))]
    pub spread: Option<FloatOrInt<-1024, 1024>>,
    #[knuffel(child, unwrap(argument))]
    pub draw_behind_window: Option<bool>,
    #[knuffel(child)]
    pub color: Option<Color>,
    #[knuffel(child)]
    pub inactive_color: Option<Color>,
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct TabIndicatorRule {
    #[knuffel(child)]
    pub active_color: Option<Color>,
    #[knuffel(child)]
    pub inactive_color: Option<Color>,
    #[knuffel(child)]
    pub urgent_color: Option<Color>,
    #[knuffel(child)]
    pub active_gradient: Option<Gradient>,
    #[knuffel(child)]
    pub inactive_gradient: Option<Gradient>,
    #[knuffel(child)]
    pub urgent_gradient: Option<Gradient>,
}

impl MergeWith<Self> for BorderRule {
    fn merge_with(&mut self, part: &Self) {
        merge_on_off!((self, part));

        merge_clone_opt!((self, part), width);

        merge_color_gradient_opt!(
            (self, part),
            (active_color, active_gradient),
            (inactive_color, inactive_gradient),
            (urgent_color, urgent_gradient),
        );
    }
}

impl MergeWith<Self> for ShadowRule {
    fn merge_with(&mut self, part: &Self) {
        merge_on_off!((self, part));

        merge_clone_opt!(
            (self, part),
            offset,
            softness,
            spread,
            draw_behind_window,
            color,
            inactive_color,
        );
    }
}

impl MergeWith<Self> for TabIndicatorRule {
    fn merge_with(&mut self, part: &Self) {
        merge_color_gradient_opt!(
            (self, part),
            (active_color, active_gradient),
            (inactive_color, inactive_gradient),
            (urgent_color, urgent_gradient),
        );
    }
}
