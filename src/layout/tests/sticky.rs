//! Sticky windows.

use super::*;

#[test]
fn sticky_window_does_not_keep_focus_on_an_empty_workspace_switch() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams {
            is_floating: true,
            ..TestWindowParams::new(0)
        },
    }
    .apply(&mut layout);
    assert!(layout.set_window_sticky(&0, "enable"));

    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("2".into()))
        .unwrap();

    assert!(layout.active_workspace().unwrap().active_window().is_none());
    assert_eq!(
        layout.window_workspace_id(&0),
        Some(layout.active_workspace().unwrap().id())
    );
}

#[test]
fn making_window_sticky_moves_before_cleaning_source_workspace() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("source".into()))
        .unwrap();
    Op::AddWindow {
        params: TestWindowParams {
            is_floating: true,
            ..TestWindowParams::new(0)
        },
    }
    .apply(&mut layout);
    let source = layout.window_workspace_id(&0).unwrap();
    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("target".into()))
        .unwrap();
    layout.active_monitor().unwrap().workspace_switch = None;

    assert!(layout.set_window_sticky(&0, "enable"));

    assert!(layout.window_workspace_id(&0).is_some());
    assert!(layout.find_workspace_by_id(source).is_none());
}
