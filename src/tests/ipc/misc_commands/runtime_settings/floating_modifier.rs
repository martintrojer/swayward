#[test]
fn floating_modifier_settings_apply_at_runtime_like_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    // Sway spells the modifier Mod1..Mod5; swayward names them. The modifier
    // and the inverse bit are independent fields, and this is its own setting
    // rather than the compositor `mod_key`, which must not move
    // (`sway/include/sway/config.h:509-510`).
    use swayward_config::input::{FloatingModifier, ModKey};
    let floating = |f: &mut Fixture| f.swayward().config.borrow().input.floating_modifier;
    let mod_key_before = f.swayward().config.borrow().input.mod_key;
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Mod4")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Super,
            inverse: false
        })
    );
    assert_eq!(
        f.swayward().config.borrow().input.mod_key,
        mod_key_before,
        "floating_modifier must not move the compositor mod key"
    );
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Alt normal")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Alt,
            inverse: false
        })
    );
    // inverse is stored, not refused: it swaps the move and resize buttons.
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Mod4 inverse")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Super,
            inverse: true
        })
    );
    // `none` is a value, not a key name, and sway returns before reading the
    // second argument (`sway/sway/commands/floating_modifier.c:11-14`), so a
    // trailing word is ignored and the inverse bit resets.
    for command in ["floating_modifier none", "floating_modifier NONE inverse"] {
        assert!(
            crate::command::execute(f.niri_state(), "floating_modifier Mod4 inverse")[0].success
        );
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command} failed"
        );
        assert_eq!(
            floating(&mut f),
            Some(FloatingModifier {
                modifier: ModKey::None,
                inverse: false
            }),
            "{command} must disable the drag rather than name a key"
        );
    }
    // Sway validates the modifier before the mode, so an invalid modifier
    // wins over an invalid trailing word.
    for (command, expected) in [
        ("floating_modifier Mod9", "Invalid modifier"),
        ("floating_modifier Mod9 sideways", "Invalid modifier"),
        (
            "floating_modifier Mod4 sideways",
            "Usage: floating_modifier <mod> [inverse|normal]",
        ),
        (
            "floating_modifier",
            "Invalid floating_modifier command (expected at least 1 argument, got 0)",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected), "{command}");
    }
}
