This audit compares swayward's command parser with the commands that sway 1.12
makes reachable over IPC. It does not audit the other IPC request rows in
[`SWAY_COMPATIBILITY.md`](SWAY_COMPATIBILITY.md).

## Scope and method

This audit uses sway 1.12 commit
`88869399f421d9180dd8b6ed0b5a1f4a3585d252`, recorded in
`tests/sway/compatibility.toml` and matching
`tests/fixtures/sway/schema-version.json`. Sway selects `command_handlers[]`
while the configuration is active, then falls back to the shared `handlers[]`
table (`sway/sway/commands.c:44-129,162-173`).
<!-- compatibility-census:tables:start -->
The two tables contain 83 entries and 82 unique command names because
`fullscreen` appears in both.
<!-- compatibility-census:tables:end -->
`config_handlers[]` is excluded: sway consults it only while reading the config
file.

Each row below uses a complete invocation rather than a bare command name.
Most valid commands reject a bare name because an argument is missing. The
`Parser` column records the result from `swayward_ipc::command::parse`. The
`Execution` column records a source-level comparison of every accepted command
family with sway 1.12. `Complete` means that swayward implements the accepted
syntax, target types, and state changes. `Partial` names the missing or
different behavior in the `Classification` column.

Reproduce the parser measurement with:

```sh
./contrib/command-census
```

The script reads the invocations from this table and runs them through the
parser. It fails if a row is missing, if a command appears twice, if a probe's
top-level command does not match its row, or if the parser result differs from
the recorded result.

## Complete sway runtime command census

<!-- compatibility-census:command-table:start -->
| Command | Sway table and implementation | Probe | Parser | Execution | Existing state and apply path | Classification | Cost |
|---|---|---|---|---|---|---|---|
| `allow_tearing` | command; `commands/allow_tearing.c` | `allow_tearing yes` | reject | — | no asynchronous page-flip path | fail-loud: required capability is absent | subsystem |
| `assign` | shared; `commands/assign.c` | `assign [app_id="probe"] workspace 2` | accept | partial | live native-Wayland assignment criteria; applied to future windows at map time | partial: X11-only criteria fields are rejected | medium |
| `bar` | shared; `commands/bar.c` | `bar mode hide` | reject | — | no managed bar or `bar {}` state | deliberate subsystem deviation | subsystem |
| `bindcode` | shared; `commands/bindcode.c` | `bindcode 38 nop` | accept | partial | live `Config.binds` or the named or active mode binds; keyboard dispatch reads the vector directly | partial: keyboard forms work; mouse-region, XKB group, --to-code, and criteria forms are rejected | medium |
| `bindgesture` | shared; `commands/bindgesture.c` | `bindgesture swipe:3:left nop` | reject | — | no command-binding model for gesture events | unimplemented gesture binding | large |
| `bindswitch` | shared; `commands/bindswitch.c` | `bindswitch lid:on nop` | accept | partial | runtime mode-keyed switch-command overlay; switch dispatch reads it before KDL spawn events | partial: runtime commands, named and active modes, toggle, and --locked work; --reload and KDL-binding replacement are rejected | medium |
| `bindsym` | shared; `commands/bindsym.c` | `bindsym Mod4+Return nop` | accept | partial | live `Config.binds` or the named or active mode binds; keyboard dispatch reads the vector directly | partial: keyboard forms work; mouse-region, XKB group, --to-code, and criteria forms are rejected | medium |
| `border` | command; `commands/border.c` | `border pixel 2` | accept | partial | per-window border state; command applies directly | partial: extra arguments and sway-style integer widths differ | medium |
| `client.background` | shared; `commands/client.c` | `client.background #000000` | reject | — | sway deliberately ignores this i3 command | missing compatible no-op | small |
| `client.focused` | shared; `commands/client.c` | `client.focused #4c7899 #285577 #ffffff #2e9ef4 #285577` | reject | — | titlebar border, indicator, and child-border rendering is incomplete | fail-loud: accepted fields would otherwise be silently discarded | medium |
| `client.focused_inactive` | shared; `commands/client.c` | `client.focused_inactive #333333 #5f676a #ffffff #484e50 #5f676a` | reject | — | titlebar border, indicator, and child-border rendering is incomplete | fail-loud: accepted fields would otherwise be silently discarded | medium |
| `client.focused_tab_title` | shared; `commands/client.c` | `client.focused_tab_title #333333 #5f676a #ffffff` | accept | complete | `Config.layout.titlebar.focused_tab_title`; titlebar renderer consumes border, background, and text | complete; sway deliberately ignores this class's optional indicator and child-border colours | done |
| `client.placeholder` | shared; `commands/client.c` | `client.placeholder #000000 #0c0c0c #ffffff` | reject | — | sway deliberately ignores this i3 command | missing compatible no-op | small |
| `client.unfocused` | shared; `commands/client.c` | `client.unfocused #333333 #222222 #888888 #292d2e #222222` | reject | — | titlebar border, indicator, and child-border rendering is incomplete | fail-loud: accepted fields would otherwise be silently discarded | medium |
| `client.urgent` | shared; `commands/client.c` | `client.urgent #2f343a #900000 #ffffff #900000 #900000` | reject | — | titlebar border, indicator, and child-border rendering is incomplete | fail-loud: accepted fields would otherwise be silently discarded | medium |
| `create_output` | command; `commands/create_output.c` | `create_output` | accept | partial | headless creates a 1920x1080 output; tty and winit return sway's unsupported-backend failure without changing output state | partial: only the headless backend is supported, and criteria targets are rejected | subsystem |
| `default_border` | shared; `commands/default_border.c` | `default_border pixel 2` | accept | partial | new tiles snapshot `Config.layout.default_border`; existing windows keep their border | partial: malformed widths and extra arguments fail loud instead of following sway's unchecked `atoi`, and criteria targets are rejected | medium |
| `default_floating_border` | shared; `commands/default_floating_border.c` | `default_floating_border pixel 2` | accept | partial | new floating windows snapshot `Config.layout.default_floating_border`; existing windows keep their border | partial: malformed widths and extra arguments fail loud instead of following sway's unchecked `atoi`, and criteria targets are rejected | medium |
| `exec` | shared; `commands/exec.c` | `exec true` | accept | partial | command launches the process directly | partial: criteria-targeted execution is rejected | small |
| `exec_always` | shared; `commands/exec_always.c` | `exec_always true` | accept | partial | command launches the process directly | partial: criteria-targeted execution is rejected | small |
| `exit` | command; `commands/exit.c` | `exit` | accept | partial | bare command stops the compositor; matched criteria fail before lifecycle state changes | partial: criteria-prefixed execution is deliberately rejected because sway repeats the global handler per matched container | small |
| `floating` | command; `commands/floating.c` | `floating toggle` | accept | partial | per-window floating state; split-container targets fail before mutation | partial: sway can float a whole split container, but swayward has no floating-group representation | large |
| `floating_maximum_size` | shared; `commands/floating_minmax_size.c` | `floating_maximum_size 0 x 0` | accept | partial | `Config.layout.floating_maximum_size`; `layout.update_config` | partial: Rust integer parsing differs from `strtol` and malformed or out-of-range values fail loud instead of relying on unchecked C casts; criteria execution matches sway's once-per-match behavior | medium |
| `floating_minimum_size` | shared; `commands/floating_minmax_size.c` | `floating_minimum_size 75 x 50` | accept | partial | `Config.layout.floating_minimum_size`; `layout.update_config` | partial: Rust integer parsing differs from `strtol` and malformed or out-of-range values fail loud instead of relying on unchecked C casts; criteria execution matches sway's once-per-match behavior | medium |
| `floating_modifier` | shared; `commands/floating_modifier.c` | `floating_modifier Mod4` | accept | partial | `Config.input.floating_modifier`, an independent modifier plus inverse bit; config command path reapplies it | partial: Lock and Mod2 are refused because swayward has no lock-modifier mod key, and criteria targets are rejected | medium |
| `focus` | shared; `commands/focus.c` | `focus left` | accept | partial | layout focus state; command applies directly | partial: named-output failures match sway; some container/output traversal differs | large |
| `focus_follows_mouse` | shared; `commands/focus_follows_mouse.c` | `focus_follows_mouse yes` | accept | partial | `Config.input.focus_follows_mouse`, storing sway's no/yes/always; config command path reapplies it | partial: the KDL config file can only spell `yes`, so a reload cannot restore `always`, and criteria targets are rejected | medium |
| `focus_on_window_activation` | shared; `commands/focus_on_window_activation.c` | `focus_on_window_activation smart` | accept | complete | `Config.focus_on_window_activation`; XDG activation fallback policy | implemented, with window-rule overrides taking precedence | done |
| `focus_wrapping` | shared; `commands/focus_wrapping.c` | `focus_wrapping yes` | accept | complete | `Config.layout.focus_wrapping`; `layout.update_config` | implemented, including criteria execution once per match | done |
| `font` | shared; `commands/font.c` | `font monospace 10` | accept | partial | `Config.layout.titlebar.font` and markup mode; Pango validation; `layout.update_config` | partial: criteria-targeted execution is rejected | small |
| `for_window` | shared; `commands/for_window.c` | `for_window [app_id="probe"] nop` | accept | partial | live window-rule list; applied to matching commands | partial: portable criteria, including xdg-toplevel tags, match sway; X11-only identity fails loud, and nested command coverage remains a subset | large |
| `force_display_urgency_hint` | shared; `commands/force_display_urgency_hint.c` | `force_display_urgency_hint 500 ms` | accept | complete | `Config.urgent_timeout_ms`; urgency timers read it | implemented, including criteria execution once per match | done |
| `force_focus_wrapping` | shared; `commands/force_focus_wrapping.c` | `force_focus_wrapping yes` | accept | complete | alias for `Config.layout.focus_wrapping`; `layout.update_config` | implemented, including criteria execution once per match | done |
| `fullscreen` | shared and command; `commands/fullscreen.c` | `fullscreen toggle` | accept | complete | per-window or workspace state; command applies directly | complete | done |
| `gaps` | shared; `commands/gaps.c` | `gaps inner current set 10` | accept | partial | global gap defaults and per-workspace gap state; command applies directly | partial: criteria-targeted execution is rejected | medium |
| `hide_edge_borders` | shared; `commands/hide_edge_borders.c` | `hide_edge_borders none` | accept | partial | `Config.layout.hide_edge_borders`; `layout.update_config` | partial: `--i3` loses hide-lone-tab behavior and criteria targets are rejected | medium |
| `inhibit_idle` | command; `commands/inhibit_idle.c` | `inhibit_idle focus` | reject | — | no user-inhibitor object or sway policy modes | fail-loud: required state is absent | subsystem |
| `input` | shared; `commands/input.c`; `commands/input/xkb_switch_layout.c` | `input type:keyboard xkb_switch_layout next` | accept | partial | live seat XKB state; command applies directly | partial: `xkb_switch_layout next|prev|N` works for `*`, `type:keyboard`, and exact identifiers; all persistent input settings remain fail-loud | subsystem |
| `kill` | command; `commands/kill.c` | `kill` | accept | complete | command closes the selected window | complete | done |
| `layout` | command; `commands/layout.c` | `layout tabbed` | accept | partial | container layout state; command applies directly | partial: direct, default, and toggle criteria targets work; deeply nested singleton layout cases remain under-tested | medium |
| `mark` | command; `commands/mark.c` | `mark probe` | accept | complete | per-container mark state; command applies directly and re-evaluates new `for_window` matches once per view | implemented | done |
| `max_render_time` | command; `commands/max_render_time.c` | `max_render_time 1` | reject | — | no per-view render deadline | fail-loud: required state is absent | subsystem |
| `mode` | shared; `commands/mode.c` | `mode default` | accept | partial | runtime mode table and binding mode state; nested `set` and bind/unbind subcommands apply directly to the named mode | partial: nested key and switch binds work; nested gesture binds and criteria targets are rejected | medium |
| `mouse_warping` | shared; `commands/mouse_warping.c` | `mouse_warping output` | accept | partial | `Config.input.mouse_warping`, storing sway's no/output/container; config command path reapplies it | partial: the KDL config file has no spelling for the policy, so a reload cannot restore it, and criteria targets are rejected | medium |
| `move` | command; `commands/move.c` | `move left` | accept | partial | tree and floating geometry; command applies directly | partial: accepted forms cover windows and tiling-tree containers; forms requiring a floating group fail before mutation | large |
| `new_float` | shared; `commands/new_float.c` | `new_float pixel 2` | accept | partial | alias changes the default only for future floating windows | partial: aliases strict default-border parsing rather than sway's unchecked `atoi`, and criteria targets are rejected | medium |
| `new_window` | shared; `commands/new_window.c` | `new_window pixel 2` | accept | partial | alias changes the default only for future tiled windows | partial: aliases strict default-border parsing rather than sway's unchecked `atoi`, and criteria targets are rejected | medium |
| `no_focus` | shared; `commands/no_focus.c` | `no_focus [app_id="probe"]` | accept | partial | live native-Wayland no-focus criteria; applied to future windows at map time | partial: X11-only criteria fields are rejected | medium |
| `nop` | command; `commands/nop.c` | `nop probe` | accept | complete | no state by definition | implemented no-op | done |
| `opacity` | command; `commands/opacity.c` | `opacity set 0.5` | reject | — | rule-derived opacity exists, but no mutable container opacity | fail-loud: required state is absent | medium |
| `output` | shared; `commands/output.c` | `output * scale 1` | accept | partial | runtime output config mutates `Config.outputs` and reapplies it; wildcard fans out over connected outputs | partial: enable/disable/mode require DRM to take effect; power/dpms and other output subcommands remain fail-loud | subsystem |
| `popup_during_fullscreen` | shared; `commands/popup_during_fullscreen.c` | `popup_during_fullscreen smart` | accept | complete | `Config.popup_during_fullscreen`; config command path reapplies it | implemented, including criteria execution once per match | done |
| `reload` | command; `commands/reload.c` | `reload` | accept | partial | bare command reloads KDL; matched criteria fail before lifecycle state changes | partial: criteria-prefixed execution is deliberately rejected because sway parses and schedules reload once per matched container | small |
| `rename` | command; `commands/rename.c` | `rename workspace to probe` | accept | complete | workspace identity; command applies directly | implemented | small |
| `resize` | command; `commands/resize.c` | `resize grow width 10 px` | accept | partial | tree or floating geometry; command applies directly | partial: directional resize requires a window target; sway also resizes containers | medium |
| `scratchpad` | command; `commands/scratchpad.c` | `scratchpad show` | accept | partial | scratchpad state; command applies directly | partial: split-container targets fail before mutation because swayward has no floating-group representation | large |
| `seat` | shared; `commands/seat.c` | `seat seat0 hide_cursor 1000` | reject | — | single-seat runtime state and KDL input model | unimplemented subsystem mutation | subsystem |
| `set` | shared; `commands/set.c` | `set $probe value` | accept | partial | runtime sway-variable table; command dispatch expands later arguments | partial: runtime substitution works, but KDL has no config-defined symbol table and criteria targets are rejected | medium |
| `shortcuts_inhibitor` | command; `commands/shortcuts_inhibitor.c` | `shortcuts_inhibitor enable` | accept | complete | per-view future-request policy; command applies directly | implemented | done |
| `show_marks` | shared; `commands/show_marks.c` | `show_marks yes` | accept | complete | `Config.layout.titlebar.show_marks`; titlebar rendering | implemented, including criteria execution once per match | done |
| `smart_borders` | shared; `commands/smart_borders.c` | `smart_borders on` | accept | partial | `Config.layout.smart_borders`; `layout.update_config` | partial: accepts the KDL `no-gaps` alias; criteria execution matches sway's once-per-match behavior | small |
| `smart_gaps` | shared; `commands/smart_gaps.c` | `smart_gaps on` | accept | complete | `Config.layout.smart_gaps`; workspace working-area recomputation | implemented, including inverse_outer and runtime toggle | done |
| `split` | command; `commands/split.c` | `split h` | accept | complete | container layout state; command applies directly | complete, including singleton-parent flattening and criteria targets | done |
| `splith` | command; `commands/splith.c` | `splith` | accept | complete | alias for split horizontal | complete | done |
| `splitt` | command; `commands/splitt.c` | `splitt` | accept | complete | alias for split toggle | complete | done |
| `splitv` | command; `commands/splitv.c` | `splitv` | accept | complete | alias for split vertical | complete | done |
| `sticky` | command; `commands/sticky.c` | `sticky toggle` | accept | partial | per-window sticky state; command applies directly | partial: split-container targets fail before mutation because swayward has no floating-group representation | large |
| `swap` | command; `commands/swap.c` | `swap container with mark probe` | accept | partial | tree state; command applies directly | partial: `id` is rejected because xwayland-satellite does not expose X11 window IDs | medium |
| `tiling_drag` | shared; `commands/tiling_drag.c` | `tiling_drag yes` | accept | complete | `Config.input.tiling_drag`; gates modifier-held tiled-window move grabs | implemented and headlessly verified with real pointer input | done |
| `tiling_drag_threshold` | shared; `commands/tiling_drag_threshold.c` | `tiling_drag_threshold 9` | accept | complete | `Config.input.tiling_drag_threshold`; tiled-window move recognition distance | implemented and headlessly verified at the threshold boundary with real pointer input | done |
| `title_align` | shared; `commands/title_align.c` | `title_align center` | accept | complete | `Config.layout.titlebar.alignment`; titlebar rendering | implemented, including criteria execution once per match | done |
| `title_format` | command; `commands/title_format.c` | `title_format %title` | accept | complete | per-window or split-container title format; command applies directly | complete | done |
| `titlebar_border_thickness` | shared; `commands/titlebar_border_thickness.c` | `titlebar_border_thickness 1` | accept | partial | `Config.layout.titlebar.border_thickness`; bounded by vertical padding and rendered as an inset ring | partial: criteria-targeted execution is rejected | small |
| `titlebar_padding` | shared; `commands/titlebar_padding.c` | `titlebar_padding 4 3` | accept | partial | `Config.layout.titlebar.padding`; both axes bounded below by titlebar border thickness; `layout.update_config` | partial: criteria-targeted execution is rejected | small |
| `unbindcode` | shared; `commands/unbindcode.c` | `unbindcode 38` | accept | partial | live `Config.binds` or active mode binds; removal is visible to keyboard dispatch | partial: keyboard forms work; mouse-region, XKB group, --to-code, and criteria forms are rejected | medium |
| `unbindgesture` | shared; `commands/unbindgesture.c` | `unbindgesture swipe:3:left` | reject | — | no command-binding model for gesture events | unimplemented gesture binding | large |
| `unbindswitch` | shared; `commands/unbindswitch.c` | `unbindswitch lid:on` | accept | partial | runtime mode-keyed switch-command overlay; removal is visible to switch dispatch | partial: runtime commands, named and active modes, toggle, and --locked work; --reload and KDL-binding replacement are rejected | medium |
| `unbindsym` | shared; `commands/unbindsym.c` | `unbindsym Mod4+Return` | accept | partial | live `Config.binds` or active mode binds; removal is visible to keyboard dispatch | partial: keyboard forms work; mouse-region, XKB group, --to-code, and criteria forms are rejected | medium |
| `unmark` | command; `commands/unmark.c` | `unmark probe` | accept | complete | per-container mark state; command applies directly | implemented | done |
| `urgent` | command; `commands/urgent.c` | `urgent toggle` | accept | partial | per-window urgency state; command applies directly | partial: manual urgency works; `allow` and `deny` remain fail-loud | medium |
| `workspace` | shared; `commands/workspace.c` | `workspace probe` | accept | partial | workspace tree state and per-workspace configuration; command applies directly | partial: criteria targets are unsupported | medium |
| `workspace_auto_back_and_forth` | shared; `commands/workspace_auto_back_and_forth.c` | `workspace_auto_back_and_forth yes` | accept | complete | `Config.workspace_auto_back_and_forth`; config command path reapplies it | implemented, including criteria execution once per match | done |
<!-- compatibility-census:command-table:end -->

## Result

<!-- compatibility-census:result:start -->
The parser accepts 68 of sway's 82 unique runtime command names and rejects 14.
Of the accepted names, 24 implement the full audited command family and
44 are partial. `Partial` includes commands that reject a sway-supported
form, accept a form with a different effect, or do not support sway's criteria
target for that command.

The 14 rejects break down as follows:

- 2 deliberate sway-compatible no-ops that swayward has not accepted yet:
  `client.background` and `client.placeholder`.
- 1 deliberate subsystem deviation: `bar`.
- 8 structured failures for capabilities or state that swayward does not have:
  `allow_tearing`, `client.focused`, `client.focused_inactive`, `client.unfocused`, `client.urgent`, `inhibit_idle`, `max_render_time`, `opacity`.
- 3 implementation gaps: 2 binding commands; 0 criteria or variable commands;
  1 input, output, or seat namespaces; 0 titlebar color commands; and 0 settings
  or behaviors.
<!-- compatibility-census:result:end -->

`urgent` counts as accepted because `urgent enable|disable|toggle` parses and
executes. Its separate `allow|deny` policy syntax remains fail-loud. An accepted
probe establishes one implemented form, not complete command-family parity.
The parser also accepts
`default_orientation`, `workspace_layout`, and `xwayland`, but those names come
from sway's config-only table and therefore do not increase the
<!-- compatibility-census:accepted-total:start -->68-command<!-- compatibility-census:accepted-total:end -->
runtime count.

## Follow-up work

The audit groups the accepted-command gaps into focused tasks:

- `runtime-accepted-criteria-targets`: apply criteria prefixes to the remaining
  accepted command families.
- `runtime-criteria-language-parity`: complete the portable criteria language
  used by `for_window` and command prefixes.
- `runtime-parser-value-parity`: match sway's accepted values and argument
  handling.
- `runtime-client-color-completeness`: retain all five sway client colour
  fields.
- `runtime-default-border-semantics`: match border parsing and default lifetime.
- `runtime-exec-startup-id`: preserve `--no-startup-id` during execution.
- `runtime-pointer-policy-modes`: represent distinct focus, warp, and floating
  modifier modes.
- `runtime-focus-layout-semantics`: complete focus, layout, and split behavior.
- `runtime-floating-group-commands`: decide commands that target floating
  container groups, which swayward does not model.
- `runtime-mode-definition`: support runtime mode definitions.
- `runtime-swap-x11-id`: resolve `swap ... id` as an X11 window ID.
- `runtime-titlebar-command-semantics`: complete font, padding, and title-format
  behavior.
- `runtime-workspace-gap-forms`: add workspace gap and output-list forms.
- `runtime-mark-rule-reevaluation`: rerun mark-dependent `for_window` rules.

The existing `runtime-criteria-commands`, `runtime-set-command`,
`runtime-binding-commands`, `runtime-config-new-fields`, and
`runtime-subsystem-commands` tasks cover rejected command families found by the
same audit.

<!-- compatibility-census:history:start -->
The prior 81-probe count omitted the duplicate-table normalization rather than
a sway command. This census uses unique runtime command names, which is the
stable quantity a user can invoke.
<!-- compatibility-census:history:end -->
