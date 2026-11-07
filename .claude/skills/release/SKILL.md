---
name: release
description: Prepare a push or a version of this repo - make check, the per-commit CI job (every-commit), secret and hygiene checks, the SemVer bump from CHANGELOG Unreleased, and an annotated tag. Use before pushing several commits, after rewriting history, or for a release.
---

# Release

Steps in this order; stop at the first red and fix it before going on.

| # | Step | Passes when |
|---|---|---|
| 1 | `make db-up && make check` | exit 0; quote the `test result` lines |
| 2 | CI `every-commit` for the range (`.github/workflows/ci.yml`; `gh run list --workflow ci.yml`, `gh run view <id>`) | green for every commit. After a history rewrite run it over the whole history: Actions → CI → *Run workflow* with `range` = `<root>..HEAD` (`git rev-list --max-parents=0 HEAD`), or `gh workflow run ci.yml -f range=<root>..HEAD` |
| 3 | CI `security` (`.github/workflows/security.yml`): cargo-deny, gitleaks, Trivy image scan; locally `cargo deny check` | green. A new advisory: `cargo update -p <crate>` (lockfile only, within semver) and a `### Security` line in the CHANGELOG |
| 3b | Secret scan of what will be pushed: `git diff <base>..HEAD \| grep -inE 'api[_-]?key\|token\|secret\|password\|-----BEGIN\|postgres://'` | the only `postgres://` is the local development URL (`transfers:transfers@localhost`, in `.env.example`, compose, CI); `git ls-files .env` prints nothing |
| 4 | Commit messages: `git log --format=%B <base>..HEAD` | conventional type, one concern each, no AI/assistant references, no session history; author from repo-local `git config user.name` / `user.email` |
| 5 | Migrations in the range | the previous release's binary still starts on the migrated schema (run the old tag once against it), or the reason it does not need to |
| 6 | Version | `CHANGELOG.md` `## Unreleased` → `## x.y.z - YYYY-MM-DD`. While 0.x: a removed or changed endpoint, error code or status → minor; Added → minor; only Fixed → patch |
| 7 | Tag | `git tag -a vX.Y.Z -m vX.Y.Z` on the release commit; never on a red or unknown CI run |

## Push

Push or merge only after the user says yes, choosing among: merge locally,
push and open a PR, keep the branch. Never force-push `main` unless the user
asks for that specific push.

## Report

Table: step × ✅/❌ × the command and the line that proves it; list anything
skipped and why.
