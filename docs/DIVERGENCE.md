swayward keeps niri's Git history and reuses inherited files where the sway model does not conflict with them. Prefer new modules. Edit an inherited niri file only when the change requires it, and record every edited path with what changed and why.

The machine-readable ledger is the [`docs/data/divergence/`](https://github.com/swayward-wm/swayward/tree/main/docs/data/divergence) directory. Each TOML file except `_meta.toml` contains exactly one `[[edit]]` with an explicit `paths` list, a `what` description, and a `why` explanation. An empty `why` means the original ledger entry did not state a separate reason. New entries use a unique descriptive filename rather than extending the numbered migration sequence.

To find every entry for one file:

```sh
python3 - <<'PY'
import tomllib
from pathlib import Path

for path in Path("docs/data/divergence").glob("*.toml"):
    if path.name == "_meta.toml":
        continue
    with path.open("rb") as file:
        edit = tomllib.load(file)["edit"][0]
    if "src/layout/mod.rs" in edit["paths"]:
        print(path, edit["what"], edit["why"], sep="\n")
PY
```

Run `./contrib/check-divergence` after editing the ledger. The check reads the niri base from [`docs/FORK-BASE.md`](https://github.com/swayward-wm/swayward/blob/main/docs/FORK-BASE.md) and rejects an inherited file that differs from that base without a matching entry. A file moved from its niri path and also edited counts as inherited: git pairs it with its base file at 50% similarity or more, and the ledger may name either path. An entry ending in `/` covers every file under that directory.
