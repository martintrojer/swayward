use super::*;

#[test]
fn tile_resolves_toggle_rule_before_storing_border_state() {
    let rules = ResolvedWindowRules {
        sway_border: Some(BorderStyle::Toggle),
        ..Default::default()
    };
    let tile = tile_from(TestWindow::with_rules(1, rules), Size::from((500., 500.)));

    assert_eq!(tile.sway_border(), (BorderStyle::None, 0));
}

#[test]
fn tile_activation_region_contains_only_server_decorations() {
    let mut options = Options::default();
    options.layout.border.off = false;
    let tile = Tile::new(
        TestWindow::new(1),
        Size::from((500., 500.)),
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(options),
    );
    let border = tile.effective_border_width().unwrap();
    let window = tile.window_loc();
    assert!(matches!(
        tile.hit((window.x - border / 2., window.y + 10.).into()),
        Some(HitType::Activate {
            is_tab_indicator: false
        })
    ));
    assert!(matches!(
        tile.hit((window.x + 10., window.y + 10.).into()),
        Some(HitType::Input { .. })
    ));
}
