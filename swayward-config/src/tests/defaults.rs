use insta::assert_snapshot;

use crate::Config;

fn diff_lines(expected: &str, actual: &str) -> String {
    let mut output = String::new();
    let mut in_change = false;

    for change in diff::lines(expected, actual) {
        match change {
            diff::Result::Both(_, _) => {
                in_change = false;
            }
            diff::Result::Left(line) => {
                if !output.is_empty() && !in_change {
                    output.push('\n');
                }
                output.push('-');
                output.push_str(line);
                output.push('\n');
                in_change = true;
            }
            diff::Result::Right(line) => {
                if !output.is_empty() && !in_change {
                    output.push('\n');
                }
                output.push('+');
                output.push_str(line);
                output.push('\n');
                in_change = true;
            }
        }
    }

    output
}

#[test]
fn diff_empty_to_default() {
    // We try to write the config defaults in such a way that empty sections (and an empty
    // config) give the same outcome as the default config bundled with niri. This test
    // verifies the actual differences between the two.
    let mut default_config = Config::load_default();
    let empty_config = Config::parse_mem("").unwrap();

    // Some notable omissions: the default config has some window rules and binding modes,
    // and an empty config will not have any binds. Clear them out so they don't spam the diff.
    default_config.window_rules.clear();
    default_config.binds.0.clear();
    default_config.binding_modes.clear();

    assert_snapshot!(
        diff_lines(
            &format!("{empty_config:#?}"),
            &format!("{default_config:#?}")
        ),
        @r#"
        -            numlock: false,
        +            numlock: true,

        -            tap: false,
        +            tap: true,

        -            natural_scroll: false,
        +            natural_scroll: true,

        -    spawn_at_startup: [],
        +    spawn_at_startup: [
        +        SpawnAtStartup {
        +            command: [
        +                "waybar",
        +            ],
        +        },
        +    ],

        -            off: false,
        +            off: true,

        -            off: true,
        +            off: false,

        -            on: false,
        +            on: true,

        -            off: false,
        +            off: true,

        -            vertical_padding: 4.0,
        -            border_thickness: 1,
        +            vertical_padding: 8.0,
        +            border_thickness: 4,

        -                    r: 0.29803923,
        -                    g: 0.47058824,
        -                    b: 0.6,
        +                    r: 1.0,
        +                    g: 0.78431374,
        +                    b: 0.49803922,

        -                    r: 0.15686275,
        -                    g: 0.33333334,
        -                    b: 0.46666667,
        +                    r: 0.28,
        +                    g: 0.46,
        +                    b: 0.64,

        -                    r: 0.2,
        -                    g: 0.2,
        -                    b: 0.2,
        +                    r: 0.3137255,
        +                    g: 0.3137255,
        +                    b: 0.3137255,

        -                    r: 0.37254903,
        -                    g: 0.40392157,
        -                    b: 0.41568628,
        +                    r: 0.16,
        +                    g: 0.16,
        +                    b: 0.16,

        -                    r: 0.2,
        -                    g: 0.2,
        -                    b: 0.2,
        +                    r: 0.3137255,
        +                    g: 0.3137255,
        +                    b: 0.3137255,

        -                    r: 0.37254903,
        -                    g: 0.40392157,
        -                    b: 0.41568628,
        +                    r: 0.16,
        +                    g: 0.16,
        +                    b: 0.16,

        -                    r: 0.2,
        -                    g: 0.2,
        -                    b: 0.2,
        +                    r: 0.3137255,
        +                    g: 0.3137255,
        +                    b: 0.3137255,

        -                    r: 0.13333334,
        -                    g: 0.13333334,
        -                    b: 0.13333334,
        +                    r: 0.16,
        +                    g: 0.16,
        +                    b: 0.16,

        -                    r: 0.53333336,
        -                    g: 0.53333336,
        -                    b: 0.53333336,
        +                    r: 1.0,
        +                    g: 1.0,
        +                    b: 1.0,

        -                    r: 0.18431373,
        -                    g: 0.20392157,
        -                    b: 0.22745098,
        +                    r: 0.60784316,
        +                    g: 0.0,
        +                    b: 0.0,

        -                    r: 0.5647059,
        -                    g: 0.0,
        -                    b: 0.0,
        +                    r: 0.16,
        +                    g: 0.16,
        +                    b: 0.16,

        -                0.3333333333333333,
        +                0.33333,

        -                0.6666666666666666,
        +                0.66667,

        -    prefer_no_csd: false,
        +    prefer_no_csd: true,

        -            off: false,
        +            off: true,

        -        on: true,
        +        on: false,
        "#,
    );
}
