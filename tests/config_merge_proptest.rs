//! Property-based tests for configuration merge operations.
//!
//! This module uses proptest to verify that the merge operation
//! satisfies important mathematical properties: idempotence, associativity,
//! and that default acts as a neutral element (when merged on the left).

use raff_core::config::{ContributorReportConfig, CouplingConfig, GeneralConfig};
use raff_core::config::{
    RaffConfig, RustCodeAnalysisConfig, StatementCountConfig, VolatilityConfig,
};
use raff_core::config_hierarchy::merge_configs;
use raff_core::config_hierarchy::Mergeable;

proptest::proptest! {
    /// Property: Merge idempotence.
    ///
    /// Merging a config with itself should yield the same config.
    /// `a.merge(a) == a`
    #[test]
    fn prop_merge_idempotence_for_statement_count(config in any_statement_count_config()) {
        let merged = config.merge(&config);
        prop_assert_eq!(merged, config);
    }

    /// Property: Merge idempotence for full RaffConfig.
    #[test]
    fn prop_merge_idempotence_for_raff_config(config in any_raff_config()) {
        let merged = merge_configs(&config, &config);
        prop_assert_eq!(merged.statement_count, config.statement_count);
        prop_assert_eq!(merged.volatility, config.volatility);
        prop_assert_eq!(merged.coupling, config.coupling);
        prop_assert_eq!(merged.rust_code_analysis, config.rust_code_analysis);
        prop_assert_eq!(merged.contributor_report, config.contributor_report);
    }

    /// Property: Merge associativity.
    ///
    /// The order of merging should not matter (when considering the same precedence order).
    /// `(a.merge(b)).merge(c) == a.merge(b.merge(c))`
    #[test]
    fn prop_merge_associativity_for_statement_count(
        a in any_statement_count_config(),
        b in any_statement_count_config(),
        c in any_statement_count_config()
    ) {
        let left = a.merge(&b).merge(&c);
        let right = a.merge(&b.merge(&c));
        prop_assert_eq!(left, right);
    }

    /// Property: Merge associativity for full RaffConfig.
    #[test]
    fn prop_merge_associativity_for_raff_config(
        a in any_raff_config(),
        b in any_raff_config(),
        c in any_raff_config()
    ) {
        let left = merge_configs(&merge_configs(&a, &b), &c);
        let right = merge_configs(&a, &merge_configs(&b, &c));
        prop_assert_eq!(left.statement_count, right.statement_count);
        prop_assert_eq!(left.volatility, right.volatility);
        prop_assert_eq!(left.coupling, right.coupling);
        prop_assert_eq!(left.rust_code_analysis, right.rust_code_analysis);
        prop_assert_eq!(left.contributor_report, right.contributor_report);
    }

    /// Property: Default is a neutral element (when merged on the left).
    ///
    /// Merging default with any config should yield that config's values.
    /// `default.merge(a) == a`
    #[test]
    fn prop_merge_default_neutral_left_statement_count(config in any_statement_count_config()) {
        let default = StatementCountConfig::default();
        let merged = default.merge(&config);
        prop_assert_eq!(merged, config);
    }

    /// Property: Default is a neutral element (when merged on the left) for full config.
    #[test]
    fn prop_merge_default_neutral_left_raff_config(config in any_raff_config()) {
        let default = RaffConfig::default();
        let merged = merge_configs(&default, &config);
        prop_assert_eq!(merged, config);
    }

    /// Property: Merging preserves non-default values.
    ///
    /// When merging, non-default values in the `other` config should win.
    #[test]
    fn prop_merge_override_wins_for_threshold(base in any_statement_count_config(), override_val in 1usize..1000) {
        let mut override_config = base.clone();
        override_config.threshold = override_val;

        let merged = base.merge(&override_config);
        prop_assert_eq!(merged.threshold, override_val);
    }
}

// ============================================================================
// Arbitrary Implementations for proptest
// ============================================================================

use proptest::prelude::*;
use std::path::PathBuf;

/// Strategy for generating arbitrary statement count configs.
fn any_statement_count_config() -> BoxedStrategy<StatementCountConfig> {
    (
        prop::option::of(prop::string::string_regex(r"[a-zA-Z0-9_/\.]+").unwrap()),
        any::<usize>(),
        prop::option::of(prop::string::string_regex(r"[a-z]+").unwrap()),
    )
        .prop_map(|(path, threshold, output)| StatementCountConfig {
            path: path.map(PathBuf::from),
            threshold,
            output,
        })
        .boxed()
}

/// Strategy for generating arbitrary volatility configs.
fn any_volatility_config() -> BoxedStrategy<VolatilityConfig> {
    (
        prop::option::of(prop::string::string_regex(r"[a-zA-Z0-9_/\.]+").unwrap()),
        any::<f64>(),
        prop::option::of(prop::string::string_regex(r"\d{4}-\d{2}-\d{2}").unwrap()),
        any::<bool>(),
        any::<bool>(),
        prop::option::of(prop::string::string_regex(r"[a-z]+").unwrap()),
    )
        .prop_map(
            |(path, alpha, since, normalize, skip_merges, output)| VolatilityConfig {
                path: path.map(PathBuf::from),
                alpha,
                since,
                normalize,
                skip_merges,
                output,
            },
        )
        .boxed()
}

/// Strategy for generating arbitrary coupling configs.
fn any_coupling_config() -> BoxedStrategy<CouplingConfig> {
    (
        prop::option::of(prop::string::string_regex(r"[a-zA-Z0-9_/\.]+").unwrap()),
        prop::option::of(prop::string::string_regex(r"[a-z]+").unwrap()),
        prop::option::of(prop::string::string_regex(r"[a-z]+").unwrap()),
    )
        .prop_map(|(path, output, granularity)| CouplingConfig {
            path: path.map(PathBuf::from),
            output,
            granularity,
        })
        .boxed()
}

/// Strategy for generating arbitrary rust code analysis configs.
fn any_rca_config() -> BoxedStrategy<RustCodeAnalysisConfig> {
    (
        prop::option::of(prop::string::string_regex(r"[a-zA-Z0-9_/\.]+").unwrap()),
        prop::collection::vec(prop::string::string_regex(r"[a-z0-9\-\_]+").unwrap(), 0..5),
        prop::option::of(any::<usize>()),
        prop::option::of(prop::string::string_regex(r"[a-z]+").unwrap()),
        any::<bool>(),
        prop::string::string_regex(r"[a-z]+").unwrap(),
    )
        .prop_map(
            |(path, extra_flags, jobs, output, metrics, language)| RustCodeAnalysisConfig {
                path: path.map(PathBuf::from),
                extra_flags,
                jobs,
                output,
                metrics,
                language,
            },
        )
        .boxed()
}

/// Strategy for generating arbitrary contributor report configs.
fn any_contributor_report_config() -> BoxedStrategy<ContributorReportConfig> {
    (
        prop::option::of(prop::string::string_regex(r"[a-zA-Z0-9_/\.]+").unwrap()),
        prop::option::of(prop::string::string_regex(r"\d{4}-\d{2}-\d{2}").unwrap()),
        any::<f64>(),
        prop::option::of(prop::string::string_regex(r"[a-z]+").unwrap()),
    )
        .prop_map(|(path, since, decay, output)| ContributorReportConfig {
            path: path.map(PathBuf::from),
            since,
            decay,
            output,
        })
        .boxed()
}

/// Strategy for generating arbitrary general configs.
fn any_general_config() -> BoxedStrategy<GeneralConfig> {
    (
        prop::option::of(prop::string::string_regex(r"[a-zA-Z0-9_/\.]+").unwrap()),
        any::<bool>(),
        prop::option::of(prop::string::string_regex(r"[a-zA-Z0-9_/\.]+").unwrap()),
    )
        .prop_map(|(path, verbose, output_file)| GeneralConfig {
            path: path.map(PathBuf::from),
            verbose,
            output_file: output_file.map(PathBuf::from),
        })
        .boxed()
}

/// Strategy for generating arbitrary full raff configs.
fn any_raff_config() -> BoxedStrategy<RaffConfig> {
    (
        any_general_config(),
        any_statement_count_config(),
        any_volatility_config(),
        any_coupling_config(),
        any_rca_config(),
        any_contributor_report_config(),
    )
        .prop_map(
            |(
                general,
                statement_count,
                volatility,
                coupling,
                rust_code_analysis,
                contributor_report,
            )| {
                RaffConfig {
                    general,
                    statement_count,
                    volatility,
                    coupling,
                    rust_code_analysis,
                    contributor_report,
                }
            },
        )
        .boxed()
}
