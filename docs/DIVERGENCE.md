# Divergence from upstream niri

Invariant I5: prefer new modules; when an inherited file must be edited, add a line here. Format: `path/to/file.rs — what changed, and why.`

## Edits to inherited files

(workspace-wide) — mechanical niri→swayward rename, with no behavioural change. See the rename commit.
swayward-ipc/src/lib.rs, swayward-ipc/src/legacy.rs, swayward-ipc/src/command.rs — retain inherited config vocabulary in `legacy` beside the sway schema until milestone 4 moves binds to sway command strings, carry internal window-move events for sway IPC translation, and parse sway's floating/tiling focus modes.
src/ipc/client.rs, src/ipc/server.rs — import colliding inherited IPC types from `legacy` while the new sway schema keeps the bare names.
src/ipc/server.rs, src/ipc/wire.rs — replace niri's line-delimited request dispatch with tested sway binary framing and honest unsupported-message replies.
src/ipc/client.rs — remove niri's client because swaymsg is swayward's supported IPC client.
src/backend/winit.rs — disable Mesa swap throttling in nested sessions because a blocking EGL swap deadlocks the compositor event loop and IPC.
src/layout/mod.rs, src/layout/workspace.rs, src/layout/tile.rs — expose focused-tree layout mutation and direct container focus for sway runtime commands, including sway's parent targeting, workspace-level layer history, one-level singleton flatten, and focus rank and parent preservation across floating transitions.
src/layout/tiling_tree/mod.rs — apply split commands to an empty workspace root, expose workspace-root focus, retain intentional nested and tabbed/stacked singleton wrappers, assign new siblings an equal share, use directional focus and resize by parent layout axis, and preserve MRU focus through removal and reinsertion, matching sway's tree behavior.
src/layout/mod.rs, src/window/mapped.rs — expose window titles to the tree renderer for sway-style server-side titlebars.
src/layout/mod.rs, src/layout/monitor.rs, src/layout/workspace.rs — give workspaces stable global sway names/numbers, sparse creation, switching, movement, output assignment, rename sorting, and sway-compatible inactive-workspace cleanup while retaining monitor animations.
src/layout/mod.rs — resolve relative move-to-workspace targets with sway's global numbered and per-output workspace ordering.
src/ipc/server.rs — include the Wayland socket name in IPC socket paths so parallel headless tests do not collide.
src/tests/mod.rs — run vendored i3 Perl assertions against the existing headless compositor and real sway IPC socket.
swayward-ipc/src/message.rs — omit absent optional command-result fields to match sway's RUN_COMMAND reply schema.
.github/FUNDING.yml — remove inherited niri sponsorship attribution because swayward is not soliciting sponsorship.
.github/ISSUE_TEMPLATE/bug_report.md, .github/ISSUE_TEMPLATE/config.yml — point issue reporting and support at swayward rather than niri.
.github/workflows/ci.yml — retain only checks that protect supported swayward builds and stop publishing inherited niri documentation.
.github/workflows/ci.yml — install the Perl modules required by the vendored i3 conformance runner.
.github/workflows/release.yml — remove niri's release process until swayward has release artifacts and a versioning policy.
src/layout/mod.rs — export `tiling_tree` and apply cargo-fmt import reordering so the new i3 tree module is compiled and tested.
src/layout/floating.rs, src/layout/mod.rs, src/layout/workspace.rs — route sway directional resize commands to a specific floating or tiled edge and wrap directional focus among floating window centers while retaining niri's configure-driven floating resize model.
src/layout/workspace.rs — replace the scrolling tiling field and render element with TilingTree while preserving FloatingSpace.
src/layout/monitor.rs, src/layout/mod.rs — transfer focused tree tiles rather than concrete scrolling columns between workspaces and outputs.
src/ipc/server.rs, src/ipc/tree.rs — serve sway GET_TREE, GET_WORKSPACES, and GET_OUTPUTS from live compositor state, including workspace focus membership, sway-compatible workspace number parsing, and back-to-front stacking order for floating children, instead of niri IPC requests.
src/layout/workspace.rs — expose a read-only TilingTree snapshot for sway GET_TREE serialization.
src/handlers/compositor.rs, src/swayward.rs — apply for_window commands when a toplevel maps and remove its marks when it unmaps.
src/layout/mod.rs, src/handlers/mod.rs, src/protocols/foreign_toplevel.rs — store sway scratchpad windows and map foreign-toplevel minimize requests to hide/show them.
src/command.rs — apply criteria-targeted scratchpad move and show commands to the matched window, matching sway's overridden-node command context.
(workspace-wide) — run `cargo fmt --all` after the rename changed identifier sort order; no behavioural change.

## Deliberate behavioural deviations from sway

src/layout/workspace.rs — retire niri's horizontal viewport offset and its gesture state; the i3 tree always occupies the workspace view.
docs/wiki/, docs/mkdocs.yaml — retire scroll-layout documentation and rebrand retained niri subsystem guides for swayward.
src/layout/mod.rs, src/layout/workspace.rs, src/layout/tiling_tree/mod.rs — expose stable focused-node identity and targeted container layout mutation for sway command contexts.
src/ipc/server.rs — emit sway-shaped workspace, window, binding-mode, and tick subscription payloads for waybar-compatible event streams, including ordered workspace init, focus, rename, and empty events.
swayward-config/src/binds.rs, src/input/mod.rs — add validated sway command-string binds while retaining inherited typed actions for unsupported compositor features.
swayward-config/src/binds.rs, src/input/mod.rs, src/ui/hotkey_overlay.rs — accept numeric bindcode triggers so translated sway bindcode directives remain functional.
swayward-ipc/src/command.rs, swayward-config/src/lib.rs, swayward-config/src/binds.rs, src/swayward.rs, src/input/mod.rs, src/command.rs, src/ipc/server.rs — support named binding modes, mode commands and events, GET_BINDING_MODES, and sway's full `layout toggle` grammar.
resources/default-config.kdl — replace scroll-layout and column-oriented default binds with sway's tree-oriented defaults; inherited typed actions remain available to existing configs.
src/ui/mru.rs — retain inherited typed focus actions as MRU navigation aliases for existing user configs; shipped sway-command binds do not generate these aliases.
CONTRIBUTING.md, resources/default-config.kdl, resources/swayward.desktop, resources/swayward.service, swayward-ipc/README.md, swayward-visual-tests/README.md, docs/wiki/Configuration:-Animations.md, docs/wiki/Configuration:-Debug-Options.md, docs/wiki/Configuration:-Input.md, docs/wiki/Configuration:-Outputs.md, docs/wiki/Configuration:-Recent-Windows.md, docs/wiki/Configuration:-Window-Rules.md — remove inherited branding and descriptions of the retired scrollable layout from user-facing text.
