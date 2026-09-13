use std::path::PathBuf;
use std::process::Command;

use swayward_config::Config;

#[test]
fn sway_outer_gaps_translate_to_equal_struts() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-outer-gaps-{}-{}.conf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(&fixture, "gaps outer 10\n").unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    let translated = String::from_utf8(output.stdout).unwrap();
    assert!(translated.contains("        left 10"), "{translated}");
    assert!(translated.contains("        right 10"), "{translated}");
    assert!(translated.contains("        top 10"), "{translated}");
    assert!(translated.contains("        bottom 10"), "{translated}");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "manual attention: none\n"
    );
    Config::parse_mem(&translated).unwrap();
}

#[test]
fn sway_titlebar_settings_translate_without_silently_dropping_colors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-titlebar-{}-{}.conf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(
        &fixture,
        "font pango:Fira Sans 11\nclient.focused #111111 #223344 #ffffff #555555 #666666\nclient.focused_inactive #111111 #334455 #eeeeee\nclient.focused_tab_title #111111 #445566 #dddddd\nclient.unfocused #111111 #556677 #cccccc\nclient.urgent #111111 #667788 #bbbbbb\n",
    )
    .unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    let translated = String::from_utf8(output.stdout).unwrap();
    assert!(translated.contains("font \"Fira Sans 11\""), "{translated}");
    for (state, background, text) in [
        ("focused", "#223344", "#ffffff"),
        ("focused-inactive", "#334455", "#eeeeee"),
        ("focused-tab-title", "#445566", "#dddddd"),
        ("unfocused", "#556677", "#cccccc"),
        ("urgent", "#667788", "#bbbbbb"),
    ] {
        assert!(
            translated.contains(&format!("        {state} {{")),
            "{translated}"
        );
        assert!(
            translated.contains(&format!("background-color \"{background}\"")),
            "{translated}"
        );
        assert!(
            translated.contains(&format!("text-color \"{text}\"")),
            "{translated}"
        );
    }
    let summary = String::from_utf8(output.stderr).unwrap();
    assert_eq!(summary.lines().skip(1).count(), 5, "{summary}");
    assert!(translated.contains("border/indicator/child_border colors have no titlebar equivalent"));
    assert!(
        translated.contains("client.focused-tab-title border colors have no titlebar equivalent")
    );
    Config::parse_mem(&translated).unwrap();
}

#[test]
fn sway_workspace_output_uses_first_preference() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-workspace-output-{}-{}.conf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(
        &fixture,
        "workspace 7:web output missing HDMI-A-1\nworkspace chat room output DP-2\n",
    )
    .unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    let translated = String::from_utf8(output.stdout).unwrap();
    // sway checks configured outputs in order and falls back normally when none exist.
    assert!(
        translated.contains("workspace \"7:web\" {\n    open-on-output \"missing\"\n}"),
        "{translated}"
    );
    assert!(!translated.contains("open-on-output \"HDMI-A-1\""));
    assert!(
        translated.contains("workspace \"chat room\" {\n    open-on-output \"DP-2\"\n}"),
        "{translated}"
    );
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "manual attention: none\n"
    );
    Config::parse_mem(&translated).unwrap();
}

#[test]
fn upstream_sway_and_swayfx_defaults_translate_to_valid_config() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for fixture in ["sway-1.11-default.conf", "swayfx-default.conf"] {
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(root.join("tests/fixtures/config").join(fixture))
            .output()
            .unwrap();
        assert!(output.status.success(), "{fixture}: {output:?}");

        let translated = String::from_utf8(output.stdout).unwrap();
        Config::parse_mem(&translated).unwrap_or_else(|error| {
            panic!("{fixture} generated invalid KDL: {error:?}\n{translated}")
        });

        let summary = String::from_utf8(output.stderr).unwrap();
        assert!(
            summary.starts_with("manual attention:"),
            "{fixture}: {summary}"
        );
        if fixture == "sway-1.11-default.conf" {
            assert!(
                summary.starts_with("manual attention: 5 directive(s)"),
                "{fixture}: {summary}"
            );
        }
        for item in summary.lines().skip(1) {
            assert!(
                translated.contains(item.trim()),
                "{fixture} silently dropped: {item}"
            );
        }
    }
}
