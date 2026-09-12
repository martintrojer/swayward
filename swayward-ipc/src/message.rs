use serde::{Deserialize, Serialize};

/// The response to `GET_VERSION`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub human_readable: String,
    pub variant: String,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub loaded_config_file_name: String,
}

/// One result in a `RUN_COMMAND` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutcome {
    pub success: bool,
    pub error: Option<String>,
    pub parse_error: Option<bool>,
}

/// Sway IPC request and reply message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum MessageType {
    RunCommand = 0,
    GetWorkspaces = 1,
    Subscribe = 2,
    GetOutputs = 3,
    GetTree = 4,
    GetMarks = 5,
    GetBarConfig = 6,
    GetVersion = 7,
    GetBindingModes = 8,
    GetConfig = 9,
    SendTick = 10,
    GetBindingState = 12,
    GetInputs = 100,
    GetSeats = 101,
}

impl TryFrom<u32> for MessageType {
    type Error = u32;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::RunCommand,
            1 => Self::GetWorkspaces,
            2 => Self::Subscribe,
            3 => Self::GetOutputs,
            4 => Self::GetTree,
            5 => Self::GetMarks,
            6 => Self::GetBarConfig,
            7 => Self::GetVersion,
            8 => Self::GetBindingModes,
            9 => Self::GetConfig,
            10 => Self::SendTick,
            12 => Self::GetBindingState,
            100 => Self::GetInputs,
            101 => Self::GetSeats,
            value => return Err(value),
        })
    }
}
