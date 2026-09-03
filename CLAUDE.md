<!-- code-review-graph MCP tools -->

## MCP Tools: code-review-graph

- ALWAYS use `code-review-graph` MCP tools BEFORE `Grep`/`Glob`/`Read` for exploration.
- Use `semantic_search_nodes` or `query_graph` for code exploration.
- Use `get_impact_radius` for impact analysis.
- Use `detect_changes` and `get_review_context` for code review.
- Use `query_graph` patterns for callers, callees, imports, and tests.
- Use `get_architecture_overview` and `list_communities` for architecture questions.
- Fall back to `Grep`/`Glob`/`Read` only when graph coverage is insufficient.

## Chart Templates

- ALWAYS pipe deployment-supplied values through `yaml_encode` in `*.j2.yaml`, keys included: `{{ key | yaml_encode }}: {{ value | yaml_encode }}`.
- NEVER add your own quotes around it, and never interpolate into a block scalar (`key: |-`).
- See `AGENTS.md` → *Chart Templates* for what counts as deployment-supplied and why (YAML break-out, plus Helm's second Go-template pass).

## Workflow

- Treat graph updates as automatic via repository hooks.
- Use `detect_changes` first for review-oriented analysis.
- Use `get_affected_flows` to understand impacted execution paths.
- Use `query_graph` with `tests_for` patterns to check test coverage links.
