use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel;

use super::*;

#[test]
fn commits_before_mapping_and_after_unmapping_keep_wayland_role_invariants() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();

    // Repeated pre-map commits must remain in the Unmapped path, whose Window was constructed
    // from this xdg-toplevel and therefore has a Wayland toplevel role.
    window.commit();
    window.commit();
    f.double_roundtrip(id);
    assert_eq!(f.swayward().unmapped_windows.len(), 1);
    assert!(f
        .swayward()
        .unmapped_windows
        .values()
        .all(|unmapped| unmapped.window.toplevel().is_some()));

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    assert!(f.swayward().unmapped_windows.is_empty());
    assert_eq!(f.swayward().layout.windows().count(), 1);
    assert!(f.swayward().layout.windows().all(|(_, mapped)| mapped
        .window
        .toplevel()
        .is_some_and(|toplevel| toplevel.alive())));

    let window = f.client(id).window(&surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(id);
    assert!(f.swayward().layout.windows().next().is_none());
    assert_eq!(f.swayward().unmapped_windows.len(), 1);

    // A later commit still takes the unmapped path and can issue a fresh initial configure.
    f.client(id).window(&surface).commit();
    f.double_roundtrip(id);
    assert_eq!(f.swayward().unmapped_windows.len(), 1);
}

#[test]
fn commit_after_unmapped_toplevel_role_is_destroyed_is_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.double_roundtrip(id);
    assert_eq!(f.swayward().unmapped_windows.len(), 1);

    f.client(id).window(&surface).destroy_role();
    f.roundtrip(id);
    assert!(f.swayward().unmapped_windows.is_empty());

    f.client(id).window(&surface).commit();
    f.double_roundtrip(id);
    assert!(f.swayward().layout.windows().next().is_none());
    assert!(f.swayward().unmapped_windows.is_empty());
}

#[test]
fn commit_after_mapped_toplevel_role_is_destroyed_is_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    assert_eq!(f.swayward().layout.windows().count(), 1);

    f.client(id).window(&surface).destroy_role();
    f.roundtrip(id);
    assert!(f.swayward().layout.windows().next().is_none());

    f.client(id).window(&surface).commit();
    f.double_roundtrip(id);
    assert!(f.swayward().layout.windows().next().is_none());
    assert!(f.swayward().unmapped_windows.is_empty());
}

#[test]
fn focusing_a_window_deactivates_the_previous_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let first = f.client(id).create_window();
    let first_surface = first.surface.clone();
    first.commit();
    f.roundtrip(id);
    let first = f.client(id).window(&first_surface);
    first.attach_new_buffer();
    first.ack_last_and_commit();
    f.double_roundtrip(id);
    let _ = f.client(id).window(&first_surface).recent_configures();

    let second = f.client(id).create_window();
    let second_surface = second.surface.clone();
    second.commit();
    f.roundtrip(id);
    let second = f.client(id).window(&second_surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(id);

    let last_states = |f: &mut Fixture, surface| {
        f.client(id)
            .window(surface)
            .configures_received
            .last()
            .unwrap()
            .1
            .states
            .clone()
    };
    assert!(!last_states(&mut f, &first_surface).contains(&xdg_toplevel::State::Activated));
    assert!(last_states(&mut f, &second_surface).contains(&xdg_toplevel::State::Activated));
}
