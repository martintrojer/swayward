# Key bindings

Key bindings are declared in the `binds` section of
`~/.config/swayward/config.kdl`.

```kdl
binds {
    Mod+Return { spawn "alacritty"; }
    Super+Alt+L { spawn "swaylock"; }
}
```

A hotkey consists of modifiers separated by `+`, followed by an XKB key name.
Valid modifiers include `Ctrl`, `Shift`, `Alt`, `Super`, `Mod3`, `Mod5`, and
`Mod`. `Mod` defaults to `Super` in a full session and `Alt` in a nested window.
Use `wev` to find XKB key names.

The inherited typed actions remain available for compositor functions such as
spawning programs, screenshots, floating state, and session control. Tree and
workspace operations use sway command strings so that config binds and
`swaymsg` share the same command language.

## Bind properties

Set `repeat=false` to prevent a held key from repeating. Set `cooldown-ms` to
limit how often a repeating bind runs.

```kdl
binds {
    Mod+T repeat=false { spawn "alacritty"; }
    XF86AudioRaiseVolume cooldown-ms=50 {
        spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "0.02+"
    }
}
```

Set `allow-when-locked=true` only on a `spawn` bind that must work while the
session is locked. Set `allow-inhibiting=false` on an escape bind that must work
while an application inhibits shortcuts.

## Pointer bindings

Mouse buttons and wheel directions can appear in bindings:

```kdl
binds {
    Mod+MouseLeft { close-window; }
    Mod+WheelScrollDown { spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "0.02-"; }
}
```

A pointer bind acts on the window focused when the event occurs.

## `spawn`

`spawn` executes a program directly. Write each argument as a separate KDL
string. It does not expand shell variables, pipelines, or `~` in arguments.

```kdl
binds {
    Mod+Return { spawn "alacritty" "-e" "/usr/bin/fish"; }
}
```

### `spawn-sh`

Use `spawn-sh` when shell syntax is required:

```kdl
binds {
    Mod+D { spawn-sh "fuzzel | sh"; }
}
```

## Common actions

### `toggle-window-rule-opacity`

This action toggles the opacity supplied by the matching window rule.

### Custom hotkey overlay titles

Set `hotkey-overlay-title` on a bind to control its label in the important
hotkeys overlay. Set it to `null` to hide that bind.

### Other actions

These inherited actions are useful outside the tree command language:

- `quit` exits after confirmation. Add `skip-confirmation=true` to bypass it.
- `screenshot`, `screenshot-screen`, and `screenshot-window` capture the screen.
- `toggle-window-floating` moves a window between the tree and floating layer.
- `toggle-windowed-fullscreen` changes the application's fullscreen state
  without covering the output.
- `toggle-keyboard-shortcuts-inhibit` escapes an application's shortcut
  inhibitor.

Run `swaymsg -t command '<command>'` for runtime tree and workspace commands.
See [IPC](./IPC.md).
