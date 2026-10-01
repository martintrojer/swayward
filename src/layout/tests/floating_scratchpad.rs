//! Single floating windows and the scratchpad: sizing, constraints, titlebar caches.

use super::*;

#[test]
fn tiled_window_restores_natural_size_when_first_floated() {
    let mut options = Options::default();
    options.layout.border.off = true;
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams {
                    bbox: Rectangle::from_size(Size::from((400, 150))),
                    ..TestWindowParams::new(1)
                },
            },
        ],
    );

    layout.toggle_window_floating(Some(&1));

    let window = layout
        .windows()
        .find(|(_, window)| window.id() == &1)
        .unwrap()
        .1;
    assert_eq!(window.0.requested_size.get(), Some(Size::from((400, 150))));
    let (_, ipc) = layout
        .active_workspace()
        .unwrap()
        .floating()
        .tiles_with_ipc_layouts()
        .next()
        .unwrap();
    assert_eq!(ipc.window_size, (400, 150));
}

#[test]
fn tiled_window_gets_sway_default_size_when_first_moved_to_scratchpad() {
    let mut options = Options::default();
    options.layout.border.off = true;
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
        ],
    );

    layout.move_to_scratchpad(Some(&1));
    layout.show_scratchpad(Some(&1));

    let window = layout
        .windows()
        .find(|(_, window)| window.id() == &1)
        .unwrap()
        .1;
    assert_eq!(window.0.requested_size.get(), Some(Size::from((640, 540))));
    let workspace = layout.active_workspace().unwrap();
    let (_, pos) = workspace
        .floating()
        .tiles_with_ipc_layouts()
        .find(|(tile, _)| tile.window().id() == &1)
        .unwrap();
    assert_eq!(pos.tile_pos_in_workspace_view, Some((320., 90.)));
}

/// Sway sizes the view's content, not the decorated container, when a tiled
/// view first enters the scratchpad: container_floating_set_default_size sets
/// content_width/height to half and three quarters of the workspace box and
/// derives the geometry from the content (sway/tree/container.c:896-918).
#[test]
fn tiled_window_scratchpad_default_size_is_the_content_size_with_borders() {
    let mut options = Options::default();
    options.layout.border.off = false;
    options.layout.border.width = 2.;
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
        ],
    );

    layout.move_to_scratchpad(Some(&1));
    layout.show_scratchpad(Some(&1));

    let window = layout
        .windows()
        .find(|(_, window)| window.id() == &1)
        .unwrap()
        .1;
    assert_eq!(window.0.requested_size.get(), Some(Size::from((640, 540))));
}

// sway/tree/container.c:990-994: every return to tiling removes the
// container from the scratchpad, including a drag toggled to tiling.
#[test]
fn toggling_a_dragged_scratchpad_window_to_tiling_removes_it_from_the_scratchpad() {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ]);
    layout.move_to_scratchpad(Some(&1));
    layout.show_scratchpad(Some(&1));
    assert!(layout.is_scratchpad_window(&1));

    check_ops_on_layout(
        &mut layout,
        [
            Op::InteractiveMoveBegin {
                window: 1,
                output_idx: 1,
                px: 0.0,
                py: 0.0,
            },
            Op::InteractiveMoveUpdate {
                window: 1,
                dx: 100.0,
                dy: 100.0,
                output_idx: 1,
                px: 0.0,
                py: 0.0,
            },
            Op::ToggleWindowFloating { id: None },
        ],
    );
    assert!(layout.is_scratchpad_window(&1), "still mid-drag");

    check_ops_on_layout(&mut layout, [Op::InteractiveMoveEnd { window: 1 }]);
    assert!(!layout.is_scratchpad_window(&1));
    assert!(layout.scratchpad_is_empty());
}

#[test]
fn scratchpad_default_size_honors_client_size_hints() {
    let mut options = Options::default();
    options.layout.border.off = true;
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams {
                    min_max_size: (Size::from((700, 100)), Size::from((800, 400))),
                    ..TestWindowParams::new(1)
                },
            },
        ],
    );

    layout.move_to_scratchpad(Some(&1));
    layout.show_scratchpad(Some(&1));

    let window = layout
        .windows()
        .find(|(_, window)| window.id() == &1)
        .unwrap()
        .1;
    assert_eq!(window.0.requested_size.get(), Some(Size::from((700, 400))));
    let workspace = layout.active_workspace().unwrap();
    let (_, layout) = workspace
        .floating()
        .tiles_with_ipc_layouts()
        .find(|(tile, _)| tile.window().id() == &1)
        .unwrap();
    assert_eq!(layout.tile_pos_in_workspace_view, Some((290., 160.)));
}

#[test]
fn configured_floating_constraints_clamp_resize_requests() {
    let mut options = Options::default();
    options.layout.border.off = true;
    options.layout.floating_minimum_size = swayward_config::FloatingSize {
        width: 60,
        height: 50,
    };
    options.layout.floating_maximum_size = swayward_config::FloatingSize {
        width: 100,
        height: 90,
    };
    let mut params = TestWindowParams::new(1);
    params.is_floating = true;
    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow { params },
            Op::SetWindowWidth {
                id: None,
                change: SizeChange::SetFixed(200),
            },
            Op::SetWindowHeight {
                id: None,
                change: SizeChange::SetFixed(10),
            },
        ],
    );

    let window = layout
        .windows()
        .find(|(_, window)| window.id() == &1)
        .unwrap()
        .1;
    assert_eq!(window.0.requested_size.get(), Some(Size::from((100, 50))));
}

#[test]
fn non_finite_floating_proportions_are_noops() {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(1)
            },
        },
    ]);

    let before = layout
        .active_workspace()
        .unwrap()
        .active_window_visual_rectangle();
    for change in [
        PositionChange::SetProportion(f64::NAN),
        PositionChange::AdjustProportion(f64::NAN),
        PositionChange::SetProportion(f64::INFINITY),
        PositionChange::AdjustProportion(f64::NEG_INFINITY),
    ] {
        layout.move_floating_window(Some(&1), change, PositionChange::AdjustFixed(0.), false);
        assert_eq!(
            layout
                .active_workspace()
                .unwrap()
                .active_window_visual_rectangle(),
            before
        );
        layout.verify_invariants();
    }

    let stored_position = |invalid_change| {
        let mut layout = check_ops([
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
        ]);
        layout.move_floating_window(
            Some(&1),
            PositionChange::SetFixed(100.),
            PositionChange::SetFixed(200.),
            false,
        );
        if let Some((x, y)) = invalid_change {
            layout.move_floating_window(Some(&1), x, y, false);
        }
        layout.toggle_window_floating(Some(&1));
        layout.verify_invariants();
        layout
            .active_workspace()
            .unwrap()
            .active_window_visual_rectangle()
    };
    let control = stored_position(None);
    assert_eq!(
        stored_position(Some((
            PositionChange::SetProportion(f64::NAN),
            PositionChange::AdjustProportion(f64::NAN),
        ))),
        control
    );
}

/// A floating titlebar's cache belongs to its window. It was keyed by the
/// stacking index, so raising a window handed every cached buffer to a
/// different window and each titlebar re-rasterised on the next frame.
#[test]
fn raising_a_floating_window_keeps_each_titlebar_cache_with_its_window() {
    let mut first = TestWindowParams::new(1);
    first.is_floating = true;
    let mut second = TestWindowParams::new(2);
    second.is_floating = true;
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow { params: first },
        Op::AddWindow { params: second },
    ]);
    let before = layout
        .active_workspace()
        .unwrap()
        .floating()
        .titlebar_slots();
    let lower = before.last().unwrap().0;

    layout.activate_window(&lower);

    let after = layout
        .active_workspace()
        .unwrap()
        .floating()
        .titlebar_slots();
    assert_eq!(after.first().unwrap().0, lower, "the window was raised");
    let slot =
        |slots: &[(usize, *const _)], id| slots.iter().find(|(window, _)| *window == id).unwrap().1;
    for id in [1, 2] {
        assert_eq!(slot(&after, id), slot(&before, id));
    }
}

#[test]
fn moving_a_tiny_window_to_scratchpad_with_a_huge_border_does_not_panic() {
    let mut layout = swayward_config::Layout::default();
    layout.border.width = 29_707.;
    check_ops_with_options(
        Options {
            layout,
            ..Default::default()
        },
        vec![
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::MoveFocusedToScratchpad,
        ],
    );
}

#[test]
fn moving_a_hidden_scratchpad_window_to_an_output_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(2),
        Op::MoveFocusedToScratchpad,
        Op::MoveWindowToOutput {
            window_id: Some(2),
            output_id: 1,
            target_ws_idx: None,
        },
    ]);
}
