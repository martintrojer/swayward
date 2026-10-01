use fixture::Fixture;

mod client;
mod client_protocol_fuzz;
pub(crate) mod fixture;
mod server;

mod border_resize;
mod ext_workspace;
mod floating;
mod floating_sway;
mod foreign_toplevel;
mod fullscreen;
mod gamma_control;
mod i3_conformance;
mod ipc;
mod layer_shell;
mod output_management;
mod remove_output;
mod screencopy;
mod session_lock;
mod tiling_clip;
mod toplevel_lifecycle;
mod virtual_pointer;
mod window_opening;
mod window_rules;
mod windows;
mod xdg_shell;
