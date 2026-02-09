.PHONY: help ci ci-docs ci-rust lint-docs lint-rust test-rust doc-rust coverage-rust deny-rust

help:
	@echo "Dark Forest quality commands"
	@echo "  make ci-docs     - Run docs quality checks (markdown, yaml, spelling)"
	@echo "  make ci-rust     - Run rust quality checks (if Cargo.toml exists)"
	@echo "  make ci          - Run docs + rust checks"

ci: ci-docs ci-rust

ci-docs: lint-docs

ci-rust: lint-rust test-rust doc-rust coverage-rust deny-rust

lint-docs:
	@command -v lychee >/dev/null || (echo "lychee is required" && exit 1)
	@if command -v markdownlint >/dev/null; then \
		find . -type f -name '*.md' -not -path './.git/*' -print0 | xargs -0 markdownlint --config .markdownlint.yml; \
	else \
		command -v bun >/dev/null || (echo "markdownlint is unavailable and bun is not installed"; exit 1); \
		find . -type f -name '*.md' -not -path './.git/*' -print0 | xargs -0 bunx --bun markdownlint-cli --config .markdownlint.yml; \
	fi
	@if command -v yamllint >/dev/null; then \
		yamllint -c .yamllint.yml .; \
	else \
		echo "yamllint not found; using syntax-only fallback checker"; \
		ruby ./scripts/lint_yaml_syntax.rb; \
	fi
	@if command -v codespell >/dev/null; then \
		codespell --config .codespellrc; \
	else \
		echo "codespell not found; using fallback typo checker"; \
		python3 ./scripts/spellcheck_fallback.py --config .codespellrc; \
	fi
	@lychee --config .lychee.toml './**/*.md' './**/*.yml' './**/*.yaml'

lint-rust:
	@if [ ! -f Cargo.toml ]; then echo "No Cargo.toml found; skipping rust lint checks."; exit 0; fi
	@cargo fmt --all -- --check
	@cargo clippy --workspace --all-targets --all-features -- -D warnings

test-rust:
	@if [ ! -f Cargo.toml ]; then echo "No Cargo.toml found; skipping rust tests."; exit 0; fi
	@cargo test --workspace --all-targets --all-features

doc-rust:
	@if [ ! -f Cargo.toml ]; then echo "No Cargo.toml found; skipping rustdoc checks."; exit 0; fi
	@RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

coverage-rust:
	@if [ ! -f Cargo.toml ]; then echo "No Cargo.toml found; skipping coverage checks."; exit 0; fi
	@command -v cargo-llvm-cov >/dev/null || (echo "cargo-llvm-cov is required" && exit 1)
	@cargo llvm-cov --workspace --all-features --all-targets --summary-only
	@echo "coverage-rust advisory: workspace-level 85% is non-blocking during bootstrap (see TESTING.md)."

deny-rust:
	@if [ ! -f Cargo.toml ]; then echo "No Cargo.toml found; skipping cargo-deny checks."; exit 0; fi
	@command -v cargo-deny >/dev/null || (echo "cargo-deny is required" && exit 1)
	@host_target=$$(rustc -vV | awk '/^host:/ {print $$2}'); \
	temp_cargo_home=$$(mktemp -d /tmp/dark-forest-cargo-home.XXXXXX); \
	metadata_file=$$(mktemp /tmp/dark-forest-cargo-metadata.XXXXXX.json); \
	trap 'rm -rf "$$temp_cargo_home" "$$metadata_file"' EXIT INT TERM; \
	mkdir -p "$$temp_cargo_home/advisory-dbs"; \
	if [ -d "$$HOME/.cargo/advisory-dbs" ]; then \
		cp -R "$$HOME/.cargo/advisory-dbs/." "$$temp_cargo_home/advisory-dbs/"; \
	else \
		echo "cargo-deny advisory DB not found at $$HOME/.cargo/advisory-dbs; run 'cargo deny fetch advisories' first"; \
		exit 1; \
	fi; \
	if [ -d "$$HOME/.cargo/registry" ]; then ln -s "$$HOME/.cargo/registry" "$$temp_cargo_home/registry"; fi; \
	if [ -d "$$HOME/.cargo/git" ]; then ln -s "$$HOME/.cargo/git" "$$temp_cargo_home/git"; fi; \
	cargo metadata --format-version 1 --filter-platform "$$host_target" > "$$metadata_file"; \
	CARGO_HOME="$$temp_cargo_home" cargo deny check advisories licenses bans sources --metadata-path "$$metadata_file" --disable-fetch
