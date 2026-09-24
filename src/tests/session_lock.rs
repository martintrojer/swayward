use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use smithay::reexports::wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_surface_v1::{self, ExtSessionLockSurfaceV1},
    ext_session_lock_v1::ExtSessionLockV1,
};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

use super::client::{ClientId, State};
use super::Fixture;
use crate::swayward::{LockRenderState, RedrawState};

type LockSurfaceConfigure = Arc<Mutex<Option<(u32, u32, u32)>>>;

impl Dispatch<ExtSessionLockSurfaceV1, LockSurfaceConfigure> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        configure: &LockSurfaceConfigure,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            *configure.lock().unwrap() = Some((serial, width, height));
        }
    }
}

fn begin_lock(fixture: &mut Fixture, client: ClientId) -> ExtSessionLockV1 {
    let client = fixture.client(client);
    client
        .state
        .session_lock_manager
        .as_ref()
        .unwrap()
        .lock(&client.qh, ())
}

fn add_lock_surface(
    fixture: &mut Fixture,
    client: ClientId,
    lock: &ExtSessionLockV1,
) -> (ExtSessionLockSurfaceV1, WlSurface, LockSurfaceConfigure) {
    let client = fixture.client(client);
    let surface = client
        .state
        .compositor
        .as_ref()
        .unwrap()
        .create_surface(&client.qh, ());
    let output = client.state.outputs.keys().next().unwrap().clone();
    let configure = LockSurfaceConfigure::default();
    let lock_surface = lock.get_lock_surface(&surface, &output, &client.qh, configure.clone());
    client.connection.flush().unwrap();
    (lock_surface, surface, configure)
}

fn commit_lock_surface(
    fixture: &mut Fixture,
    client: ClientId,
    lock_surface: &ExtSessionLockSurfaceV1,
    surface: &WlSurface,
    configure: &LockSurfaceConfigure,
) {
    let (serial, width, height) = configure.lock().unwrap().take().unwrap();
    let client = fixture.client(client);
    lock_surface.ack_configure(serial);
    let viewport = client
        .state
        .viewporter
        .as_ref()
        .unwrap()
        .get_viewport(surface, &client.qh, ());
    viewport.set_destination(width as i32, height as i32);
    let buffer = client.state.spbm.as_ref().unwrap().create_u32_rgba_buffer(
        0,
        0,
        0,
        u32::MAX,
        &client.qh,
        (),
    );
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, width as i32, height as i32);
    surface.commit();
    client.connection.flush().unwrap();
}

fn dispatch_for(fixture: &mut Fixture, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        fixture
            .state
            .server
            .event_loop
            .dispatch(Duration::from_millis(10), &mut fixture.state.server.state)
            .unwrap();
        fixture.state.server.state.refresh_and_flush_clients();
        fixture.dispatch();
    }
}

fn wait_for_locked(fixture: &mut Fixture, client: ClientId) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !fixture.client(client).state.session_locked && Instant::now() < deadline {
        dispatch_for(fixture, Duration::from_millis(10));
    }
    assert!(fixture.client(client).state.session_locked);
}

#[test]
fn a_committed_lock_surface_is_confirmed_before_the_deadline() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let lock = begin_lock(&mut fixture, client);
    let (lock_surface, surface, configure) = add_lock_surface(&mut fixture, client, &lock);
    fixture.roundtrip(client);
    commit_lock_surface(&mut fixture, client, &lock_surface, &surface, &configure);

    wait_for_locked(&mut fixture, client);
    lock.unlock_and_destroy();
}

#[test]
fn missing_and_late_lock_surface_commits_fall_back_to_the_secure_background() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let lock = begin_lock(&mut fixture, client);
    let (lock_surface, surface, configure) = add_lock_surface(&mut fixture, client, &lock);
    fixture.roundtrip(client);

    wait_for_locked(&mut fixture, client);
    commit_lock_surface(&mut fixture, client, &lock_surface, &surface, &configure);
    dispatch_for(&mut fixture, Duration::from_millis(20));
    assert!(fixture.client(client).state.session_locked);
    lock.unlock_and_destroy();
}

#[test]
fn removing_the_only_output_during_lock_wait_confirms_and_readded_output_stays_locked() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let lock = begin_lock(&mut fixture, client);
    fixture.roundtrip(client);

    let output = fixture.niri_output(1);
    fixture.swayward().remove_output(&output);
    wait_for_locked(&mut fixture, client);
    fixture.add_output(1, (800, 600));
    dispatch_for(&mut fixture, Duration::from_millis(20));
    assert!(fixture.client(client).state.session_locked);
    assert!(
        fixture
            .swayward()
            .output_state
            .values()
            .next()
            .unwrap()
            .lock_render_state
            == LockRenderState::Locked
    );
    lock.unlock_and_destroy();
}

#[test]
fn inactive_outputs_are_securely_confirmed_without_a_framebuffer() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let lock = begin_lock(&mut fixture, client);
    fixture.roundtrip(client);
    {
        let state = fixture.niri_state();
        state.swayward.deactivate_monitors(&mut state.backend);
    }

    wait_for_locked(&mut fixture, client);
    lock.unlock_and_destroy();
}

#[test]
fn lock_watchdog_never_queues_over_an_in_flight_frame() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    fixture
        .swayward()
        .output_state
        .values_mut()
        .next()
        .unwrap()
        .redraw_state = RedrawState::WaitingForVBlank {
        redraw_needed: false,
    };
    let client = fixture.add_client();
    let lock = begin_lock(&mut fixture, client);
    fixture.roundtrip(client);

    let deadline = Instant::now() + Duration::from_millis(1_250);
    while Instant::now() < deadline {
        fixture
            .state
            .server
            .event_loop
            .dispatch(Duration::from_millis(10), &mut fixture.state.server.state)
            .unwrap();
    }

    assert!(matches!(
        fixture
            .swayward()
            .output_state
            .values()
            .next()
            .unwrap()
            .redraw_state,
        RedrawState::WaitingForVBlank {
            redraw_needed: true
        }
    ));
    lock.destroy();
}

#[test]
fn a_skipped_lock_redraw_is_retried_until_the_client_is_confirmed() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let lock = begin_lock(&mut fixture, client);
    fixture.roundtrip(client);
    fixture
        .state
        .server
        .state
        .backend
        .headless()
        .skip_next_render();
    wait_for_locked(&mut fixture, client);
    lock.unlock_and_destroy();
}
