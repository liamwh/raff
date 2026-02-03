set dotenv-load := true

# Show available commands
default:
    @just --list --justfile {{justfile()}}

# Format the Rust code
[private]
fmt:
    cargo +nightly fmt --all

# Open the code documentation in the browser
code-docs:
    cargo doc --workspace --no-deps --open

# Lint the markdown files
lint-docs:
    markdownlint --disable MD013 MD059 -- "**/*.md"

alias fd := fix-docs
# Lint and fix our markdown files. Uses .markdownlintignore to ignore irrelevant files.
fix-docs: fmt
    markdownlint --disable MD013 MD059 -- --fix "**/*.md"

alias lint := fix

# Fix linting errors where possible
fix: fmt && check
    cargo clippy --fix --allow-staged --workspace -- -D warnings --no-deps

# Check for linting errors
check:
    cargo clippy --workspace -- -D warnings --no-deps


alias test := nextest
alias t := nextest
# Run rust tests with `cargo nextest` (all unit-tests, no doc-tests, faster)
nextest *FLAGS="--all":
    cargo nextest run {{ FLAGS }}

# Show unused dependencies
udeps:
    cargo +nightly udeps

# Install the tools suggested for development
install-tools:
    @echo "Installing tools..."
    @echo "If you get an error, try running `brew install cargo-binstall`"

    @echo "Installing cargo-binstall (faster cargo installations)"
    cargo binstall cargo-binstall

    @echo "Installing cargo-llvm-cov (code coverage report generation: https://github.com/taiki-e/cargo-llvm-cov)"
    cargo binstall -y cargo-llvm-cov

    @echo "Installing ripgrep (search tool: https://github.com/BurntSushi/ripgrep)"
    cargo binstall -y ripgrep

    @echo "Installing cargo-udeps (identify unused dependencies: https://github.com/est31/cargo-udeps)"
    cargo binstall -y cargo-udeps

    @echo "Installing cargo-nextest (faster test runner for rust: https://github.com/nextest-rs/nextest)"
    cargo binstall cargo-nextest

    @echo "Installing mdbook (book tool: https://github.com/rust-lang/mdBook)"
    cargo binstall -y mdbook && cargo binstall -y mdbook-toc

    @echo "Installing Rust nightly toolchain"
    rustup toolchain install nightly
    rustup component add rustfmt --toolchain nightly

    @echo "Installing golang-migrate (https://github.com/golang-migrate/migrate)"
    brew install golang-migrate

    @echo "Installing cargo-autoinherit (https://github.com/mainmatter/cargo-autoinherit)"
    cargo install --locked cargo-autoinherit

    @echo "installing Playwright (required for likec4 to export to PNGs)"
    npx playwright install

    @echo "Installing vhs (record terminal sessions as animated GIFs)"
    brew install vhs

    @echo "Installing yh (yaml humanizer)"
    brew install yh

    @echo "Installing kubeconform (validate kubernetes manifests)"
    brew install kubeconform

    @echo "Installing kube-linter (lint kubernetes manifests)"
    brew install kube-linter

    @echo "Installing bruno (recommended API client)"
    brew install bruno

    @echo "Installing dependencies for the root package.json"
    bun install

    @echo "Done!"

# Keeps dependencies DRY
clean-dev-deps:
    cargo autoinherit

# Test a crate and module
test-module package test:
    cargo nextest run --filterset "package({{package}}) & test({{test}})"

# Install pre-commit hooks (prek)
install-hooks:
    @echo "🪝 Installing prek hooks..."
    @prek install
    @echo "✅ Pre-commit hooks installed!"

# Uninstall pre-commit hooks
uninstall-hooks:
    @echo "🪝 Uninstalling prek hooks..."
    @prek uninstall
    @echo "✅ Pre-commit hooks uninstalled!"

# Run pre-commit hooks manually on all files
run-hooks:
    @echo "🪝 Running pre-commit hooks manually..."
    @prek run --all-files

# Validate pre-commit config
validate-hooks:
    @echo "🔍 Validating pre-commit config..."
    @prek validate-config

# Install raff to ~/bin
install:
    #!/bin/bash
    set -e
    BIN_DIR="$HOME/bin"
    mkdir -p "$BIN_DIR"
    echo "🔨 Building raff..."
    cargo build --release
    echo "📦 Installing raff to $BIN_DIR..."
    cp target/release/raff "$BIN_DIR/raff"
    echo "✅ raff installed to $BIN_DIR/raff"

