# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## clockify-agent (`clockify-agent/`)

CLI for logging time entries and viewing summaries from Clockify.

- **Build**: `cd clockify-agent && go build -o clockify .`
- **Test**: `cd clockify-agent && go test ./...`
- **Single test**: `cd clockify-agent && go test ./internal/validator/ -run TestHasOverlap`
- **Config**: `~/.config/clockify-agent/config.yaml` (TOML), overridden by env vars. Also reads from RMS Clockify and Trello `.env` files (hardcoded paths in `internal/config/config.go`).

Architecture:
- `main.go` — CLI entry point with `log` and `summary` subcommands
- `internal/clockify/` — Clockify API client (POST time entries, GET detailed reports)
- `internal/config/` — Layered config: RMS .env → user TOML → env vars → save back to TOML
- `internal/summary/` — Aggregates time entries by project/task for date ranges
- `internal/trello/` — Calls `trello-agent meta` to resolve card URLs → board/project info for Clockify project mapping
- `internal/timeutil/` — Duration parsing (flexible format: "2h30m", "1.5h", "45m")
- `internal/validator/` — Daily 14h cap check and time overlap detection before creating entries

## Patterns

- All Go projects use standard library `testing` package — no external test frameworks
- Config is TOML with env var override pattern (env vars take highest priority)
- `internal/` packages have corresponding `*_test.go` files in the same package
- API clients use `net/http` directly, no third-party HTTP libraries
