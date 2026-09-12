# Divergence from upstream niri

Invariant I5: prefer new modules; when an inherited file must be edited, add a line here. Format: `path/to/file.rs — what changed, and why.`

## Edits to inherited files

(workspace-wide) — mechanical niri→swayward rename, with no behavioural change. See the rename commit.
swayward-ipc/src/lib.rs — retain inherited config vocabulary in `legacy` beside the sway schema until milestone 4 moves binds to sway command strings.
src/ipc/client.rs, src/ipc/server.rs — import colliding inherited IPC types from `legacy` while the new sway schema keeps the bare names.
src/ipc/server.rs, src/ipc/wire.rs — replace niri's line-delimited request dispatch with tested sway binary framing and honest unsupported-message replies.
src/ipc/client.rs — remove niri's client because swaymsg is swayward's supported IPC client.
src/layout/mod.rs — export `tiling_tree` and apply cargo-fmt import reordering so the new i3 tree module is compiled and tested.

## Deliberate behavioural deviations from sway

(none yet)
