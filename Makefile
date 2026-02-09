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
	@command -v markdownlint >/dev/null || (echo "markdownlint-cli is required" && exit 1)
	@command -v yamllint >/dev/null || (echo "yamllint is required" && exit 1)
	@command -v codespell >/dev/null || (echo "codespell is required" && exit 1)
	@command -v lychee >/dev/null || (echo "lychee is required" && exit 1)
	@find . -type f -name '*.md' -not -path './.git/*' -print0 | xargs -0 markdownlint --config .markdownlint.yml
	@yamllint -c .yamllint.yml .
	@codespell --config .codespellrc
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
	@cargo llvm-cov --workspace --all-features --all-targets --summary-only --fail-under-lines 85

deny-rust:
	@if [ ! -f Cargo.toml ]; then echo "No Cargo.toml found; skipping cargo-deny checks."; exit 0; fi
	@command -v cargo-deny >/dev/null || (echo "cargo-deny is required" && exit 1)
	@cargo deny check advisories licenses bans sources
