# Repository instructions

## Graphify

The project-local Graphify skill is in `.agents/skills/graphify/SKILL.md`. The code graph is in `graphify-out/graph.json`.

- For every codebase task, query the graph before searching or opening full source files. Start with `graphify query "<task terms>" --budget 700`. Use `graphify explain "<symbol>"` or `graphify path "<A>" "<B>"` for a narrower lookup.
- Read only the source ranges needed to verify the result. If the graph is missing, stale, or has no relevant result, use a targeted search and refresh the graph after code changes.
- Run `graphify update .` after changing code. This uses local AST extraction and respects `.graphifyignore`. On Windows, use `python -m graphify` if the Graphify executable is unavailable.
- Keep generated graph output local. If a new checkout has no graph, run `graphify update .` from the repository root.
