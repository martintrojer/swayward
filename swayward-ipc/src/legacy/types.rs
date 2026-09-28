use super::*;

/// Overview information.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct Overview {
    /// Whether the overview is currently open.
    pub is_open: bool,
}

/// Color picked from the screen.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct PickedColor {
    /// Color values as red, green, blue, each ranging from 0.0 to 1.0.
    pub rgb: [f64; 3],
}

/// Change in window or column size.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum SizeChange {
    /// Set the size in logical pixels.
    SetFixed(i32),
    /// Set the size as a proportion of the working area.
    SetProportion(f64),
    /// Add or subtract to the current size in logical pixels.
    AdjustFixed(i32),
    /// Add or subtract to the current size as a proportion of the working area.
    AdjustProportion(f64),
}

/// Change in floating window position.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum PositionChange {
    /// Set the position in logical pixels.
    SetFixed(f64),
    /// Set the position as a proportion of the working area.
    SetProportion(f64),
    /// Add or subtract to the current position in logical pixels.
    AdjustFixed(f64),
    /// Add or subtract to the current position as a proportion of the working area.
    AdjustProportion(f64),
}

/// Workspace reference (id, index or name) to operate on.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum WorkspaceReferenceArg {
    /// Id of the workspace.
    Id(u64),
    /// Index of the workspace.
    Index(u8),
    /// Name of the workspace.
    Name(String),
}

/// Layout to switch to.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum LayoutSwitchTarget {
    /// The next configured layout.
    Next,
    /// The previous configured layout.
    Prev,
    /// The specific layout by index.
    Index(u8),
}

/// How windows display in a column.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ColumnDisplay {
    /// Windows are tiled vertically across the working area height.
    Normal,
    /// Windows are in tabs.
    Tabbed,
}
