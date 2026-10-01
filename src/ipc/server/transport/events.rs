use super::*;

#[repr(u32)]
pub(super) enum SwayEventType {
    Workspace = 0x8000_0000,
    Output = 0x8000_0001,
    Mode = 0x8000_0002,
    Window = 0x8000_0003,
    Binding = 0x8000_0005,
    Shutdown = 0x8000_0006,
    Tick = 0x8000_0007,
    Input = 0x8000_0015,
}

impl SwayEventType {
    pub(super) fn subscription_name(&self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Output => "output",
            Self::Mode => "mode",
            Self::Window => "window",
            Self::Binding => "binding",
            Self::Shutdown => "shutdown",
            Self::Tick => "tick",
            Self::Input => "input",
        }
    }
}

pub(super) fn sway_event(
    event: Event,
    query_state: &QueryState,
) -> Option<(SwayEventType, serde_json::Value)> {
    Some(match event {
        Event::OutputChanged => (
            SwayEventType::Output,
            serde_json::json!({"change":"unspecified"}),
        ),
        Event::SwayInputChanged { change, input } => (
            SwayEventType::Input,
            serde_json::json!({"change":change,"input":input}),
        ),
        Event::WorkspaceEmptied { current } => (
            SwayEventType::Workspace,
            serde_json::json!({"change":"empty","old":null,"current":current}),
        ),
        Event::WorkspaceReloaded => (
            SwayEventType::Workspace,
            serde_json::json!({"change":"reload","old":null,"current":null}),
        ),
        Event::WorkspaceInitialized { current } => (
            SwayEventType::Workspace,
            serde_json::json!({"change":"init","old":null,"current":current}),
        ),
        Event::WorkspaceRenamed { current } => (
            SwayEventType::Workspace,
            serde_json::json!({"change":"rename","old":null,"current":current}),
        ),
        Event::WorkspaceMoved { current } => (
            SwayEventType::Workspace,
            serde_json::json!({"change":"move","old":null,"current":current}),
        ),
        Event::WorkspaceUrgencyChanged { current, .. } => (
            SwayEventType::Workspace,
            serde_json::json!({"change":"urgent","old":null,"current":current}),
        ),
        Event::WorkspaceFocusChanged { old, current } => (
            SwayEventType::Workspace,
            serde_json::json!({"change":"focus","old":old,"current":current}),
        ),
        Event::WorkspaceActiveWindowChanged { .. }
        | Event::WorkspacesChanged { .. }
        | Event::WorkspaceActivated { .. } => return None,
        Event::Shutdown { reason } => (
            SwayEventType::Shutdown,
            serde_json::json!({"change":reason}),
        ),
        Event::Tick { payload, first } => (
            SwayEventType::Tick,
            serde_json::json!({"first":first,"payload":payload}),
        ),
        Event::BindingModeChanged { mode, pango_markup } => (
            SwayEventType::Mode,
            serde_json::json!({"change":mode,"pango_markup":pango_markup}),
        ),
        Event::SwayBinding {
            command,
            event_state_mask,
            input_codes,
            input_code,
            symbols,
            symbol,
            input_type,
        } => (
            SwayEventType::Binding,
            serde_json::json!({
                "change":"run",
                "binding": {
                    "command": command,
                    "event_state_mask": event_state_mask,
                    "input_codes": input_codes,
                    "input_code": input_code,
                    "symbols": symbols,
                    "symbol": symbol,
                    "input_type": input_type,
                }
            }),
        ),
        Event::SwayWindowChanged { change, container } => (
            SwayEventType::Window,
            serde_json::json!({"change":change,"container":container}),
        ),
        Event::WindowMoved { id } => {
            let container = serde_json::from_str::<serde_json::Value>(&query_state.tree)
                .ok()
                .and_then(|tree| find_node_by_id(&tree, id).cloned());
            (
                SwayEventType::Window,
                serde_json::json!({"change":"move","container":container}),
            )
        }
        Event::WindowsChanged { .. }
        | Event::WindowOpenedOrChanged { .. }
        | Event::WindowClosed { .. }
        | Event::WindowFocusTimestampChanged { .. }
        | Event::WindowUrgencyChanged { .. }
        | Event::WindowLayoutsChanged { .. }
        | Event::WindowFocusChanged { .. } => return None,
        _ => return None,
    })
}
