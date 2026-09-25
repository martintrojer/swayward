#[test]
fn popup_during_fullscreen_policies_use_xdg_toplevel_parent() {
    assert_eq!(
        fullscreen_parent_after_child_map(swayward_config::PopupDuringFullscreen::Smart),
        (true, true)
    );
    assert_eq!(
        fullscreen_parent_after_child_map(swayward_config::PopupDuringFullscreen::Ignore),
        (true, false)
    );
    assert_eq!(
        fullscreen_parent_after_child_map(swayward_config::PopupDuringFullscreen::LeaveFullscreen),
        (false, false)
    );
}

#[test]
fn dialog_placement_uses_parent_layout_position_during_animation() {
    assert_eq!(
        dialog_rect_after_parent_move(false),
        dialog_rect_after_parent_move(true)
    );
}

#[test]
fn disabled_focus_follows_mouse_keeps_focus_when_pointer_crosses_outputs() {
    let mut f = Fixture::new();
    f.add_output(1, (1024, 768));
    f.add_output(2, (1024, 768));
    let client = f.add_client();

    f.niri_focus_output(2);
    let focused = f.client(client).create_window();
    focused.commit();
    let surface = focused.surface.clone();
    f.roundtrip(client);
    let focused = f.client(client).window(&surface);
    focused.attach_new_buffer();
    focused.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused_id = f.swayward().layout.focus().unwrap().id();

    let location = (500., 0.).into();
    let under = f.swayward().contents_under(location);
    f.swayward().handle_focus_follows_mouse(&under);
    f.niri_state().move_cursor(location);

    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused_id);
    let active = f.swayward().layout.active_output().unwrap().clone();
    assert_eq!(active, f.niri_output(2));
    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        location
    );
}

/// Focus a second window by command with the pointer parked over the first,
/// then re-run the focus-follows-mouse hook without moving the pointer.
///
/// Sway's `yes` leaves focus alone because the hovered window did not change,
/// while `always` pulls focus back under the pointer
/// (`sway/sway/input/seatop_default.c:590-598`). Returns whether focus
/// returned to the hovered window.
fn focus_returns_under_stationary_pointer(
    mode: swayward_config::input::FocusFollowsMouseMode,
) -> bool {
    let mut config = swayward_config::Config::default();
    config.input.focus_follows_mouse = Some(swayward_config::input::FocusFollowsMouse {
        mode,
        max_scroll_amount: None,
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();

    // Two tiled windows side by side, so a point exists over each.
    let mut ids = Vec::new();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        // Commit the size the compositor asked for, so the surface has a
        // real hit-testable region.
        let size = window.configures_received.last().unwrap().1.size;
        window.set_size(size.0 as u16, size.1 as u16);
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let focus = f.swayward().layout.focus().unwrap();
        ids.push((focus.id(), focus.window.clone()));
    }
    let (first, first_window) = ids[0].clone();
    let (second, second_window) = ids[1].clone();

    // Find a point the compositor itself reports as over the first window,
    // rather than assuming a tiling geometry.
    let location = (0..128)
        .map(|step| {
            smithay::utils::Point::<f64, smithay::utils::Logical>::from((
                f64::from(step) * 10. + 5.,
                400.,
            ))
        })
        .find(|&point| {
            f.swayward()
                .contents_under(point)
                .window
                .is_some_and(|(window, _)| window == first_window)
        })
        .expect("no point over the first window");

    // Park the pointer over the first window; it becomes the hovered node.
    f.niri_state().move_cursor(location);

    // Move focus to the other window for a reason unrelated to the pointer,
    // which stays exactly where it is. This is sway's "focus got moved due to,
    // say, a workspace switch" case.
    f.swayward().layout.activate_window(&second_window);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), second);

    // Re-run the hook with the pointer still over the first window. The
    // hovered node did not change, so only `always` acts.
    let under = f.swayward().contents_under(location);
    f.swayward().handle_focus_follows_mouse(&under);
    f.swayward().layout.focus().unwrap().id() == first
}

#[test]
fn focus_follows_mouse_always_refocuses_the_still_hovered_window() {
    use swayward_config::input::FocusFollowsMouseMode;

    // The two modes must disagree here. If they agreed, the assertion would
    // also pass against an implementation that stored `always` as `yes`.
    assert!(
        !focus_returns_under_stationary_pointer(FocusFollowsMouseMode::Yes),
        "`yes` must not re-focus a window the pointer never left"
    );
    assert!(
        focus_returns_under_stationary_pointer(FocusFollowsMouseMode::Always),
        "`always` must re-focus the hovered window after focus moved away"
    );
}

/// Warp policy applied to a focus change within the output the pointer is
/// already on.
///
/// `WARP_OUTPUT` returns early when the pointer already sits on the focused
/// output, so a same-output focus change does not move it. `WARP_CONTAINER`
/// warps to the focused container regardless
/// (`sway/sway/input/seat.c:1526-1547`). Returns whether the pointer moved.
fn pointer_moves_on_same_output_focus(mode: swayward_config::input::MouseWarping) -> bool {
    let mut config = swayward_config::Config::default();
    config.input.mouse_warping = mode;
    config.layout.gaps = 16.;
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();

    for _ in 0..2 {
        let window = f.client(client).create_window();
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        let size = window.configures_received.last().unwrap().1.size;
        window.set_size(size.0 as u16, size.1 as u16);
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    // Put the pointer somewhere on the focused output that is not the centre
    // of the focused window.
    let start = smithay::utils::Point::<f64, smithay::utils::Logical>::from((5., 5.));
    f.niri_state().move_cursor(start);
    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        start
    );

    // A focus change that stays on this one output.
    assert!(crate::command::execute(f.niri_state(), "focus left")[0].success);
    f.niri_state().maybe_warp_cursor_to_focus();

    f.swayward().seat.get_pointer().unwrap().current_location() != start
}

/// What a modifier-held click on the given button starts.
///
/// Sway derives the move and resize buttons from the inverse bit, so `normal`
/// is left-move/right-resize and `inverse` swaps them
/// (`sway/sway/input/seatop_default.c:359-363`).
#[derive(Debug, PartialEq, Eq)]
enum DragKind {
    Nothing,
    Move,
    Resize,
}

fn drag_started_by(inverse: bool, button: u32) -> DragKind {
    let mut config = swayward_config::Config::default();
    config.input.floating_modifier = Some(swayward_config::input::FloatingModifier {
        modifier: swayward_config::input::ModKey::Super,
        inverse,
    });
    // Float the window: a lone tiled window has no neighbour to take space
    // from, so a tiling resize would refuse and hide the button mapping.
    config.window_rules.push(swayward_config::WindowRule {
        open_floating: Some(true),
        ..Default::default()
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();

    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    // A floating window is configured with a zero size, meaning the client
    // picks, so ask for a concrete one.
    window.set_size(600, 400);
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(600, 400);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    // A resize needs a resize edge under the cursor, so pick a point that is
    // both over a window and on one of its edges. Then the same position can
    // start either gesture and only the button decides which.
    let output = f.niri_output(1);
    let location = (0..1280)
        .map(|step| {
            smithay::utils::Point::<f64, smithay::utils::Logical>::from((f64::from(step), 400.))
        })
        .find(|&point| {
            f.niri_state().move_cursor(point);
            let over_window = f.swayward().window_under_cursor().is_some();
            let on_edge = f
                .swayward()
                .global_space
                .output_geometry(&output)
                .and_then(|geo| {
                    f.swayward()
                        .layout
                        .resize_edges_under(&output, point - geo.loc.to_f64())
                })
                .is_some_and(|edges| !edges.is_empty());
            over_window && on_edge
        })
        .expect("no point both over a window and on a resize edge");

    pointer_motion_absolute(&mut f, location.x, location.y);

    // Hold the floating modifier, then press the button.
    key_event(&mut f, 133, true);
    pointer_button(&mut f, button, true);

    // Inspect the concrete grab the press installed, which names the gesture
    // exactly rather than inferring it.
    let pointer = f.swayward().seat.get_pointer().unwrap();
    let kind = pointer
        .with_grab(|_, grab| {
            if grab.is::<crate::input::move_grab::MoveGrab>() {
                DragKind::Move
            } else if grab.is::<crate::input::resize_grab::ResizeGrab>() {
                DragKind::Resize
            } else {
                DragKind::Nothing
            }
        })
        .unwrap_or(DragKind::Nothing);

    pointer_button(&mut f, button, false);
    key_event(&mut f, 133, false);
    kind
}

#[test]
fn floating_modifier_inverse_swaps_the_move_and_resize_buttons() {
    const LEFT: u32 = 0x110;
    const RIGHT: u32 = 0x111;

    // normal: left moves, right resizes.
    assert_eq!(drag_started_by(false, LEFT), DragKind::Move);
    assert_eq!(drag_started_by(false, RIGHT), DragKind::Resize);
    // inverse: the two swap. If the inverse bit were dropped, these two
    // assertions would match the normal case above and fail.
    assert_eq!(drag_started_by(true, LEFT), DragKind::Resize);
    assert_eq!(drag_started_by(true, RIGHT), DragKind::Move);
}

fn tiled_drag_fixture() -> Fixture {
    let mut config = swayward_config::Config::default();
    config.window_rules.push(swayward_config::WindowRule {
        sway_border: Some(swayward_config::SwayWindowBorderStyle::Normal),
        ..Default::default()
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    let size = window.configures_received.last().unwrap().1.size;
    window.set_size(size.0 as u16, size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    pointer_motion_absolute(&mut f, 640., 400.);
    f
}

fn tiled_titlebar_point(f: &mut Fixture) -> smithay::utils::Point<f64, smithay::utils::Logical> {
    let rect = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiling()
        .titlebar_rects()[0]
        .1;
    smithay::utils::Point::from((rect.loc.x + rect.size.w / 2., rect.loc.y + rect.size.h / 2.))
}

fn move_grab_state(f: &mut Fixture) -> Option<bool> {
    f.swayward()
        .seat
        .get_pointer()
        .unwrap()
        .with_grab(|_, grab| {
            grab.downcast_ref::<crate::input::move_grab::MoveGrab>()
                .map(|grab| grab.is_move())
        })
        .flatten()
}

#[test]
fn tiling_drag_disabled_does_not_start_a_modifier_drag() {
    let mut f = tiled_drag_fixture();
    assert!(crate::command::execute(f.niri_state(), "tiling_drag no")[0].success);

    key_event(&mut f, 133, true);
    pointer_button(&mut f, 0x110, true);

    assert_eq!(move_grab_state(&mut f), None);
}

#[test]
fn tiling_drag_threshold_delays_a_titlebar_move() {
    let mut f = tiled_drag_fixture();
    assert!(crate::command::execute(f.niri_state(), "tiling_drag_threshold 100")[0].success);
    let start = tiled_titlebar_point(&mut f);
    pointer_motion_absolute(&mut f, start.x, start.y);
    pointer_button(&mut f, 0x110, true);
    pointer_motion_absolute(&mut f, start.x + 100., start.y);
    assert_eq!(move_grab_state(&mut f), Some(false));

    pointer_motion_absolute(&mut f, start.x + 101., start.y);
    assert_eq!(move_grab_state(&mut f), Some(true));
}

#[test]
fn tiling_drag_defaults_remain_enabled_with_a_nine_pixel_threshold() {
    let config = swayward_config::Config::default();
    assert!(config.input.tiling_drag);
    assert_eq!(config.input.tiling_drag_threshold, 9);

    let mut f = tiled_drag_fixture();
    let start = tiled_titlebar_point(&mut f);
    pointer_motion_absolute(&mut f, start.x, start.y);
    pointer_button(&mut f, 0x110, true);
    pointer_motion_absolute(&mut f, start.x + 9., start.y);
    assert_eq!(move_grab_state(&mut f), Some(false));
    pointer_motion_absolute(&mut f, start.x + 10., start.y);
    assert_eq!(move_grab_state(&mut f), Some(true));
}

#[test]
fn floating_modifier_none_disables_the_drag() {
    let mut config = swayward_config::Config::default();
    config.input.floating_modifier = Some(swayward_config::input::FloatingModifier {
        modifier: swayward_config::input::ModKey::None,
        inverse: false,
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    let size = window.configures_received.last().unwrap().1.size;
    window.set_size(size.0 as u16, size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    pointer_motion_absolute(&mut f, 640., 400.);
    // Super is held, but `none` means no modifier can arm the drag.
    key_event(&mut f, 133, true);
    pointer_button(&mut f, 0x110, true);
    let pointer = f.swayward().seat.get_pointer().unwrap();
    let dragging = pointer
        .with_grab(|_, grab| {
            grab.is::<crate::input::move_grab::MoveGrab>()
                || grab.is::<crate::input::resize_grab::ResizeGrab>()
        })
        .unwrap_or(false);
    assert!(!dragging, "`floating_modifier none` must not start a drag");
    pointer_button(&mut f, 0x110, false);
    key_event(&mut f, 133, false);
}

#[test]
fn mouse_warping_output_warps_when_focus_crosses_outputs() {
    // The other half of `output`: the pointer is not on the newly focused
    // output, so the early return does not apply and it warps.
    let mut config = swayward_config::Config::default();
    config.input.mouse_warping = swayward_config::input::MouseWarping::Output;
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    f.add_output(2, (1280, 800));
    let client = f.add_client();

    // A window on the second output, with the pointer left on the first.
    f.niri_focus_output(2);
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    let size = window.configures_received.last().unwrap().1.size;
    window.set_size(size.0 as u16, size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let start = smithay::utils::Point::<f64, smithay::utils::Logical>::from((5., 5.));
    f.niri_state().move_cursor(start);
    let first_output = f.niri_output(1);
    let second_output = f.niri_output(2);
    let output_at = |f: &mut Fixture, point| {
        f.swayward()
            .global_space
            .output_under(point)
            .next()
            .cloned()
    };
    assert_eq!(
        output_at(&mut f, start),
        Some(first_output),
        "the pointer should start on the output that is not focused"
    );

    f.niri_state().maybe_warp_cursor_to_focus();

    let moved = f.swayward().seat.get_pointer().unwrap().current_location();
    assert_ne!(moved, start, "`output` must warp across an output boundary");
    assert_eq!(
        output_at(&mut f, moved),
        Some(second_output),
        "the warp must land on the focused output"
    );
}

#[test]
fn mouse_warping_container_warps_within_an_output_and_output_does_not() {
    use swayward_config::input::MouseWarping;

    // The two modes must disagree here, otherwise the assertion would also
    // pass against an implementation that collapsed them to one boolean.
    assert!(
        !pointer_moves_on_same_output_focus(MouseWarping::Output),
        "`output` must leave the pointer alone while it is already on the focused output"
    );
    assert!(
        pointer_moves_on_same_output_focus(MouseWarping::Container),
        "`container` must warp to the focused container on a same-output focus change"
    );
    assert!(
        !pointer_moves_on_same_output_focus(MouseWarping::No),
        "`none` must never warp"
    );
}

#[test]
fn dialog_with_hidden_scratchpad_parent_does_not_panic() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let parent = f.client(client).create_window();
    let parent_surface = parent.surface.clone();
    let parent_toplevel = parent.xdg_toplevel.clone();
    parent.commit();
    f.roundtrip(client);
    let parent = f.client(client).window(&parent_surface);
    parent.attach_new_buffer();
    parent.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);

    let child = f.client(client).create_window();
    child.set_parent(Some(&parent_toplevel));
    let child_surface = child.surface.clone();
    child.commit();
    f.roundtrip(client);
    let child = f.client(client).window(&child_surface);
    child.attach_new_buffer();
    child.ack_last_and_commit();
    f.double_roundtrip(client);

    assert_eq!(f.swayward().layout.windows().count(), 2);
}

#[test]
fn floating_rejects_hidden_scratchpad_window_without_panicking() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hidden".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="hidden"] floating enable"#);
    assert_eq!(outcome.len(), 1);
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Can't change floating on hidden scratchpad container")
    );
}

#[test]
fn resize_rejects_hidden_scratchpad_window_without_panicking() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hidden".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id="hidden"] resize grow width 10 px"#,
    );
    assert_eq!(outcome.len(), 1);
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot resize a hidden scratchpad container")
    );
}

#[test]
fn bare_directional_move_crosses_each_adjacent_output_without_wrapping() {
    let mut f = Fixture::new();
    for (name, position) in [
        ("top-left", (0, 0)),
        ("top-right", (800, 0)),
        ("bottom-right", (800, 600)),
        ("bottom-left", (0, 600)),
    ] {
        f.add_named_output_at(name.into(), (800, 600), Some(position));
        assert!(crate::command::execute(
            f.niri_state(),
            &format!("focus output {name}, workspace {name}-workspace")
        )
        .iter()
        .all(|outcome| outcome.success));
    }

    assert!(crate::command::execute(f.niri_state(), "workspace top-left-workspace")[0].success);
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let window_id = f.swayward().layout.focus().unwrap().id();

    for (command, expected_output) in [
        ("move right", "top-right"),
        ("move down", "bottom-right"),
        ("move left", "bottom-left"),
        ("move up", "top-left"),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(
            f.swayward()
                .layout
                .windows()
                .find(|(_, mapped)| mapped.id() == window_id)
                .unwrap()
                .0
                .unwrap()
                .output_name(),
            expected_output,
            "{command}"
        );
    }
}

#[test]
fn criteria_directional_move_crosses_outputs_without_changing_focus() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    let moved = f.client(client).create_window();
    moved.xdg_toplevel.set_app_id("moved".into());
    moved.commit();
    let surface = moved.surface.clone();
    f.roundtrip(client);
    let moved = f.client(client).window(&surface);
    moved.attach_new_buffer();
    moved.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    let focused = f.client(client).create_window();
    focused.commit();
    let surface = focused.surface.clone();
    f.roundtrip(client);
    let focused = f.client(client).window(&surface);
    focused.attach_new_buffer();
    focused.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused = f.swayward().layout.focus().unwrap().id();

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="moved"] move right"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    assert!(f
        .swayward()
        .layout
        .windows()
        .find(
            |(_, mapped)| crate::utils::with_toplevel_role(mapped.toplevel(), |role| {
                role.app_id.as_deref() == Some("moved")
            })
        )
        .unwrap()
        .0
        .is_some_and(|monitor| monitor.output_name() == "right"));
}

#[test]
fn criteria_move_output_right_uses_layout_positions_during_workspace_animation() {
    let mut f = Fixture::new();
    for (name, position) in [
        ("top-left", (0, 0)),
        ("top-right", (800, 0)),
        ("bottom-left", (0, 600)),
        ("bottom-right", (800, 600)),
    ] {
        f.add_named_output_at(name.into(), (800, 600), Some(position));
    }
    for (output, workspace) in [
        ("top-left", "top-left-workspace"),
        ("top-right", "top-right-workspace"),
        ("bottom-left", "bottom-left-workspace"),
        ("bottom-right", "bottom-right-workspace"),
    ] {
        assert!(crate::command::execute(
            f.niri_state(),
            &format!("focus output {output}, workspace {workspace}")
        )
        .iter()
        .all(|outcome| outcome.success));
    }

    let client = f.add_client();
    for workspace in ["top-left-workspace", "bottom-left-workspace"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id("moveme".into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(f.swayward().layout.are_animations_ongoing(None));
    let bottom = f
        .swayward()
        .layout
        .windows()
        .find(|(monitor, mapped)| {
            monitor.is_some_and(|monitor| monitor.output_name() == "bottom-left")
                && crate::utils::with_toplevel_role(mapped.toplevel(), |role| {
                    role.app_id.as_deref() == Some("moveme")
                })
        })
        .map(|(_, mapped)| mapped.window.clone())
        .unwrap();
    assert!(f.swayward().layout.window_center(&bottom).unwrap().y >= 600);
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="moveme"] move output right"#)[0]
            .success
    );
    let workspace_counts = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| {
            workspace
                .name()
                .map(|name| (name.to_owned(), workspace.windows().count()))
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(workspace_counts["top-right-workspace"], 1);
    assert_eq!(workspace_counts["bottom-right-workspace"], 1);
}

#[test]
fn move_output_direction_uses_the_windows_output_and_stops_at_the_edge() {
    let mut f = Fixture::new();
    f.add_named_output_at("right".into(), (100, 100), Some((200, 100)));
    f.add_named_output_at("middle".into(), (100, 100), Some((100, 0)));
    f.add_named_output_at("left".into(), (100, 100), Some((0, 100)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("moveme".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let window_id = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    for expected in ["middle", "left"] {
        assert!(
            crate::command::execute(f.niri_state(), r#"[app_id="moveme"] move output left"#)[0]
                .success
        );
        assert_eq!(
            f.swayward()
                .layout
                .windows()
                .find(|(_, mapped)| mapped.id() == window_id)
                .unwrap()
                .0
                .unwrap()
                .output_name(),
            expected
        );
    }

    let outcome =
        &crate::command::execute(f.niri_state(), r#"[app_id="moveme"] move output left"#)[0];
    assert!(outcome.success, "{outcome:?}");
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == window_id)
            .unwrap()
            .0
            .unwrap()
            .output_name(),
        "right"
    );
}

#[test]
fn move_split_container_to_output_preserves_the_subtree() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    let client = f.add_client();
    for app_id in ["first", "second", "third"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if app_id == "second" {
            assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
        }
    }
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "move container to output right");
    assert!(outcome[0].success, "{outcome:?}");
    let counts = f.swayward().layout.windows().fold(
        std::collections::HashMap::<_, usize>::new(),
        |mut counts, (monitor, _)| {
            *counts
                .entry(monitor.unwrap().output_name().clone())
                .or_default() += 1;
            counts
        },
    );
    assert_eq!(counts.get("right"), Some(&2));
    assert_eq!(counts.get("left"), Some(&1));
}

#[test]
fn move_split_container_direction_crosses_output_as_a_subtree() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    let client = f.add_client();
    for app_id in ["first", "second", "third"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if app_id == "second" {
            assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
        }
    }
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "move right");
    assert!(outcome[0].success, "{outcome:?}");
    let counts = f.swayward().layout.windows().fold(
        std::collections::HashMap::<_, usize>::new(),
        |mut counts, (monitor, _)| {
            *counts
                .entry(monitor.unwrap().output_name().clone())
                .or_default() += 1;
            counts
        },
    );
    assert_eq!(counts.get("right"), Some(&2));
    assert_eq!(counts.get("left"), Some(&1));
}

#[test]
fn unscoped_move_output_wraps_from_the_edge() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    assert!(
        crate::command::execute(f.niri_state(), "focus output right, workspace right-ws")[0]
            .success
    );
    assert!(
        crate::command::execute(f.niri_state(), "focus output left, workspace left-ws")[0].success
    );
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for expected in ["right", "left"] {
        assert!(
            crate::command::execute(f.niri_state(), "move container to output right")[0].success
        );
        assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
        assert_eq!(
            f.swayward()
                .layout
                .windows()
                .next()
                .unwrap()
                .0
                .unwrap()
                .output_name(),
            expected
        );
    }
}

#[test]
fn move_output_accepts_direction_name_current_and_workspace_forms() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let outputs = [f.niri_output(1).name(), f.niri_output(2).name()];
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "move output current")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move output right")[0].success);
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == focused)
            .unwrap()
            .0
            .unwrap()
            .output_name(),
        &outputs[1]
    );
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("move container to output {}", outputs[0])
        )[0]
        .success
    );
    assert!(crate::command::execute(f.niri_state(), "move workspace output right")[0].success);
}

#[test]
fn sticky_accepts_sway_boolean_words_and_reports_tree_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "sticky enable")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    assert_eq!(find_json_node(&tree, "con", false).unwrap()["sticky"], true);

    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    for (value, expected) in [
        ("enable", true),
        ("toggle", false),
        ("enabled", true),
        ("off", false),
        ("yes", true),
        ("0", false),
        ("1", true),
        ("no", false),
        ("on", true),
        ("disable", false),
        ("active", true),
        ("unknown", false),
    ] {
        assert!(crate::command::execute(f.niri_state(), &format!("sticky {value}"))[0].success);
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        assert_eq!(
            find_json_node(&tree, "floating_con", false).unwrap()["sticky"],
            expected
        );
    }
}

#[test]
fn sticky_without_a_container_matches_sway_failure() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert_eq!(
        crate::command::execute(f.niri_state(), "sticky enable")[0]
            .error
            .as_deref(),
        Some("No current container")
    );
}

#[test]
fn workspace_criteria_uses_sparse_and_named_sway_identities() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for workspace in ["1", "7", "mail"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(workspace.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    for (workspace, mark) in [("7", "sparse"), ("mail", "named")] {
        assert!(
            crate::command::execute(
                f.niri_state(),
                &format!(r#"[workspace="^{workspace}$"] mark {mark}"#)
            )[0]
            .success
        );
    }

    let swayward = f.swayward();
    let marked_apps = swayward
        .layout
        .windows()
        .filter_map(|(_, window)| {
            swayward.marks_by_window.get(&window.id()).map(|marks| {
                let app_id = crate::utils::with_toplevel_role(window.toplevel(), |role| {
                    role.app_id.clone().unwrap()
                });
                (app_id, marks.clone())
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        marked_apps,
        [
            ("7".into(), vec!["sparse".into()]),
            ("mail".into(), vec!["named".into()])
        ]
    );
}

#[test]
fn workspace_next_and_prev_on_output_wrap_in_stored_order() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for workspace in ["1", "5", "6:a", "6:b"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), "workspace next_on_output")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("1".into())
    );
    assert!(crate::command::execute(f.niri_state(), "workspace prev_on_output")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("6:b".into())
    );
}

#[test]
fn workspace_next_and_prev_follow_global_output_order_for_equal_numbers() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let first_output = f.niri_output(1).name();
    let second_output = f.niri_output(2).name();
    let client = f.add_client();
    for (workspace, output) in [
        ("1", &first_output),
        ("2", &second_output),
        ("6:c", &second_output),
        ("5", &first_output),
        ("6:a", &first_output),
        ("6:b", &first_output),
    ] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(
            crate::command::execute(
                f.niri_state(),
                &format!("workspace {workspace} output {output}")
            )[0]
            .success
        );
    }

    assert!(crate::command::execute(f.niri_state(), "workspace 5")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace next")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("6:a".into())
    );

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace prev")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("6:c".into())
    );
}

#[test]
fn workspace_next_and_prev_cross_outputs() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let first_output = f.niri_output(1).name();
    let second_output = f.niri_output(2).name();
    let client = f.add_client();
    for (workspace, output) in [("1", &first_output), ("2", &second_output)] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(
            crate::command::execute(
                f.niri_state(),
                &format!("workspace {workspace} output {output}")
            )[0]
            .success
        );
    }
    assert!(crate::command::execute(f.niri_state(), "workspace prev")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        first_output
    );
    assert!(crate::command::execute(f.niri_state(), "workspace next")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        second_output
    );
}

#[test]
fn killing_focused_workspace_closes_tiled_and_floating_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace 9")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    for floating in [false, true] {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if floating {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "kill")[0].success);
    f.double_roundtrip(client);

    assert_eq!(
        f.client(client)
            .state
            .windows
            .iter()
            .filter(|window| window.close_requested)
            .count(),
        2
    );
    let closed = f
        .client(client)
        .state
        .windows
        .iter()
        .filter(|window| window.close_requested)
        .map(|window| window.surface.clone())
        .collect::<Vec<_>>();
    for surface in closed {
        let window = f.client(client).window(&surface);
        window.attach_null();
        window.commit();
    }
    f.double_roundtrip(client);
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.number(), Some(7));
    assert_eq!(workspace.windows().count(), 0);
    let mut numbers = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| workspace.number())
        .collect::<Vec<_>>();
    numbers.sort_unstable();
    // Workspace 1 is gone, not missing. It was created empty with the output,
    // and focus left it for workspace 9 without ever placing a window on it,
    // so sway destroys it. Measured on real sway 1.11 (headless, one output):
    // focusing an empty workspace 7 then switching away leaves
    // get_workspaces reporting ['1', '2', '9'] with no 7, while an empty
    // workspace that still holds focus is reported. See
    // workspace_consider_destroy, sway/tree/workspace.c:313-330, reached from
    // seat_set_focus, sway/input/seat.c:1244.
    assert_eq!(numbers, [7, 9]);
}

#[test]
fn closing_last_window_removes_inactive_named_workspace_from_ipc() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "workspace active")[0].success);
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut f, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);
    let window = f.client(client).window(&surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(client);

    let mut stream = UnixStream::connect(socket).unwrap();
    let workspaces = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    let names = workspaces
        .as_array()
        .unwrap()
        .iter()
        .map(|workspace| workspace["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["active"]);
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let mut tree_workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut tree_workspaces);
    let names = tree_workspaces
        .iter()
        .map(|workspace| workspace["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["__i3_scratch", "active"]);

    let (event_type, payload) = read_ipc_reply(&mut f, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    let actual = serde_json::from_str::<Value>(&payload).unwrap();
    let expected =
        serde_json::from_str::<Value>(sway_fixture!("events/workspace.empty.json")).unwrap();
    assert_eq!(
        actual.as_object().unwrap().keys().collect::<BTreeSet<_>>(),
        expected
            .as_object()
            .unwrap()
            .keys()
            .collect::<BTreeSet<_>>()
    );
    assert_same_shape(
        &expected["current"],
        &actual["current"],
        "$workspace.current",
    );
    assert_eq!(actual["change"], "empty");
    assert_eq!(actual["current"]["name"], "7");
    assert_eq!(actual["current"]["focused"], false);
    assert_eq!(actual["current"]["nodes"], serde_json::json!([]));

    assert!(crate::command::execute(f.niri_state(), "workspace prev")[0].success);
    let after_prev = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(
        after_prev
            .as_array()
            .unwrap()
            .iter()
            .find(|workspace| workspace["focused"] == true)
            .unwrap()["name"],
        "active"
    );

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let recreated = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(
        recreated
            .as_array()
            .unwrap()
            .iter()
            .filter(|workspace| workspace["num"] == 7)
            .count(),
        1
    );
}

#[test]
fn initial_workspace_name_comes_from_the_first_available_default_mode_binding() {
    for (config, expected) in [
        (
            r#"binds {
                code:24 { command "workspace keycode-first"; }
                X { command "workspace keysym-second"; }
            }"#,
            "keycode-first",
        ),
        (
            r#"binds {
                X { command "workspace keysym-first"; }
                code:24 { command "workspace keycode-second"; }
            }"#,
            "keysym-first",
        ),
        (
            r#"binds {
                X { command "workspace next"; }
                Y { command "workspace prev"; }
                Z { command "workspace next_on_output"; }
                A { command "workspace prev_on_output"; }
                B { command "workspace back_and_forth"; }
                C { command "workspace current"; }
                D { command "workspace number"; }
                code:24 { command "workspace number 7: eggs"; }
            }"#,
            "7: eggs",
        ),
        (
            r#"binds {
                X { focus-workspace "typed"; }
                Y { command "workspace string-second"; }
            }"#,
            "typed",
        ),
        (
            r#"binds {
                X { focus-workspace 7; }
            }
            mode "other" {
                Y { command "workspace ignored-mode"; }
            }"#,
            "7",
        ),
        (
            r#"binds {
                X { command "workspace   3"; }
            }"#,
            "3",
        ),
        (
            r#"binds {
                X { command "workspace 3; exec foo"; }
            }"#,
            "3",
        ),
        (
            r#"binds {
                X { command "workspace 3"; }
            }"#,
            "3",
        ),
        (
            r#"binds {
                X { command "workspace --no-auto-back-and-forth number 3:three"; }
            }"#,
            "3:three",
        ),
    ] {
        let config = swayward_config::Config::parse_mem(config).unwrap();
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1920, 1080));
        assert_eq!(
            f.swayward().layout.active_workspace().unwrap().sway_name(),
            Some(expected.to_owned())
        );
    }

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    for (command, expected) in [
        ("workspace foobar", "foobar"),
        ("workspace   3", "3"),
        ("workspace 3; exec foo", "3"),
        ("workspace 3", "3"),
        (
            "workspace --no-auto-back-and-forth number 3:three",
            "3:three",
        ),
    ] {
        let config = swayward_config::Config::parse_mem(&format!(
            "binds {{\n    X {{ command {command:?}; }}\n}}"
        ))
        .unwrap();
        f.swayward()
            .layout
            .initialize_workspaces_from_bindings(&config);
        assert_eq!(
            f.swayward().layout.active_workspace().unwrap().sway_name(),
            Some(expected.to_owned())
        );
    }

    let config = swayward_config::Config::parse_mem(
        r#"binds {
            X { command "workspace taken"; }
            code:24 { command "workspace fresh"; }
        }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("taken".to_owned())
    );
    f.add_output(2, (1920, 1080));
    f.niri_focus_output(2);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("fresh".to_owned())
    );
}

#[test]
fn configured_workspace_is_destroyed_when_empty_and_inactive() {
    let config = swayward_config::Config::parse_mem(r#"workspace "configured" {}"#).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    assert!(
        crate::command::execute(f.niri_state(), "rename workspace configured to renamed")[0]
            .success
    );
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    f.swayward().clock.set_complete_instantly(true);
    f.swayward().layout.advance_animations();
    f.swayward().clock.set_complete_instantly(false);

    assert!(!f
        .swayward()
        .layout
        .workspaces()
        .any(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("renamed")));
}

#[test]
fn named_workspace_has_no_number_and_active_empty_workspace_remains_visible() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "workspace mail")[0].success);
    f.niri_state().ipc_refresh_layout();

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(workspaces.len(), 1);
    let named = workspaces
        .iter()
        .find(|workspace| workspace.name == "mail")
        .unwrap();
    assert_eq!(named.num, -1);
    assert!(named.visible);
    assert!(named.focused);
}

#[test]
fn negative_and_unnumbered_workspace_names_report_minus_one_without_affecting_order() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    for workspace in ["mail", "-42: negative", "7: numbered"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
    }
    assert!(crate::command::execute(f.niri_state(), "rename workspace mail to inbox")[0].success);
    f.niri_state().ipc_refresh_layout();

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .map(|workspace| (workspace.name.as_str(), workspace.num))
            .collect::<Vec<_>>(),
        [("7: numbered", 7)]
    );
}

#[test]
fn relative_move_includes_empty_active_workspace_and_uses_direction() {
    for (source, direction) in [(1, "next"), (3, "prev")] {
        let mut f = Fixture::new();
        for output in 1..=3 {
            f.add_output(output, (1920, 1080));
        }
        let outputs = [
            f.niri_output(1).name(),
            f.niri_output(2).name(),
            f.niri_output(3).name(),
        ];
        let client = f.add_client();

        assert!(crate::command::execute(f.niri_state(), &format!("workspace {source}"))[0].success);
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);

        for workspace in 1..=3 {
            assert!(
                crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0]
                    .success
            );
            assert!(
                crate::command::execute(
                    f.niri_state(),
                    &format!("workspace {workspace} output {}", outputs[workspace - 1])
                )[0]
                .success
            );
        }

        assert!(crate::command::execute(
            f.niri_state(),
            &format!("workspace {source}, move workspace {direction}")
        )
        .iter()
        .all(|outcome| outcome.success));

        let swayward = f.swayward();
        let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
        let window_counts = workspaces
            .iter()
            .map(|workspace| (workspace.num, workspace.focus.len()))
            .collect::<Vec<_>>();
        assert_eq!(window_counts, [(1, 0), (2, 1), (3, 0)]);
    }
}

#[test]
fn targeted_focus_reveals_a_hidden_scratchpad_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.set_title("target");
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let outcome = crate::command::execute(f.niri_state(), r#"[title="target"] focus workspace"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 0);
    assert!(f.swayward().layout.focus().is_some());
}

fn set_test_window_urgent_at(f: &mut Fixture, app_id: &str, now: Duration) {
    f.swayward().layout.with_windows_mut(|window, _| {
        if crate::utils::with_toplevel_role(window.toplevel(), |role| {
            role.app_id.as_deref() == Some(app_id)
        }) {
            window.set_urgent_for_test(true, now);
        }
    });
}

fn set_test_window_urgent(f: &mut Fixture, app_id: &str) {
    set_test_window_urgent_at(f, app_id, crate::utils::get_monotonic_time());
}

fn test_window_is_urgent(f: &mut Fixture, app_id: &str) -> bool {
    f.swayward()
        .layout
        .windows()
        .find(|(_, window)| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| {
                role.app_id.as_deref() == Some(app_id)
            })
        })
        .unwrap()
        .1
        .is_urgent()
}
