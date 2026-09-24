swayward is a permanent fork of niri. It keeps niri's whole history, replaces
the layout engine and the IPC layer, and expects to absorb upstream work for
as long as both projects exist.

See [`docs/FORK-BASE.md`](https://github.com/martintrojer/swayward/blob/main/docs/FORK-BASE.md) for the current base and
[`docs/DIVERGENCE.md`](DIVERGENCE.md) for every edit to an inherited file.

## The rule

**Rebase before the first release. Merge forever after.**

Until beta1 is cut, our commits are ours to rewrite: squash, amend, reorder,
force-push. Nobody has a checkout that a rewrite would break.

After beta1, that stops. A published tag is a promise that a commit hash means
one thing, and rewriting history under people who have cloned, packaged or
bisected the repository breaks it. From beta1 onward the branch only grows.

## Upstream comes in at niri releases

We merge niri **release tags**, not upstream `main`.

    git fetch upstream --tags
    git merge v26.08          # whatever the next tag is

Tracking `main` would mean absorbing upstream's in-progress refactors one
commit at a time, each with its own conflict against our layout engine, and
reconciling against a tree that upstream may still change before it ships. A
release is a point upstream has already stabilised and tested. It is a bigger
merge, less often, against a known-good tree — which is the trade we want,
because the expensive part of a merge is judgement about our divergence, not
the number of hunks.

The log will accumulate a large merge commit a few times a year, forever.
That is the correct shape for a long-lived fork and not a problem to engineer
away. Each one is a single reviewable event with a known upstream name
attached.

Squashing our own commits does not make these merges cheaper. `git merge`
compares base, ours and theirs as *trees*; it never walks our commit list.
This was measured: two branches with byte-identical trees, one of two commits
and one squashed to one, produce identical conflict and hunk counts. Squash
for human legibility only.

### Merging a release

1. Read the upstream release notes first. Anything touching layout, IPC or
   window management is where our divergence lives.
2. `git fetch upstream --tags && git merge vXX.YY` on a branch, never on a
   dirty tree.
3. Resolve conflicts in favour of **sway's behaviour**, not niri's. When a
   conflict is about which model wins, sway wins, and the reason
   goes in `docs/KNOWN_DEVIATIONS.md` with a citation into sway's source.
4. Run the full gate before pushing:
   - `cargo test --all`
   - `RUN_SLOW_TESTS=1 PROPTEST_CASES=20000 cargo test -p swayward --lib tiling_tree`
   - `./contrib/coverage-report --check`
   - `cargo +nightly fmt --all -- --check`
5. Update `docs/FORK-BASE.md` to the new tag, and add any new inherited-file
   edits to `docs/DIVERGENCE.md`.

Expect breakage the tests find only after the merge is resolved. A previous
merge produced three integration failures that appeared in no individual
commit, and one conflict resolution silently deleted five tests until
`cargo test --all` caught it. The suite is the safety net; do not skip it
because the conflicts looked mechanical.

## Do not rebase onto a niri release before beta1

This was considered and rejected on evidence.

Our base is upstream commit `9e72e491` (2026-09-11), which is **93 commits
after the `v26.04` tag** and the newest release available. Rebasing onto
`v26.04` therefore moves us *backwards*, discarding four and a half months of
upstream work, including real fixes in code we inherit and do not maintain:

| Commit | What we would lose |
| --- | --- |
| `9e72e491` | screencopy buffers when buffer-size != shm-pool-size |
| `fec411ea` | deadlock opening the screenshot UI with a tablet connected |
| `92a25a0a` | Smithay update: frame callback and late-bound enter fixes |
| `0777769e` | wrong colour format for copy on multi-GPU setups |
| `e5d463e1` | blur on OpenGL ES 2.0 GPUs |
| `7f26c3ee` | 32-bit build failure |

Eighteen of the 93 commits are fixes. Giving up a deadlock fix and a
multi-GPU rendering fix to gain a tidier base is a bad trade in a project
whose stated invariant is that stability regressions outrank everything else.

Being based on a commit rather than a tag costs us only a slightly longer
sentence in `FORK-BASE.md`. The next merge target is the first niri tag after
`9e72e491`, and the merge base is computed from the graph, so nothing about
the ongoing strategy depends on our base being a tag.

Revisit only if upstream tags a release that is a descendant of `9e72e491`
**before** we cut beta1. Then the rebase is forwards, costs nothing, and is
worth doing.

## Versioning

swayward already uses niri's date scheme and should keep it: `YY.MM`, with a
patch component for point releases.

    26.4.0        # Cargo.toml, matching niri's own 26.4.0 at v26.04
    26.04         # display form, zero-padded month

`src/utils/version()` renders `MAJOR.MINOR` zero-padded, appending the patch
only when it is non-zero, and always with the build commit. `get_version` over
IPC reports the same numbers plus `"variant": "swayward"`, which is how a
client tells us apart from sway.

Two consequences worth stating:

- **Our version number is not sway's.** Clients checking for a sway feature
  level by version will read a date, not `1.x`. The `variant` field exists for
  exactly this, and any client that needs the distinction should use it.
- **Matching niri's number does not mean matching niri's content.** We are on
  26.4.0 while based on a commit 93 ahead of niri's v26.04. The version says
  when, not what. `FORK-BASE.md` says what.

A release is tagged `vYY.MM`, matching niri's tag convention, so the two
histories read consistently in a log that contains both.

## Never send AI-generated pull requests upstream

Fixes that belong to niri are reported as issues, or rewritten by hand by
someone who can stand behind them. See
[`CONTRIBUTING.md`](https://github.com/martintrojer/swayward/blob/main/CONTRIBUTING.md).
