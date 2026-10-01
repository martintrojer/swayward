use serde::{Deserialize, Serialize};

/// A rectangle in the global compositor coordinate space.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Root,
    Output,
    Workspace,
    Con,
    FloatingCon,
}

#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeLayout {
    None,
    #[serde(rename = "splith")]
    SplitH,
    #[serde(rename = "splitv")]
    SplitV,
    Stacked,
    Tabbed,
    Output,
    Dockarea,
}

#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeBorder {
    Normal,
    None,
    Pixel,
    Csd,
}

/// A node returned by `GET_TREE`.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub border: NodeBorder,
    pub current_border_width: i32,
    pub deco_rect: Rect,
    pub floating: Option<String>,
    pub floating_nodes: Vec<Node>,
    pub focus: Vec<i64>,
    pub focused: bool,
    pub fullscreen_mode: i32,
    pub geometry: Rect,
    pub id: i64,
    pub layout: NodeLayout,
    pub marks: Vec<String>,
    pub name: Option<String>,
    pub nodes: Vec<Node>,
    pub orientation: String,
    pub percent: Option<f64>,
    pub rect: Rect,
    pub scratchpad_state: Option<String>,
    pub sticky: bool,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub urgent: bool,
    pub window: Option<i64>,
    pub window_rect: Rect,
    #[serde(flatten)]
    pub properties: NodeProperties,
}

/// Fields which vary between sway tree node kinds.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NodeProperties {
    View(ViewProperties),
    Output(OutputProperties),
    Workspace(WorkspaceProperties),
    None {},
}

#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewProperties {
    pub allow_tearing: bool,
    pub app_id: Option<String>,
    pub foreign_toplevel_identifier: Option<String>,
    pub idle_inhibitors: IdleInhibitors,
    pub inhibit_idle: bool,
    pub max_render_time: i32,
    pub pid: Option<i64>,
    pub sandbox_app_id: Option<String>,
    pub sandbox_engine: Option<String>,
    pub sandbox_instance_id: Option<String>,
    pub shell: Option<String>,
    pub tag: Option<String>,
    pub visible: bool,
}

#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdleInhibitors {
    pub application: String,
    pub user: String,
}

#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceProperties {
    pub num: i32,
    pub output: String,
    pub representation: Option<String>,
}

#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputFeatures {
    pub adaptive_sync: bool,
    pub hdr: bool,
}

#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputProperties {
    pub active: bool,
    pub adaptive_sync_status: String,
    pub allow_tearing: bool,
    pub current_mode: OutputMode,
    pub current_workspace: Option<String>,
    pub dpms: bool,
    pub features: OutputFeatures,
    pub hdr: bool,
    pub make: String,
    pub max_render_time: i32,
    pub model: String,
    pub modes: Vec<OutputMode>,
    pub non_desktop: bool,
    pub power: bool,
    pub primary: bool,
    pub scale: f64,
    pub scale_filter: String,
    pub serial: String,
    pub transform: String,
}

/// A workspace returned by `GET_WORKSPACES`.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub border: NodeBorder,
    pub current_border_width: i32,
    pub deco_rect: Rect,
    pub floating: Option<String>,
    pub floating_nodes: Vec<Node>,
    pub focus: Vec<i64>,
    pub focused: bool,
    pub fullscreen_mode: i32,
    pub geometry: Rect,
    pub id: i64,
    pub layout: NodeLayout,
    pub marks: Vec<String>,
    pub name: String,
    pub nodes: Vec<Node>,
    pub num: i32,
    pub orientation: String,
    pub output: String,
    pub percent: Option<f64>,
    pub rect: Rect,
    pub representation: Option<String>,
    pub scratchpad_state: Option<String>,
    pub sticky: bool,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub urgent: bool,
    pub visible: bool,
    pub window: Option<i64>,
    pub window_rect: Rect,
}

/// A mode advertised by an output.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputMode {
    pub width: i32,
    pub height: i32,
    pub refresh: i32,
}

/// An output returned by `GET_OUTPUTS`.
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub active: bool,
    pub adaptive_sync_status: String,
    pub allow_tearing: bool,
    pub border: NodeBorder,
    pub current_border_width: i32,
    pub current_mode: OutputMode,
    pub current_workspace: Option<String>,
    pub deco_rect: Rect,
    pub dpms: bool,
    pub features: OutputFeatures,
    pub floating: Option<String>,
    pub floating_nodes: Vec<Node>,
    pub focus: Vec<i64>,
    pub focused: bool,
    pub fullscreen_mode: i32,
    pub geometry: Rect,
    pub hdr: bool,
    pub id: i64,
    pub layout: NodeLayout,
    pub make: String,
    pub marks: Vec<String>,
    pub max_render_time: i32,
    pub model: String,
    pub modes: Vec<OutputMode>,
    pub name: String,
    pub nodes: Vec<Node>,
    pub non_desktop: bool,
    pub orientation: String,
    pub percent: Option<f64>,
    pub power: bool,
    pub primary: bool,
    pub rect: Rect,
    pub scale: f64,
    pub scale_filter: String,
    pub scratchpad_state: Option<String>,
    pub serial: String,
    pub sticky: bool,
    pub subpixel_hinting: String,
    pub transform: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub urgent: bool,
    pub window: Option<i64>,
    pub window_rect: Rect,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENARIOS: &[&str] = &[
        "empty",
        "empty_named",
        "fullscreen",
        "marked",
        "named_workspace",
        "nested_h_in_v",
        "numbered_sparse",
        "one_floating",
        "two_floating",
        "three_floating_before_raise",
        "three_floating_after_raise",
        "one_window",
        "stacked",
        "tabbed",
        "two_split_h",
        "two_split_v",
        "two_workspaces",
    ];

    fn fixture(scenario: &str, kind: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.cache/sway-ipc-oracle/sway-ipc/fixtures")
            .join(format!("{scenario}.{kind}.json"));
        std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "cannot read sway IPC oracle fixture {}: {error}; run ./contrib/fetch-oracle",
                path.display()
            )
        })
    }

    fn assert_round_trip<T>(scenario: &str, kind: &str, json: &str)
    where
        T: serde::de::DeserializeOwned + Serialize,
    {
        let original: serde_json::Value = serde_json::from_str(json).unwrap();
        let parsed: T = serde_json::from_value(original.clone())
            .unwrap_or_else(|error| panic!("failed to parse {scenario}.{kind}.json: {error}"));
        let reserialised = serde_json::to_value(parsed).unwrap();
        assert_eq!(
            original, reserialised,
            "schema drift in {scenario}.{kind}.json"
        );
    }

    #[test]
    fn round_trips_all_real_sway_fixtures_without_schema_drift() {
        for &scenario in SCENARIOS {
            assert_round_trip::<Node>(scenario, "tree", &fixture(scenario, "tree"));
            assert_round_trip::<Vec<Workspace>>(
                scenario,
                "workspaces",
                &fixture(scenario, "workspaces"),
            );
            assert_round_trip::<Vec<Output>>(scenario, "outputs", &fixture(scenario, "outputs"));
        }
    }
}
