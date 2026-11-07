---
name: plan
description: Clarify a request against docs/requirements.md, then plan it for this service as commits that each pass make check, with the tests in tests/api, the invariants touched, the interfaces later tasks rely on and the docs to update. Use before any change to accounts, transfers, idempotency, the schema or config, and before writing code for anything bigger than a one-line fix.
---

# Plan

## Gate

No edit under `src/`, `migrations/` or `tests/` until the user approved the
plan (or explicitly said "just do it"). The plan lives in the PR description;
before a PR exists, in an untracked `plans/<date>-<slug>.md`.

## 0. Clarify

List what the request leaves open: how many decimals (today 20 integer + 18
fraction digits), the balance limit (balances stay below 10^20), key scope
(`Idempotency-Key` is global, not per client), which status (409 vs 422 vs
503). Record each in `docs/requirements.md` → *Assumptions and defaults* with a
default; README *Assumptions* keeps the note that defaults are confirmed with
the product owner.

Ask the user only when the answer changes the schema, a public error `code` or
status, or money semantics. One question at a time, 2–4 options, the
recommended one first.

## 1. Tasks

One row per commit; each commit is green on its own (CI `every-commit` checks
out every SHA).

| Field | Content |
|---|---|
| Goal | one sentence |
| Files | exact paths; `migrations/` new file only (`make migration`) |
| Interfaces | names later tasks rely on, e.g. `AppError::X` → code `x_y`, `fn balance_limit()` |
| Tests | `tests/api/<file>.rs` + test names written as sentences |
| Verify | command + expected result: `cargo test --test api <name>` fails before, passes after; `make check` exit 0 |
| Invariants | Money 20/18 digits · balance < 10^20 · ordered `FOR UPDATE` · append-only ledger + reconciliation · Idempotency-Key + `ON CONFLICT` · `AppError` code |
| Docs | README tables, `#[utoipa::path]` + `ApiDoc`, ADR, `docs/architecture.md`, `CHANGELOG.md` Unreleased |

Order: domain type + unit tests → migration (expand first, ADR 0006) →
`src/db/` → route + `AppError` → concurrency / idempotency / resilience tests →
docs. More than ~5 files, or a refactor mixed with a feature → split.

## 2. Self-check before asking for approval

- Coverage: every changed or added acceptance criterion in
  `docs/requirements.md` maps to a task and a test; a criterion with no test is
  a gap.
- Interfaces: names and signatures are identical across tasks.
- *Review focus*: up to 5 failure modes the request implies but no task tests
  yet (same key concurrently, balance exactly 10^20 − smallest unit, client
  disconnect, lock timeout, previous binary on the new schema); each gets a test.
- For schema or locking changes, read the plan once as a failure-mode analyst:
  each finding = location + concrete failure scenario + accept/reject.
- A task that needs an undecided choice gets a `research` task first.

## Red flags (rewrite the plan)

- Lines like "TBD", "handle edge cases", "add validation", "write tests".
- Code bodies in the plan: signatures and test names are enough.
- A migration and the code that depends on it in the wrong order.
- Checkpoints missing after risky tasks (a migration, `src/db/transfers.rs`):
  `make check` and a user look before continuing.

`docs/plan.md` records the S0 → S7 delivery order; a new phase of work follows
the same order (setup work first).
