//! Workspace creation, naming, destruction, and moves between workspaces and outputs.

use super::*;

#[test]
fn removing_active_output_focuses_its_evacuated_workspace() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    // Sway keeps focus on the evacuated non-empty workspace rather than the
    // surviving output's previously active empty workspace.
    assert_eq!(monitors[0].active_workspace_idx, 0);
}

#[test]
fn removing_active_output_reaps_the_empty_workspace_that_loses_focus() {
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(4),
        Op::MoveWorkspaceToMonitor {
            ws_name: None,
            output_id: 4,
        },
        Op::RemoveOutput(4),
    ]);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };
    assert_eq!(monitors[0].workspaces.len(), 1);
    assert!(monitors[0].active_workspace_ref().has_window(&1));
}

#[test]
fn move_down_creates_named_destination_before_moving_window() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);

    layout.move_to_workspace_down(true);

    let workspace = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&0))
        .map(|(_, _, workspace)| workspace)
        .unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("2"));
}

#[test]
fn move_column_down_creates_named_destination_before_detaching() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);

    layout.move_focused_to_workspace_down(true);

    let workspace = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&0))
        .map(|(_, _, workspace)| workspace)
        .unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("2"));
}

#[test]
fn workspace_focus_history_tracks_every_visited_workspace() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for name in ["93", "92", "94", "96", "foo"] {
        layout
            .activate_sway_workspace(crate::command::WorkspaceTarget::Name(name.into()))
            .unwrap();
    }

    let monitor = layout.active_monitor_ref().unwrap();
    let names = monitor
        .workspace_focus_history
        .iter()
        .map(|id| {
            monitor
                .workspaces
                .iter()
                .find(|workspace| workspace.id() == *id)
                .and_then(Workspace::sway_name)
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["foo", "96", "94", "92", "93", "1"]);
}

#[test]
fn move_focused_to_output_names_destination_before_detaching() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    let output = layout
        .outputs()
        .find(|output| output.name() == "output2")
        .unwrap()
        .clone();

    layout.move_focused_to_output(&output, None, true);

    let workspace = layout
        .monitor_for_output(&output)
        .unwrap()
        .workspaces
        .iter()
        .find(|workspace| workspace.has_window(&0))
        .unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("2"));
}

#[test]
fn sway_move_sorts_new_destination_before_moving_window() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("10".into()))
        .unwrap();
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);

    layout
        .move_window_to_sway_workspace(&0, crate::command::WorkspaceTarget::Name("2".into()), false)
        .unwrap();

    let monitor = layout.active_monitor_ref().unwrap();
    let names = monitor
        .workspaces
        .iter()
        .filter_map(Workspace::sway_name)
        .collect::<Vec<_>>();
    assert_eq!(names, ["1", "2", "10"]);
    assert!(monitor
        .workspaces
        .iter()
        .find(|workspace| workspace.has_window(&0))
        .is_some_and(|workspace| workspace.sway_name().as_deref() == Some("2")));
}

#[test]
fn move_to_output_names_destination_before_moving_window() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    let output = layout
        .outputs()
        .find(|output| output.name() == "output2")
        .unwrap()
        .clone();

    layout.move_to_output(Some(&0), &output, None, ActivateWindow::No);

    let workspace = layout
        .monitor_for_output(&output)
        .unwrap()
        .workspaces
        .iter()
        .find(|workspace| workspace.has_window(&0))
        .unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("2"));
}

#[test]
fn empty_named_workspace_is_destroyed_with_its_output() {
    let ops = [
        Op::AddOutput(1),
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
    ];

    let layout = check_ops(ops);
    assert!(layout.workspaces().all(|(_, _, ws)| ws.name().is_none()));
}

#[test]
fn inactive_empty_configured_workspace_is_destroyed_after_focus_changes() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    layout.set_workspace_name("source".into(), None);
    let source = layout.active_workspace().unwrap().id();

    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("target".into()))
        .unwrap();
    Op::CompleteAnimations.apply(&mut layout);

    assert!(layout.find_workspace_by_id(source).is_none());
}

#[test]
fn moving_the_only_workspace_replaces_it_before_reparenting() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    Op::FocusOutput(2).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    Op::FocusOutput(1).apply(&mut layout);

    let MonitorSet::Normal {
        monitors,
        active_monitor_idx,
        ..
    } = &mut layout.monitor_set
    else {
        unreachable!()
    };
    *active_monitor_idx = 0;
    monitors[0].workspaces[0].set_sway_identity(None, Some(2));
    monitors[0].add_workspace_at(1);
    monitors[1].workspaces[0].set_sway_identity(None, Some(3));
    let source = monitors[0].workspaces[0].id();
    let destination = monitors[1].output.clone();

    assert!(layout.move_workspace_to_output_by_id(source, None, &destination));

    let MonitorSet::Normal {
        monitors,
        active_monitor_idx,
        ..
    } = &layout.monitor_set
    else {
        unreachable!()
    };
    assert_eq!(*active_monitor_idx, 1);
    assert_eq!(
        monitors[0]
            .workspaces
            .iter()
            .filter_map(Workspace::sway_name)
            .collect::<Vec<_>>(),
        ["1"]
    );
    assert_eq!(
        monitors[1]
            .workspaces
            .iter()
            .filter_map(Workspace::sway_name)
            .collect::<Vec<_>>(),
        ["2", "3"]
    );
    assert_eq!(
        monitors[1].workspaces[monitors[1].active_workspace_idx].id(),
        source
    );
}

#[test]
fn move_to_named_target_index_preserves_addressable_workspace() {
    // Fuzzer seed: a numbered-but-unnamed workspace is addressable and must
    // survive a targeted move even though it has no windows.
    let layout = check_ops([
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: None,
            layout_config: None,
        },
        Op::AddOutput(4),
        Op::MoveWindowToOutput {
            window_id: None,
            output_id: 4,
            target_ws_idx: Some(1),
        },
    ]);
    assert!(layout
        .workspaces()
        .any(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("ws1")));
}

#[test]
fn unname_then_implicit_rename_preserves_workspace_invariants() {
    // CI shrank `random_operations_dont_panic` to this six-op sequence. Both
    // the move and rename use the implicit target (`None`), and the same output
    // is added twice.
    let layout = check_ops([
        Op::UnnameWorkspace { ws_name: 1 },
        Op::AddOutput(1),
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveWindowDownOrToWorkspaceDown,
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 0,
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
    ]);
    assert_eq!(
        layout.active_workspace().unwrap().sway_name().as_deref(),
        Some("ws1")
    );
    assert!(layout.active_workspace().unwrap().has_window(&1));
}

#[test]
fn mapping_a_window_does_not_create_a_ghost_workspace() {
    // sway creates a workspace on demand and destroys it when it empties
    // (`sway/tree/workspace.c:313-330`). Mapping a window onto the only
    // workspace must not append niri's trailing scrolling-strip placeholder,
    // which a bar and `workspace next` would both show as a ghost.
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
    ]);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };
    assert_eq!(monitors[0].workspaces.len(), 1);
    assert!(monitors[0].workspaces[0].has_windows());
}

#[test]
fn named_workspace_uses_first_available_sway_output_assignment() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);

    layout.ensure_named_workspace(&WorkspaceConfig {
        name: WorkspaceName("assigned".into()),
        sway_output_assignment: Some(vec!["missing".into(), "output2".into(), "output1".into()]),
        open_on_output: None,
        layout: None,
    });

    let (monitor, _, _) = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.name().is_some_and(|name| name == "assigned"))
        .unwrap();
    assert_eq!(monitor.unwrap().output_name(), "output2");
}

#[test]
fn closing_a_window_mapped_before_any_output_leaves_one_workspace() {
    // The window maps before any output exists, then the output adopts its
    // workspace. Closing it leaves that workspace focused and empty; sway
    // keeps a focused workspace even when empty (workspace_consider_destroy,
    // sway/tree/workspace.c:313-330, returns while it is the active one).
    let ops = [
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(1),
        Op::CloseWindow(1),
    ];
    let layout = check_ops(ops);
    let workspaces = layout.workspaces().collect::<Vec<_>>();
    assert_eq!(workspaces.len(), 1);
    assert!(workspaces[0].0.is_some(), "the workspace is on the output");
    assert!(!workspaces[0].2.has_windows());
}

#[test]
fn move_focused_to_workspace_unfocused_with_multiple_monitors() {
    let ops = [
        Op::AddOutput(1),
        Op::SetWorkspaceName {
            new_ws_name: 101,
            ws_name: None,
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddNamedWorkspace {
            ws_name: 102,
            output_name: Some(1),
            layout_config: None,
        },
        Op::FocusWorkspace(1),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(2),
        Op::FocusOutput(2),
        Op::SetWorkspaceName {
            new_ws_name: 201,
            ws_name: None,
        },
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::AddWindow {
            params: TestWindowParams::new(4),
        },
        Op::MoveFocusedToOutput {
            output_id: 1,
            target_ws_idx: Some(0),
            activate: false,
        },
        Op::FocusOutput(1),
    ];

    let layout = check_ops(ops);

    assert_eq!(layout.active_workspace().unwrap().name().unwrap(), "ws102");

    for (mon, win) in layout.windows() {
        let mon = mon.unwrap();
        let ws = mon
            .workspaces
            .iter()
            .find(|w| w.has_window(win.id()))
            .unwrap();

        assert_eq!(
            ws.name().unwrap(),
            match win.id() {
                1 | 4 => "ws101",
                2 => "ws102",
                3 => "ws201",
                _ => unreachable!(),
            }
        );
    }
}

#[test]
fn move_focused_to_workspace_down_focus_false_on_floating_window() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ToggleWindowFloating { id: None },
        Op::MoveFocusedToWorkspaceDown(false),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert!(monitors[0].active_workspace_ref().has_window(&1));
}

#[test]
fn move_focused_to_workspace_focus_false_on_floating_window() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ToggleWindowFloating { id: None },
        Op::MoveFocusedToWorkspace(1, false),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert!(monitors[0].active_workspace_ref().has_window(&1));
}

#[test]
fn render_geometry_has_no_workspace_creation_slot() {
    let layout = check_ops([Op::AddOutput(1)]);
    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };
    let monitor = &monitors[0];

    assert_eq!(
        monitor.workspaces_render_geo().count(),
        monitor.workspaces.len()
    );
}
