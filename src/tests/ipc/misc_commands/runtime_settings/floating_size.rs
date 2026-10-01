#[test]
fn floating_size_settings_apply_at_runtime_like_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let layout = |f: &mut Fixture| f.swayward().config.borrow().layout.clone();

    // sway parses these with strtol and requires a literal x between two
    // integers, rejecting a trailing suffix because it checks the remainder
    // (`sway/sway/commands/floating_minmax_size.c:23-36`).
    assert!(crate::command::execute(f.niri_state(), "floating_minimum_size 100 x 50")[0].success);
    assert_eq!(layout(&mut f).floating_minimum_size.width, 100);
    assert_eq!(layout(&mut f).floating_minimum_size.height, 50);
    assert!(crate::command::execute(f.niri_state(), "floating_maximum_size 800 x 600")[0].success);
    assert_eq!(layout(&mut f).floating_maximum_size.width, 800);
    assert_eq!(layout(&mut f).floating_maximum_size.height, 600);
    for (bad, expected) in [
        (
            "floating_minimum_size 100 50",
            "Invalid floating_minimum_size command (expected 3 arguments, got 2)",
        ),
        (
            "floating_minimum_size 100 x 50px",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
        (
            "floating_minimum_size 100 by 50",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), bad);
        assert!(!outcome[0].success, "{bad} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected));
    }
}
