# Delivery plan

Stages in the order they are delivered. Each stage ends with `make check`
green; every commit builds, passes fmt, clippy and the tests, and starts (CI
checks each commit of a push). `CHANGELOG.md` → `Unreleased` is updated in
the commit that makes the change.

| Stage | Goal | Main tasks | Done when |
|---|---|---|---|
| S0 Setup | Walking skeleton | Cargo project, toolchain pin, lints, `CHANGELOG.md`; Makefile, `.env.example`, compose PostgreSQL, `AGENTS.md`; typed config, logging, graceful shutdown; `/health`, `/ready`, request ids, JSON errors; OpenAPI skeleton; Dockerfile; CI; test harness; README skeleton | `docker compose up` serves `/health`; CI green |
| S1 Requirements | Agree on behaviour | [requirements.md](requirements.md): scope, assumptions with defaults, acceptance criteria | Every open question has a default |
| S2 Research and design | Decide before coding | ADRs 0001–0005; locking benchmark; pitfalls and test plan | Locking strategy chosen from measurements |
| S3 Build and test | Implement the business endpoints | `Money`/`AccountId` + unit tests → schema migration → accounts → transfers with ordered locking and retries → history tests → concurrency tests → `Idempotency-Key` | All acceptance criteria covered by tests |
| S4 Review | Fix what review finds | Review the busy paths, MSRV, test realism, replays, error messages, the balance limit | Review findings closed with tests |
| S5 Production readiness | Run it for real | OpenAPI for business endpoints, TLS to managed PostgreSQL, migrations as a release step and forward-only migration rules (ADR 0006), pinned images, connection budget | Twelve-factor checklist passes |
| S6 Pipeline | CI follow-ups | Toolchain from `rust-toolchain.toml`, pinned service images, a job that checks every commit of a push or PR | CI reproduces local results |
| S7 Docs and release | Hand over | Final README, architecture doc with diagrams, project skills, CHANGELOG `0.1.0`, tag `v0.1.0` | A new reader can run, test and change the service from the docs |
| S8 Harden and ship | Run it in public | Agent guardrails (settings, hooks), property tests, k6 load test in CI, coverage, CodeQL and workflow lint, digest pins, signed release images, tunnel-only deployment with a release-step migration | `make loadtest` green in CI; the tagged image is signed and serves the public URL |

## S3 task breakdown

| # | Task | Tests added |
|---|---|---|
| 1 | `Money` (strict parsing, limits, canonical format), `AccountId` | Unit tests for every pitfall in M1–M8 |
| 2 | Migration: `accounts`, `transactions`, constraints, immutability triggers | Covered by the integration tests below |
| 3 | `POST /accounts`, `GET /accounts/{account_id}`, JSON extractors, `AppError` variants | Success, duplicate (also concurrent), invalid input, not found, JSON errors |
| 4 | `POST /transactions`: ordered locks, check after lock, relative updates, retries | Success, insufficient funds, missing accounts, same account, invalid amounts, DB-level guard |
| 5 | History guarantees | Immutability, reconciliation |
| 6 | Concurrency suite | Opposite directions, concurrent withdrawals, random transfers with invariants |
| 7 | `Idempotency-Key` | Replay, key reuse, concurrent same key, invalid key, no key |

## Risks

| Risk | Mitigation |
|---|---|
| sqlx 0.9 is recent and has few examples | Built the skeleton first; fall back to 0.8 if blocked |
| A concurrency test passes by luck | Re-introduce each bug and watch the test fail |
| Scope creep | Out-of-scope list in the requirements; improvements go to the README |
