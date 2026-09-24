> **A note on `Since:` annotations.** Options throughout these pages carry a
> `Since: 25.08` style marker. Those are **niri** version numbers, inherited
> along with the options themselves. swayward is a fork of niri and has not
> made releases under those numbers; treat the annotation as "this option has
> existed for a while" rather than as swayward release history.
>
> A few options say `Since: unreleased niri`. Those landed in niri after its
> last release and before our fork point, so they work here but carry no niri
> version number to cite.

### Per-Section Documentation

You can find documentation for various sections of the config on these wiki pages:

* [`input {}`](./Configuration:-Input.md)
* [`output "eDP-1" {}`](./Configuration:-Outputs.md)
* [`binds {}`](./Configuration:-Key-Bindings.md)
* [`switch-events {}`](./Configuration:-Switch-Events.md)
* [`layout {}`](./Configuration:-Layout.md)
* [top-level options](./Configuration:-Miscellaneous.md)
* [`window-rule {}`](./Configuration:-Window-Rules.md)
* [`layer-rule {}`](./Configuration:-Layer-Rules.md)
* [`animations {}`](./Configuration:-Animations.md)
* [`gestures {}`](./Configuration:-Gestures.md)
* [`recent-windows {}`](./Configuration:-Recent-Windows.md)
* [`debug {}`](./Configuration:-Debug-Options.md)
* [`include "other.kdl"`](./Configuration:-Include.md)

### Loading

swayward will load configuration from `$XDG_CONFIG_HOME/swayward/config.kdl` or `~/.config/swayward/config.kdl`, falling back to `/etc/swayward/config.kdl`.
If both of these files are missing, swayward will create `$XDG_CONFIG_HOME/swayward/config.kdl` with the contents of [the default configuration file](https://github.com/martintrojer/swayward/blob/main/resources/default-config.kdl), which are embedded into the swayward binary at build time.
Please use the default configuration file as the starting point for your custom configuration.

The configuration is live-reloaded.
Edit and save the config file, and swayward applies the changes.
This includes key bindings, output settings like mode, window rules, and everything else.

You can run `swayward validate` to parse the config and see any errors.

To use a different config file path, pass it in the `--config` or `-c` argument to `swayward`.

You can also set `$SWAYWARD_CONFIG` to the path of the config file.
`--config` always takes precedence.
If `--config` or `$SWAYWARD_CONFIG` doesn't point to a real file, the config will not be loaded.
If `$SWAYWARD_CONFIG` is set to an empty string, it is ignored and the default config location is used instead.

### Syntax

The config is written in [KDL].

#### Comments

Lines starting with `//` are comments; they are ignored.

Also, you can put `/-` in front of a section to comment out the entire section:

```kdl
/-output "eDP-1" {
    // Everything inside here is ignored.
    // The display won't be turned off
    // as the whole section is commented out.
    off
}
```

#### Flags

Toggle options in swayward are commonly represented as flags.
Writing out the flag enables it, and omitting it or commenting it out disables it.
For example:

```kdl
// "Focus follows mouse" is enabled.
input {
    focus-follows-mouse

    // Other settings...
}
```

```kdl
// "Focus follows mouse" is disabled.
input {
    // focus-follows-mouse

    // Other settings...
}
```

#### Sections

Most sections cannot be repeated. For example:

```kdl
// This is valid: every section appears once.
input {
    keyboard {
        // ...
    }

    touchpad {
        // ...
    }
}
```

```kdl,must-fail
// This is NOT valid: input section appears twice.
input {
    keyboard {
        // ...
    }
}

input {
    touchpad {
        // ...
    }
}
```

Exceptions are, for example, sections that configure different devices by name:

<!-- NOTE: this may break in the future -->
```kdl
output "eDP-1" {
    // ...
}

// This is valid: this section configures a different output.
output "HDMI-A-1" {
    // ...
}

// This is NOT valid: "eDP-1" already appeared above.
// It will either throw a config parsing error, or otherwise not work.
output "eDP-1" {
    // ...
}
```

### Defaults

Omitting most of the sections of the config file will leave you with the default values for that section.
A notable exception is [`binds {}`](./Configuration:-Key-Bindings.md): they do not get filled with defaults, so make sure you do not erase this section.

### Breaking Change Policy

As a rule, swayward updates should not break existing config files. This
policy is inherited from niri, where config files written for the first
release still parse years later.

Exceptions can be made for parsing bugs.
For example, niri used to accept multiple binds to the same key, but this was not intended and did not do anything (the first bind was always used).
A patch release changed it from silently accepting this to causing a parsing failure.
This is not a blanket rule: the impact of a breaking change is weighed before it lands.

Keep in mind that the breaking change policy applies only to releases.
Commits between releases can and do occasionally break the config as new features are ironed out.

[KDL]: https://kdl.dev/

---

*This page is adapted from the niri documentation.*
