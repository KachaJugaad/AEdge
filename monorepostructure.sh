# TypeScript/Node packages
mkdir -p packages/contracts/src
mkdir -p packages/contracts/__tests__
mkdir -p packages/bus/src
mkdir -p packages/bus/__tests__
mkdir -p packages/web-terminal/src
mkdir -p gate-tests
mkdir -p cloud-api

# Rust core crate (Person A owns)
mkdir -p crates/anomedge-core/src
mkdir -p crates/anomedge-core/tests

# Telematics adapters
mkdir -p crates/anomedge-core/src/adapters

# Scenarios and policy
mkdir -p scenarios
mkdir -p policy
mkdir -p models

# CLAUDE.md files — these are how you give Claude Code its instructions
touch CLAUDE.md
touch crates/anomedge-core/CLAUDE.md
touch packages/contracts/CLAUDE.md
