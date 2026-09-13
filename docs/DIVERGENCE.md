# Divergence from upstream niri

Invariant I5: prefer new modules; when an inherited file must be edited, add a line here. Format: `path/to/file.rs — what changed, and why.`

## Edits to inherited files

(workspace-wide) — mechanical niri→swayward rename, with no behavioural change. See the rename commit.
swayward-ipc/src/lib.rs — retain inherited config vocabulary in `legacy` beside the sway schema until milestone 4 moves binds to sway command strings.
src/ipc/client.rs, src/ipc/server.rs — import colliding inherited IPC types from `legacy` while the new sway schema keeps the bare names.
src/ipc/server.rs, src/ipc/wire.rs — replace niri's line-delimited request dispatch with tested sway binary framing and honest unsupported-message replies.
src/ipc/client.rs — remove niri's client because swaymsg is swayward's supported IPC client.
src/backend/winit.rs — disable Mesa swap throttling in nested sessions because a blocking EGL swap deadlocks the compositor event loop and IPC.
src/layout/mod.rs, src/layout/workspace.rs — expose focused-tree layout mutation and container focus navigation for sway runtime commands.
src/layout/mod.rs, src/layout/monitor.rs, src/layout/workspace.rs — give workspaces stable global sway names/numbers, sparse creation, switching, movement, and output assignment while retaining monitor animations.
src/ipc/server.rs — include the Wayland socket name in IPC socket paths so parallel headless tests do not collide.
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
src/handlers/compositor.rs, src/swayward.rs — apply for_window commands when a toplevel maps and remove its marks when it unmaps.
src/layout/mod.rs, src/handlers/mod.rs, src/protocols/foreign_toplevel.rs — store sway scratchpad windows and map foreign-toplevel minimize requests to hide/show them.
(workspace-wide) — run `cargo fmt --all` after the rename changed identifier sort order; no behavioural change.

## Deliberate behavioural deviations from sway

src/layout/workspace.rs — retire niri's horizontal viewport offset and its gesture state; the i3 tree always occupies the workspace view.
docs/wiki/, docs/mkdocs.yaml — retire scroll-layout documentation and rebrand retained niri subsystem guides for swayward.
src/layout/mod.rs, src/layout/workspace.rs, src/layout/tiling_tree/mod.rs — expose stable focused-node identity and targeted container layout mutation for sway command contexts.
src/ipc/server.rs — emit sway-shaped workspace, window, and binding-mode subscription payloads for waybar-compatible event streams.
swayward-config/src/binds.rs, src/input/mod.rs — add validated sway command-string binds while retaining inherited typed actions for unsupported compositor features.
swayward-config/src/binds.rs, src/input/mod.rs, src/ui/hotkey_overlay.rs — accept numeric bindcode triggers so translated sway bindcode directives remain functional.
