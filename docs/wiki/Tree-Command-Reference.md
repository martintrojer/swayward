The tutorial is over. This page is the dry lookup table we carefully removed
from its path. Keep it nearby while muscle memory negotiates the change of
management.

## Cheat sheet

Every binding below is in
[`resources/default-config.kdl`](https://github.com/martintrojer/swayward/blob/main/resources/default-config.kdl). Rows
marked *(unbound by default)* name a command swayward implements but ships no
key for. Bind those yourself.

### Open and close

| Key | Action |
| --- | --- |
| `Mod+Return` | Open a terminal (`exec foot`) |
| `Mod+D` | Run launcher (`exec fuzzel`) |
| `Mod+Shift+Q` | Close the focused window (`kill`) |
| *(unbound by default)* | Open a browser, an editor, or any other app: add an `exec` bind |

### Focus and move

| Key | Action |
| --- | --- |
| `Mod+h/j/k/l` or `Mod+←/↓/↑/→` | Focus left, down, up, right |
| `Mod+Shift+h/j/k/l` or `Mod+Shift+arrows` | Move the focused window |
| `Mod+A` | Focus the parent container |
| `Mod+Ctrl+A` | Focus the child container |
| *(unbound by default)* | `focus mode_toggle`, `focus tiling`, `focus floating` |

### Layout

| Key | Action |
| --- | --- |
| `Mod+B` | Split the focused window horizontally (`split h`) |
| `Mod+V` | Split the focused window vertically (`split v`) |
| `Mod+W` | Set the parent layout to tabbed |
| `Mod+S` | Set the parent layout to stacking |
| `Mod+E` | Toggle the parent layout between splith and splitv |
| `Mod+F` | Fullscreen toggle |
| `Mod+Shift+Space` | Floating toggle, on a single window only |

### Resize

| Key | Action |
| --- | --- |
| `Mod+R` | Enter the `resize` binding mode |
| `h/j/k/l` or arrows, in resize mode | Resize the focused window by 10 px |
| `Return` or `Escape`, in resize mode | Return to the default mode |

### Workspaces

| Key | Action |
| --- | --- |
| `Mod+1` through `Mod+0` | Jump to workspace 1 through 10 |
| `Mod+Shift+1` through `Mod+Shift+0` | Send the focused window or container to that workspace |
| *(unbound by default)* | `workspace back_and_forth`, `workspace next`, `workspace prev` |
| *(unbound by default)* | `move workspace to output left`, and the other output targets |

### Scratchpad

| Key | Action |
| --- | --- |
| `Mod+Shift+Minus` | Stash the focused window into the scratchpad |
| `Mod+Minus` | Summon, hide, and cycle scratchpad windows |

### System

| Key | Action |
| --- | --- |
| `Mod+Shift+C` | Reload the config |
| `Mod+Shift+E` | Quit, with the confirmation dialog |
| `Ctrl+Alt+Delete` | Quit, with the confirmation dialog |
| `Mod+Shift+Slash` | Show the hotkey overlay |
| `Mod+Escape` | Toggle the keyboard-shortcuts inhibitor |
| `Super+Alt+L` | Lock the screen (`swaylock`) |
| `Print` | Screenshot |
| `Ctrl+Print` | Screenshot the whole screen |
| `Alt+Print` | Screenshot the focused window |

Volume, microphone, playback, and brightness keys are bound to their `XF86`
equivalents and keep working while the session is locked.

### Mouse (with `Mod` held)

| Gesture | Action |
| --- | --- |
| `Mod` and left-drag | Move the window |
| `Mod` and right-drag | Resize the window from the corner nearest the pointer, both ways at once |

### Inspect the tree

```
# Compact view
swaywardmsg -t get_tree -p | grep -E '"(name|layout)"'

# Find an app's app_id
swaywardmsg -t get_tree | jq -r '.. | select(.type?=="con" or .type?=="floating_con") | "\(.app_id // .window_properties.class // "?")  ::  \(.name)"'

# Send commands directly (bypass keybinds)
swaywardmsg "resize grow width 100 px"
swaywardmsg "[app_id=firefox] focus"
```

*Induction complete. Go build something structurally inadvisable. 🌳*

---

[← Induction V: floating, scratchpad, and sticky windows](Tree-School:-Floating-and-Scratchpad.md) · [Back to Cult of the Tree](Sway-School.md)
