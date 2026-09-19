# Migrate a sway config

The translator converts a sway or SwayFX configuration to swayward KDL. It keeps
every unsupported active directive as a source-located comment and also reports
it on standard error.

## Translate and validate the config

From the swayward repository, run:

```sh
contrib/sway-to-kdl ~/.config/sway/config >config.kdl
swayward validate -c config.kdl
```

The translator needs Python. Validation needs the installed `swayward` binary.
Cargo is not required.

Do not discard the translator's standard error. For example:

```text
manual attention: 3 directive(s)
  /home/user/.config/sway/config:24: unhandled output directive: bg /path/to/wallpaper fill
```

The generated file contains the same item as a `// sway-to-kdl:` comment. Search
for every such comment before using the configuration:

```sh
rg 'sway-to-kdl:' config.kdl
```

Top-level sway `exec` becomes `spawn-sh-at-startup`, which preserves its
startup-only behavior and shell syntax. `exec_always` also keeps its startup
effect, but emits manual attention because sway runs it again on reload while
swayward has no reload-time startup hook.

The repository tests translate pinned copies of the sway 1.11 and SwayFX default
configs, check that reported directives remain in the output, and parse the
resulting KDL. They also pin the counts, because this document quotes them: the
sway 1.11 default produces three manual-attention items and the SwayFX default
seven. A rise fails the suite. Directives that explicitly request behavior already
provided by swayward's defaults remain visible as `// sway-to-kdl: satisfied by
swayward defaults` comments, but do not count as manual-attention items. The project
has also translated and validated the upstream sway and SwayFX default files once
by hand. These checks do not prove that the translator covers a personal
configuration.

## Review bindings first

`bindsym` becomes a binding with a quoted sway command:

```sway
bindsym $mod+h focus left
```

```kdl
binds {
    Super+H { command "focus left"; }
}
```

`bindcode` becomes a `code:N` binding. Variables are expanded in source order.
The same command parser handles key bindings and `swaymsg` requests.

Sway binding modes become KDL `mode "name" { ... }` blocks. Bindings can enter
and leave modes with commands such as `mode "resize"` and `mode "default"`.

Commands outside swayward's current subset are also reported. Check the
[compatibility matrix](SWAY_COMPATIBILITY.md#runtime-commands) before replacing
or retaining a command by hand.

## Review unsupported directives

Check these areas after bindings:

- **Bars:** `bar {}` is not supported. Configure waybar separately. Waybar 0.15.0
  has passed a manual smoke test against swayward, but that test is not automated.
- **Inputs:** generic `type:touchpad` and `type:keyboard` blocks are translated.
  `xkb_numlock` becomes the keyboard `numlock` flag and preserves sway's
  boolean parsing. Device-specific selectors need manual conversion.
- **Gesture bindings:** `bindgesture` and `unbindgesture` are reported. Swayward
  keeps its additive four-finger overview swipe and forwards pinch and hold
  gestures to clients, but cannot bind swipe, pinch, or hold gestures to sway
  commands. Sway stores these
  bindings per mode and dispatches their commands when a tracked gesture ends
  (`sway/commands/gesture.c:40-57`; `sway/input/seatop_default.c:898-917`).
- **Switch bindings:** `bindswitch` and `unbindswitch` are reported. Swayward's
  `switch-events {}` supports only unconditional process spawning. It cannot
  preserve sway command dispatch, binding modes, `toggle`, `--locked`, or
  `--reload` behavior. Sway filters switch bindings by mode and lock state, and
  `--reload` re-runs state-specific bindings during reload
  (`sway/commands/bind.c:496-556`; `sway/input/switch.c:29-67`).
- **Outputs:** common mode, position, scale, enable, and disable settings are
  translated. Wallpaper and unmatched output settings are reported.
- **Window rules:** supported `for_window` effects with an `app_id` criterion
  become `window-rule` blocks. Other criteria or commands are reported.
- **Includes:** sway include globs are expanded during translation. Included sway
  directives are translated in place. Missing, repeated, or recursive files are
  reported.
- **Titlebars:** swayward renders fixed sway-like titlebars. There is no titlebar
  font or color configuration yet. The inherited `tab-indicator` remains available
  as a separate configurable accent. Sway itself accepts but ignores the legacy
  `client.background` and `client.placeholder` directives; the translator retains
  each line as a manual-attention comment with that distinction and source citation.

Read [Known deviations](KNOWN_DEVIATIONS.md) before switching sessions.

## Review SwayFX effects

SwayFX stores effects as flat directives. Swayward uses nested KDL blocks with
additional controls.

| SwayFX | swayward KDL |
|---|---|
| `blur` | Global `blur {}` plus `window-rule { background-effect {} }` |
| `corner_radius` | `window-rule { geometry-corner-radius; clip-to-geometry; }` |
| `shadows` | `layout { shadow {} }` |
| `dim_inactive` or `default_dim_inactive` | An unfocused `window-rule` with equivalent opacity |
| `layer_effects` | A namespace-matched `layer-rule` with nested effect settings |

Generated comments identify each mapping. Swayward keeps its defaults for
controls that SwayFX does not expose, including gradient interpolation and
animation curves. Review the generated blocks if you need the same appearance.

## Understand the compatibility boundary

The translator converts supported syntax. It does not make swayward
configuration-compatible with sway, and it does not add missing runtime
features.

Swayward's IPC tests compare live replies with captured sway fixtures, but they
do not verify every scalar value, request, event change, or byte-level JSON
encoding. Read [IPC oracle coverage](IPC_ORACLE_COVERAGE.md) for the measured
boundary before relying on an untested client behavior.
