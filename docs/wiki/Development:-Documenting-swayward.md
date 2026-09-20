# Documenting swayward

Documentation files live in `docs/wiki/`. They must render in GitHub's Markdown
preview and in MkDocs. The repository copy is the source of truth. Do not edit
the published GitHub wiki directly.

Serve the site locally from `docs/`:

```sh
uv sync
uv run mkdocs serve
```

Use relative links for pages and local images. Add anchors when a link targets a
specific section. MkDocs runs in strict mode and reports missing pages and
anchors.

Audit links before publishing:

```sh
contrib/sync-github-wiki --check
```

Publish the tracked pages and assets:

```sh
contrib/sync-github-wiki
```

The first publish requires an initialized GitHub wiki. If the wiki repository
does not exist, the command prints the one-time setup step.

Write GitHub-style admonitions:

```md
> [!WARNING]
> Describe the hazard and the safe action.
```

Annotate fenced code blocks with their language. Use `kdl` for configuration.
Give every image useful alternative text.

The retained subsystem guides derive from niri's GPL documentation. Keep their
attribution lines when editing them. A wholly rewritten page does not need a
per-page attribution.
