//! Interactive move and drag-and-drop.

use super::*;

#[test]
fn moving_popup_target_ignores_tile_animation_offset() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();

    assert!(layout.interactive_move_begin(0, &output, Point::from((50., 100.))));
    assert!(layout.interactive_move_update(
        &0,
        Point::from((1000., 0.)),
        output,
        Point::from((500., 500.)),
    ));

    let InteractiveMoveState::Moving(move_) = layout.interactive_move.as_ref().unwrap() else {
        panic!("window must be moving");
    };
    assert_ne!(move_.tile.render_offset().y, 0.);
    let pointer_offset_y = move_.tile.window_size().h * move_.pointer_ratio_within_window.1;
    let stable_tile_y =
        move_.pointer_pos_within_output.y - pointer_offset_y - move_.tile.window_loc().y;
    let target = layout.popup_target_rect(&0);
    assert_eq!(target.loc.y, -stable_tile_y - move_.tile.window_loc().y);
}

#[test]
fn interactive_move_keeps_source_until_drop_is_attached() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    let source = layout.window_workspace_id(&0).unwrap();
    Op::FocusWorkspaceDown.apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();

    assert!(layout.interactive_move_begin(0, &output, Point::default()));
    assert!(layout.interactive_move_update(&0, Point::from((1000., 0.)), output, Point::default(),));

    assert!(layout.find_workspace_by_id(source).is_some());
    layout.interactive_move_end(&0);
    assert!(layout.window_workspace_id(&0).is_some());
}

#[test]
fn interactive_drop_creates_named_destination_before_inserting_window() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();

    assert!(layout.interactive_move_begin(0, &output, Point::default()));
    assert!(layout.interactive_move_update(
        &0,
        Point::from((1000., 0.)),
        output,
        Point::from((0., 10000.)),
    ));
    layout.interactive_move_end(&0);

    let workspace = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&0))
        .map(|(_, _, workspace)| workspace)
        .unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("2"));
}

#[test]
fn drag_over_creation_slot_uses_preview_then_materializes_workspace() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();
    let monitor = layout.monitor_for_output(&output).unwrap();
    let last = monitor.workspaces.last().unwrap().id();
    let last_geo = monitor.workspaces_render_geo().last().unwrap();
    // The creation slot is the empty region below the last workspace. There is
    // no trailing placeholder workspace to drop onto.
    let pointer = last_geo.loc + Point::from((last_geo.size.w / 2., last_geo.size.h * 2.));
    let workspace_count = monitor.workspaces.len();
    let (target, preview_geo) = monitor.insert_position(pointer);
    let InsertWorkspace::Preview(preview) = target else {
        panic!("creation slot must be represented by preview state");
    };
    assert_eq!(preview.insertion_index, workspace_count);
    assert_eq!(preview_geo, preview.geometry);

    assert!(layout.interactive_move_begin(0, &output, Point::default()));
    assert!(layout.interactive_move_update(&0, Point::from((1000., 0.)), output, pointer,));
    layout.update_insert_hint(None);

    let monitor = layout.active_monitor_ref().unwrap();
    assert_eq!(monitor.workspaces.len(), workspace_count);
    assert!(matches!(
        monitor.insert_hint.as_ref().map(|hint| hint.workspace),
        Some(InsertWorkspace::Preview(_))
    ));

    layout.interactive_move_end(&0);
    let (_, _, destination) = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&0))
        .unwrap();
    assert_ne!(destination.id(), last);
    assert!(destination.has_sway_identity());
}

#[test]
fn first_interactive_move_update_focuses_the_destination_output() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    let outputs = layout.outputs().cloned().collect::<Vec<_>>();
    layout.focus_output(&outputs[0]);

    assert!(layout.interactive_move_begin(0, &outputs[0], Point::default()));
    assert!(layout.interactive_move_update(
        &0,
        Point::from((1000., 0.)),
        outputs[1].clone(),
        Point::default(),
    ));

    assert_eq!(layout.active_output(), Some(&outputs[1]));
}

#[test]
fn drop_on_a_tile_centre_across_outputs_exchanges_windows() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    let outputs = layout.outputs().cloned().collect::<Vec<_>>();
    layout.focus_output(&outputs[1]);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    layout.focus_output(&outputs[0]);

    let source_workspace = layout
        .monitor_for_output(&outputs[0])
        .unwrap()
        .active_workspace_ref()
        .id();
    let target_workspace = layout
        .monitor_for_output(&outputs[1])
        .unwrap()
        .active_workspace_ref()
        .id();
    let target_workspace_ref = layout
        .monitor_for_output(&outputs[1])
        .unwrap()
        .active_workspace_ref();
    let target_node = target_workspace_ref.tiling().node_for_window(&1).unwrap();
    let target_rect = target_workspace_ref
        .tiling()
        .node_geometry(target_node)
        .unwrap();
    let (target_pos, target_size) = (target_rect.loc, target_rect.size);
    let target_centre = target_pos + target_size.to_point().downscale(2.);

    assert!(layout.interactive_move_begin(0, &outputs[0], Point::default()));
    assert!(layout.interactive_move_update(
        &0,
        Point::from((1000., 0.)),
        outputs[1].clone(),
        target_centre,
    ));
    layout.interactive_move_end(&0);

    let source = layout
        .workspaces()
        .find(|(_, _, ws)| ws.id() == source_workspace)
        .unwrap()
        .2;
    let target = layout
        .workspaces()
        .find(|(_, _, ws)| ws.id() == target_workspace)
        .unwrap()
        .2;
    assert!(source.has_window(&1));
    assert!(target.has_window(&0));
}

#[test]
fn drop_on_a_tile_centre_swaps_instead_of_inserting() {
    // Sway decides a tiling drop by edge: a centre hit on a container swaps the
    // two windows rather than inserting beside one
    // (sway/sway/input/seatop_move_tiling.c:365-388).
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for id in [0, 1] {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
    }

    let output = layout.outputs().next().unwrap().clone();
    let monitor = layout.monitor_for_output(&output).unwrap();
    let workspace = monitor.active_workspace_ref();
    let target = workspace.tiling().node_for_window(&0).unwrap();
    let target_rect = workspace.tiling().node_geometry(target).unwrap();
    let geo = (target_rect.loc, target_rect.size);

    // The middle of the first tile must be a swap, and its outer quarter an
    // insertion, so the two regions are distinguishable.
    let centre = geo.0 + geo.1.to_point().downscale(2.);
    let left_edge = geo.0 + Size::from((geo.1.w / 8., geo.1.h / 2.)).to_point();
    let centre_position = workspace.scrolling_insert_position(centre);
    assert!(
        matches!(
            centre_position,
            crate::layout::monitor::InsertPosition::SwapWith(_)
        ),
        "a centre drop must swap: {centre:?} {geo:?} {centre_position:?}"
    );
    assert!(
        matches!(
            workspace.scrolling_insert_position(left_edge),
            crate::layout::monitor::InsertPosition::InsertAt(_, _)
        ),
        "an edge drop must insert"
    );
}

#[test]
fn refreshing_after_hiding_an_interactive_move_does_not_panic() {
    check_ops([
        Op::AddWindow {
            params: TestWindowParams::new(5),
        },
        Op::AddOutput(2),
        Op::InteractiveMoveBegin {
            window: 5,
            output_idx: 2,
            px: 0.,
            py: 0.,
        },
        Op::ToggleFocusedContainerFloating,
        Op::MoveFocusedToScratchpad,
        Op::Refresh { is_active: false },
    ]);
}

#[test]
fn interactive_move_update_after_scratchpad_transfer_does_not_panic() {
    check_ops([
        Op::AddOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::InteractiveMoveBegin {
            window: 1,
            output_idx: 2,
            px: 0.,
            py: 0.,
        },
        Op::MoveFocusedToScratchpad,
        Op::ToggleFocusedContainerFloating,
        Op::InteractiveMoveUpdate {
            window: 1,
            dx: 1.,
            dy: 0.,
            output_idx: 2,
            px: 0.,
            py: 0.,
        },
    ]);
}

#[test]
fn interactive_move_on_a_floating_container_does_not_panic() {
    check_ops([
        Op::AddOutput(5),
        Op::AddWindow {
            params: TestWindowParams::new(5),
        },
        Op::ToggleFocusedContainerFloating,
        Op::InteractiveMoveBegin {
            window: 5,
            output_idx: 5,
            px: 0.,
            py: 0.,
        },
    ]);
}
