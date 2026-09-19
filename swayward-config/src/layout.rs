use std::str::FromStr;

use knuffel::errors::DecodeError;
use swayward_ipc::{ColumnDisplay, SizeChange};

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
    pub insert_hint: InsertHint,
    pub preset_column_widths: Vec<PresetSize>,
    pub default_column_width: Option<PresetSize>,
    pub preset_window_heights: Vec<PresetSize>,
    pub center_focused_column: CenterFocusedColumn,
    pub always_center_single_column: bool,
    pub default_column_display: ColumnDisplay,
    pub focus_wrapping: FocusWrapping,
    pub workspace_layout: WorkspaceLayout,
    pub default_orientation: DefaultOrientation,
    pub hide_edge_borders: HideEdgeBorders,
    pub smart_borders: SmartBorders,
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
            insert_hint: InsertHint::default(),
            preset_column_widths: vec![
                PresetSize::Proportion(1. / 3.),
                PresetSize::Proportion(0.5),
                PresetSize::Proportion(2. / 3.),
            ],
            default_column_width: Some(PresetSize::Proportion(0.5)),
            center_focused_column: CenterFocusedColumn::Never,
            always_center_single_column: false,
            default_column_display: ColumnDisplay::Normal,
            focus_wrapping: FocusWrapping::Yes,
            workspace_layout: WorkspaceLayout::Default,
            default_orientation: DefaultOrientation::Auto,
            hide_edge_borders: HideEdgeBorders::None,
            smart_borders: SmartBorders::Off,
            floating_minimum_size: FloatingSize {
                width: 75,
                height: 50,
            },
            floating_maximum_size: FloatingSize {
                width: 0,
                height: 0,
            },
            gaps: 16.,
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
            insert_hint,
            always_center_single_column,
            gaps,
        );

        merge_clone!(
            (self, part),
            preset_column_widths,
            preset_window_heights,
            center_focused_column,
            default_column_display,
            focus_wrapping,
            workspace_layout,
            default_orientation,
            hide_edge_borders,
            smart_borders,
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
    pub insert_hint: Option<InsertHintPart>,
    #[knuffel(child, unwrap(children))]
    pub preset_column_widths: Option<Vec<PresetSize>>,
    #[knuffel(child)]
    pub default_column_width: Option<DefaultPresetSize>,
    #[knuffel(child, unwrap(children))]
    pub preset_window_heights: Option<Vec<PresetSize>>,
    #[knuffel(child, unwrap(argument))]
    pub center_focused_column: Option<CenterFocusedColumn>,
    #[knuffel(child)]
    pub always_center_single_column: Option<Flag>,
    #[knuffel(child, unwrap(argument, str))]
    pub default_column_display: Option<ColumnDisplay>,
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
        match value {
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
        match value {
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
mod focus_wrapping_tests {
    use super::*;

    #[test]
    fn parses_all_modes_case_insensitively() {
        for (value, expected) in [
            ("YES", FocusWrapping::Yes),
            ("No", FocusWrapping::No),
            ("force", FocusWrapping::Force),
            ("wOrKsPaCe", FocusWrapping::Workspace),
        ] {
            assert_eq!(value.parse::<FocusWrapping>().unwrap(), expected);
        }
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
            "no-gaps" => Ok(Self::NoGaps),
            _ => Err(miette::miette!("unknown smart border mode `{value}`")),
        }
    }
}

#[derive(knuffel::DecodeScalar, Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum CenterFocusedColumn {
    /// Focusing a column will not center the column.
    #[default]
    Never,
    /// The focused column will always be centered.
    Always,
    /// Focusing a column will center it if it doesn't fit on the screen together with the
    /// previously focused column.
    OnOverflow,
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
