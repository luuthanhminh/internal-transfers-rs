@AGENTS.md

Claude Code specifics (the rules above apply to every agent and human):

- Project skills live in `.claude/skills/`; pick the one for the phase you are in (table in AGENTS.md).
- `.claude/settings.json` enforces part of AGENTS.md mechanically: applied migrations cannot be edited, `.env` files cannot be read, `git push` / `reset --hard` / `clean` are denied, `cargo fmt` runs after every edit, and a Stop hook refuses to end a turn while `cargo fmt --check` or clippy fail.
- Review with a fresh session (the `review` skill), never in the session that wrote the change.
