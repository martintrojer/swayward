use super::{Color, Gradient};
use crate::utils::{Flag, MergeWith};
use crate::FloatOrInt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabIndicator {
    pub off: bool,
    pub hide_when_single_tab: bool,
    pub place_within_column: bool,
    pub gap: f64,
    pub width: f64,
    pub length: TabIndicatorLength,
    pub position: TabIndicatorPosition,
    pub gaps_between_tabs: f64,
    pub corner_radius: f64,
    pub active_color: Option<Color>,
    pub inactive_color: Option<Color>,
    pub urgent_color: Option<Color>,
    pub active_gradient: Option<Gradient>,
    pub inactive_gradient: Option<Gradient>,
    pub urgent_gradient: Option<Gradient>,
}

impl Default for TabIndicator {
    fn default() -> Self {
        Self {
            off: false,
            hide_when_single_tab: false,
            place_within_column: false,
            gap: 5.,
            width: 4.,
            length: TabIndicatorLength {
                total_proportion: Some(0.5),
            },
            position: TabIndicatorPosition::Left,
            gaps_between_tabs: 0.,
            corner_radius: 0.,
            active_color: None,
            inactive_color: None,
            urgent_color: None,
            active_gradient: None,
            inactive_gradient: None,
            urgent_gradient: None,
        }
    }
}

impl MergeWith<TabIndicatorPart> for TabIndicator {
    fn merge_with(&mut self, part: &TabIndicatorPart) {
        self.off |= part.off;
        if part.on {
            self.off = false;
        }

        merge!(
            (self, part),
            hide_when_single_tab,
            place_within_column,
            gap,
            width,
            gaps_between_tabs,
            corner_radius,
        );

        merge_clone!((self, part), length, position);

        merge_color_gradient_opt!(
            (self, part),
            (active_color, active_gradient),
            (inactive_color, inactive_gradient),
            (urgent_color, urgent_gradient),
        );
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct TabIndicatorPart {
    #[knuffel(child)]
    pub off: bool,
    #[knuffel(child)]
    pub on: bool,
    #[knuffel(child)]
    pub hide_when_single_tab: Option<Flag>,
    #[knuffel(child)]
    pub place_within_column: Option<Flag>,
    #[knuffel(child, unwrap(argument))]
    pub gap: Option<FloatOrInt<-65535, 65535>>,
    #[knuffel(child, unwrap(argument))]
    pub width: Option<FloatOrInt<0, 65535>>,
    #[knuffel(child)]
    pub length: Option<TabIndicatorLength>,
    #[knuffel(child, unwrap(argument))]
    pub position: Option<TabIndicatorPosition>,
    #[knuffel(child, unwrap(argument))]
    pub gaps_between_tabs: Option<FloatOrInt<0, 65535>>,
    #[knuffel(child, unwrap(argument))]
    pub corner_radius: Option<FloatOrInt<0, 65535>>,
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

#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub struct TabIndicatorLength {
    #[knuffel(property)]
    pub total_proportion: Option<f64>,
}

#[derive(knuffel::DecodeScalar, Debug, Clone, Copy, PartialEq)]
pub enum TabIndicatorPosition {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InsertHint {
    pub off: bool,
    pub color: Color,
    pub gradient: Option<Gradient>,
}

impl Default for InsertHint {
    fn default() -> Self {
        Self {
            off: false,
            color: Color::from_rgba8_unpremul(127, 200, 255, 128),
            gradient: None,
        }
    }
}

impl MergeWith<InsertHintPart> for InsertHint {
    fn merge_with(&mut self, part: &InsertHintPart) {
        self.off |= part.off;
        if part.on {
            self.off = false;
        }

        merge_color_gradient!((self, part), (color, gradient));
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct InsertHintPart {
    #[knuffel(child)]
    pub off: bool,
    #[knuffel(child)]
    pub on: bool,
    #[knuffel(child)]
    pub color: Option<Color>,
    #[knuffel(child)]
    pub gradient: Option<Gradient>,
}
