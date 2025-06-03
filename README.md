# Rust Architecture Fitness Functions

Inspired by [Mark Richard](https://developertoarchitect.com/mark-richards.html)'s [workshop](https://2025.dddeurope.com/program/architecture-the-hard-parts/) at [DDD Europe in 2025](https://2025.dddeurope.com/) I created this tool to help me identify and determine the size of the logical components in my Rust codebases.

## TODOS

- [ ] FF: Is codebase flat? (Flatten codebase into components)
- [ ] FF: Identify code volatility per component by using git history
- [ ] FF: No source code should reside in the root namespace
- [ ] Map the imports and output a diagram of the dependencies
