# Shortcuts for macOS and Linux. On Windows, run the npm and cargo commands directly.
.PHONY: help dev ci

help: ## List the targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F ':.*## ' '{printf "  %-6s %s\n", $$1, $$2}'

# Reinstall the UI dependencies only when the manifest or lockfile changed.
ui/node_modules/.package-lock.json: ui/package.json ui/package-lock.json
	npm --prefix ui ci
	@touch $@

dev: ui/node_modules/.package-lock.json ## Run Tolon locally with hot reload
	npm --prefix ui run tauri dev

ci: ui/node_modules/.package-lock.json ## Run the full check every change must pass
	cargo xtask ci
