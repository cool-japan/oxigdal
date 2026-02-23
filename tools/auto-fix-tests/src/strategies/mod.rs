use anyhow::Result;
use syn::ItemFn;

/// Strategy for automatically fixing a test
pub trait FixStrategy {
    /// Check if this strategy can be applied to the given test function
    fn can_apply(&self, test_fn: &ItemFn) -> bool;

    /// Apply the fix to the test function (modifies the AST in place)
    fn apply(&self, test_fn: &mut ItemFn) -> Result<()>;

    /// Get a human-readable description of what this strategy does
    fn description(&self) -> &str;

    /// Get the name of this strategy
    fn name(&self) -> &str;
}

// TODO: Implement concrete strategies:
// - IgnoreStrategy: Adds #[ignore] attribute
// - ShouldPanicStrategy: Adds #[should_panic] attribute
// - EnvCheckStrategy: Wraps test in environment variable check
// - TimeoutStrategy: Adds timeout configuration
// - SkipIfUnavailableStrategy: Adds resource availability check
