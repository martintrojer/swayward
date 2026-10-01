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
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NodeProperties {
    View(ViewProperties),
    Output(OutputProperties),
    Workspace(WorkspaceProperties),
    None {},
}

/// Keys that only a view, output or workspace node carries. A node holding
/// any of them is one of those kinds, so it must not fall back to
/// `NodeProperties::None` when one of its fields is missing.
const KIND_KEYS: &[&str] = &[
    "app_id",
    "pid",
    "shell",
    "visible",
    "idle_inhibitors",
    "foreign_toplevel_identifier",
    "active",
    "current_mode",
    "modes",
    "scale",
    "transform",
    "num",
    "representation",
];

impl<'de> Deserialize<'de> for NodeProperties {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;

        let value = serde_json::Value::deserialize(deserializer)?;
        if let Ok(view) = ViewProperties::deserialize(&value) {
            return Ok(Self::View(view));
        }
        if let Ok(output) = OutputProperties::deserialize(&value) {
            return Ok(Self::Output(output));
        }
        if let Ok(workspace) = WorkspaceProperties::deserialize(&value) {
            return Ok(Self::Workspace(workspace));
        }
        match KIND_KEYS.iter().find(|key| value.get(**key).is_some()) {
            Some(key) => Err(D::Error::custom(format!(
                "node has `{key}` but not the full view, output or workspace field set"
            ))),
            None => Ok(Self::None {}),
        }
    }
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

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.cache/sway-ipc-oracle/sway-ipc/fixtures")
    }

    /// Every scenario that has a captured `<scenario>.tree.json`. The pinned
    /// oracle has 55; requiring a floor makes an empty or partial cache fail
    /// instead of passing vacuously.
    fn scenarios() -> Vec<String> {
        let dir = fixtures_dir();
        let mut scenarios = std::fs::read_dir(&dir)
            .unwrap_or_else(|error| {
                panic!(
                    "cannot list sway IPC oracle fixtures in {}: {error}; run ./contrib/fetch-oracle",
                    dir.display()
                )
            })
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".tree.json").map(str::to_owned)
            })
            .collect::<Vec<_>>();
        scenarios.sort();
        assert!(
            scenarios.len() >= 50,
            "only {} tree fixtures in {}; run ./contrib/fetch-oracle",
            scenarios.len(),
            dir.display()
        );
        scenarios
    }

    fn fixture(scenario: &str, kind: &str) -> String {
        let path = fixtures_dir().join(format!("{scenario}.{kind}.json"));
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
        for scenario in scenarios() {
            let scenario = scenario.as_str();
            assert_round_trip::<Node>(scenario, "tree", &fixture(scenario, "tree"));
            assert_round_trip::<Vec<Workspace>>(
                scenario,
                "workspaces",
                &fixture(scenario, "workspaces"),
            );
            assert_outputs_round_trip(scenario, &fixture(scenario, "outputs"));
            assert_round_trip::<crate::Version>(scenario, "version", &fixture(scenario, "version"));
            let commands: Vec<serde_json::Value> =
                serde_json::from_str(&fixture(scenario, "commands")).unwrap();
            for command in commands {
                assert_round_trip::<Vec<crate::CommandOutcome>>(
                    scenario,
                    "commands",
                    &command["reply"].to_string(),
                );
            }
        }
    }

    /// The fields sway serialises for a disabled output: the common output
    /// fields plus `percent: null` and an empty rect, without the
    /// active-output block (`ipc_json_describe_disabled_output`,
    /// sway/sway/ipc-json.c:415-442). swayward builds that entry as raw JSON,
    /// so the typed [`Output`] covers only active outputs.
    const DISABLED_OUTPUT_KEYS: &[&str] = &[
        "active",
        "current_workspace",
        "dpms",
        "features",
        "make",
        "model",
        "modes",
        "name",
        "non_desktop",
        "percent",
        "power",
        "primary",
        "rect",
        "serial",
        "type",
    ];

    fn assert_outputs_round_trip(scenario: &str, json: &str) {
        let outputs: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
        for output in outputs {
            if output["active"] == false {
                let mut keys = output
                    .as_object()
                    .map(|object| object.keys().map(String::as_str).collect::<Vec<_>>())
                    .unwrap_or_default();
                keys.sort_unstable();
                assert_eq!(
                    keys, DISABLED_OUTPUT_KEYS,
                    "disabled output shape in {scenario}.outputs.json"
                );
            } else {
                assert_round_trip::<Output>(scenario, "outputs", &output.to_string());
            }
        }
    }

    /// `NodeProperties` is untagged with `None {}` last, so a view node that
    /// lost one view field must not quietly decode as a property-less node and
    /// re-serialise without its view fields.
    #[test]
    fn a_view_missing_a_view_field_does_not_decode_as_a_plain_node() {
        /// JSON pointer to the first view node.
        fn first_view(node: &serde_json::Value, path: String) -> Option<String> {
            if node.get("app_id").is_some() {
                return Some(path);
            }
            ["nodes", "floating_nodes"].into_iter().find_map(|key| {
                node.get(key)?
                    .as_array()?
                    .iter()
                    .enumerate()
                    .find_map(|(index, child)| first_view(child, format!("{path}/{key}/{index}")))
            })
        }
        let mut tree: serde_json::Value =
            serde_json::from_str(&fixture("one_window", "tree")).unwrap();
        let pointer = first_view(&tree, String::new()).expect("one_window has a view");
        let view = tree.pointer_mut(&pointer).unwrap();
        view.as_object_mut().unwrap().remove("app_id");
        let decoded = serde_json::from_value::<Node>(view.clone());
        assert!(
            !matches!(
                decoded,
                Ok(Node {
                    properties: NodeProperties::None {},
                    ..
                })
            ),
            "a view without app_id decoded as a plain node: {decoded:?}"
        );
    }
}
