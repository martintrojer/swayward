use std::str::FromStr;

use knuffel::errors::DecodeError;
use swayward_ipc::SizeChange;

use crate::appearance::{
    Border, FocusRing, InsertHint, Shadow, TabIndicator, Titlebar, DEFAULT_BACKGROUND_COLOR,
};
use crate::utils::{expect_only_children, Flag, MergeWith};
use crate::{BorderRule, Color, FloatOrInt, InsertHintPart, ShadowRule, TabIndicatorPart};

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub focus_ring: FocusRing,
    pub border: Border,
    pub shadow: Shadow,
    pub tab_indicator: TabIndicator,
    pub titlebar: Titlebar,
    pub draw_uncovered_top_border: bool,
    pub insert_hint: InsertHint,
    pub preset_column_widths: Vec<PresetSize>,
    pub default_column_width: Option<PresetSize>,
    pub preset_window_heights: Vec<PresetSize>,
    pub focus_wrapping: FocusWrapping,
    pub workspace_layout: WorkspaceLayout,
    pub default_orientation: DefaultOrientation,
    pub hide_edge_borders: HideEdgeBorders,
    pub smart_borders: SmartBorders,
    pub smart_gaps: SmartGaps,
    /// Border style applied to a new window when no rule overrides it.
    ///
    /// sway's `default_border` / `default_floating_border`. Stored as the IPC
    /// border style so the command, the window rule and the tile all speak
    /// the same type.
    pub default_border: SwayBorderDefault,
    pub default_floating_border: SwayBorderDefault,
    pub floating_minimum_size: FloatingSize,
    pub floating_maximum_size: FloatingSize,
    pub gaps: f64,
    pub outer_gaps: OuterGaps,
    pub outer_gaps_configured: bool,
    pub struts: Struts,
    pub background_color: Color,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            focus_ring: FocusRing::default(),
            border: Border::default(),
            shadow: Shadow::default(),
            tab_indicator: TabIndicator::default(),
            titlebar: Titlebar::default(),
            draw_uncovered_top_border: true,
            insert_hint: InsertHint::default(),
            preset_column_widths: vec![
                PresetSize::Proportion(1. / 3.),
                PresetSize::Proportion(0.5),
                PresetSize::Proportion(2. / 3.),
            ],
            default_column_width: Some(PresetSize::Proportion(0.5)),
            focus_wrapping: FocusWrapping::Yes,
            workspace_layout: WorkspaceLayout::Default,
            default_orientation: DefaultOrientation::Auto,
            hide_edge_borders: HideEdgeBorders::None,
            smart_borders: SmartBorders::Off,
            smart_gaps: SmartGaps::Off,
            default_border: SwayBorderDefault::default(),
            default_floating_border: SwayBorderDefault::default(),
            floating_minimum_size: FloatingSize {
                width: 75,
                height: 50,
            },
            floating_maximum_size: FloatingSize {
                width: 0,
                height: 0,
            },
            gaps: 0.,
            outer_gaps: OuterGaps::default(),
            outer_gaps_configured: false,
            struts: Struts::default(),
            preset_window_heights: vec![
                PresetSize::Proportion(1. / 3.),
                PresetSize::Proportion(0.5),
                PresetSize::Proportion(2. / 3.),
            ],
            background_color: DEFAULT_BACKGROUND_COLOR,
        }
    }
}

impl MergeWith<LayoutPart> for Layout {
    fn merge_with(&mut self, part: &LayoutPart) {
        merge!(
            (self, part),
            focus_ring,
            border,
            shadow,
            tab_indicator,
            titlebar,
            draw_uncovered_top_border,
            insert_hint,
            gaps,
        );

        merge_clone!(
            (self, part),
            preset_column_widths,
            preset_window_heights,
            focus_wrapping,
            workspace_layout,
            default_orientation,
            hide_edge_borders,
            smart_borders,
            smart_gaps,
            default_border,
            default_floating_border,
            floating_minimum_size,
            floating_maximum_size,
            struts,
            background_color,
        );

        if let Some(x) = part.default_column_width {
            self.default_column_width = x.0;
        }
        if let Some(x) = &part.outer_gaps {
            self.outer_gaps.merge_with(x);
            self.outer_gaps_configured = true;
        }

        if self.preset_column_widths.is_empty() {
            self.preset_column_widths = Layout::default().preset_column_widths;
        }

        if self.preset_window_heights.is_empty() {
            self.preset_window_heights = Layout::default().preset_window_heights;
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct LayoutPart {
    #[knuffel(child)]
    pub focus_ring: Option<BorderRule>,
    #[knuffel(child)]
    pub border: Option<BorderRule>,
    #[knuffel(child)]
    pub shadow: Option<ShadowRule>,
    #[knuffel(child)]
    pub tab_indicator: Option<TabIndicatorPart>,
    #[knuffel(child)]
    pub titlebar: Option<crate::appearance::TitlebarPart>,
    #[knuffel(child)]
    pub draw_uncovered_top_border: Option<Flag>,
    #[knuffel(child)]
    pub insert_hint: Option<InsertHintPart>,
    #[knuffel(child, unwrap(children))]
    pub preset_column_widths: Option<Vec<PresetSize>>,
    #[knuffel(child)]
    pub default_column_width: Option<DefaultPresetSize>,
    #[knuffel(child, unwrap(children))]
    pub preset_window_heights: Option<Vec<PresetSize>>,
    #[knuffel(child)]
    pub center_focused_column: Option<RetiredScrollingLayoutSetting>,
    #[knuffel(child)]
    pub always_center_single_column: Option<RetiredScrollingLayoutSetting>,
    #[knuffel(child)]
    pub default_column_display: Option<RetiredScrollingLayoutSetting>,
    #[knuffel(child, unwrap(argument, str))]
    pub focus_wrapping: Option<FocusWrapping>,
    #[knuffel(child, unwrap(argument, str))]
    pub workspace_layout: Option<WorkspaceLayout>,
    #[knuffel(child, unwrap(argument, str))]
    pub default_orientation: Option<DefaultOrientation>,
    #[knuffel(child, unwrap(argument, str))]
    pub hide_edge_borders: Option<HideEdgeBorders>,
    #[knuffel(child, unwrap(argument, str))]
    pub smart_borders: Option<SmartBorders>,
    #[knuffel(child, unwrap(argument, str))]
    pub smart_gaps: Option<SmartGaps>,
    #[knuffel(child)]
    pub default_border: Option<SwayBorderDefault>,
    #[knuffel(child)]
    pub default_floating_border: Option<SwayBorderDefault>,
    #[knuffel(child)]
    pub floating_minimum_size: Option<FloatingSize>,
    #[knuffel(child)]
    pub floating_maximum_size: Option<FloatingSize>,
    #[knuffel(child, unwrap(argument))]
    pub gaps: Option<FloatOrInt<0, 65535>>,
    #[knuffel(child)]
    pub outer_gaps: Option<OuterGapsPart>,
    #[knuffel(child)]
    pub struts: Option<Struts>,
    #[knuffel(child)]
    pub background_color: Option<Color>,
}

#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub enum PresetSize {
    Proportion(#[knuffel(argument)] f64),
    Fixed(#[knuffel(argument)] i32),
}

impl From<PresetSize> for SizeChange {
    fn from(value: PresetSize) -> Self {
        match value {
            PresetSize::Proportion(prop) => SizeChange::SetProportion(prop * 100.),
            PresetSize::Fixed(fixed) => SizeChange::SetFixed(fixed),
        }
    }
}

#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingSize {
    #[knuffel(argument)]
    pub width: i32,
    #[knuffel(argument)]
    pub height: i32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DefaultOrientation {
    Horizontal,
    Vertical,
    #[default]
    Auto,
}

impl FromStr for DefaultOrientation {
    type Err = miette::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match &*value.to_ascii_lowercase() {
            "horizontal" => Ok(Self::Horizontal),
            "vertical" => Ok(Self::Vertical),
            "auto" => Ok(Self::Auto),
            _ => Err(miette::miette!("unknown default orientation `{value}`")),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceLayout {
    #[default]
    Default,
    Stacking,
    Tabbed,
}

impl FromStr for WorkspaceLayout {
    type Err = miette::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match &*value.to_ascii_lowercase() {
            "default" => Ok(Self::Default),
            "stacking" => Ok(Self::Stacking),
            "tabbed" => Ok(Self::Tabbed),
            _ => Err(miette::miette!("unknown workspace layout `{value}`")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefaultPresetSize(pub Option<PresetSize>);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SmartGaps {
    #[default]
    Off,
    On,
    InverseOuter,
}

impl FromStr for SmartGaps {
    type Err = miette::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "on" => Ok(Self::On),
            "inverse-outer" => Ok(Self::InverseOuter),
            _ => Err(miette::miette!("unknown smart gaps mode `{value}`")),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FocusWrapping {
    #[default]
    Yes,
    No,
    Force,
    Workspace,
}

impl FromStr for FocusWrapping {
    type Err = miette::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match &*value.to_ascii_lowercase() {
            "yes" => Ok(Self::Yes),
            "no" => Ok(Self::No),
            "force" => Ok(Self::Force),
            "workspace" => Ok(Self::Workspace),
            _ => Err(miette::miette!("unknown focus wrapping mode `{value}`")),
        }
    }
}

#[cfg(test)]
mod sway_enum_tests {
    use super::*;

    #[test]
    fn parses_case_insensitive_sway_enums() {
        assert_eq!(
            "Horizontal".parse::<DefaultOrientation>().unwrap(),
            DefaultOrientation::Horizontal
        );
        assert_eq!(
            "Tabbed".parse::<WorkspaceLayout>().unwrap(),
            WorkspaceLayout::Tabbed
        );
        for (value, expected) in [
            ("YES", FocusWrapping::Yes),
            ("No", FocusWrapping::No),
            ("force", FocusWrapping::Force),
            ("wOrKsPaCe", FocusWrapping::Workspace),
        ] {
            assert_eq!(value.parse::<FocusWrapping>().unwrap(), expected);
        }
    }

    #[test]
    fn layout_defaults_to_sways_zero_gaps() {
        assert_eq!(Layout::default().gaps, 0.);
    }

    #[test]
    fn accepts_sways_smart_border_no_gaps_spelling() {
        assert_eq!(
            "no_gaps".parse::<SmartBorders>().unwrap(),
            SmartBorders::NoGaps
        );
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct OuterGaps {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

impl OuterGaps {
    pub fn all(value: f64) -> Self {
        Self {
            left: value,
            right: value,
            top: value,
            bottom: value,
        }
    }
}

impl MergeWith<OuterGapsPart> for OuterGaps {
    fn merge_with(&mut self, part: &OuterGapsPart) {
        merge!((self, part), left, right, top, bottom);
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct OuterGapsPart {
    #[knuffel(child, unwrap(argument))]
    pub left: Option<FloatOrInt<-65535, 65535>>,
    #[knuffel(child, unwrap(argument))]
    pub right: Option<FloatOrInt<-65535, 65535>>,
    #[knuffel(child, unwrap(argument))]
    pub top: Option<FloatOrInt<-65535, 65535>>,
    #[knuffel(child, unwrap(argument))]
    pub bottom: Option<FloatOrInt<-65535, 65535>>,
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct Struts {
    #[knuffel(child, unwrap(argument), default)]
    pub left: FloatOrInt<-65535, 65535>,
    #[knuffel(child, unwrap(argument), default)]
    pub right: FloatOrInt<-65535, 65535>,
    #[knuffel(child, unwrap(argument), default)]
    pub top: FloatOrInt<-65535, 65535>,
    #[knuffel(child, unwrap(argument), default)]
    pub bottom: FloatOrInt<-65535, 65535>,
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum HideEdgeBorders {
    #[default]
    None,
    Vertical,
    Horizontal,
    Both,
}

impl FromStr for HideEdgeBorders {
    type Err = miette::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "vertical" => Ok(Self::Vertical),
            "horizontal" => Ok(Self::Horizontal),
            "both" => Ok(Self::Both),
            _ => Err(miette::miette!("unknown edge border mode `{value}`")),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum SmartBorders {
    #[default]
    Off,
    On,
    NoGaps,
}

impl FromStr for SmartBorders {
    type Err = miette::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "off" => Ok(Self::Off),
            "on" => Ok(Self::On),
            "no-gaps" | "no_gaps" => Ok(Self::NoGaps),
            _ => Err(miette::miette!("unknown smart border mode `{value}`")),
        }
    }
}

/// sway's `default_border`: a style and an optional explicit width.
#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwayBorderDefault {
    #[knuffel(argument, str)]
    pub style: SwayBorderStyle,
    #[knuffel(property)]
    pub width: Option<u16>,
}

impl Default for SwayBorderDefault {
    fn default() -> Self {
        Self {
            style: SwayBorderStyle::Normal,
            width: None,
        }
    }
}

/// The subset of sway border styles a default may take.
///
/// `csd` and `toggle` are valid for the `border` command but not as a default
/// (`sway/sway/commands/default_border.c:12-21`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SwayBorderStyle {
    None,
    #[default]
    Normal,
    Pixel,
}

impl FromStr for SwayBorderStyle {
    type Err = miette::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "normal" => Ok(Self::Normal),
            "pixel" => Ok(Self::Pixel),
            _ => Err(miette::miette!("unknown border style `{value}`")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetiredScrollingLayoutSetting;

impl<S> knuffel::Decode<S> for RetiredScrollingLayoutSetting
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
            "setting was retired with the scrolling layout engine",
        ));
        Ok(Self)
    }
}

impl<S> knuffel::Decode<S> for DefaultPresetSize
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        expect_only_children(node, ctx);

        let mut children = node.children();

        if let Some(child) = children.next() {
            if let Some(unwanted_child) = children.next() {
                ctx.emit_error(DecodeError::unexpected(
                    unwanted_child,
                    "node",
                    "expected no more than one child",
                ));
            }
            PresetSize::decode_node(child, ctx).map(Some).map(Self)
        } else {
            Ok(Self(None))
        }
    }
}
