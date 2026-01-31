// Module declarations
pub mod all_rules;
pub mod cli;
pub mod config;
pub mod contributor_report;
pub mod counter;
pub mod coupling_rule;
pub mod file_utils;
pub mod html_utils;
pub mod reporting;
pub mod rust_code_analysis_rule;
pub mod statement_count_rule;
pub mod table_utils;
pub mod volatility_rule;

// Public API exports
pub use crate::all_rules::run_all;
pub use crate::cli::{
    AllArgs, AllOutputFormat, Cli, Commands, ContributorReportArgs, ContributorReportOutputFormat,
    CouplingArgs, CouplingGranularity, CouplingOutputFormat, RustCodeAnalysisArgs,
    RustCodeAnalysisOutputFormat, StatementCountArgs, StatementCountOutputFormat, VolatilityArgs,
    VolatilityOutputFormat,
};
pub use crate::contributor_report::ContributorReportRule;
pub use crate::coupling_rule::CouplingRule;
pub use crate::rust_code_analysis_rule::RustCodeAnalysisRule;
pub use crate::statement_count_rule::StatementCountRule;
pub use crate::volatility_rule::VolatilityRule;

// Config exports
pub use crate::config::{
    load_config, load_config_from_path, ContributorReportConfig, CouplingConfig, GeneralConfig,
    RaffConfig, RustCodeAnalysisConfig, StatementCountConfig, VolatilityConfig,
};
