.PHONY: fmt lint test web-install web-check web-test web-build verify e2e

fmt:
	cargo fmt --all

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace --all-targets

web-install:
	cd web && bun install --frozen-lockfile

web-check:
	cd web && bun run check

web-test:
	cd web && bun test

web-build:
	cd web && bun run build

verify: lint test web-install web-check web-test web-build

e2e:
	./scripts/e2e.sh
