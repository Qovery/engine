# AGENTS Instructions

## Instruction Style

- One line equals one instruction.
- Write imperative instructions as single-line bullets.
- Keep every instruction atomic.
- Split combined instructions into separate bullets.

## Scope

- Apply these instructions to all agents working in this repository.
- Keep code maintainable.
- Keep code correct.
- Keep code idiomatic.
- Explain key technical decisions so readers can learn.

## General Programming Preferences

### Imports

- Prefer explicit imports over fully qualified paths in expressions.
- Avoid fully qualified paths inside function bodies when an import is clearer.
- Apply the same import rule across all languages that support imports.

### Strong Typing

- Use explicit domain types instead of raw strings and generic primitives.
- Use enums for closed sets of valid states.
- Use newtype wrappers for semantically distinct primitive values.
- Avoid `Any`, `Object`, `interface{}`, and `dynamic` unless clearly justified.
- Revisit type design when frequent casting appears.
- Make invalid states unrepresentable at the type level.

### Pre-Commit Review

- Apply the pre-commit review checklist only when preparing a commit.
- Keep detailed pre-commit checklist logic in an external skill.

## Rust Preferences

### Imports and Strong Typing

- Apply the general import rules directly to Rust.
- Apply the general strong typing rules directly to Rust.
- Prefer enums and newtypes over stringly-typed Rust APIs.

### Prefer Enums Over Trait Abstractions

- Prefer enums when behavior or state variants are known and stable.
- Prefer concrete types when extensibility is not required.
- Introduce traits only when open extension is required.
- Introduce dynamic dispatch only when open extension is required.
- Avoid abstraction layers that do not provide concrete value.

### Clippy and Formatting

- Always run `cargo clippy`.
- Fix `cargo clippy` warnings and issues before considering work done.
- Always run `cargo fmt` before finalizing changes.

### Cloning and Ownership

- Limit `.clone()` usage.
- Avoid `.clone()` when borrowing or moving is sufficient.
- Prefer borrowing (`&T`, `&str`, `&[T]`) when ownership is unnecessary.
- Choose iterators (`iter`, `iter_mut`, `into_iter`) based on ownership intent.
- Derive `Copy` for small stack-only value types when appropriate.
- Return captured values from closures when that avoids cloning.

### Safe and Unsafe Rust

- Prefer safe Rust by default.
- Use `unsafe` only when no reasonable safe alternative exists.
- Document why each `unsafe` usage is sound.
- Ensure safe Rust paths cannot cause undefined behavior.

### Ownership, Borrowing, and Lifetimes

- Keep one owner per value.
- Allow many shared references or one mutable reference, but not both at once.
- Ensure references never outlive their referents.
- Add lifetime annotations when function boundaries require them.
- Respect invariance rules for `&mut T`.

### Panic Safety and Unsafe Traits

- Preserve invariants even when panics occur mid-operation.
- Use guard patterns when needed to restore invariants.
- Mark traits as `unsafe` only when incorrect impls can cause UB.

### Practical Rust API Rules

- Prefer `&str` over `&String` in borrowed read-only APIs.
- Prefer `&[T]` over `&Vec<T>` in borrowed read-only APIs.
- Prefer `?` over `.unwrap()` in production paths.
- Reserve `.unwrap()` for tests or proven impossible-failure paths.
- Prefer `#[derive]` for common traits unless custom behavior is required.
- Prefer `From<T>` and `Into<U>` for conversions.
- Prefer implementing complex conditional logic in Rust code.
- Keep template logic minimal in Jinja and template files.
- Keep YAML files as declarative as possible.
- Avoid embedding complex branching in templates and YAML when Rust can express it.
- Keep templates simple to make updates and diffs easier to review.

### Constructor Parameters vs Builder Methods

- Encode mandatory preconditions as constructor parameters, not opt-in builder methods.
- Reserve builder methods for optional configuration.
- Prefer touching every call site over a chained mutator that hides a required choice.

## Repository-Specific Tooling

### Compatibility

- Always read `CLAUDE.md` at repository root at the start of work.
- Always read `@AGENT.md` at repository root at the start of work.
- Always merge `CLAUDE.md` instructions with `@AGENT.md` instructions.
- Apply the stricter rule when instructions overlap.
- Apply the more specific rule when instructions overlap.

### Chart Templates (`lib-engine/lib/**/*.j2.yaml`)

- Interpolate every deployment-supplied value through `yaml_encode`: `{{ value | yaml_encode }}`.
- Never hand-quote an interpolation — `yaml_encode` emits its own quotes, and `"{{ v }}"` still lets a value containing `"` close the scalar and add sibling fields to the manifest.
- Apply it to mapping keys too: `{{ key | yaml_encode }}: {{ value | yaml_encode }}`.
- Treat labels, annotations, tolerations, affinity, env var keys, command args, probe commands, mount paths, headers, and every free-form advanced setting as deployment-supplied.
- Skip it only for engine-generated identifiers, numeric and enum fields, and base64 payloads — and only when a passing reader can tell which it is.
- Never interpolate a value into a literal block scalar (`key: |-`): a newline in it leaves the scalar and the manifest. Use `yaml_encode`, which folds newlines into `\n`.
- Register filters through `tera_utils::register_filters`, and render in tests through `tera_utils::render_one_off` — `Tera::one_off` knows no filters and fails with `FilterNotFound`.
- Remember Helm re-renders the manifest as a Go template after Tera: a `{{` surviving from deployment input is executed, and `lookup` reads anything the engine's credentials can. `yaml_encode` neutralizes it; raw passthroughs (`raw_yaml`, cluster snippets) do not.
- Add a rendering test for any new interpolation: render the real template with a break-out payload, parse the result, and assert no field was added.

### Cloud Provider Abstractions

- Pass `cloud_provider::Kind` to abstractions whose behavior varies per provider.
- Centralize provider-specific rules inside the abstraction, not at each call site.
- Test cross-provider abstractions by exercising every `Kind` variant when behavior diverges.
- Tag upstream-pending workarounds with `HACK(QOV-XXXX)`, link the tracking issue, and state the removal condition.

### code-review-graph MCP

- Use `code-review-graph` MCP tools before grep/glob/broad reads for exploration and review.
- Use `semantic_search_nodes` or `query_graph` for structural discovery.
- Use `get_impact_radius` and `get_affected_flows` for blast radius analysis.
- Use `detect_changes` and `get_review_context` for review context.
- Use graph queries to trace callers, callees, imports, and tests.
- Use `get_architecture_overview` and `list_communities` for architecture-level understanding.
- Fall back to grep/glob/read only when graph coverage is insufficient.
- Start review flows with graph-based discovery.
- Use `query_graph` with `tests_for` patterns to verify test coverage links.

## Open Source Rust Quality

### Required

- Keep the public API surface minimal.
- Treat public API changes as semver-impacting.
- Do not panic for recoverable errors in library and runtime paths.
- Prefer structured error types over plain string errors.
- Document all public modules, types, and functions with rustdoc.
- Include minimal usage examples for public APIs when practical.
- Keep `cargo clippy` warning-free.
- Keep `cargo fmt` applied.
- Add or update unit tests for changed behavior.
- Add or update integration tests for changed behavior.
- Add regression tests for bug fixes.
- Keep `unsafe` scope minimal.
- Add a `SAFETY:` comment for every required `unsafe` block.

### Preferred

- Use `#[non_exhaustive]` on public enums and structs likely to evolve.
- Add contextual error messages that improve diagnostics.
- Default to synchronous Rust.
- Introduce async only for clear high-concurrency I/O needs.
- Document why sync and threaded designs are insufficient when choosing async.
- Avoid holding locks across `.await`.
- Prefer bounded queues and channels unless unbounded usage is justified.
- Keep feature flags additive.
- Keep feature flags loosely coupled.
- Keep minimal-feature and full-feature builds healthy.
- Prioritize algorithmic improvements over micro-optimizations.
- Avoid unnecessary allocations and clones in hot paths and public APIs.
