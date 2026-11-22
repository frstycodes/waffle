# Waffle's everyday commands. `make check` is what "done" means: lint, then tests.
.PHONY: help run release demo demos design design-page lint clippy ui-lint ui-lint-update test check fmt

SCENE ?= home

help: ## List the commands
	@grep -hE '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  make %-15s %s\n", $$1, $$2}'

run: ## Run the app (debug build)
	cargo run

release: ## Run the app (release build)
	cargo run --release

demo: ## Run one offline demo scene: make demo SCENE=call-streams
	WAFFLE_DEMO=$(SCENE) cargo run

demos: ## Step through demo scenes: make demos SCENES="profile call"
	tools/demos.sh $(SCENES)

design: ## Open the design-system page in the app
	WAVE_DESIGN=1 cargo run

design-page: ## Serve design/ (the HTML source of truth) on http://127.0.0.1:4319
	python3 -m http.server 4319 --bind 127.0.0.1 --directory design

lint: clippy ui-lint ## Every lint: clippy, then the design-system ratchet

clippy: ## Clippy over every target; warnings fail
	cargo clippy --all-targets -- -D warnings

ui-lint: ## The design-system ratchet (tools/ui_lint.py) and its own tests
	python3 tools/ui_lint.py --check
	python3 tools/test_ui_lint.py

ui-lint-update: ## Lock in counts that fell and regenerate src/ui/CATALOG.md
	python3 tools/ui_lint.py --update

test: ## Every test
	cargo test

check: lint test ## Lint, then test: run before calling anything done

fmt: ## Format the Rust sources
	cargo fmt
