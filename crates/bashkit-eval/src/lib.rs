// bashkit-eval: a mira eval study for bashkit tool usage.
// See specs/eval.md for design decisions.
//
// The transport + execution layer (providers, agent loops, dataset types) is
// reused as-is; the mira integration (samples, subjects, scorer, eval builders)
// lives in `mira_study`. The `#[eval]` registration wrappers live in `main.rs`.

pub mod agent;
pub mod checks;
pub mod dataset;
pub mod mira_study;
pub mod provider;
pub mod scripting_agent;
pub mod scripting_dataset;
pub mod snapshot;
