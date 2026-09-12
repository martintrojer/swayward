# Divergence from upstream niri

Invariant I5: prefer new modules; when an inherited file must be edited, add a line here. Format: `path/to/file.rs — what changed, and why.`

## Edits to inherited files

(workspace-wide) — mechanical niri→swayward rename, with no behavioural change. See the rename commit.
swayward-ipc/src/lib.rs — retain inherited config vocabulary in `legacy` beside the sway schema until milestone 4 moves binds to sway command strings.
src/ipc/client.rs, src/ipc/server.rs — import colliding inherited IPC types from `legacy` while the new sway schema keeps the bare names.
src/ipc/server.rs, src/ipc/wire.rs — replace niri's line-delimited request dispatch with tested sway binary framing and honest unsupported-message replies.
src/ipc/client.rs — remove niri's client because swaymsg is swayward's supported IPC client.
src/backend/winit.rs — disable Mesa swap throttling in nested sessions because a blocking EGL swap deadlocks the compositor event loop and IPC.
src/layout/mod.rs, src/layout/workspace.rs — expose focused-tree layout mutation for sway runtime commands.
swayward-ipc/src/message.rs — omit absent optional command-result fields to match sway's RUN_COMMAND reply schema.
.github/FUNDING.yml — remove inherited niri sponsorship attribution because swayward is not soliciting sponsorship.
.github/ISSUE_TEMPLATE/bug_report.md, .github/ISSUE_TEMPLATE/config.yml — point issue reporting and support at swayward rather than niri.
.github/workflows/ci.yml — retain only checks that protect supported swayward builds and stop publishing inherited niri documentation.
.github/workflows/release.yml — remove niri's release process until swayward has release artifacts and a versioning policy.
src/layout/mod.rs — export `tiling_tree` and apply cargo-fmt import reordering so the new i3 tree module is compiled and tested.
src/layout/workspace.rs — replace the scrolling tiling field and render element with TilingTree while preserving FloatingSpace.
src/layout/monitor.rs, src/layout/mod.rs — transfer focused tree tiles rather than concrete scrolling columns between workspaces and outputs.
src/ipc/server.rs — serve sway GET_TREE, GET_WORKSPACES, and GET_OUTPUTS from live compositor state instead of niri IPC requests.
src/layout/workspace.rs — expose a read-only TilingTree snapshot for sway GET_TREE serialization.
(workspace-wide) — run `cargo fmt --all` after the rename changed identifier sort order; no behavioural change.

## Deliberate behavioural deviations from sway

src/layout/workspace.rs — retire niri's horizontal viewport offset and its gesture state; the i3 tree always occupies the workspace view.
docs/wiki/, docs/mkdocs.yaml — retire scroll-layout documentation and rebrand retained niri subsystem guides for swayward.
