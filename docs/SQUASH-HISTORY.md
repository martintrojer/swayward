# Squash fork history

Use this procedure to compress swayward-only history without rewriting the niri merge base. The result groups commits by their paths at the target tree, not by old commit subjects.

## Before you rewrite

1. Fetch `origin` and confirm that local `main` equals `origin/main`.
2. Record the fork base and target commit.
3. Create an undo branch before changing `main`:

   ```sh
   git branch pre-squash-backup main
   ```

4. Confirm that the fork base is an ancestor of the target:

   ```sh
   git merge-base --is-ancestor 9e72e491 pre-squash-backup
   ```

5. Record the target tree hash:

   ```sh
   git rev-parse pre-squash-backup^{tree}
   ```

Do not rewrite commit `9e72e491` or any ancestor. Tags below that commit remain valid upstream history.

## Group the target tree by path

Classify files from the target tree. Do not classify old commits by subject prefix. A commit named `docs:` can contain compositor code, tests, resources, or tools.

The 2026 rewrite used these groups:

1. The mechanical niri-to-swayward rename.
2. Vendored i3 `.t` files and their license.
3. Real sway captures and the i3 adapter files.
4. Source, configuration, resources, tools, tests, and snapshots.
5. Documentation.

Keep generated snapshots with the code that requires them. A clean-looking snapshot-only commit is not useful if checking it out makes the tests fail. Prefer a bisectable commit over a narrower label.

## Replace a tree without leaving deleted files behind

Do not use `git checkout <commit> -- .` to replace a tree. That command does not remove paths that are absent from the target, so stale files survive.

Use the index as the source of truth:

```sh
git read-tree --empty
git read-tree <target-commit>
git checkout-index -a -f
git clean -qfd
```

To add one path group, apply the difference from the current tree to the saved target:

```sh
git diff --binary HEAD pre-squash-backup -- <pathspecs> | git apply --index
```

Use exclusion pathspecs for files reserved for later groups. After each group, inspect `git diff --cached --name-status` before creating the commit.

## Verify the result by tree hash

A clean `git diff --stat` is not sufficient. Compare tree objects:

```sh
expected=$(git rev-parse <expected-final-commit>^{tree})
actual=$(git rev-parse HEAD^{tree})
test "$actual" = "$expected"
```

If the rewrite intentionally removes or edits files, first create an expected tree from the saved target plus only those changes. Compare `HEAD^{tree}` with that expected tree.

Run the full test, clippy, formatting, conformance-manifest, vendored-file, snapshot, and slow-test gates after the final commit. Record which intermediate commits are green. Do not describe an untested intermediate commit as bisectable.

## Exclude build artifacts

Remove tracked Python bytecode such as `__pycache__/*.pyc`. Add both patterns to `.gitignore`:

```gitignore
__pycache__/
*.pyc
```

A generated artifact can otherwise become part of every rewritten tree and obscure the intended path groups.

## Publish safely

Confirm that `origin/main` still points to the commit used for the rewrite. Push with a lease:

```sh
git -c lfs.locksverify=false push --force-with-lease origin main
```

Never replace this command with `--force`. If the lease fails, stop. Fetch and inspect the remote changes before deciding how to continue.

Keep `pre-squash-backup` until the rewritten history and upstream merge base have been verified. To undo locally, reset `main` to that branch.
