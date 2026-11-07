# AGENTS.md

Instructions for coding agents (and humans) working on this repository. The
README explains the service to users; this file explains how to change it
safely. Keep it short and true: update it in the same PR that changes a rule.

## Project

HTTP service (Rust, axum 0.8, sqlx 0.9, PostgreSQL 17) that creates accounts,
reports balances and transfers money between accounts. Correctness under
concurrent transfers is the core requirement; everything else is secondary.

## Commands

| Task | Command |
|---|---|
| Start PostgreSQL only | `make db-up` (host port 56432) |
| Run the service | `make run` (migrates, serves on :8080, docs at `/docs`) |
| All tests | `make test` (needs PostgreSQL; `make db-up` is run for you) |
| One test | `cargo test --test api <name_substring>` |
| Format / lint | `make fmt` / `make lint` |
| What CI runs on every commit | `make check` — run before each commit |
| Dependencies: advisories, licenses, sources | `cargo deny check` (config `deny.toml`; also in CI `security`) |
| Full stack in Docker | `docker compose up --build` |
| HTTP load test (k6, Docker) | `make loadtest` (`PROFILE=load` for the heavier run; [loadtest/README.md](loadtest/README.md)) |
| Release and deploy | tag `vX.Y.Z` → `release.yml`; on the host `deploy/deploy.sh <image>@sha256:…` ([docs/deployment.md](docs/deployment.md)); humans only |

`DATABASE_URL` comes from `.env` (or `.env.example`) via the Makefile. Plain
`cargo test` needs it exported.

## Workflow

1. Before changing an area, read its invariant below and the ADR in
   `docs/adr/` that covers it. A change that contradicts an ADR needs a new ADR.
   If the requirement is ambiguous, ask, or record the assumption in
   `docs/requirements.md`; do not pick an interpretation silently.
2. Define "done" as a test before writing code; for a bug, the test that
   reproduces it fails first.
3. Make the simplest change that keeps the invariants: no speculative
   features, settings or abstractions. Keep the diff surgical: every changed
   line traces to the task; mention unrelated problems instead of fixing them.
4. While iterating, run the narrowest test (`cargo test --test api <name>`);
   run `make check` before every commit. Never assume a clippy warning or a
   failing test is pre-existing: `main` is always green.
5. Update README, `CHANGELOG.md` (`Unreleased`), this file and the ADRs in
   the same commit as the behaviour they describe.

## Layout

```
src/routes/   HTTP only: extract, validate into domain types, call db, shape response
src/domain/   validated types (Money, AccountId); invalid values cannot be constructed
src/db/       ALL SQL lives here; transfers.rs holds the transfer transaction
src/error.rs  AppError → status code + {"error":{"code","message"}}
src/config.rs environment configuration (12-factor), validated at startup
migrations/   append-only; never edit an applied migration (`make migration name=x`)
tests/api/    integration tests through the real router, one DB per test
bench/        pgbench harness that justifies the locking strategy
loadtest/     k6 HTTP load test with invariant checks (runs in CI)
deploy/       single-host production stack, signed-image deploy script
.claude/      skills, settings (permission rules) and hooks for agents
```

## Invariants (do not break)

- Money is `BigDecimal`, travels as a JSON **string**, parsed by `Money::parse`;
  never `f64`, never rounded. Column type is unconstrained `NUMERIC`.
- A transfer is one DB transaction: lock both accounts with
  `SELECT … ORDER BY account_id FOR UPDATE` (ascending id = no deadlocks), check
  balance after locking, relative `UPDATE balance = balance ± amount`, commit.
- Every balance stays below 10^20 (`balance_limit()`), checked under the lock
  and backed by `CHECK (balance < 1e20)`.
- The ledger (`transactions`) is append-only (triggers); balances must
  reconcile: `initial_balance + incoming − outgoing = balance`.
- Idempotency key is claimed with `INSERT … ON CONFLICT DO NOTHING` before
  moving money. Only 40001/40P01/55P03 are retried; never retry a connection
  error at COMMIT.
- No `unwrap`/`expect`/`panic!` outside tests (clippy denies them).
- Errors: add a variant to `AppError` with a stable `code`; 5xx never leak
  internals to the client.

## Rust style

- Match our own enums (`AppError`, `MoneyError`, …) exhaustively, without a
  `_` arm, so a new variant must be handled everywhere it matters.
- Prefer `let … else` and let chains over nested `if let`.
- If a lint must be silenced, use `#[expect(lint, reason = "…")]`, not `#[allow]`.
- Comments explain current behaviour and why, not how the code got here.

## Migrations

- Forward only and append only ([ADR 0006](docs/adr/0006-forward-only-migrations.md)):
  `make migration name=<what_it_does>` creates the next file; never edit an
  applied one (sqlx checksums it and refuses to start).
- Start each file with `SET LOCAL lock_timeout = '5s'` (the template does).
- Index on an existing table: `CREATE INDEX CONCURRENTLY` in a file whose
  first line is `-- no-transaction`.
- Constraint on an existing table: add it `NOT VALID`, validate in a later
  migration; add an upgrade test in `tests/api/migrations.rs` (old version →
  old data → upgrade).
- Breaking shape changes go expand → deploy → backfill → switch → contract,
  across releases; the previous binary must keep working on the new schema.

## Guardrails

`.claude/settings.json` enforces part of this file: edits to applied
migrations, reading `.env` / `deploy/.env`, `git push`, `reset --hard`,
`checkout --` and `clean` are denied; `git commit` asks. Hooks run `cargo fmt`
after each edit and refuse to end a turn while `cargo fmt --check` or clippy
fail (`.claude/hooks/lint-gate.sh`). A new applied migration is added to the
deny list in the same commit. Do not weaken a guardrail to get past it; raise
it instead.

## Skills

Project skills in `.claude/skills/` (Claude Code loads them; other agents can
read them as checklists). Each names the files, commands and codes of this repo.

| Skill | Use it to |
|---|---|
| `research` | decide with evidence (bench v1–v7, ADRs) and write an ADR |
| `plan` | clarify against `docs/requirements.md`, split into green commits |
| `implement` | change code test-first without breaking the invariants |
| `test-qa` | place, write and RED-prove tests; Docker smoke |
| `review` | review a diff with fresh eyes against the invariants and ADRs |
| `debug` | find the root cause of a red/flaky test, 500, 503 or hang before fixing |
| `docs` | update README, OpenAPI, ADRs, Mermaid diagrams, CHANGELOG |
| `release` | per-commit CI check, hygiene, version, tag |

## Adding or changing an endpoint

1. Handler in `src/routes/` with `#[utoipa::path(...)]` listing every status.
2. Register it in `routes::router` and in `ApiDoc` paths (`src/routes/openapi.rs`).
3. Integration tests in `tests/api/` (success, invalid input, not found, …).
4. Update the API and error tables in `README.md`.

## Tests

- Integration tests use `#[sqlx::test]` (fresh migrated DB per test). Its pool
  has only 5 connections: concurrency tests build a 16-connection app (`app_with_many_connections` in `concurrency.rs`).
- Test bodies read as given / when / then and call helpers in
  `tests/api/helpers.rs`; HTTP, JSON and SQL plumbing live only there.
- A test that guards a bug must be proven: re-introduce the bug, see it fail,
  restore. Do this from a backup copy, not `git checkout` (it discards other
  uncommitted work).
- Lock-timeout tests take ~6 s by design (3 attempts × 2 s).

## Config

All config is environment variables (`src/config.rs`, table in README). New
setting: add it to `Config`, `.env.example`, the README table, and a unit test.
Never log secrets; `Config`'s `Debug` redacts the database password.

## Commits and PRs

- Conventional commits (`feat:`, `fix:`, `test:`, `docs:`, `chore:`), one
  concern per commit, `make check` green.
- Never commit `.env` or credentials.
- Write comments, commit messages and docs for a reader who never saw the
  working session: no conversation or iteration history, no discarded attempts
  (rejected alternatives belong in an ADR).
