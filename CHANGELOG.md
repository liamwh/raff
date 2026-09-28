# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-09-28

### Changed

- Coupling now checks the Stable Dependencies Principle: it warns when a crate
  depends on a less stable crate (higher instability), naming both crates and
  their instability. The old warning for any crate with instability above 0.7
  is gone; it flagged binaries and other leaf crates that are unstable by design.
- In staged (pre-commit) mode the Stable Dependencies check still runs on the
  whole workspace graph and reports every violating edge that touches a staged
  crate.
- Dev-dependencies no longer count towards crate coupling.
- Volatility score is now `commit touches + alpha * (lines added + lines deleted)`,
  matching the documented meaning of `--alpha`. It previously computed the
  reverse, so with the default alpha the score was almost pure churn.
- Volatility only counts `.rs` files, so test fixtures and generated data no
  longer dominate the ranking.
- Statement count groups files by workspace member crate when `--path` is the
  root of a Cargo workspace with two or more members. Other paths keep the
  per-directory grouping.
- Command-line flags now take precedence over configuration files, which take
  precedence over built-in defaults.
- The CLI findings table prints messages in full and puts the summary line
  underneath the table.
- `raff --version` and `--help` now say `raff` instead of `rust-ff` and `aff`.
- Replaced `chrono` with `jiff`.
- `raff all` fails closed: a rule that cannot run becomes an error finding
  and fails the run, in table, JSON, HTML, SARIF and JUnit output. A missing
  `rust-code-analysis-cli` is reported as a skipped note instead.
- `--fail-on-warnings` fails on warnings only, not on informational notes.
- Volatility and contributor-report accept any `--path` inside a repository.
  Volatility reports the crates under the path plus the crate enclosing it;
  contributor-report counts only changes under the path.
- Volatility no longer warns when only one crate is in scope, because there is
  nothing to compare it with.

### Fixed

- JUnit output was malformed XML whenever a finding was a warning.
- JUnit testcase names could panic when truncated inside a multi-byte character.
- `--no-cache` was ignored by the individual rules, and cached volatility
  results written without `--normalize` failed to load on the next run. Entries
  that cannot be decoded are now treated as cache misses.
- Statement-count cache entries now include file contents, so edits are never
  served stale results.
- A file that `syn` cannot parse is skipped with a warning naming the file,
  instead of aborting the whole statement count.
- A crate at the repository root owned no files in volatility analysis.
- Contributor report showed today's date as every contributor's last commit.
- Help links in findings pointed at pages that do not exist; they now point at
  README sections.

### Removed

- The `sc_threshold` setting from the documented pre-commit profile: the
  profile skips statement count, so the value had no effect.
- Unused `anyhow`, `quick-junit` and `sarif_rust` dependencies, and leftover
  release-plz, cargo-dist and git-cliff configuration.

### Added

- CI workflow running formatting, clippy, tests and the end-to-end script; the
  release workflow now requires it to pass.
- `RAFF_CACHE_DIR` to override the cache location.

## [0.1.4] - 2026-06-21

### Added

- Pre-commit mode: `--staged`, `--fast`, `--quiet` and the `pre-commit` profile.
- SARIF and JUnit output (`--ci-output`) for every rule, and `--output-file`.
- Actionable CLI table output for `raff all`.
- Hierarchical configuration, including `.raff/raff.toml` discovered at the
  git root.
- Result caching with versioned entries.
- Automatic exclusion of the target directory and `.gitignore` support.
- Tag-driven releases to crates.io, GitHub releases and the Homebrew tap.

## [0.1.3] - 2025-06-18

### Fixed

- Ignore files by language for the rust-code-analysis rule.

## [0.1.2] - 2025-06-18

### Changed

- Release builds with cargo-dist.

## [0.1.1] - 2025-06-18

### Changed

- Support for cargo-binstall.

## [0.1.0] - 2025-06-13

### Added

- Statement count, volatility, coupling and rust-code-analysis rules.
- HTML, JSON, CSV and DOT output.
