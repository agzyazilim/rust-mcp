#![forbid(unsafe_code)]

//! Client-independent protocol and release verification shared by the CLI and tests.

pub mod benchmark;
pub mod benchmark_tasks;
mod child_process;
pub mod evidence;
pub mod mcp;
