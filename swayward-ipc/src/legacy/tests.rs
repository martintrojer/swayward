#[test]
fn output_action_validation_preserves_error_precedence() {
    let modeline =
        |hdisplay, hsync_start, hsync_end, htotal, vdisplay, vsync_start, vsync_end, vtotal| {
            OutputAction::Modeline {
                clock: 1.,
                hdisplay,
                hsync_start,
                hsync_end,
                htotal,
                vdisplay,
                vsync_start,
                vsync_end,
                vtotal,
                hsync_polarity: HSyncPolarity::PHSync,
                vsync_polarity: VSyncPolarity::PVSync,
            }
        };

    assert_eq!(
        modeline(2, 2, 2, 2, 2, 2, 2, 2).validate(),
        Err("hdisplay 2 must be < hsync_start 2".to_string())
    );
    assert_eq!(
        modeline(1, 2, 2, 2, 1, 2, 2, 2).validate(),
        Err("hsync_start 2 must be < hsync_end 2".to_string())
    );
    assert_eq!(
        modeline(1, 2, 3, 3, 1, 2, 2, 2).validate(),
        Err("hsync_end 3 must be < htotal 3".to_string())
    );
    assert_eq!(
        modeline(1, 2, 3, 4, 2, 2, 2, 2).validate(),
        Err("vdisplay 2 must be < vsync_start 2".to_string())
    );
    assert_eq!(
        modeline(1, 2, 3, 4, 1, 2, 2, 2).validate(),
        Err("vsync_start 2 must be < vsync_end 2".to_string())
    );
    assert_eq!(
        modeline(1, 2, 3, 4, 1, 2, 3, 3).validate(),
        Err("vsync_end 3 must be < vtotal 3".to_string())
    );
    assert!(modeline(1, 2, 3, 4, 1, 2, 3, 4).validate().is_ok());

    let custom_mode = |refresh| OutputAction::CustomMode {
        mode: ConfiguredMode {
            width: 1920,
            height: 1080,
            refresh,
        },
    };
    assert_eq!(
        custom_mode(None).validate(),
        Err("refresh rate is required for custom modes".to_string())
    );
    assert_eq!(
        custom_mode(Some(0.)).validate(),
        Err("custom mode refresh rate 0 must be > 0".to_string())
    );
    assert!(custom_mode(Some(60.)).validate().is_ok());
}

use super::*;

#[test]
fn parse_size_change() {
    assert_eq!(
        "10".parse::<SizeChange>().unwrap(),
        SizeChange::SetFixed(10),
    );
    assert_eq!(
        "+10".parse::<SizeChange>().unwrap(),
        SizeChange::AdjustFixed(10),
    );
    assert_eq!(
        "-10".parse::<SizeChange>().unwrap(),
        SizeChange::AdjustFixed(-10),
    );
    assert_eq!(
        "10%".parse::<SizeChange>().unwrap(),
        SizeChange::SetProportion(10.),
    );
    assert_eq!(
        "+10%".parse::<SizeChange>().unwrap(),
        SizeChange::AdjustProportion(10.),
    );
    assert_eq!(
        "-10%".parse::<SizeChange>().unwrap(),
        SizeChange::AdjustProportion(-10.),
    );

    assert!("-".parse::<SizeChange>().is_err());
    assert!("10% ".parse::<SizeChange>().is_err());
}

#[test]
fn parse_position_change() {
    assert_eq!(
        "10".parse::<PositionChange>().unwrap(),
        PositionChange::SetFixed(10.),
    );
    assert_eq!(
        "+10".parse::<PositionChange>().unwrap(),
        PositionChange::AdjustFixed(10.),
    );
    assert_eq!(
        "-10".parse::<PositionChange>().unwrap(),
        PositionChange::AdjustFixed(-10.),
    );

    assert_eq!(
        "10%".parse::<PositionChange>().unwrap(),
        PositionChange::SetProportion(10.)
    );
    assert_eq!(
        "+10%".parse::<PositionChange>().unwrap(),
        PositionChange::AdjustProportion(10.)
    );
    assert_eq!(
        "-10%".parse::<PositionChange>().unwrap(),
        PositionChange::AdjustProportion(-10.)
    );
    assert!("-".parse::<PositionChange>().is_err());
    assert!("10% ".parse::<PositionChange>().is_err());
}
