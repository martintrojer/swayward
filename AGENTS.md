# swayward — agent conventions

swayward is a fork of niri that replaces the layout engine with i3's nested
container tree and the IPC layer with sway's protocol. Design of record:
[`docs/internal/specs/2026-09-12-swayward-foundation.md`](docs/internal/specs/2026-09-12-swayward-foundation.md).

## Build and test

The host is in most cases immutable (Fedora Atomic). Everything builds in a container:

```
distrobox enter swayward-dev -- bash -lc 'cd <repo> && cargo test --all'
```

Setup: [`docs/BUILDING.md`](docs/BUILDING.md).

Format with **nightly**: `cargo +nightly fmt --all`. `rustfmt.toml` uses four
nightly-only options and the CI fmt job runs nightly, so stable rustfmt reports
clean while leaving nightly diffs in place.

Prefer the headless harness wherever it can answer the question. `src/tests/`
drives a real compositor and real `wayland-client` clients with no nested
session, and unlike a human watching a screen it runs in CI.

## Start from main, finish on the full gate

Begin every fix from current `main` (`git checkout --detach main` in a
worktree), and run the whole gate on that tree before calling it done:
nightly fmt, clippy, `cargo test --all`, and the slow gates below when
`src/layout/` changed. A fix verified on a stale base or with focused tests
only broke main four times in one day: two fixes that each passed alone
collided, and a command change regressed green i3 files that only the full
conformance runner exercises.

Done means pushed, or committed where the orchestrator cherry-picks. A close
note that names a commit must name one that exists on the remote.

## Scratch space lives on disk

`/tmp` is a RAM-backed tmpfs: whatever accumulates there is memory, and then
swap on the operator's session. Put extra `CARGO_TARGET_DIR`s, clones, logs
and measurement output under `~/hacking/<name>` and delete them when done.
A stray 5.5 GB target directory and 16,000 leaked test socket directories
once took `/tmp` to 5.8 GB. Tests clean up what they create, including
directories that still hold a socket (`remove_dir_all`).

## Cap a nested compositor or it eats the machine

Testing against a live swayward runs a second compositor inside the operator's
session. An uncapped test client has reached 9 GB RSS and OOM-killed the host.
Cap memory and wall time, and reap on exit:

```bash
#!/bin/bash
trap 'pkill -9 -f "^\./target/debug/swayward"' EXIT

systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 \
  timeout 60 ./target/debug/swayward -c /tmp/swcfg/config.kdl
```

`MemorySwapMax=0` is the load-bearing half. Without it a leak grinds through
31 GB of swap before anything dies.

`contrib/dev-run.sh` applies every rule below. Use it rather than rolling your
own launcher.

- **One script, one lifetime.** Launch, query and reap in a single shell
  invocation. Agent tool calls are isolated processes, so a `pkill` in a later
  call cannot see what an earlier call started.
- **Strip `spawn-at-startup` from the test config.** A spawned bar competes for
  the IPC socket and can starve `swaymsg`.
- **Match the exact binary when reaping.** `pkill -f swayward` also matches the
  operator's editor holding the word in a buffer.
- **Force test clients to be server-less.** `foot` and `footclient` default to
  the operator's running server, so the window opens in their session.
- **Delete only the sockets you created.** A blanket glob deletes live sockets
  belonging to a concurrent test suite.

### Never let a test adopt the ambient `SWAYSOCK`

A Unix listener survives `unlink`: the name disappears, the bound socket does
not. If something removes the file while real sway still holds the listener,
`select_socket_path` sees no file at `$SWAYSOCK` and adopts it, faithfully,
because sway does the same (`sway/sway/ipc-server.c:99-104`). The test process
binds a second listener on that name and takes every new connection, so
`swaymsg` reaches the test instead of the compositor. When `swayidle` cannot
run `swaymsg "output * power on"`, the display stays black; `wlr-randr` is the
way back.

The production behaviour is correct. The hazard is that `cargo test --all`
inherits the operator's interactive `SWAYSOCK`, so no test may call
`IpcServer::start`. Use `start_at` with an explicit private path.
`src/tests/ipc/fixtures.rs::test_socket_path` provides one, and
`no_test_server_adopts_the_ambient_swaysock` fails if a caller reappears.

A probe connection cannot detect the victim, because an unlinked path is
unreachable by name. The kernel still lists the live listener. Before blaming a
nested run for an unreachable session, check whether the host is already a
victim: `ls -la "$SWAYSOCK"` showing no file while
`grep sway-ipc /proc/net/unix` shows a live listener means the damage predates
you and needs a session restart.

An interactive `swaymsg` carries the same hazard with no test involved. It
talks to whatever `$SWAYSOCK` names, which in an interactive session is the
operator's live compositor, and `swaymsg workspace 42` moves them there.
Before driving a nested instance, prove the target is not the host:

```sh
test "$SWAYSOCK" != "$(ls -1 /run/user/$UID/sway-ipc.*.sock | head -1)" || echo HOST
swaywardmsg -t get_version    # confirm the pid, not just that something answers
```

Resolve the nested socket from the pid you spawned rather than a glob, and
prefer read-only queries.

## `gh` needs a default repo

`upstream` points at `niri-wm/niri`, so `gh` can resolve there and report
upstream's CI as though it were ours. Set the default once per clone:

```sh
gh repo set-default martintrojer/swayward
```

This writes `remote.origin.gh-resolved = base` into `.git/config`, which is
clone-local and does not travel. Re-run it in a fresh checkout.

## History is rebase fuel

We merge upstream niri releases forever
(`git fetch upstream --tags && git merge vXX.YY`). Every commit we author is a
commit a future merge has to reason through, so our history is a tool for
rebasing, not a diary of how the work happened.

[`docs/UPSTREAM.md`](docs/UPSTREAM.md) is the full strategy: release tags only,
never `main`; rebase freely until beta1 and merge only after it; and why
rebasing onto `v26.04` is rejected.

Rewrite freely while a branch is unmerged. `origin` is ours alone:

- **Amend** rather than adding "fix typo" or "address review".
- **Squash** a task's exploratory commits into the one commit that lands.
- **Force push** after rewriting, with `--force-with-lease`.

One commit is one reviewable idea. Keep unrelated changes out of it: a rename
commit contains only renames, a fmt commit only formatting. Mixed commits turn
a trivial merge into an archaeology problem.

**Squashing does not reduce merge cost.** `git merge` compares base, our and
their *trees*; it never walks our commit list. Measured: two branches with
byte-identical trees, one of two commits and one squashed to one, produce the
same conflict and hunk counts against the same upstream change. Squash for
human legibility, never at the price of a commit's verifiability.

What does need rewriting is content that should never have been committed:
test artefacts, secrets, generated files. Those live in the trees a merge
reads.

## Divergence ledger

Prefer new modules. When an inherited niri file must be edited, add an entry to
[`docs/data/divergence.toml`](docs/data/divergence.toml) saying what changed and why.
Run `./contrib/check-divergence` before committing.

## Invariants

- Every `SWAYSOCK` reply is sway-shaped or a structured failure. It never hangs.
- **On the wire there is no partial compliance.** A request is answered exactly
  as sway answers it, or it is not implemented and returns
  `{"success": false}`. Never return a sway-shaped envelope holding
  swayward-shaped content: the reply parses, so the client fails somewhere else
  with nothing to attribute it to, which is worse than an honest refusal. Sway
  declines `IPC_SYNC` exactly this way (`sway/sway/ipc-server.c:919-925`).
  `GET_CONFIG` is the worked example: sway returns the verbatim sway config
  file, swayward's config is KDL, so it is not implemented rather than
  approximated.
- The container tree stays well formed after every mutation.
- A live-session path must not panic. An `ERROR` in the log is a bug.
- A feature needs executable evidence before it counts as working.
- Stability regressions outrank new features.
- **An AI does not sign off.** A human reviews every change, checks the
  licensing and is responsible for the result, following the kernel's
  [`coding-assistants.rst`](https://www.kernel.org/doc/html/next/process/coding-assistants.html).
  Disclosure is project-level: do not add per-file AI markers. Every file
  here was touched the same way, so a marker would either mean nothing or
  imply the unmarked files were written by hand, and putting one at the top
  of 95 inherited niri files would plant a conflict in every future merge.

## Tests

The unchanged i3 tests live in `martintrojer/sway-ipc-oracle` at the commit in
`tests/oracle.toml`. Never edit those files: they are the external oracle, and a
test you can edit to pass is not evidence. Run `./contrib/fetch-oracle` before
the in-process harness. Where i3 and sway differ, record a skip with a citation
into sway's source rather than changing the assertion.

Measure swayward against the oracle from a clean clone of it, passing `--out`
outside the repo: the runners' default output path is a tracked results file,
and a measurement taken from a worker's half-edited clone gave misleading
numbers.

`tests/i3/coverage.toml` is the source of truth for what the in-process harness measures.
Use `./contrib/coverage-report` for the current census; its `--json` output is
machine-readable. `./contrib/coverage-report --check` must report 0 violations.

Count assertions with `contrib/tap-count`, never by hand. Only the first
unindented TAP plan is authoritative.

Run the slow gate for changes under `src/layout/`:

```sh
RUN_SLOW_TESTS=1 PROPTEST_CASES=20000 cargo test -p swayward --lib tiling_tree
```

The randomized layout test needs `RUN_SLOW_TESTS=1` too: without it it runs
zero cases and reports success. CI runs 200,000 in release mode; locally, run
at least

```sh
RUN_SLOW_TESTS=1 PROPTEST_CASES=20000 cargo test --release -p swayward --lib random_operations_dont_panic
```

`random_operations_dont_panic` searches fresh cases rather than replaying the
checked-in seeds, so it finds defects a default run does not. Keep the seeds in
`proptest-regressions/`: they reproduce in under a second what the search takes
minutes to find.

## Never reset a worktree that holds a live claim

Resetting a worker's worktree to `origin/main` after merging their commit is
destructive if they have started something else. Check `mu task list` for an
open claim first, and prefer leaving the worktree alone: a worker that needs to
rebase will do it themselves. When a commit goes missing,
`git -C <worktree> reflog` is the first place to look.

A bisect worktree sharing `CARGO_TARGET_DIR` with the main tree produces bogus
compile errors in both. Give it its own, or `rm -rf` the worktree and
`git worktree prune` when finished.

## An audit's result is bounded by its scope

State what an audit covered and what it did not. "We found nothing" means "the
sites we examined are safe", and the second clause is the load-bearing one. A
panic audit that read protocol files statically concluded no panic was
reachable; one was found hours later in `src/layout/`, which the audit had not
executed.

A coverage row records a measurement, not a guarantee that the measurement is
current. Only a fresh `contrib/tap-count` run makes it current.

## Check whether the thing you are defending is niri's

When a fix keeps moving rather than landing, ask whether the behaviour being
preserved is a niri mechanic that sway does not have. swayward keeps sway's
model; it keeps niri's only where the two do not conflict.

The trailing placeholder workspace cost seven commits of call-site patches
before anyone checked: it is niri's scrolling-strip affordance, sway has no
such concept, and `grep -r placeholder sway/tree/` returns nothing. Deleting
the invariant fixed every remaining failure at once.
