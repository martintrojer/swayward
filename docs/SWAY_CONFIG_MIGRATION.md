# Translate a sway config

Run the translator once, review every reported item, and then use the generated KDL as your swayward config:

```sh
contrib/sway-to-kdl ~/.config/sway/config >config.kdl
swayward validate -c config.kdl
```

The translator uses Python because sway syntax needs quoted command preservation, variable expansion, block tracking, and glob expansion. Python's standard library provides those operations without adding a project dependency. End users need only Python and the installed `swayward` binary; Cargo is not required.

Sway and swayward both support includes. The translator expands sway include globs while translating because swayward KDL includes name one path at a time. It detects repeated or recursive files and reports them for manual attention. The generated file is self-contained; the included directives are translated in place rather than left as references to sway syntax. This is a syntax conversion, not a new include feature.

Every unsupported directive is retained as a `// sway-to-kdl:` comment in the output and listed on stderr. The script never silently drops an active directive. In particular, `bar {}` becomes a comment that directs you to waybar.

## SwayFX mappings

SwayFX stores effects as flat directives. Swayward uses the richer inherited KDL blocks, which also expose options that SwayFX does not have. The translator maps:

| SwayFX | swayward KDL |
|---|---|
| `blur` | global `blur {}` plus `window-rule { background-effect {} }` |
| `corner_radius` | `window-rule { geometry-corner-radius; clip-to-geometry; }` |
| `shadows` | `layout { shadow {} }` |
| `dim_inactive` or `default_dim_inactive` | an unfocused `window-rule` with the equivalent opacity |
| `layer_effects` | a namespace-matched `layer-rule` with nested `background-effect`, `shadow`, and corner-radius settings |

The generated comments name each mapping. A flat option can produce several nested keys. Swayward keeps its inherited defaults for controls that SwayFX does not expose, such as gradient interpolation and animation curves. Review those blocks if you want to tune the additional controls.

## Limits

- `bindsym` becomes a quoted `command "..."` bind after variable expansion.
- `bindcode` becomes a `code:<number>` bind.
- The translator converts supported `for_window` effects with an `app_id` criterion to `window-rule`. Other criteria or commands are reported for manual conversion.
- Sway binding modes and commands outside swayward's current command subset are reported, not guessed.
- Device-specific input selectors require manual conversion. Generic `type:touchpad` and `type:keyboard` blocks map to inherited input blocks.
- Common output mode, position, scale, enable, and disable options map to inherited output blocks. Wallpaper and other unmatched output options are reported.
