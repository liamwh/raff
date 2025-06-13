# raff - Rust Architecture Fitness Functions 🦀

Inspired by [Mark Richards](https://developertoarchitect.com/mark-richards.html)\'s [workshop](https://2025.dddeurope.com/program/architecture-the-hard-parts/) on software architecture, this tool aims to provide practical ways to make architectural goals measurable and continuously verified ✅.

## Features 🌟

* **Statement Count Analysis:** 📝 Determine the number of statements in your Rust files or directories. Useful for gauging code volume and complexity of components.
* **Code Volatility Analysis:** 🔄 Identifies parts of your codebase that change most frequently, leveraging Git history. Helps pinpoint unstable areas or potential refactoring candidates.
* **Module Coupling Analysis:** 🔗 Measures dependencies between different Rust modules or components, helping you manage and reduce unwanted coupling.
* **General Rust Code Analysis:** 🔬 A flexible command for various static analyses on Rust source code.
* **Command-Line Interface:** 💻 Easy-to-use CLI for running analyses and configuring options.
* **Multiple Output Formats:** 📊 HTML, JSON, CSV and DOT (GraphViz).

## Getting Started 🚀

### Prerequisites

* Rust toolchain (latest stable version recommended). Install from [rustup.rs](https://rustup.rs/).
* Git (for volatility analysis).

### Installation

You can install `raff` directly using `cargo`:

```bash
cargo install raff-cli
```

*(Assuming your crate is published on crates.io as `raff-cli`. If installing from a Git repository, the command would be `cargo install --git https://github.com/liamwh/raff.git` or similar if it's not yet on crates.io)*

Once installed, the `raff` binary will be available in your Cargo binary path.

### Running

To see the list of available commands and their options:

```bash
raff --help
```

## Usage 📖

The general command structure is:

```bash
raff <COMMAND> [OPTIONS]
```

### Available Commands

* **`StatementCount`**: Analyzes statement counts.
  * Example: `raff statement-count --path ./src --output-format table`
  * *(You might need to add specific options based on how `StatementCountArgs` is defined. Common options could include `--path <directory/file>`, `--exclude <patterns>`, etc.)*

* **`Volatility`**: Analyzes code churn from Git history.
  * Example: `raff volatility --path . --output-format json`
  * *(Typically requires the target to be a Git repository. Options might include date ranges, file patterns.)*

* **`Coupling`**: Analyzes dependencies between modules.
  * Example: `raff coupling --path ./src`
  * *(Might require specifying module boundaries or analysis depth.)*

* **`RustCodeAnalysis`**: Performs general Rust code analysis.
  * Example: `raff rust-code-analysis --path ./src --rule <specific_rule_name>`
  * *(The exact options will depend on the implemented analysis rules.)*

For detailed options for each command, run:

```bash
raff <COMMAND> --help
```

## TODOs / Future Work 🗺️

The following enhancements are planned or could be valuable additions:

* [ ] 🔶 FF: Do any domain objects use primitive types? (e.g. `String` instead of `Name`).
* [ ] 🏛️ FF: Is codebase flat? (Analyze and visualize component hierarchy).
* [ ] 🚫 FF: No source code should reside in the root namespace (or other configurable namespace rules).
* [ ] 🕸️ Map imports and output a dependency graph (e.g., using DOT format or an interactive visualization).
* [ ] ⚖️ Configurable thresholds for fitness functions to produce pass/fail results.
* [ ] 🚀 Integration with CI/CD pipelines / github actions.
