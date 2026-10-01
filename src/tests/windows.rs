use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use wayland_client::protocol::wl_surface::WlSurface;

use super::client::ClientId;
use super::Fixture;

#[derive(Default)]
pub(super) struct WindowSpec<'a> {
    pub app_id: Option<&'a str>,
    pub title: Option<&'a str>,
    pub size: Option<(u16, u16)>,
    pub parent: Option<&'a XdgToplevel>,
    pub min_size: Option<(i32, i32)>,
    pub max_size: Option<(i32, i32)>,
}

impl<'a> WindowSpec<'a> {
    pub fn titled(title: &'a str) -> Self {
        Self {
            title: Some(title),
            ..Default::default()
        }
    }

    pub fn sized(width: u16, height: u16) -> Self {
        Self {
            size: Some((width, height)),
            ..Default::default()
        }
    }

    pub fn titled_size(title: &'a str, width: u16, height: u16) -> Self {
        Self {
            title: Some(title),
            size: Some((width, height)),
            ..Default::default()
        }
    }
}

pub(super) fn map_window(
    fixture: &mut Fixture,
    client: ClientId,
    spec: WindowSpec<'_>,
) -> WlSurface {
    let window = fixture.client(client).create_window();
    let surface = window.surface.clone();
    if let Some(app_id) = spec.app_id {
        window.xdg_toplevel.set_app_id(app_id.to_owned());
    }
    if let Some(title) = spec.title {
        window.set_title(title);
    }
    if let Some(parent) = spec.parent {
        window.set_parent(Some(parent));
    }
    if let Some((width, height)) = spec.min_size {
        window.set_min_size(width, height);
    }
    if let Some((width, height)) = spec.max_size {
        window.set_max_size(width, height);
    }
    window.commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    if let Some((width, height)) = spec.size {
        window.set_size(width, height);
    }
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    surface
}
