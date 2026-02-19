# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.3] - 2025-06-18

### Bug Fixes

- Ignoring files based on the language for the rca rule ([`454b68a`](454b68a04425f5186204e571266f2b44e30029e2))

### CI

- Rename workflow for releasing to crates.io ([`4ee81bd`](4ee81bd3bb3c6c6b2df79268af563a4dd2ccdfbc))
- Add manual trigger for cargo-dist workflow ([`478f37d`](478f37d35cb28f0cbc86b8825da574100966388c))


# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] - 2025-06-18

### CI

- Add cargo-dist ([`dad3534`](dad3534984360ba2ffcb92bca2b3a82b3ee293bc))


# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2025-06-18

### CI

- Add support for cargo-binstall ([`f88118a`](f88118a29d6bcc65259a9f68326cb779ab8af343))

### Documentation

- Remove dep graph todo as its done ([`ad1a33d`](ad1a33d961b137996e044fd214fe0ee4efb7dd0a))

### Miscellaneous Tasks

- Release v0.1.0 ([`1520e3e`](1520e3e83f6cf095b987a94775b2905059a8755d))


## [Unreleased]

## [0.1.4] - 2026-02-19

### Bug Fixes

- Fix error.rs test compilation issues ([`5203e92`](5203e92a612db391e8052629d9575f1f7e9b51b2))
- Add cache version to invalidate stale volatility data ([`758bc6e`](758bc6eabd78f033299cfa55f19ad9b15f3a3aac))
- Add cache entry versioning for graceful deserialization handling ([`aa54485`](aa54485fccfe9d9e6d001b06fc5684149eddc6aa))
- Collapse remaining nested if in config_hierarchy.rs ([`6130153`](613015382ed4d9f28f9d9096b1b29a1406477cbd))

### Documentation

- Add crate and module-level documentation ([`b0ae26f`](b0ae26f4f0dc9ab1c45edb65e4dbf3fb947b764e))
- Add module-level documentation to core rule modules ([`7d3f571`](7d3f57170a5d127ac46501226e26c53b14489c4d))
- Fix doctest compilation errors ([`817b51f`](817b51fb77eaf7a25d98626a2dd9de788bb7edb2))
- Add raff pre-commit hook integration documentation ([`259b73d`](259b73d44077f9c1fe9f2e802f393579e8e5a09e))

### Features

- Add contributor report ([`f4fdb10`](f4fdb103a78f6fea63548e0da52b2ed139f60051))
- Add examples ([`57710e4`](57710e4e65decab643d30035dc11e704ad18a7c1))
- Add end-to-end testing script for project validation ([`63b9efc`](63b9efc00a1bb265429c20304f97c61145fae057))
- Add configuration file support ([`ed69add`](ed69add9a25b21dc45a726b10a0815728e47af7d))
- Add cache module for result caching ([`7563a3a`](7563a3a7b1e96e086991863b4d1c27d3d73278b8))
- Add CLI options for cache control ([`edacc05`](edacc05114363dc9963e73945e2ca51583822caa))
- Integrate cache into volatility and statement count rules ([`29c1966`](29c1966b90e7b975891f21330d7f5a7c18fa856d))
- Add Rule trait for custom analysis rules ([`cd1b759`](cd1b759a98f32abd30a03f11412cc6ae3c76c7e3))
- Implement Rule trait for StatementCountRule ([`63c877a`](63c877a27355e66e6c7465b4d48d3ba3a31aacba))
- Implement Rule trait for VolatilityRule ([`6b3ab73`](6b3ab7365439a9b03e543089ed7ca79cf5e53240))
- Implement Rule trait for remaining rules ([`aeada91`](aeada91a660318739ec849300bed73b1fe4d58df))
- Add example custom rule demonstrating Rule trait usage ([`8ce2102`](8ce21020719c833eae86aaa8a2c3842e4e8b3385))
- Implement hierarchical configuration loading ([`609d95e`](609d95e6cb99a068c08d1f831f73b48613fe6852))
- Integrate hierarchical configuration loading into main ([`ca7debb`](ca7debbe4946e12d0dc745cfc05e5d253ec737b4))
- Add CI/CD report generation infrastructure (Phase 1) ([`d5d80a9`](d5d80a94cef2f9a97d1a52451a3aa47f0cc7da91))
- Add CiOutputFormat CLI flag for CI/CD report generation (Phase 2) ([`275e491`](275e491d9c7e6fccf5a72c219d8be846e765e342))
- Add global --output-file argument for CI/CD report output ([`accb7c6`](accb7c61d2846edd720d17c296f0c9403d073702))
- Implement ToFindings for StatementCountData (Phase 3) ([`bcfaad0`](bcfaad09dbc5637bc2a0af907c57f91b830d595b))
- Implement ToFindings for VolatilityData (Phase 3 continued) ([`012dfd5`](012dfd50718f30455a6ba8acd9bd61a264d0fb5f))
- Implement ToFindings for CouplingData (Phase 3 continued) ([`f621525`](f621525232d8ab7494799d002ceb1b35cd7785e6))
- Implement ToFindings for RustCodeAnalysisData (Phase 3 continued) ([`0190722`](0190722d4afc7550dc3e44fb5f909dcdb5e9f894))
- Implement ToFindings for ContributorReportData (Phase 3 complete) ([`8631cff`](8631cffeba0d9924ca6a3a2545d56e902b15ee45))
- Implement unified CI output for all rules (Phase 4) ([`42e5541`](42e5541a75d0723f70ccd43f41386eeaf17f4cc9))
- Add golden snapshot tests for CI report output (Phase 7) ([`581b4b5`](581b4b53fd6b598e3dd613503d743e79f3b81a64))
- Implement actionable CLI table output format for 'raff all' ([`af2cf0c`](af2cf0cf7d1b2f702423b67c8156a360f2162092))
- Add install recipe to justfile ([`7d3ee18`](7d3ee18ebff081992f19ca44d6ad4f0009bbf83e))
- Add git_utils module for staged file discovery (Phase 1) ([`9282990`](928299095ee8ac0ff3f6e5f81bc5d53f68f1df35))
- Add --fast flag for fast-only rule execution (Phase 2) ([`02329ed`](02329ed688e55514f1fce97a6881c87c9e4750b0))
- Add --quiet flag for minimal output mode (Phase 3) ([`5425bc8`](5425bc87d4608129a8bdc57300821fc7f4db28ee))
- Add global --staged flag for git-staged file analysis (Phase 1) ([`547364e`](547364eaf286954b1f8cbebd3e9c3700a14d984e))
- Add ProfileConfig and PreCommitProfile structures (Phase 4) ([`25113f3`](25113f33c7a1ba0ebf008f4e925f11e612bcf806))
- Implement apply_pre_commit_profile and --profile flag (Phase 4) ([`ae0ed2c`](ae0ed2c6a01478d94b8c7544d57ec26a6135715f))
- Auto-discover .raff/raff.toml at git root and add repo config ([`c7fd1f3`](c7fd1f3f276612fc3052eba5a341fda8305d0620))
- Add automatic exclusion of target directory and gitignore support ([`bd4eea4`](bd4eea476bb9889c4a61b2b5d15aa7884f6353ec))

### Miscellaneous Tasks

- Improve linting ([`2660c80`](2660c802a99d003dae49ee070d1fdea744dfc55f))
- Migrate from Husky to prek for pre-commit hooks ([`def9a7e`](def9a7e51ad7eff87d1e45e6782003d3a1fa1e29))
- Update Rust edition to 2024 ([`93817af`](93817af0c364fbb1d54a63e934ac709d145fe50a))

### Refactoring

- Improve error handling foundation ([`12066e6`](12066e64f65ffd4cf9249bfc4b575710d5d44424))
- Migrate config.rs to RaffError ([`0486ca5`](0486ca502839ba7b872ba5ac3eb30e3abeba3a85))
- Migrate remaining rule files to RaffError ([`5ab6016`](5ab6016894ada9c1fcd42628dcecba561db69cdf))

### Testing

- Add comprehensive unit tests for counter.rs StmtCounter ([`0c6cd60`](0c6cd606ef3c5fb32039d2c0d161de2517be0627))
- Add comprehensive unit tests for file_utils.rs ([`4c5c389`](4c5c38919f6ca5fad5f00563495972e713929abe))
- Add comprehensive unit tests for statement_count_rule.rs ([`1a6e1d2`](1a6e1d2fcb025d2ece1bbd110ed001b8c335c7cd))
- Add comprehensive unit tests for all_rules.rs ([`b893a47`](b893a472176300fe5d25183d540e7ed214982d12))
- Add comprehensive unit tests for contributor_report.rs ([`4af107e`](4af107e5beaeb79355635cfa8b1663699010e147))
- Add comprehensive unit tests for coupling_rule.rs ([`c69e991`](c69e9916595d9bdff62ad5c6cd5894bd343a8121))
- Add comprehensive unit tests for volatility_rule.rs ([`04fab7a`](04fab7af5e1dc7c90caa2d3fc631c84416e050ed))
- Add property-based tests for config merge operations ([`7c68036`](7c68036f0d6f1dcfd640511c2060996839f1459b))
- Add snapshot tests for hierarchical configuration loading ([`191883a`](191883a11264bea89801d0f224f58b3fa642651b))
- Fix snapshot test isolation with serial test execution ([`bf5a6a2`](bf5a6a2d1e333b8a634888fc21d1856614dc71f0))
- Add integration tests for hierarchical configuration loading ([`43af40a`](43af40a149ad16c1963eaecc4c0915a6702e05d6))



## [0.1.0](https://github.com/liamwh/raff/releases/tag/v0.1.0) - 2025-06-13

### Added

- support html output format
- rust-code-analysis support
- Add coupling check
- Add code volatility support
- initial commit

### Other

- Add CD pipeline
- improve html styling
- use maud and csv crates
- improve terminal output, modularise codebase
