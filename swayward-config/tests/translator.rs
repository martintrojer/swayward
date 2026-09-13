use std::path::PathBuf;
use std::process::Command;

use swayward_config::Config;

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
