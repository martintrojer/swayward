The translator converts a sway or SwayFX configuration to swayward KDL. It keeps
every unsupported active directive as a source-located comment and also reports
it on standard error.

## Translate and validate the config

```sh
swayward-sway-to-kdl ~/.config/sway/config >~/.config/swayward/config.kdl
swayward validate -c ~/.config/swayward/config.kdl
```

`swayward-sway-to-kdl` installs with swayward and needs Python. From a source
checkout it is `contrib/sway-to-kdl`. Cargo is not required either way.

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
[Sway compatibility guide](https://github.com/martintrojer/swayward/wiki/Sway-Compatibility)
before replacing or retaining a command by hand.

## Review unsupported directives

Check these areas after bindings:

### Replace swaybar

Swayward does not ship swaybar or read `bar {}` blocks. Configure Waybar in its
own JSON or JSONC and CSS files, then start it with:

```kdl
spawn-at-startup "waybar"
```

Do not copy `status_command`, `colors`, or other swaybar settings into KDL;
convert them to the corresponding Waybar modules and style. Waybar 0.15.0 has
passed a manual smoke test against swayward, but that test is not automated.

### Convert device-specific inputs

Generic `type:keyboard`, `type:pointer`, `type:touchpad`, `type:tablet_tool`, and
`type:touch` selectors are translated in block and single-line forms. A `*`
selector is translated for global XKB and repeat settings. Swayward cannot
configure an individual device by sway's `vendor:product:name` identifier;
input settings apply to every device of a class. Move settings from an
`input "<identifier>" {}` block to the matching `keyboard`, `mouse`, `touchpad`,
`tablet`, or `touch` KDL section only if applying them to the whole class is
acceptable. Otherwise keep the generated manual-attention comment.
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
- **Titlebars:** configure the font, Pango markup, padding, border thickness,
  and per-state colors in `layout { titlebar { ... } }`. The translator maps
  sway's `font` and `client.*` directives where titlebar fields have equivalent
  semantics. It reports the unmatched indicator and child-border colors; see
  [Client titlebar colors](KNOWN_DEVIATIONS.md#client-titlebar-colors). The
  inherited `tab-indicator` remains a separate configurable accent. Sway itself
  accepts but ignores the legacy `client.background` and `client.placeholder`
  directives; the translator retains each line as a manual-attention comment
  with that distinction and source citation.

## Real-world corpus check

A September 2026 audit translated 30 public personal and distribution sway
configs, including Waybar setups, old i3-derived configs, multi-output configs,
large `for_window` rule sets, modular includes, and SwayFX effects. After fixing
two KDL scalar-format bugs found by the audit, all 30 translations validated,
started in a capped nested swayward session, reloaded successfully through
`swaymsg`, and remained responsive. Every one of the 787 unsupported active
source directives appeared in both standard error and the generated KDL; none
was silently dropped.

The initial pass found 308 unsupported top-level or block-syntax items, 156
binding modifiers, 63 `for_window` commands, 59 `exec_always` directives, 55
X11-only `class` criteria, 37 device-specific input selectors, and 23 bars. A
translator follow-up reduced the total from 787 to 393. Grouped `set`, binding,
`exec`, and `for_window` blocks now translate, as do case-insensitive modifier
names, `Mod5`, `nofocus`, opacity, fixed-size, and shortcut-inhibitor window
rules. The remaining common categories are deliberate compatibility limits:
`exec_always` keeps its startup effect but cannot run again on reload;
device-specific input policy cannot be narrowed below a device class; swaybar
configuration belongs in Waybar; and X11 `class` is unavailable through
xwayland-satellite. [X11 window identity](KNOWN_DEVIATIONS.md#x11-window-identity)
describes the last limit. Preserve or replace these items by hand rather than
removing their comments. The pinned 12-entry reproducible audit remains in
the repository's internal migration audit;
the larger one-time sample was kept outside the repository because many source
repositories declare no reusable license.

Read [Differences from sway](https://github.com/martintrojer/swayward/wiki/Differences-from-Sway)
before switching sessions.

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
encoding. Read [Testing and conformance](https://github.com/martintrojer/swayward/wiki/Testing-and-Conformance)
before relying on an untested client behavior.
