pub(super) struct AllowedRejection {
    pub(super) test: &'static str,
    pub(super) command: &'static str,
    pub(super) count: usize,
    pub(super) reason: &'static str,
}

pub(super) fn glob_matches(pattern: &str, value: &str) -> bool {
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }
    if !value.starts_with(parts[0]) || !value.ends_with(parts.last().unwrap()) {
        return false;
    }
    let mut remaining = &value[parts[0].len()..];
    for part in &parts[1..parts.len() - 1] {
        let Some(index) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[index + part.len()..];
    }
    true
}

impl AllowedRejection {
    fn matches(&self, test: &str, command: &str) -> bool {
        self.test == test && glob_matches(self.command, command)
    }
}

const LAYOUT_STACKED: &str = "i3 accepts `layout stacked`; sway accepts only `layout stacking` (sway/sway/commands/layout.c:18-27)";

// Every rejection from a passing conformance file must be reviewed here. Keying by both file and
// exact command prevents a new rejected setup command from hiding behind an unrelated exception.
pub(super) const ALLOWED_REJECTIONS: &[AllowedRejection] = &[
    AllowedRejection {
        test: "113-urgent.t",
        command: "layout stacked",
        count: 2,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "122-split.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "135-floating-focus.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "138-floating-attach.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "140-focus-lost.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "141-resize.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "167-workspace_layout.t",
        command: "layout stacked",
        count: 4,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "192-layout.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "200-urgency-timer.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "246-window-decoration-focus.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },

    AllowedRejection {
        test: "319-gaps.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "510-focus-across-outputs.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "541-resize-set-tiling.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },

    AllowedRejection {
        test: "308-focus_wrapping.t",
        command: "[con_id=*] layout stacked",
        count: 32,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "550-split-redundant-containers.t",
        command: "layout tabbed, layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "176-workspace-baf.t",
        command: "restart",
        count: 1,
        reason: "sway has no runtime restart command; coverage classifies the dependent assertion as unproven",
    },
    AllowedRejection {
        test: "111-goto.t",
        command: "[con_mark=\"*\"] focus",
        count: 1,
        reason: "test asserts that an unknown mark leaves focus unchanged",
    },
    AllowedRejection {
        test: "218-regress-floating-split.t",
        command: "layout stacked",
        count: 1,
        reason: LAYOUT_STACKED,
    },
    AllowedRejection {
        test: "202-scratchpad-criteria.t",
        command: "[title=\"nomatch\"] scratchpad show",
        count: 1,
        reason: "the test expects unmatched criteria to leave focus unchanged",
    },
    AllowedRejection {
        test: "202-scratchpad-criteria.t",
        command: "[title=\"non-scratch\"] scratchpad show",
        count: 1,
        reason: "the test expects a matching non-scratchpad window to remain unchanged",
    },
    AllowedRejection {
        test: "202-scratchpad-criteria.t",
        command: "[title=\"nothingmatchthistitle\"] scratchpad show",
        count: 1,
        reason: "the test expects unmatched criteria to leave focus unchanged",
    },
    AllowedRejection {
        test: "184-regress-float-split-resize.t",
        command: "layout stacking",
        count: 1,
        reason: "the test targets the floating group root, which sway rejects with \
                 `Unable to change layout of floating windows`; only the following \
                 liveness assertion matters (sway/sway/commands/layout.c:128-131)",
    },
    AllowedRejection {
        test: "191-resize-levels.t",
        command: "resize grow left 10px or 25ppt",
        count: 1,
        reason: "sway changes an ancestor branch but compares only the targeted \
                 container's own fractions and therefore answers `Cannot resize any \
                 further` (sway/sway/commands/resize.c:265-279); the test asserts \
                 the ancestor proportions and they still match",
    },
    AllowedRejection {
        test: "189-floating-constraints.t",
        command: "resize grow up 10px or 10ppt",
        count: 1,
        reason: "the window is already at the configured floating maximum, so sway \
                 answers `Cannot resize any further` too; the next two assertions \
                 check the window did not move",
    },
    AllowedRejection {
        test: "132-move-workspace.t",
        command: "mark a",
        count: 1,
        reason: "sway rejects marks when no container is focused",
    },
    AllowedRejection {
        test: "120-multiple-cmds.t",
        command: "move gibberish",
        count: 11,
        reason: "the regression intentionally sends this invalid command eleven times",
    },
    AllowedRejection {
        test: "120-multiple-cmds.t",
        command: "bullshit-command-which-we-never-implement meh",
        count: 1,
        reason: "the test asserts that this invalid command returns an error",
    },
    AllowedRejection {
        test: "169-border-toggle.t",
        command: "border 1pixel",
        count: 1,
        reason: "i3-only alias; sway accepts the equivalent border pixel 1",
    },
    AllowedRejection {
        test: "141-resize.t",
        command: "resize grow right 10 px or 25 ppt",
        count: 1,
        reason: "the adapter's float is already at sway's automatic maximum",
    },
    AllowedRejection {
        test: "134-invalid-command.t",
        command: "blargh!",
        count: 1,
        reason: "the regression intentionally sends an invalid command",
    },
    AllowedRejection {
        test: "101-focus.t",
        command: "layout default",
        count: 1,
        reason: "sway rejects layout default before any previous split has been recorded",
    },
    AllowedRejection {
        test: "101-focus.t",
        command: "[con_mark=__does_not_exist] focus",
        count: 1,
        reason: "the assertion expects this unmatched criterion to fail",
    },
    AllowedRejection {
        test: "119-match.t",
        command: "[con_id=\"99999\"] kill",
        count: 1,
        reason: "the test verifies that an unmatched criterion leaves the window alive",
    },
    AllowedRejection {
        test: "260-invalid-criteria.t",
        command: "[con_id=foobar] kill",
        count: 1,
        reason: "the test intentionally sends a malformed con_id criterion",
    },
    AllowedRejection {
        test: "261-match-con_id-con_mark-combinations.t",
        command: "[con_id=__focused__ app_id=doesnotmatch] kill",
        count: 1,
        reason: "the test expects the combined criterion not to match",
    },
    AllowedRejection {
        test: "261-match-con_id-con_mark-combinations.t",
        command: "[con_mark=marked app_id=doesnotmatch] kill",
        count: 1,
        reason: "the test expects the combined criterion not to match",
    },
    AllowedRejection {
        test: "502-focus-output.t",
        command: "[con_mark=doesnotexist] focus output right",
        count: 1,
        reason: "the assertion expects the unmatched criterion to leave output focus unchanged",
    },
    AllowedRejection {
        test: "502-focus-output.t",
        command: "[*id= . *] focus output right",
        count: 1,
        reason:
            "unchanged upstream file contains this malformed criterion and expects no focus change",
    },
    AllowedRejection {
        test: "294-focus-order.t",
        command: "[id=*] swap container with id *",
        count: 3,
        reason: "sway's id swap target is an X11 window id unavailable to native Wayland clients",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[app_id=b] swap with id *",
        count: 1,
        reason: "i3's optional swap words and X11 id target are unavailable in sway",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[con_mark=S1] swap with mark V1",
        count: 1,
        reason: "i3 permits omitted swap words; sway requires swap container with mark",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[con_mark=S1] swap with mark T1",
        count: 1,
        reason: "i3 permits omitted swap words; sway requires swap container with mark",
    },
    AllowedRejection {
        test: "126-regress-close.t",
        command: "mode toggle",
        count: 1,
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "127-regress-floating-parent.t",
        command: "mode toggle",
        count: 2,
        reason: "obsolete setup cannot create or restore the floating container under test",
    },
    AllowedRejection {
        test: "142-regress-move-floating.t",
        command: "mode toggle",
        count: 1,
        reason: "obsolete setup leaves the window tiled instead of testing a floating move",
    },
    AllowedRejection {
        test: "144-regress-floating-resize.t",
        command: "mode toggle",
        count: 1,
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "147-regress-floatingmove.t",
        command: "mode toggle",
        count: 1,
        reason: "obsolete setup leaves the parent tiled instead of testing floating-tree moves",
    },
    AllowedRejection {
        test: "148-regress-floatingmovews.t",
        command: "mode toggle",
        count: 1,
        reason: "obsolete setup leaves the window tiled, but the focus assertion remains valid",
    },
    AllowedRejection {
        test: "151-regress-float-size.t",
        command: "mode toggle",
        count: 2,
        reason: "obsolete setup omits both floating-to-tiling transitions under test",
    },
    AllowedRejection {
        test: "152-regress-level-up.t",
        command: "mode toggle",
        count: 1,
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "192-layout.t",
        command: "layout toggle stacked",
        count: 1,
        reason: "documented i3/sway layout-toggle divergence",
    },
    AllowedRejection {
        test: "292-regress-layout-toggle.t",
        command: "layout toggle 1337 1337",
        count: 1,
        reason: "the regression intentionally sends invalid layout names",
    },
    AllowedRejection {
        test: "273-regress-focus-toggle.t",
        command: "focus mode_toggle",
        count: 1,
        reason: "the liveness regression runs this command on an empty workspace",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace to 2",
        count: 1,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace to baz",
        count: 1,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace 5 to 2",
        count: 1,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace 1 to baz",
        count: 1,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "271-for_window_tilingfloating.t",
        command: "[tiling_from=\"auto\" con_mark=\"tiling\"] mark --add tiling_auto",
        count: 1,
        reason: "sway has no tiling provenance criterion",
    },
    AllowedRejection {
        test: "271-for_window_tilingfloating.t",
        command: "[floating_from=\"auto\" con_mark=\"floating\"] mark --add floating_auto",
        count: 1,
        reason: "sway has no floating provenance criterion",
    },
];

pub(super) fn rejected_commands(stderr: &str) -> impl Iterator<Item = &str> {
    stderr.lines().filter_map(|line| {
        line.trim_start()
            .strip_prefix("# swayward rejected `")
            .and_then(|line| line.split_once("`: "))
            .map(|(command, _)| command)
    })
}

pub(super) fn allowed_rejections(test: &str) -> Vec<&'static AllowedRejection> {
    ALLOWED_REJECTIONS
        .iter()
        .filter(|allowed| allowed.test == test)
        .collect()
}

pub(super) fn expected_rejections(test: &str) -> Vec<&'static AllowedRejection> {
    allowed_rejections(test)
        .into_iter()
        .flat_map(|allowed| std::iter::repeat_n(allowed, allowed.count))
        .collect()
}

pub(super) fn rejections_match(test: &str, rejected: &[&str]) -> bool {
    let expected = expected_rejections(test);
    rejected.len() == expected.len()
        && rejected
            .iter()
            .zip(expected)
            .all(|(command, allowed)| allowed.matches(test, command))
}
