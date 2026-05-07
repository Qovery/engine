<!-- code-review-graph MCP tools -->

## MCP Tools: code-review-graph

- ALWAYS use `code-review-graph` MCP tools BEFORE `Grep`/`Glob`/`Read` for exploration.
- Use `semantic_search_nodes` or `query_graph` for code exploration.
- Use `get_impact_radius` for impact analysis.
- Use `detect_changes` and `get_review_context` for code review.
- Use `query_graph` patterns for callers, callees, imports, and tests.
- Use `get_architecture_overview` and `list_communities` for architecture questions.
- Fall back to `Grep`/`Glob`/`Read` only when graph coverage is insufficient.

## Workflow

- Treat graph updates as automatic via repository hooks.
- Use `detect_changes` first for review-oriented analysis.
- Use `get_affected_flows` to understand impacted execution paths.
- Use `query_graph` with `tests_for` patterns to check test coverage links.
