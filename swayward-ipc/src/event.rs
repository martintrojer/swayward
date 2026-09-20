use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A sway IPC event payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Event {
    Workspace(WorkspaceEvent),
    Window(WindowEvent),
    Mode(ModeEvent),
    BarconfigUpdate(BarconfigUpdateEvent),
    Binding(BindingEvent),
    Shutdown(ShutdownEvent),
    Tick(TickEvent),
    BarStateUpdate(BarStateUpdateEvent),
    Input(InputEvent),
}

macro_rules! event_payload {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Value);
    )+};
}

event_payload!(
    WorkspaceEvent,
    WindowEvent,
    ModeEvent,
    BarconfigUpdateEvent,
    BindingEvent,
    ShutdownEvent,
    TickEvent,
    BarStateUpdateEvent,
    InputEvent,
);
