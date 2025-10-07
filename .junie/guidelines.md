# Project Guidelines for Qovery Engine (Rust)

This document guides Junie (the autonomous programmer) when working on this repository. It provides a brief project overview, structure, conventions, and how to build and test.

## Project overview
This repository is a Rust workspace that powers Qovery's deployment engine. It is primarily an event-driven system orchestrating process deployments (e.g., via Terraform and other providers). The code emphasizes reliability, idempotency, and observability across asynchronous workflows.

## Repository structure (high level)
- `Cargo.toml` — Workspace definition (members, profiles)
- `lib-engine/` — Core engine library crate
  - `src/` — Engine domain, actions, models, I/O models
    - `environment/` — Environment actions and models (e.g., Terraform service deploy)
    - `io_models/` — Input/output models exchanged with the engine
    - other feature modules under `src/` as needed
- `app/` — Binary crate(s) using lib-engine (CLI or long-running service)
- `deployment.json` — Deployment configuration used by the engine
- `docker/`, `Dockerfile`, `goreleaser.yml` — Containerization and release pipeline
- `.junie/` — Junie configuration and this guidelines file
- `rust-toolchain`, `rustfmt.toml` — Toolchain pinning and formatting rules

## How Junie should work on issues
- Prefer minimal, targeted changes that satisfy the issue.
- Keep user informed via the status tools: provide a plan with `update_status`, then finalize with `submit`.
- When touching code, add or update tests if feasible.
- For non-code changes (docs/config), avoid triggering unnecessary builds.

## Building
Use Cargo from the project root:
- Build debug: `cargo build`
- Build release: `cargo build --release`
- Check without building artifacts: `cargo check`

If the change only affects documentation, a build is not required unless requested.

## Running tests
- All tests: `cargo test`
- Specific package: `cargo test -p lib-engine`
- Specific test by name: `cargo test <test_name>`
- Specific feature by name: `cargo test --features <feature-name> --no-default-features --manifest-path Cargo.toml -- --color always --test-threads=20`

Junie should run relevant tests when modifying production code and before submitting. Prefer fast unit tests for logic and add integration tests for engine flows when feasible.

## Linting and style
- Follow `code-conventions.md` if present. Otherwise:
  - Format: `cargo fmt --all` (rustfmt configured by `rustfmt.toml`).
  - Lint: `cargo clippy --all-targets --all-features -D warnings`.
- Keep functions small and focused; prefer explicit structs/enums and idiomatic Rust.
- Avoid unsafe code unless absolutely necessary and well-justified.

## Architecture and coding practices (event-driven engine)
- Prefer explicit commands/events for engine operations.
  - Commands initiate work (e.g., DeployTerraformServiceCommand).
  - Events represent outcomes/state transitions (e.g., TerraformServiceDeployed, TerraformPlanFailed).
- Idempotency: design actions so they can be retried safely. Use stable identifiers and persistent checkpoints when applicable.
- Determinism: avoid hidden global state; make inputs explicit via arguments/struct fields.
- Error handling:
  - Return `Result<T, E>` from fallible functions.
  - Use domain-specific error enums (e.g., via `thiserror`) and map lower-level errors with context (consider `anyhow::Context` for top-level app boundaries).
  - Never swallow errors; bubble them up or convert to events.
- Logging & observability:
  - Use `tracing` for structured, contextual logs. Do not use `println!` for application logs.
  - Include causes in error logs and attach correlation identifiers (e.g., environment_id, service_id) as span fields.
  - Emit progress metrics/events where appropriate.
- Concurrency & async:
  - Prefer `tokio` for async tasks; keep blocking I/O off async executors (`spawn_blocking` if needed).
  - Limit concurrency with bounded tasks/semaphores where external rate limits or Terraform apply constraints exist.
- Separation of concerns:
  - Keep environment/action logic in `lib-engine` and keep `app` thin (argument parsing, wiring, runtime setup).
  - Isolate external command/process execution (e.g., Terraform) behind small, testable adapters.
- Configuration:
  - Prefer environment variables and typed config structs deserialized with `serde`.
  - Validate configuration eagerly at startup; fail fast when invalid.
- Data/serialization:
  - Define IO models under `lib-engine/src/io_models`. Keep them stable and versioned if they are part of external contracts.
  - Use `serde` derive with explicit field naming (snake_case) and defaulting where appropriate. Document breaking changes.
- Testing strategy:
  - Unit tests close to the code under `mod tests {}`.
  - Integration tests under `tests/` exercising end-to-end flows. Use ephemeral resources or containers when feasible.
  - For Terraform/process interactions, prefer fakes/mocks by abstracting command executors. Use real `terraform` only in opt-in tests.

## Commit and PR hygiene
- Keep diffs minimal and scoped to the issue.
- Avoid unrelated refactors.
- Add comments where intent might be non-obvious.
- Ensure CI passes `fmt` and `clippy` locally before pushing.

## Configuration notes
- The engine relies on resource/config files such as `deployment.json` at the repository root; keep its schema documented and validate early.
- Terraform-related logic lives under `lib-engine/src/environment/...` and `lib-engine/src/cmd/...`. Keep these modules cohesive and well-documented.

## Runtime and process execution guidelines
- Wrap external process execution (Terraform, CLI tools) in a dedicated module that:
  - Captures stdout/stderr, exit codes, and timing.
  - Redacts sensitive data in logs.
  - Provides structured results for higher-level actions.
- Implement retries with exponential backoff for transient failures; avoid infinite retries.
- Guard long-running operations with timeouts and cancellation propagation.

## Logging
- Use `tracing` macros (`info!`, `warn!`, `error!`, `debug!`, `trace!`) with structured fields.
- Protect sensitive data: never log credentials, tokens, or secrets.
- Guard expensive log calls with level checks or `tracing` lazy fields/closures when needed.

## Example local workflows
- Format & lint: `cargo fmt --all && cargo clippy --all-targets --all-features -D warnings`
- Test all: `cargo test`
- Run the app (if present): `cargo run -p app -- <args>`
