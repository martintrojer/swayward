swayward keeps niri's Git history and reuses inherited files where the sway model does not conflict with them. Prefer new modules. Edit an inherited niri file only when the change requires it, and record every edited path with what changed and why.

The machine-readable ledger is [`docs/data/divergence.toml`](https://github.com/martintrojer/swayward/blob/main/docs/data/divergence.toml). Each `[[edit]]` contains an explicit `paths` list, a `what` description, and a `why` explanation. An empty `why` means the original ledger entry did not state a separate reason.

To find every entry for one file:

```sh
python3 - <<'PY'
import tomllib

with open("docs/data/divergence.toml", "rb") as file:
    for edit in tomllib.load(file)["edit"]:
        if "src/layout/mod.rs" in edit["paths"]:
            print(edit["what"], edit["why"], sep="\n")
PY
```

Run `./contrib/check-divergence` after editing the ledger. The check reads the niri base from [`docs/FORK-BASE.md`](https://github.com/martintrojer/swayward/blob/main/docs/FORK-BASE.md) and rejects an inherited file that differs from that base without a matching entry.
