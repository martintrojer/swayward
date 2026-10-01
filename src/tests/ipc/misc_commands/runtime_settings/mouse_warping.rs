#[test]
fn mouse_warping_settings_apply_at_runtime_like_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    // Sway keeps the three warping modes apart: `output` warps only across
    // outputs, `container` warps on every qualifying focus change
    // (`sway/sway/input/seat.c:1526-1547`). This is its own policy and must
    // not be folded into the inherited `warp-mouse-to-focus` centering mode.
    use swayward_config::input::MouseWarping;
    let warping = |f: &mut Fixture| f.swayward().config.borrow().input.mouse_warping;
    let warp_to_focus_before = f.swayward().config.borrow().input.warp_mouse_to_focus;
    assert!(crate::command::execute(f.niri_state(), "mouse_warping output")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::Output);
    assert!(crate::command::execute(f.niri_state(), "mouse_warping container")[0].success);
    assert_eq!(
        warping(&mut f),
        MouseWarping::Container,
        "`container` must be stored distinctly from `output`"
    );
    assert!(crate::command::execute(f.niri_state(), "mouse_warping none")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::No);
    assert_eq!(
        f.swayward().config.borrow().input.warp_mouse_to_focus,
        warp_to_focus_before,
        "mouse_warping must not overwrite the inherited centering option"
    );
    // strcasecmp, unlike focus_follows_mouse
    // (`sway/sway/commands/mouse_warping.c:9-16`).
    assert!(crate::command::execute(f.niri_state(), "mouse_warping CONTAINER")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::Container);
    let outcome = crate::command::execute(f.niri_state(), "mouse_warping sideways");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'mouse_warping output|container|none'")
    );
    assert!(crate::command::execute(f.niri_state(), "mouse_warping none")[0].success);
}

