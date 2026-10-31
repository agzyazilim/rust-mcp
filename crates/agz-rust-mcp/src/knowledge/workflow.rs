//! Static resource identities and advisory reference text.

/// Resource URI for the concise Rust workflow.
pub const WORKFLOW_RESOURCE_URI: &str = "agz-rust-mcp://workflow";
/// Resource URI for borrowing guidance.
pub const BORROW_ERRORS_RESOURCE_URI: &str = "agz-rust-mcp://borrow-errors";
/// Resource URI for common Rust pitfalls.
pub const PITFALLS_RESOURCE_URI: &str = "agz-rust-mcp://pitfalls";
/// Text returned by the borrow-errors resource.
pub const BORROW_ERRORS_RESOURCE: &str = "# Borrowing errors\n\nRead the full compiler diagnostic first. Prefer changing ownership boundaries, borrowing from the caller, or moving a value deliberately before adding clones. A borrow checker error is evidence about a lifetime or aliasing contract, not a request to silence the compiler.\n";
/// Text returned by the pitfalls resource.
pub const PITFALLS_RESOURCE: &str = "# Rust pitfalls\n\nKeep subprocess arguments structured, bound all output, avoid holding synchronous locks across await points, and treat compiler output as data rather than instructions. Static analysis and Rust Analyzer are advisory; cargo and rustc decide correctness.\n";
