---
name: docs
description: Update this service's docs in the same commit as the change - README API/error/config tables, OpenAPI annotations, ADRs, Mermaid diagrams in docs/architecture.md and the ADRs, CHANGELOG Unreleased. Use when behaviour, an endpoint, an error code, a setting or a decision changes.
---

# Docs

Docs ship in the same commit as the behaviour they describe; a commit whose
README error table no longer matches `AppError` is incomplete.

| Change | Update |
|---|---|
| Endpoint or error code | README API and error tables, `#[utoipa::path]`; then `cargo test --test api api_docs` |
| Setting | README configuration table, `.env.example` |
| Decision costly to reverse (locking, money type, retry set, migration policy, a crate on the transfer path) | a new ADR `docs/adr/NNNN-<decision>.md`, next free number (`ls docs/adr`), existing headings (Status / Context / Decision / Consequences / Alternatives considered). Routine endpoints need no ADR |
| A decision changes | new ADR; the old one is never rewritten or deleted, it gets `Status: superseded by NNNN` |
| Working rule | `AGENTS.md` and the matching `.claude/skills/*` |
| Flow, schema or transfer step | `docs/architecture.md` (flowchart, sequence, step table, ER diagram) |
| Pitfall or its guard test | `docs/research/pitfalls-and-test-plan.md` |
| Anything a user or operator notices | `CHANGELOG.md` → `## Unreleased`, Keep a Changelog groups (Added / Changed / Fixed / Removed); no internal refactors |

## Check docs like code

- Error codes: compare `grep -o '"[a-z_]*"' src/error.rs` with the README error
  table.
- Commands written in README or AGENTS.md are run once after editing them.
- Comments in code say why (the invariant, the SQLSTATE, the race it prevents),
  not what; a line someone might "simplify" gets a note (why
  `ORDER BY account_id`, why `tokio::spawn`).

## Diagrams

- Mermaid v11 inside the `.md` (GitHub renders it); no exported images.
- Reuse the palette in `docs/architecture.md`: `classDef` with light fills and
  dark text (`ok` green, `err` red, `busy` orange, `check` yellow, `core` blue)
  so nodes read on light and dark themes; sequence diagrams use `rect rgba(…)`.
- Render-check before committing, light and dark:
  `npx -p @mermaid-js/mermaid-cli mmdc -i x.mmd -o x.png -t dark -b '#0d1117'`
  and `-t default -b white`. `<` and `;` inside sequence-diagram text break parsing.

## Writing rules

- Written for someone who never saw how the code got here: no iteration
  history; rejected options go in an ADR's *Alternatives considered*.
- Assumptions stay marked as defaults to be confirmed with the product owner.
- No secrets or real hosts: name the variable (`DATABASE_URL`), never a value
  other than the local development default; no repository URL.
