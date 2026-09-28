# raff - Rust Architecture Fitness Functions

Inspired by [Mark Richards](https://developertoarchitect.com/mark-richards.html)'s [workshop](https://2025.dddeurope.com/program/architecture-the-hard-parts/) on software architecture, raff turns architectural goals for a Rust codebase into checks you can run locally, in a pre-commit hook or in CI.

## Getting started

### Prerequisites

* A Rust toolchain. Install it from [rustup.rs](https://rustup.rs/).
* Git, for the volatility and contributor reports.
* `rust-code-analysis-cli` on your `PATH`, for the `rust-code-analysis` command only (`cargo install rust-code-analysis-cli`).

### Installation

```bash
cargo install raff-cli

brew install liamwh/raff/raff

# from source, install to ~/bin
just install
```

Once installed, the `raff` binary is on your `PATH`. `raff --help` lists the commands and `raff <COMMAND> --help` lists each command's flags.

## Fitness functions

Every command takes `--path` (default `.`), `--output` for the human-readable report, `--output-file` to write it to disk, and `--ci-output sarif` or `--ci-output j-unit` for CI tooling. Global flags include `--config`, `--profile`, `--staged` (analyse only git-staged changes), `--no-cache` and `--clear-cache`.

### Statement count

```bash
raff statement-count --path . --threshold 10
```

raff parses every `.rs` file under `--path` with `syn`, counts statements, and groups the counts into components. When `--path` is the root of a Cargo workspace with two or more members, each member package is a component, named by package name; a file belongs to the member whose manifest or target directory is its closest ancestor, and files no member owns are grouped under `(outside workspace members)`. Anywhere else, including a single-package crate, each top-level directory under `--path` is a component.

Each component's share of the total is reported as a percentage. A component above `--threshold` percent (default 10) is an error, and the command exits non-zero. Files that `syn` cannot parse are skipped with a warning naming the file.

Output formats: `table`, `html`.

### Module coupling

```bash
raff coupling --path . --granularity both
```

For each workspace crate, and each module within a crate, raff reports:

* **Ce** (efferent coupling), the number of other components this one depends on.
* **Ca** (afferent coupling), the number of other components that depend on this one.
* **I** (instability), `Ce / (Ce + Ca)`, from 0 (stable) to 1 (unstable).

Crate edges come from `cargo metadata`. Normal and build dependencies count; dev-dependencies do not. Module edges come from `use` statements and paths in the source.

A high I is not a problem on its own. Binaries and other entry points should sit at or near 1. raff instead checks the Stable Dependencies Principle: a crate should depend only on crates at least as stable as itself. For every workspace edge `A -> B` where `I(B) > I(A)`, raff emits a warning such as:

```text
Crate 'helix-tui' (I=0.67) depends on less stable crate 'helix-view' (I=0.70), violating the Stable Dependencies Principle
```

`--granularity` accepts `crate`, `module` or `both`. Output formats: `table`, `json`, `yaml`, `html`, `dot` (Graphviz).

### Volatility

```bash
raff volatility --path . --alpha 0.01 --since 2024-01-01
```

raff walks the git history and, for each crate, counts commit touches and churn (lines added plus lines deleted) in the crate's `.rs` files. A commit counts as a touch only if it changes at least one Rust file in the crate. The score is:

```text
raw_score = touches + alpha * churn
```

Touches always carry a weight of 1. The default `--alpha 0.01` keeps the score focused on how often a crate changes; raise it to give lines changed more influence. `--normalize` also divides the score by the crate's lines of code, `--skip-merges` ignores merge commits and `--since YYYY-MM-DD` limits the window. Crates in the top quartile of raw scores get a warning. `--path` can be any directory in the repository: raff reports the crates under it plus the crate that contains it, so `--path src` in a single-crate repository reports that crate.

Output formats: `table`, `csv`, `json`, `yaml`, `html`.

### Rust code analysis

```bash
raff rust-code-analysis --path . --metrics
```

A wrapper around `rust-code-analysis-cli` that runs it over every `src/` folder and collects the metrics (cyclomatic and cognitive complexity, Halstead, maintainability index and so on) into one report. Pass extra arguments through with `-f`, set parallelism with `--jobs`. It reports metrics as notes and does not fail the run.

Output formats: `table`, `json`, `yaml`, `html`.

### Contributor report

```bash
raff contributor-report --path . --decay 0.01 --since 2024-01-01
```

Ranks committers from git history. Each commit scores `(1 + churn + files_touched) * e^(-decay * days_since_commit)`, so recent work counts for more. When `--path` is a subdirectory of the repository, only changes under it count.

Output formats: `table`, `html`, `json`, `yaml`.

### Everything at once

```bash
raff all --path . --output html --output-file raff-report.html
```

Runs every analysis and writes one consolidated report (`html`, `json` or `cli`). Per-rule settings take prefixed flags such as `--sc-threshold`, `--vol-alpha` and `--coup-granularity`. `--fast` skips volatility and rust-code-analysis, and `--fail-on-warnings` makes warnings fail the run as well as errors. A rule that cannot run (for example volatility outside a git repository) is reported as an error and fails the run. A missing `rust-code-analysis-cli` is the exception: it is reported as a note and the run still passes.

## Configuration

raff reads settings, one section per rule, from these files. Later files override earlier ones, and flags passed on the command line override all of them:

1. `~/.config/raff/raff.toml` (or under `$XDG_CONFIG_HOME`)
2. `.raff/raff.local.toml` at the git repository root
3. `Raff.toml`, `.raff.toml` or `raff.toml`, searched upwards from the current directory, or `.raff/raff.toml` at the repository root

Pass `--config <PATH>` to use a specific file. See [`.raff/raff.toml`](.raff/raff.toml) for this repository's own configuration.

Statement count and volatility results are cached in `~/.cache/raff` (set `RAFF_CACHE_DIR` to move it). `--no-cache` skips the cache for that run and `--clear-cache` empties it.

## Pre-commit hook

raff has a built-in `pre-commit` profile for use as a hook. It runs the coupling SDP check only. Statement count needs the whole tree to compute each component's share, so run `raff all` in CI for it; volatility and rust-code-analysis are too slow for a hook.

With `--profile pre-commit`, `raff all`:

* analyses only git-staged changes,
* prints a one-line summary on success and a CLI table on failure,
* sets `fail_on_warnings`, so any SDP warning fails the hook.

Add raff to your `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: raff-architecture-check
        name: Architecture fitness functions
        entry: raff --profile pre-commit all
        language: system
        pass_filenames: false
        files: '(^|/)Cargo\.toml$|(^|/)Cargo\.lock$|(^|/)rust-toolchain(\.toml)?$|^\.cargo/|\.rs$'
```

The defaults work without extra configuration. To override them, add a profile to `.raff/raff.toml`:

```toml
[profile.pre_commit]
fast = true
staged = true
quiet = true
```

To try it by hand, stage some files and run the profile:

```bash
git add src/
raff --profile pre-commit all
```

## Future work

* [ ] Do any domain objects use primitive types (e.g. `String` instead of `Name`)?
* [ ] Is the codebase flat? Analyse and visualise the component hierarchy.
* [ ] No source code should live in the root namespace (or other configurable namespace rules).
