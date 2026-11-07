.PHONY: up down db-up run test lint fmt check deny loadtest migration

# Load .env when present (falls back to .env.example values otherwise).
ENV_FILE := $(if $(wildcard .env),.env,.env.example)
include $(ENV_FILE)
export

## One command: build the image, start PostgreSQL, apply migrations and serve on :8080.
up:
	docker compose up --build

down:
	docker compose down -v

## Start only PostgreSQL (for running the service or tests locally with cargo).
db-up:
	docker compose up -d --wait db

## Apply migrations and start the service locally.
run: db-up
	cargo run --release

## Unit + integration tests (each integration test gets its own fresh database).
test: db-up
	cargo test --locked

lint:
	cargo fmt --check
	cargo clippy --all-targets --locked -- -D warnings

fmt:
	cargo fmt

check: lint test

## Dependencies: RustSec advisories, licenses, sources (needs `cargo install cargo-deny`).
deny:
	cargo deny check

## HTTP load test (k6) against the docker compose stack: PROFILE=smoke (default) or load.
## Report: loadtest/results/report.html; details in loadtest/README.md.
loadtest:
	mkdir -p loadtest/results && chmod a+w loadtest/results
	docker compose up -d --build --wait db app
	@for i in $$(seq 1 60); do curl -sf -o /dev/null http://localhost:$${APP_PORT:-8080}/ready && break; sleep 1; done
	docker compose --profile loadtest run --rm k6

## New migration file with the next version: make migration name=add_account_status
## Rules: docs/adr/0006-forward-only-migrations.md
migration:
	@# Only accept `name` from the command line, never from an exported shell variable.
	@test "$(origin name)" = "command line" || { echo "usage: make migration name=<what_it_does>"; exit 1; }
	@echo "$(name)" | grep -Eq '^[a-z0-9_]+$$' || { echo "name must be snake_case: [a-z0-9_]+"; exit 1; }
	@next=$$(printf "%04d" $$(( $$(ls migrations/*.sql | wc -l) + 1 ))); \
	file="migrations/$${next}_$(name).sql"; \
	printf -- "-- %s\n-- Forward only; see docs/adr/0006-forward-only-migrations.md\nSET LOCAL lock_timeout = '5s';\n\n" "$(name)" > "$$file"; \
	echo "created $$file"
