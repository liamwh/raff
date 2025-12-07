#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${ROOT_DIR}/target/debug/raff"

log() { printf "==> %s\n" "$*"; }
warn() { printf "WARN: %s\n" "$*" >&2; }
fail() { printf "ERROR: %s\n" "$*" >&2; exit 1; }

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "Missing required command: $1"
}

require_cmd cargo
require_cmd git
require_cmd jq

log "Building raff binary (debug) ..."
cargo build --quiet --bin raff --manifest-path "${ROOT_DIR}/Cargo.toml"

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/raff-e2e-XXXXXX")"
cleanup() {
  rm -rf "${WORKDIR}"
}
trap cleanup EXIT

create_workspace() {
  log "Seeding fixture workspace at ${WORKDIR}"
  cat > "${WORKDIR}/Cargo.toml" <<'EOF'
[workspace]
members = ["crate_a", "crate_b"]
resolver = "2"
EOF

  mkdir -p "${WORKDIR}/crate_a/src" "${WORKDIR}/crate_b/src"

  cat > "${WORKDIR}/crate_a/Cargo.toml" <<'EOF'
[package]
name = "crate_a"
version = "0.1.0"
edition = "2021"

[dependencies]
crate_b = { path = "../crate_b" }
EOF

  cat > "${WORKDIR}/crate_a/src/utils.rs" <<'EOF'
pub fn double(x: i32) -> i32 {
    let twice = x * 2;
    let triple = x * 3;
    twice + triple
}
EOF

  cat > "${WORKDIR}/crate_a/src/main.rs" <<'EOF'
mod utils;

fn main() {
    let v = crate_b::helper(1);
    let doubled = utils::double(v);
    println!("{}", doubled);
}
EOF

  cat > "${WORKDIR}/crate_b/Cargo.toml" <<'EOF'
[package]
name = "crate_b"
version = "0.1.0"
edition = "2021"

[lib]
name = "crate_b"
path = "src/lib.rs"
EOF

  cat > "${WORKDIR}/crate_b/src/lib.rs" <<'EOF'
pub fn helper(x: i32) -> i32 {
    let y = x + 1;
    y
}
EOF
}

advance_history() {
  log "Creating deterministic git history"
  git -C "${WORKDIR}" init -q
  git -C "${WORKDIR}" config user.name "Raff E2E"
  git -C "${WORKDIR}" config user.email "raff-e2e@example.com"

  git -C "${WORKDIR}" add .
  GIT_AUTHOR_NAME="Alice" GIT_AUTHOR_EMAIL="alice@example.com" \
  GIT_COMMITTER_NAME="Alice" GIT_COMMITTER_EMAIL="alice@example.com" \
  GIT_AUTHOR_DATE="2024-01-01T00:00:00Z" GIT_COMMITTER_DATE="2024-01-01T00:00:00Z" \
    git -C "${WORKDIR}" commit -q -m "Initial workspace"

  cat > "${WORKDIR}/crate_b/src/lib.rs" <<'EOF'
pub fn helper(x: i32) -> i32 {
    let y = x + 1;
    y
}

pub fn square(x: i32) -> i32 {
    x * x
}
EOF
  git -C "${WORKDIR}" add crate_b/src/lib.rs
  GIT_AUTHOR_NAME="Bob" GIT_AUTHOR_EMAIL="bob@example.com" \
  GIT_COMMITTER_NAME="Bob" GIT_COMMITTER_EMAIL="bob@example.com" \
  GIT_AUTHOR_DATE="2024-01-02T00:00:00Z" GIT_COMMITTER_DATE="2024-01-02T00:00:00Z" \
    git -C "${WORKDIR}" commit -q -m "Add square helper"

  cat > "${WORKDIR}/crate_a/src/main.rs" <<'EOF'
mod utils;

fn main() {
    let v = crate_b::helper(1);
    let doubled = utils::double(v);
    let squared = crate_b::square(doubled);
    println!("{}", squared);
}
EOF
  git -C "${WORKDIR}" add crate_a/src/main.rs
  GIT_AUTHOR_NAME="Alice" GIT_AUTHOR_EMAIL="alice@example.com" \
  GIT_COMMITTER_NAME="Alice" GIT_COMMITTER_EMAIL="alice@example.com" \
  GIT_AUTHOR_DATE="2024-01-03T00:00:00Z" GIT_COMMITTER_DATE="2024-01-03T00:00:00Z" \
    git -C "${WORKDIR}" commit -q -m "Use square in main"
}

assert_statement_count() {
  log "Checking statement-count output"
  local output
  output="$("${BIN}" statement-count --path "${WORKDIR}" --threshold 80 --output table)"

  echo "${output}" | grep -Eq "crate_a\\s+│\\s+70 %\\s+│\\s+7\\s+│\\s+2" \
    || fail "crate_a row missing or values unexpected"
  echo "${output}" | grep -Eq "crate_b\\s+│\\s+30 %\\s+│\\s+3\\s+│\\s+1" \
    || fail "crate_b row missing or values unexpected"
  echo "${output}" | grep -q "Total statements = 10" \
    || fail "Total statements summary missing"
}

assert_volatility() {
  log "Checking volatility output (JSON)"
  local raw sorted expected
  raw="$("${BIN}" volatility --path "${WORKDIR}" --alpha 1 --output json)"
  sorted="$(echo "${raw}" | jq -c '[.[] | {crate_name, birth_date, commit_touch_count, lines_added, lines_deleted, raw_score}] | sort_by(.crate_name)')"
  expected='[
  {
    "crate_name": "crate_a",
    "birth_date": "2024-01-01",
    "commit_touch_count": 2,
    "lines_added": 21,
    "lines_deleted": 1,
    "raw_score": 24.0
  },
  {
    "crate_name": "crate_b",
    "birth_date": "2024-01-01",
    "commit_touch_count": 2,
    "lines_added": 16,
    "lines_deleted": 0,
    "raw_score": 18.0
  }
]'
  [[ "${sorted}" == "$(echo "${expected}" | jq -c 'sort_by(.crate_name)')" ]] \
    || fail "Volatility output did not match golden data"
}

assert_coupling() {
  log "Checking coupling output (JSON)"
  local raw projected expected
  raw="$("${BIN}" coupling --path "${WORKDIR}" --granularity crate --output json)"
  projected="$(echo "${raw}" | jq -c '{crates: (.crates | map({name, ce, ca}) | sort_by(.name))}')"
  expected='{"crates":[{"name":"crate_a","ce":1,"ca":0},{"name":"crate_b","ce":0,"ca":1}]}'
  [[ "${projected}" == "$(echo "${expected}" | jq -c .)" ]] || fail "Coupling output did not match golden data"
}

assert_contributor_report() {
  log "Checking contributor-report output (JSON)"
  local raw projected expected
  raw="$("${BIN}" contributor-report --path "${WORKDIR}" --decay 0 --output json)"
  projected="$(echo "${raw}" | jq -c 'map({author, commit_count, lines_added, lines_deleted, files_touched, score}) | sort_by(.author)')"
  expected='[
  {
    "author": "Alice",
    "commit_count": 2,
    "lines_added": 36,
    "lines_deleted": 1,
    "files_touched": 7,
    "score": 46.0
  },
  {
    "author": "Bob",
    "commit_count": 1,
    "lines_added": 4,
    "lines_deleted": 0,
    "files_touched": 1,
    "score": 6.0
  }
]'
  [[ "${projected}" == "$(echo "${expected}" | jq -c 'sort_by(.author)')" ]] \
    || fail "Contributor report did not match golden data"
}

assert_rca_if_available() {
  if ! command -v rust-code-analysis-cli >/dev/null 2>&1; then
    warn "rust-code-analysis-cli not found; skipping rust-code-analysis and all command checks"
    return
  fi

  log "Checking rust-code-analysis output (JSON)"
  local raw simplified expected
  raw="$("${BIN}" rust-code-analysis --path "${WORKDIR}" --output json --language rust --jobs 1)"
  simplified="$(echo "${raw}" | jq -c '[.[].name | capture("(?<p>crate_[ab]/src/.*)$").p] | sort')"
  expected='[
  "crate_a/src/main.rs",
  "crate_a/src/utils.rs",
  "crate_b/src/lib.rs"
]'
  [[ "${simplified}" == "$(echo "${expected}" | jq -c 'sort')" ]] \
    || fail "rust-code-analysis output missing expected files"

  log "Checking all output (JSON)"
  local all_json errors
  all_json="$("${BIN}" all --path "${WORKDIR}" --output json --vol-alpha 1)"
  errors="$(echo "${all_json}" | jq '.errors | length')"
  [[ "${errors}" == "0" ]] || fail "all command reported errors: ${all_json}"
}

create_workspace
advance_history

assert_statement_count
assert_volatility
assert_coupling
assert_contributor_report
assert_rca_if_available

log "All e2e checks passed."

