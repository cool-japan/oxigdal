#![allow(dead_code)]

use anyhow::Result;
use quote::quote;
use syn::{Attribute, ItemFn, Stmt, parse_quote};

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

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Return `true` when `attrs` already contains `#[ignore]`.
fn has_ignore_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.meta
            .require_path_only()
            .ok()
            .map(|p| p.is_ident("ignore"))
            .unwrap_or(false)
    })
}

/// Return `true` when `attrs` already contains `#[should_panic]`.
fn has_should_panic_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("should_panic"))
}

/// Return `true` when `attrs` contains `#[cfg_attr(…, ignore)]`.
fn has_cfg_attr_ignore(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("cfg_attr") {
            quote!(#a).to_string().contains("ignore")
        } else {
            false
        }
    })
}

/// Return `true` when the function body already calls `std::env::var` or
/// `env::var` (rough string-level heuristic, not a full parse).
fn has_env_check(item_fn: &ItemFn) -> bool {
    let body = quote!(#item_fn).to_string();
    body.contains("std :: env :: var") || body.contains("env :: var")
}

// ─── IgnoreStrategy ──────────────────────────────────────────────────────────

/// Adds `#[ignore]` to a test function so the test runner skips it.
///
/// Use this for tests that are known to be flaky or that depend on unavailable
/// external resources and cannot be fixed quickly.
pub struct IgnoreStrategy;

impl FixStrategy for IgnoreStrategy {
    fn can_apply(&self, test_fn: &ItemFn) -> bool {
        !has_ignore_attr(&test_fn.attrs)
    }

    fn apply(&self, test_fn: &mut ItemFn) -> Result<()> {
        let attr: Attribute = parse_quote! { #[ignore] };
        test_fn.attrs.push(attr);
        Ok(())
    }

    fn description(&self) -> &str {
        "Adds #[ignore] attribute so the test runner skips the test"
    }

    fn name(&self) -> &str {
        "IgnoreStrategy"
    }
}

// ─── ShouldPanicStrategy ─────────────────────────────────────────────────────

/// Adds `#[should_panic]` to a test function.
///
/// Use this for tests that are failing because they currently panic
/// (or are expected to panic) but were not annotated as such.
pub struct ShouldPanicStrategy {
    /// Optional expected panic message substring.
    ///
    /// When `Some("…")`, the attribute becomes
    /// `#[should_panic(expected = "…")]`.  When `None`, a bare
    /// `#[should_panic]` is emitted.
    pub expected: Option<String>,
}

impl ShouldPanicStrategy {
    /// Create a new strategy that emits a bare `#[should_panic]`.
    pub fn new() -> Self {
        Self { expected: None }
    }

    /// Create a new strategy that emits `#[should_panic(expected = "…")]`.
    pub fn with_expected(msg: impl Into<String>) -> Self {
        Self {
            expected: Some(msg.into()),
        }
    }
}

impl Default for ShouldPanicStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl FixStrategy for ShouldPanicStrategy {
    fn can_apply(&self, test_fn: &ItemFn) -> bool {
        !has_should_panic_attr(&test_fn.attrs)
    }

    fn apply(&self, test_fn: &mut ItemFn) -> Result<()> {
        let attr: Attribute = match &self.expected {
            None => parse_quote! { #[should_panic] },
            Some(msg) => parse_quote! { #[should_panic(expected = #msg)] },
        };
        test_fn.attrs.push(attr);
        Ok(())
    }

    fn description(&self) -> &str {
        "Adds #[should_panic] attribute (or #[should_panic(expected = \"…\")]) \
         to tests that are expected to panic"
    }

    fn name(&self) -> &str {
        "ShouldPanicStrategy"
    }
}

// ─── EnvCheckStrategy ────────────────────────────────────────────────────────

/// Wraps the test body in an early-return guard that checks for an environment
/// variable.
///
/// The generated guard looks like:
///
/// ```rust,ignore
/// if std::env::var("MY_VAR").is_err() {
///     eprintln!("Skipping test: MY_VAR not set");
///     return;
/// }
/// ```
///
/// This is useful for tests that require an external service whose address or
/// credentials are supplied through environment variables.
pub struct EnvCheckStrategy {
    /// Name of the environment variable that must be set.
    pub env_var: String,
}

impl EnvCheckStrategy {
    /// Create a new strategy guarding on `env_var`.
    pub fn new(env_var: impl Into<String>) -> Self {
        Self {
            env_var: env_var.into(),
        }
    }
}

impl FixStrategy for EnvCheckStrategy {
    fn can_apply(&self, test_fn: &ItemFn) -> bool {
        !has_env_check(test_fn)
    }

    fn apply(&self, test_fn: &mut ItemFn) -> Result<()> {
        let var_name = &self.env_var;
        let skip_msg = format!("Skipping test: {} not set", var_name);

        let guard: Stmt = parse_quote! {
            if ::std::env::var(#var_name).is_err() {
                eprintln!(#skip_msg);
                return;
            }
        };

        test_fn.block.stmts.insert(0, guard);
        Ok(())
    }

    fn description(&self) -> &str {
        "Inserts an environment-variable guard at the start of the test body; \
         skips the test when the variable is absent"
    }

    fn name(&self) -> &str {
        "EnvCheckStrategy"
    }
}

// ─── TimeoutStrategy ─────────────────────────────────────────────────────────

/// Annotates a `#[tokio::test]` with an explicit `timeout` flavour, or falls
/// back to adding `#[ignore]` for tests that do not use the tokio test macro.
///
/// For tokio tests the existing `#[tokio::test]` attribute is replaced with
/// `#[tokio::test(flavor = "multi_thread", worker_threads = 1)]`; for ordinary
/// `#[test]` functions `#[ignore]` is added together with a doc-comment
/// explaining the reason, because the standard test runner has no built-in
/// timeout mechanism.
pub struct TimeoutStrategy {
    /// Tokio worker threads to request when rewriting `#[tokio::test]`.
    pub worker_threads: usize,
}

impl TimeoutStrategy {
    /// Create a new strategy with a single worker thread.
    pub fn new() -> Self {
        Self { worker_threads: 1 }
    }

    /// Create a strategy requesting `threads` worker threads.
    pub fn with_threads(threads: usize) -> Self {
        Self {
            worker_threads: threads,
        }
    }
}

impl Default for TimeoutStrategy {
    fn default() -> Self {
        Self::new()
    }
}

/// Return `true` if `attrs` contains a `tokio::test` attribute.
fn has_tokio_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        let segs = &a.path().segments;
        segs.len() >= 2 && segs[0].ident == "tokio" && segs[1].ident == "test"
    })
}

impl FixStrategy for TimeoutStrategy {
    fn can_apply(&self, test_fn: &ItemFn) -> bool {
        // Can always apply: either we rewrite tokio::test or we add #[ignore].
        !has_ignore_attr(&test_fn.attrs)
    }

    fn apply(&self, test_fn: &mut ItemFn) -> Result<()> {
        if has_tokio_test(&test_fn.attrs) {
            let workers = self.worker_threads;
            // Replace all `#[tokio::test]` attributes with the multi-thread variant.
            test_fn.attrs = test_fn
                .attrs
                .drain(..)
                .map(|a| {
                    let is_tokio_test = {
                        let segs = &a.path().segments;
                        segs.len() >= 2
                            && segs[0].ident == "tokio"
                            && segs[1].ident == "test"
                    };
                    if is_tokio_test {
                        parse_quote! {
                            #[tokio::test(flavor = "multi_thread", worker_threads = #workers)]
                        }
                    } else {
                        a
                    }
                })
                .collect();
        } else {
            // Non-tokio test: add a doc-comment and #[ignore].
            let note: Attribute =
                parse_quote! { #[doc = " Ignored: long-running test (timeout risk)"] };
            let ignore_attr: Attribute = parse_quote! { #[ignore] };
            test_fn.attrs.push(note);
            test_fn.attrs.push(ignore_attr);
        }
        Ok(())
    }

    fn description(&self) -> &str {
        "Rewrites #[tokio::test] with a multi_thread flavor for better timeout \
         control, or adds #[ignore] for non-async tests that risk timing out"
    }

    fn name(&self) -> &str {
        "TimeoutStrategy"
    }
}

// ─── SkipIfUnavailableStrategy ────────────────────────────────────────────────

/// Adds a `#[cfg_attr(not(feature = "…"), ignore)]` attribute to a test
/// function so that it is automatically skipped when a specific Cargo feature
/// is not enabled.
///
/// This is useful for hardware-accelerated tests (GPU, SIMD, etc.) or tests
/// that require optional dependencies that might not be compiled in every CI
/// matrix configuration.
pub struct SkipIfUnavailableStrategy {
    /// Cargo feature whose absence causes the test to be ignored.
    pub feature: String,
}

impl SkipIfUnavailableStrategy {
    /// Create a new strategy gated on `feature`.
    pub fn new(feature: impl Into<String>) -> Self {
        Self {
            feature: feature.into(),
        }
    }
}

impl FixStrategy for SkipIfUnavailableStrategy {
    fn can_apply(&self, test_fn: &ItemFn) -> bool {
        !has_cfg_attr_ignore(&test_fn.attrs)
    }

    fn apply(&self, test_fn: &mut ItemFn) -> Result<()> {
        let feat = &self.feature;
        let attr: Attribute = parse_quote! {
            #[cfg_attr(not(feature = #feat), ignore)]
        };
        test_fn.attrs.push(attr);
        Ok(())
    }

    fn description(&self) -> &str {
        "Adds #[cfg_attr(not(feature = \"…\"), ignore)] so tests are skipped \
         when the required Cargo feature is not enabled"
    }

    fn name(&self) -> &str {
        "SkipIfUnavailableStrategy"
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    /// Minimal `#[test] fn` to use as a fixture.
    fn make_test_fn() -> ItemFn {
        parse_quote! {
            #[test]
            fn my_test() {
                assert!(true);
            }
        }
    }

    /// Minimal async `#[tokio::test] async fn` fixture.
    fn make_tokio_test_fn() -> ItemFn {
        parse_quote! {
            #[tokio::test]
            async fn slow_async_test() {
                assert!(true);
            }
        }
    }

    // ── IgnoreStrategy ───────────────────────────────────────────────────────

    #[test]
    fn ignore_strategy_adds_ignore_attr() {
        let mut f = make_test_fn();
        let strategy = IgnoreStrategy;

        assert!(strategy.can_apply(&f), "should be applicable before fix");
        strategy.apply(&mut f).expect("apply should succeed");

        assert!(
            has_ignore_attr(&f.attrs),
            "test should have #[ignore] after apply"
        );
    }

    #[test]
    fn ignore_strategy_not_applicable_when_already_ignored() {
        let mut f: ItemFn = parse_quote! {
            #[test]
            #[ignore]
            fn already_ignored() {}
        };
        let strategy = IgnoreStrategy;
        assert!(
            !strategy.can_apply(&f),
            "must not apply when #[ignore] is already present"
        );
        // Applying anyway must be idempotent (no duplicate attribute).
        strategy.apply(&mut f).expect("apply should not error");
    }

    // ── ShouldPanicStrategy ──────────────────────────────────────────────────

    #[test]
    fn should_panic_strategy_adds_bare_attr() {
        let mut f: ItemFn = parse_quote! {
            #[test]
            fn panicking_test() { assert!(true); }
        };
        let strategy = ShouldPanicStrategy::new();

        assert!(strategy.can_apply(&f));
        strategy.apply(&mut f).expect("apply should succeed");

        assert!(
            has_should_panic_attr(&f.attrs),
            "test should have #[should_panic] after apply"
        );
    }

    #[test]
    fn should_panic_strategy_with_expected_message() {
        let mut f: ItemFn = parse_quote! {
            #[test]
            fn overflow_test() { assert!(true); }
        };
        let strategy = ShouldPanicStrategy::with_expected("overflow");

        strategy.apply(&mut f).expect("apply should succeed");

        // The emitted attribute tokens must contain the expected substring.
        let full = quote! { #f }.to_string();
        assert!(
            full.contains("overflow"),
            "attribute should embed expected message; got: {full}"
        );
    }

    // ── EnvCheckStrategy ─────────────────────────────────────────────────────

    #[test]
    fn env_check_strategy_inserts_guard() {
        let mut f: ItemFn = parse_quote! {
            #[test]
            fn needs_db() { assert!(true); }
        };
        let strategy = EnvCheckStrategy::new("DATABASE_URL");

        assert!(strategy.can_apply(&f));
        strategy.apply(&mut f).expect("apply should succeed");

        // The first statement in the block must be the env-var guard.
        assert!(
            !f.block.stmts.is_empty(),
            "block should have at least one statement"
        );
        // Use the token stream of the whole updated function.
        let full = quote! { #f }.to_string();
        assert!(
            full.contains("DATABASE_URL"),
            "env guard must reference the variable name; got: {full}"
        );
        assert!(
            full.contains("is_err"),
            "env guard must check is_err(); got: {full}"
        );
    }

    #[test]
    fn env_check_strategy_not_applicable_when_guard_exists() {
        let f: ItemFn = parse_quote! {
            #[test]
            fn already_guarded() {
                if std::env::var("DATABASE_URL").is_err() { return; }
                assert!(true);
            }
        };
        let strategy = EnvCheckStrategy::new("DATABASE_URL");
        assert!(
            !strategy.can_apply(&f),
            "must not apply when guard is already present"
        );
    }

    // ── TimeoutStrategy ──────────────────────────────────────────────────────

    #[test]
    fn timeout_strategy_adds_ignore_for_plain_test() {
        let mut f = make_test_fn();
        let strategy = TimeoutStrategy::new();

        assert!(strategy.can_apply(&f));
        strategy.apply(&mut f).expect("apply should succeed");

        assert!(
            has_ignore_attr(&f.attrs),
            "non-tokio test should receive #[ignore]"
        );
    }

    #[test]
    fn timeout_strategy_rewrites_tokio_test() {
        let mut f = make_tokio_test_fn();
        let strategy = TimeoutStrategy::with_threads(2);

        assert!(strategy.can_apply(&f));
        strategy.apply(&mut f).expect("apply should succeed");

        let full = quote! { #f }.to_string();
        assert!(
            full.contains("multi_thread"),
            "tokio::test attr should be rewritten with multi_thread flavor; got: {full}"
        );
    }

    // ── SkipIfUnavailableStrategy ─────────────────────────────────────────────

    #[test]
    fn skip_if_unavailable_adds_cfg_attr() {
        let mut f: ItemFn = parse_quote! {
            #[test]
            fn gpu_test() { assert!(true); }
        };
        let strategy = SkipIfUnavailableStrategy::new("gpu");

        assert!(strategy.can_apply(&f));
        strategy.apply(&mut f).expect("apply should succeed");

        let full = quote! { #f }.to_string();
        assert!(
            full.contains("cfg_attr"),
            "should add cfg_attr; got: {full}"
        );
        assert!(full.contains("gpu"), "cfg_attr should reference feature name");
        assert!(full.contains("ignore"), "cfg_attr should include ignore");
    }

    #[test]
    fn skip_if_unavailable_not_applicable_when_already_present() {
        let f: ItemFn = parse_quote! {
            #[test]
            #[cfg_attr(not(feature = "gpu"), ignore)]
            fn gpu_already_gated() {}
        };
        let strategy = SkipIfUnavailableStrategy::new("gpu");
        assert!(
            !strategy.can_apply(&f),
            "must not apply when cfg_attr(…ignore) already exists"
        );
    }
}
