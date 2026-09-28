use super::*;

impl XdgToplevelTagHandler for State {
    fn set_tag(&mut self, toplevel: xdg_toplevel::XdgToplevel, _tag: String) {
        let Some(toplevel) = self.swayward.xdg_shell_state.get_toplevel(&toplevel) else {
            return;
        };
        self.update_window_rules(&toplevel);
        let id = self
            .swayward
            .layout
            .find_window_and_output(toplevel.wl_surface())
            .map(|(mapped, _)| mapped.id());
        if let Some(id) = id {
            crate::command::run_for_window(self, id);
        }
    }
}

impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: zxdg_toplevel_decoration_v1::Mode) {
        // Match sway: tiled windows always use server-side decorations, while floating windows
        // honour the client's requested mode (sway/xdg_decoration.c:64-90).
        let mode = if self.window_is_or_will_be_floating(&toplevel) {
            mode
        } else {
            zxdg_toplevel_decoration_v1::Mode::ServerSide
        };
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(mode);
        });

        // A configure is required in response to this event. However, if an initial configure
        // wasn't sent, then we will send this as part of the initial configure later.
        if toplevel.is_initial_configure_sent() {
            // If this is a mapped window, flag it as needs configure to avoid duplicate configures.
            let surface = toplevel.wl_surface();
            if let Some((mapped, _)) = self.swayward.layout.find_window_and_output_mut(surface) {
                mapped.set_needs_configure();
            } else {
                toplevel.send_configure();
            }
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });

        // A configure is required in response to this event. However, if an initial configure
        // wasn't sent, then we will send this as part of the initial configure later.
        if toplevel.is_initial_configure_sent() {
            // If this is a mapped window, flag it as needs configure to avoid duplicate configures.
            let surface = toplevel.wl_surface();
            if let Some((mapped, _)) = self.swayward.layout.find_window_and_output_mut(surface) {
                mapped.set_needs_configure();
            } else {
                toplevel.send_configure();
            }
        }
    }
}

/// Whether KDE server decorations are in use.
#[derive(Default, Clone)]
pub struct KdeDecorationsModeState {
    server: Cell<bool>,
}

impl KdeDecorationsModeState {
    pub fn is_server(&self) -> bool {
        self.server.get()
    }
}

impl KdeDecorationHandler for State {
    fn kde_decoration_state(&self) -> &KdeDecorationState {
        &self.swayward.kde_decoration_state
    }

    fn request_mode(
        &mut self,
        surface: &WlSurface,
        decoration: &org_kde_kwin_server_decoration::OrgKdeKwinServerDecoration,
        mode: wayland_server::WEnum<org_kde_kwin_server_decoration::Mode>,
    ) {
        let WEnum::Value(mode) = mode else {
            return;
        };

        decoration.mode(mode);

        with_states(surface, |states| {
            let state = states
                .data_map
                .get_or_insert(KdeDecorationsModeState::default);
            state
                .server
                .set(mode == org_kde_kwin_server_decoration::Mode::Server);
        });
    }
}

impl XdgForeignHandler for State {
    fn xdg_foreign_state(&mut self) -> &mut XdgForeignState {
        &mut self.swayward.xdg_foreign_state
    }
}
