// Runner for unmodified layout tests from i3's Perl testsuite.

use std::any::Any;
use std::collections::HashSet;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel;
use wayland_client::Proxy as _;
use wayland_server::Resource as _;

use super::Fixture;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

fn pause_i3_poll() {
    thread::sleep(Duration::from_millis(1));
}

fn oracle_i3_dir() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/sway-ipc-oracle/i3");
    assert!(
        path.join("t").is_dir(),
        "i3 oracle is missing; run ./contrib/fetch-oracle"
    );
    path
}

struct AllowedRejection {
    test: &'static str,
    command: &'static str,
    reason: &'static str,
    /// Allow this command to be rejected any number of times.
    ///
    /// Only for a file whose own input is randomised, where the count is not
    /// reproducible. Every other entry stays exact, so an unexpected rejection
    /// or a changed count still fails.
    repeatable: bool,
}

impl AllowedRejection {
    fn matches(&self, test: &str, command: &str) -> bool {
        self.test == test
            && (self.command == command
                || (self.command == "[con_mark=\"*\"] focus"
                    && command.starts_with("[con_mark=\"")
                    && command.ends_with("\"] focus"))
                || (self.command == "[con_mark=a] move to workspace *"
                    && command.starts_with("[con_mark=a] move to workspace "))
                || (self.command == "[id= . *] focus output right"
                    && (command.starts_with("[id= . ") || command.starts_with("[con_id= . "))
                    && command.ends_with("] focus output right"))
                || (self.command == "[id=*] swap container with id *"
                    && command.starts_with("[id=")
                    && command.contains("] swap container with id "))
                || (self.command == "[app_id=b] swap with id *"
                    && command.starts_with("[app_id=b] swap with id "))
                || (self.command == "[con_id=*] layout stacked"
                    && command.starts_with("[con_id=")
                    && command.ends_with("] layout stacked")))
    }
}

// Every rejection from a passing conformance file must be reviewed here. Keying by both file and
// exact command prevents a new rejected setup command from hiding behind an unrelated exception.
const ALLOWED_REJECTIONS: &[AllowedRejection] = &[
    AllowedRejection {
        test: "113-urgent.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "122-split.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "135-floating-focus.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "138-floating-attach.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "140-focus-lost.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "141-resize.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "167-workspace_layout.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "192-layout.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "200-urgency-timer.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "246-window-decoration-focus.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },

    AllowedRejection {
        test: "319-gaps.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "510-focus-across-outputs.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "541-resize-set-tiling.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },

    AllowedRejection {
        test: "308-focus_wrapping.t",
        command: "[con_id=*] layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "550-split-redundant-containers.t",
        command: "layout tabbed, layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "176-workspace-baf.t",
        command: "restart",
        repeatable: false,
        reason: "sway has no runtime restart command; coverage classifies the dependent assertion as unproven",
    },
    AllowedRejection {
        test: "111-goto.t",
        command: "[con_mark=\"*\"] focus",
        repeatable: false,
        reason: "test asserts that an unknown mark leaves focus unchanged",
    },
    AllowedRejection {
        test: "218-regress-floating-split.t",
        command: "layout stacked",
        repeatable: false,
        reason: "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)",
    },
    AllowedRejection {
        test: "202-scratchpad-criteria.t",
        command: "[title=\"nomatch\"] scratchpad show",
        repeatable: false,
        reason: "the test expects unmatched criteria to leave focus unchanged",
    },
    AllowedRejection {
        test: "202-scratchpad-criteria.t",
        command: "[title=\"non-scratch\"] scratchpad show",
        repeatable: false,
        reason: "the test expects a matching non-scratchpad window to remain unchanged",
    },
    AllowedRejection {
        test: "202-scratchpad-criteria.t",
        command: "[title=\"nothingmatchthistitle\"] scratchpad show",
        repeatable: false,
        reason: "the test expects unmatched criteria to leave focus unchanged",
    },
    AllowedRejection {
        test: "184-regress-float-split-resize.t",
        command: "layout stacking",
        repeatable: false,
        reason: "the test targets the floating group root, which sway rejects with \
                 `Unable to change layout of floating windows`; only the following \
                 liveness assertion matters (sway/sway/commands/layout.c:128-131)",
    },
    AllowedRejection {
        test: "184-regress-float-split-resize.t",
        command: "resize grow up 10 px or 10 ppt",
        repeatable: false,
        reason: "the test only checks that the compositor remains live; sway \
                 answers `Cannot resize any further` when the grouped resize changes \
                 neither size fraction (sway/sway/commands/resize.c:273-279)",
    },
    AllowedRejection {
        test: "191-resize-levels.t",
        command: "resize grow left 10px or 25ppt",
        repeatable: false,
        reason: "sway changes an ancestor branch but compares only the targeted \
                 container's own fractions and therefore answers `Cannot resize any \
                 further` (sway/sway/commands/resize.c:265-279); the test asserts \
                 the ancestor proportions and they still match",
    },
    AllowedRejection {
        test: "189-floating-constraints.t",
        command: "resize grow up 10px or 10ppt",
        repeatable: false,
        reason: "the window is already at the configured floating maximum, so sway \
                 answers `Cannot resize any further` too; the next two assertions \
                 check the window did not move",
    },
    AllowedRejection {
        test: "132-move-workspace.t",
        command: "mark a",
        repeatable: false,
        reason: "sway rejects marks when no container is focused",
    },
    AllowedRejection {
        test: "120-multiple-cmds.t",
        command: "move gibberish",
        repeatable: false,
        reason: "the regression intentionally sends this invalid command eleven times",
    },
    AllowedRejection {
        test: "120-multiple-cmds.t",
        command: "bullshit-command-which-we-never-implement meh",
        repeatable: false,
        reason: "the test asserts that this invalid command returns an error",
    },
    AllowedRejection {
        test: "169-border-toggle.t",
        command: "border 1pixel",
        repeatable: false,
        reason: "i3-only alias; sway accepts the equivalent border pixel 1",
    },
    AllowedRejection {
        test: "141-resize.t",
        command: "resize grow right 10 px or 25 ppt",
        repeatable: false,
        reason: "the adapter's float is already at sway's automatic maximum",
    },
    AllowedRejection {
        test: "134-invalid-command.t",
        command: "blargh!",
        repeatable: false,
        reason: "the regression intentionally sends an invalid command",
    },
    AllowedRejection {
        test: "101-focus.t",
        command: "layout default",
        repeatable: false,
        reason: "sway rejects layout default before any previous split has been recorded",
    },
    AllowedRejection {
        test: "101-focus.t",
        command: "[con_mark=__does_not_exist] focus",
        repeatable: false,
        reason: "the assertion expects this unmatched criterion to fail",
    },
    AllowedRejection {
        test: "119-match.t",
        command: "[con_id=\"99999\"] kill",
        repeatable: false,
        reason: "the test verifies that an unmatched criterion leaves the window alive",
    },
    AllowedRejection {
        test: "260-invalid-criteria.t",
        command: "[con_id=foobar] kill",
        repeatable: false,
        reason: "the test intentionally sends a malformed con_id criterion",
    },
    AllowedRejection {
        test: "261-match-con_id-con_mark-combinations.t",
        command: "[con_id=__focused__ app_id=doesnotmatch] kill",
        repeatable: false,
        reason: "the test expects the combined criterion not to match",
    },
    AllowedRejection {
        test: "261-match-con_id-con_mark-combinations.t",
        command: "[con_mark=marked app_id=doesnotmatch] kill",
        repeatable: false,
        reason: "the test expects the combined criterion not to match",
    },
    AllowedRejection {
        test: "502-focus-output.t",
        command: "[con_mark=doesnotexist] focus output right",
        repeatable: false,
        reason: "the assertion expects the unmatched criterion to leave output focus unchanged",
    },
    AllowedRejection {
        test: "502-focus-output.t",
        command: "[id= . *] focus output right",
        repeatable: false,
        reason:
            "unchanged upstream file contains this malformed criterion and expects no focus change",
    },
    AllowedRejection {
        test: "294-focus-order.t",
        command: "[id=*] swap container with id *",
        repeatable: false,
        reason: "sway's id swap target is an X11 window id unavailable to native Wayland clients",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[app_id=b] swap with id *",
        repeatable: false,
        reason: "i3's optional swap words and X11 id target are unavailable in sway",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[con_mark=S1] swap with mark V1",
        repeatable: false,
        reason: "i3 permits omitted swap words; sway requires swap container with mark",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[con_mark=S1] swap with mark T1",
        repeatable: false,
        reason: "i3 permits omitted swap words; sway requires swap container with mark",
    },
    AllowedRejection {
        test: "126-regress-close.t",
        command: "mode toggle",
        repeatable: false,
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "127-regress-floating-parent.t",
        command: "mode toggle",
        repeatable: false,
        reason: "obsolete setup cannot create or restore the floating container under test",
    },
    AllowedRejection {
        test: "142-regress-move-floating.t",
        command: "mode toggle",
        repeatable: false,
        reason: "obsolete setup leaves the window tiled instead of testing a floating move",
    },
    AllowedRejection {
        test: "144-regress-floating-resize.t",
        command: "mode toggle",
        repeatable: false,
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "147-regress-floatingmove.t",
        command: "mode toggle",
        repeatable: false,
        reason: "obsolete setup leaves the parent tiled instead of testing floating-tree moves",
    },
    AllowedRejection {
        test: "148-regress-floatingmovews.t",
        command: "mode toggle",
        repeatable: false,
        reason: "obsolete setup leaves the window tiled, but the focus assertion remains valid",
    },
    AllowedRejection {
        test: "151-regress-float-size.t",
        command: "mode toggle",
        repeatable: false,
        reason: "obsolete setup omits both floating-to-tiling transitions under test",
    },
    AllowedRejection {
        test: "152-regress-level-up.t",
        command: "mode toggle",
        repeatable: false,
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "192-layout.t",
        command: "layout toggle stacked",
        repeatable: false,
        reason: "documented i3/sway layout-toggle divergence",
    },
    AllowedRejection {
        test: "292-regress-layout-toggle.t",
        command: "layout toggle 1337 1337",
        repeatable: false,
        reason: "the regression intentionally sends invalid layout names",
    },
    AllowedRejection {
        test: "273-regress-focus-toggle.t",
        command: "focus mode_toggle",
        repeatable: false,
        reason: "the liveness regression runs this command on an empty workspace",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace to 2",
        repeatable: false,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace to baz",
        repeatable: false,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace 5 to 2",
        repeatable: false,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace 1 to baz",
        repeatable: false,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "271-for_window_tilingfloating.t",
        command: "[tiling_from=\"auto\" con_mark=\"tiling\"] mark --add tiling_auto",
        repeatable: false,
        reason: "sway has no tiling provenance criterion",
    },
    AllowedRejection {
        test: "271-for_window_tilingfloating.t",
        command: "[floating_from=\"auto\" con_mark=\"floating\"] mark --add floating_auto",
        repeatable: false,
        reason: "sway has no floating provenance criterion",
    },
];

fn rejected_commands(stderr: &str) -> impl Iterator<Item = &str> {
    stderr.lines().filter_map(|line| {
        line.trim_start()
            .strip_prefix("# swayward rejected `")
            .and_then(|line| line.split_once("`: "))
            .map(|(command, _)| command)
    })
}

fn allowed_rejections(test: &str) -> Vec<&'static AllowedRejection> {
    ALLOWED_REJECTIONS
        .iter()
        .filter(|allowed| allowed.test == test)
        .collect()
}

fn expected_rejections(test: &str) -> Vec<&'static AllowedRejection> {
    allowed_rejections(test)
        .into_iter()
        .flat_map(|allowed| {
            let count = if allowed.command == "layout stacked" {
                match test {
                    "113-urgent.t" => 2,
                    "167-workspace_layout.t" => 4,
                    _ => 1,
                }
            } else if test == "308-focus_wrapping.t"
                && allowed.command == "[con_id=*] layout stacked"
            {
                32
            } else if test == "120-multiple-cmds.t" && allowed.command == "move gibberish" {
                11
            } else if matches!(
                test,
                "127-regress-floating-parent.t" | "151-regress-float-size.t"
            ) && allowed.command == "mode toggle"
            {
                2
            } else if test == "294-focus-order.t"
                && allowed.command == "[id=*] swap container with id *"
            {
                3
            } else {
                1
            };
            std::iter::repeat_n(allowed, count)
        })
        .collect()
}

fn rejections_match(test: &str, rejected: &[&str]) -> bool {
    let expected = expected_rejections(test);
    // Collapse runs of a repeatable command, whose count is not reproducible
    // because the file randomises its own input. Everything else is compared
    // exactly, in order.
    let repeatable = |command: &str| {
        expected
            .iter()
            .any(|allowed| allowed.repeatable && allowed.matches(test, command))
    };
    let mut collapsed: Vec<&str> = Vec::new();
    for command in rejected {
        if repeatable(command) && collapsed.last() == Some(command) {
            continue;
        }
        collapsed.push(command);
    }
    collapsed.len() == expected.len()
        && collapsed
            .iter()
            .zip(expected)
            .all(|(command, allowed)| allowed.matches(test, command))
}
