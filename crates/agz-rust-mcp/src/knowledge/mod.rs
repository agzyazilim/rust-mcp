//! Static advisory knowledge used by audit, lookup tools, and MCP resources.

pub mod pitfalls;
pub mod std_modules;
pub mod workflow;

pub use pitfalls::{AUDIT_PATTERNS, PatternId, PitfallDefinition, Severity, pattern_by_id};
pub use std_modules::{STD_MODULES, STD_POLICY, StdModuleEntry, std_module_lookup};
