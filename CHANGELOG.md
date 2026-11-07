# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## Unreleased

### Added

- CI workflow `security` on every push and weekly: `cargo deny check`
  (RustSec advisories, yanked crates, licenses, crate sources; `deny.toml`),
  gitleaks over the git history, Trivy over the runtime image. Weekly
  Dependabot updates for Cargo (patch releases grouped), Docker (except the
  Rust image, kept in step with `rust-toolchain.toml`) and GitHub Actions.

## 0.1.1 - 2026-11-06

### Security

- Updated `rustls` 0.23.46, `rustls-webpki` 0.103.16, `webpki-roots` 1.0.11
  and `ctutils` 0.4.8 (lockfile only) for RUSTSEC-2026-0104, -0285, -0346,
  -0348, -0349, -0381, -0382 and -0385. They affect TLS connections to
  PostgreSQL (`sslmode=require`) and SCRAM authentication.

## 0.1.0 - 2026-11-06

First release.

### Added

- Development setup: Makefile targets, `.env.example`, PostgreSQL via
  docker compose, and `AGENTS.md` with the working rules.
- Service skeleton: environment configuration validated at startup, text or
  JSON logs, graceful shutdown, `/health` and `/ready` probes, request ids,
  uniform JSON errors `{"error": {"code", "message"}}`, OpenAPI document
  (`/api-docs/openapi.json`) and reference page (`/docs`).
- Docker image (non-root, exec-form `CMD`) and the service in docker compose.
- CI: formatting, clippy with warnings denied, tests against PostgreSQL.
- Requirements, assumptions with chosen defaults and acceptance criteria
  (`docs/requirements.md`).
- Architecture decision records 0001–0005 (`docs/adr/`).
- `bench/`: pgbench comparison of transfer locking strategies; pitfalls
  catalogue, test plan and delivery plan (`docs/`).
- Amounts as strict decimal strings (up to 20 integer digits and 18 decimal
  places), never rounded, stored as unconstrained `NUMERIC`.
- Schema: accounts and an append-only transaction ledger enforced by
  database triggers; every balance reconciles with its opening balance and
  history.
- `POST /accounts` and `GET /accounts/{account_id}`: accounts with a
  client-chosen positive id and an exact decimal opening balance.
- `POST /transactions`: transfers in one database transaction, with both
  accounts locked in ascending id order (no deadlocks, no double spending),
  balance checked under the lock, relative updates and bounded retries.
- Optional `Idempotency-Key` header: a retry returns the original transfer
  (`Idempotent-Replayed: true`) without moving money again; a different
  transfer under the same key is rejected.
- Balance limit: every balance stays below 10^20; a transfer that would
  reach it is rejected with `422 balance_limit_exceeded`. Migration `0002`
  adds `CHECK (balance < 1e20) NOT VALID` as a backstop, so upgrading a
  database that already holds a larger balance still succeeds.
- Tests for a retry that succeeds after a lock timeout, a transfer that
  completes after the client disconnects, and upgrading a database with
  existing data; lock-dependent tests wait on `pg_stat_activity`, not sleeps.
- The OpenAPI document describes the account and transfer endpoints, every
  status and error code.
- TLS to PostgreSQL via the connection string (`sslmode`), `migrate`
  subcommand and `RUN_MIGRATIONS` for migrations as a release step, `make
  migration` and forward-only migration rules (ADR 0006).
- CI job `every-commit`: every commit of a push or pull request must pass
  fmt, clippy and the tests and start with `/health` = 200 (manual runs take a
  commit range, e.g. the whole history).
- Architecture overview with diagrams, the engineering approach (timeline
  of activities, reasoning and outcomes, `docs/approach.md`) and project skills for contributors
  (`.claude/skills/`).

### Changed

- PostgreSQL is published on host port 56432 by default, so it does not
  clash with a local PostgreSQL.
- Docker base images are pinned and dependency builds are cached; README
  documents the connection budget per instance.
- CI installs the toolchain pinned in `rust-toolchain.toml` and uses the
  same PostgreSQL image as docker compose.

### Fixed (during review, before the release)

- A busy database (pool exhausted, statement timeout, lock contention after
  retries) was answered with 500 instead of 503 + `Retry-After`.
- Minimum supported Rust version corrected to 1.94 (required by sqlx 0.9);
  concurrency tests now run 16 overlapping transactions.
- An idempotent replay could wait behind, or fail on, locks held by other
  transfers on the same accounts.
- Negative amounts were reported as malformed instead of negative.
