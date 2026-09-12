# Divergence from upstream niri

Invariant I5: prefer new modules; when an inherited file must be edited, add a line here. Format: `path/to/file.rs — what changed, and why.`

## Edits to inherited files

(workspace-wide) — mechanical niri→swayward rename, with no behavioural change. See the rename commit.
swayward-ipc/src/lib.rs — retain inherited config vocabulary in `legacy` beside the sway schema until milestone 4 moves binds to sway command strings.
src/ipc/client.rs, src/ipc/server.rs — import colliding inherited IPC types from `legacy` while the new sway schema keeps the bare names.

## Deliberate behavioural deviations from sway

(none yet)
