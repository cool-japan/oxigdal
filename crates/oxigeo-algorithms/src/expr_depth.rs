//! Nesting-depth limits for the raster-algebra expression front-ends
//!
//! Both expression front-ends in this crate — the Pest-based [`crate::dsl`]
//! parser and the hand-written [`mod@crate::raster`] calculator parser — are
//! recursive descent. Without a bound, a deeply nested expression such as
//! `((((((… x …))))))` drives one native stack frame chain per nesting level
//! and aborts the process with a stack overflow. Because a stack overflow is a
//! `SIGABRT`/`SIGSEGV` rather than a catchable panic, it cannot be recovered
//! from, so untrusted expression text would be a denial-of-service vector.
//!
//! The mitigation is a hard, documented depth limit — [`MAX_EXPRESSION_DEPTH`]
//! — enforced *before* any recursion is entered, so over-deep input yields
//! [`AlgorithmError::NestingTooDeep`]
//! instead of aborting.
//!
//! # Choosing the limit
//!
//! The limit is sized against the most stack-hungry front-end. Measured on
//! aarch64-apple-darwin with the workspace release-ish test profile, by
//! bisecting the nesting depth at which a thread of a given stack size aborts:
//!
//! | Front-end / input shape                       | stack per nesting level |
//! |-----------------------------------------------|-------------------------|
//! | `dsl::parse_expression`, `(x + 1)` nesting    | ~12.5 KiB               |
//! | `dsl::parse_expression`, `if … else if …`     | ~12.5 KiB               |
//! | `dsl::parse_expression`, `---x` unary run     | ~0.8 KiB                |
//! | `RasterCalculator::evaluate`, `(…)` nesting   | ~2.4 KiB                |
//! | `RasterCalculator::evaluate`, `---B1` run     | ~0.4 KiB                |
//!
//! The DSL figure is dominated by Pest's generated parser: the expression
//! grammar threads ten rules (`expression` → `logical_or` → … → `primary`)
//! per nesting level, and each generated rule frame is roughly a kilobyte.
//!
//! At the worst-case 12.5 KiB per level, [`MAX_EXPRESSION_DEPTH`] = 64 costs
//! about 768 KiB of stack — the measured floor below which an expression at
//! exactly the limit stops fitting. That is 37% of the 2 MiB stack Rust gives
//! a non-main thread, the smallest stack any realistic caller (a Rayon worker,
//! a Tokio blocking task, a request handler) will have, leaving ~1.28 MiB for
//! the caller's own frames. Put the other way: the unguarded DSL parser aborts
//! at depth 165 on a 2 MiB stack, so the limit sits 2.6x below the cliff.
//!
//! For comparison, real raster algebra is shallow: NDVI `(B1 - B2) / (B1 + B2)`
//! nests 2 deep, and even elaborate multi-index expressions stay under 10. A
//! limit of 64 is therefore far above any legitimate expression while staying
//! far below the stack budget.

use crate::error::{AlgorithmError, Result};

/// Maximum nesting depth accepted by the raster-algebra expression parsers.
///
/// Expressions nesting more deeply than this are rejected with
/// [`AlgorithmError::NestingTooDeep`]
/// rather than being allowed to exhaust the thread stack. See the
/// [module documentation](self) for how the value was derived.
pub const MAX_EXPRESSION_DEPTH: usize = 64;

/// Returns an error if `depth` has passed [`MAX_EXPRESSION_DEPTH`].
///
/// Called on entry to every recursive parser function so that the recursion is
/// bounded explicitly rather than by catching an overflow after the fact.
#[inline]
pub(crate) fn guard_depth(depth: usize, context: &'static str) -> Result<()> {
    if depth > MAX_EXPRESSION_DEPTH {
        return Err(AlgorithmError::NestingTooDeep {
            context,
            depth,
            max: MAX_EXPRESSION_DEPTH,
        });
    }
    Ok(())
}

/// Conservative upper bound on the recursive-descent depth needed to parse
/// `input`.
///
/// This exists for the Pest-backed [`crate::dsl`] parser. Pest generates the
/// recursive descent itself, so no depth counter can be threaded through it;
/// the only way to keep it off the stack cliff is to reject over-deep source
/// text *before* handing it to the generated parser. The scan is a single
/// allocation-light pass over the bytes and never under-estimates the depth the
/// grammar will reach.
///
/// Three constructs in `dsl/grammar.pest` drive recursion back into
/// `expression`, and all three are accounted for here:
///
/// * bracketing — `"(" ~ expression ~ ")"`, `function_call`, and `block`;
/// * `if` / `for` prefixes — `conditional` recurses through its `else` branch,
///   so `if a then 1 else if b then 1 else 0` nests without any bracket;
/// * unary sign runs — `unary = (sub_op | add_op) ~ unary` recurses per sign,
///   so `-----x` nests without any bracket.
///
/// Comments and numeric exponents are skipped so that parentheses inside a
/// comment, or the `-` of `1e-3`, are not miscounted.
///
/// The scan short-circuits as soon as the running depth passes
/// [`MAX_EXPRESSION_DEPTH`], so the return value is exact only while it is
/// within the limit; beyond that it is the first over-limit depth observed.
#[cfg(feature = "dsl")]
pub(crate) fn source_nesting_depth(input: &str) -> usize {
    let bytes = input.as_bytes();
    let len = bytes.len();

    // One counter per open bracket group, holding the number of `if`/`for`
    // prefixes opened in that group since the last `;`. Prefixes are dropped
    // when their group closes or when a statement ends, which keeps sibling
    // statements from accumulating depth against each other.
    let mut groups: alloc_vec::Vec<usize> = alloc_vec::vec![0];
    let mut prefix_total: usize = 0;
    let mut unary_run: usize = 0;
    let mut max_depth: usize = 0;
    // True where a `+`/`-` would be a unary sign rather than a binary operator.
    let mut unary_position = true;
    let mut index = 0usize;

    while index < len {
        let byte = bytes[index];
        match byte {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,

            // Line comment: `// … \n`
            b'/' if index + 1 < len && bytes[index + 1] == b'/' => {
                index += 2;
                while index < len && bytes[index] != b'\n' {
                    index += 1;
                }
            }

            // Block comment: `/* … */`
            b'/' if index + 1 < len && bytes[index + 1] == b'*' => {
                index += 2;
                while index + 1 < len && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                    index += 1;
                }
                index = index.saturating_add(2).min(len);
            }

            b'(' | b'{' => {
                groups.push(0);
                unary_run = 0;
                unary_position = true;
                index += 1;
                max_depth = max_depth.max(groups.len() - 1 + prefix_total);
            }

            b')' | b'}' => {
                if groups.len() > 1 {
                    let closed = groups.pop().unwrap_or(0);
                    prefix_total = prefix_total.saturating_sub(closed);
                }
                unary_run = 0;
                unary_position = false;
                index += 1;
            }

            // Statement boundary: prefixes opened in this group are complete.
            b';' => {
                if let Some(top) = groups.last_mut() {
                    prefix_total = prefix_total.saturating_sub(*top);
                    *top = 0;
                }
                unary_run = 0;
                unary_position = true;
                index += 1;
            }

            b'+' | b'-' if unary_position => {
                unary_run += 1;
                index += 1;
                max_depth = max_depth.max(groups.len() - 1 + prefix_total + unary_run);
            }

            // Identifiers and keywords.
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let start = index;
                while index < len && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                // Identifier bytes are ASCII, so this slice is always on a
                // character boundary.
                let word = input.get(start..index).unwrap_or("");
                unary_run = 0;
                if word.eq_ignore_ascii_case("if") || word.eq_ignore_ascii_case("for") {
                    if let Some(top) = groups.last_mut() {
                        *top += 1;
                    }
                    prefix_total += 1;
                    unary_position = true;
                    max_depth = max_depth.max(groups.len() - 1 + prefix_total);
                } else {
                    // Keywords are followed by an operand, so a following sign
                    // is unary; a plain identifier ends an operand, so it is not.
                    unary_position = matches!(
                        word.to_ascii_lowercase().as_str(),
                        "then" | "else" | "in" | "let" | "return" | "and" | "or" | "not"
                    );
                }
            }

            // Numeric literal, including an exponent whose sign must not be
            // mistaken for a unary operator.
            b'0'..=b'9' => {
                while index < len && bytes[index].is_ascii_digit() {
                    index += 1;
                }
                if index + 1 < len && bytes[index] == b'.' && bytes[index + 1].is_ascii_digit() {
                    index += 1;
                    while index < len && bytes[index].is_ascii_digit() {
                        index += 1;
                    }
                }
                if index < len && (bytes[index] | 0x20) == b'e' {
                    let mut lookahead = index + 1;
                    if lookahead < len && (bytes[lookahead] == b'+' || bytes[lookahead] == b'-') {
                        lookahead += 1;
                    }
                    if lookahead < len && bytes[lookahead].is_ascii_digit() {
                        index = lookahead;
                        while index < len && bytes[index].is_ascii_digit() {
                            index += 1;
                        }
                    }
                }
                unary_run = 0;
                unary_position = false;
            }

            // Every other byte is an operator, a separator, or invalid input
            // that Pest will reject; all of them put us back in unary position.
            _ => {
                unary_run = 0;
                unary_position = true;
                index += 1;
            }
        }

        if max_depth > MAX_EXPRESSION_DEPTH {
            return max_depth;
        }
    }

    max_depth
}

/// Rejects `input` when [`source_nesting_depth`] exceeds [`MAX_EXPRESSION_DEPTH`].
#[cfg(feature = "dsl")]
pub(crate) fn check_source_nesting_depth(input: &str, context: &'static str) -> Result<()> {
    let depth = source_nesting_depth(input);
    if depth > MAX_EXPRESSION_DEPTH {
        return Err(AlgorithmError::NestingTooDeep {
            context,
            depth,
            max: MAX_EXPRESSION_DEPTH,
        });
    }
    Ok(())
}

/// `Vec`/`vec!` regardless of whether `std` is in play.
#[cfg(feature = "dsl")]
mod alloc_vec {
    #[cfg(feature = "std")]
    pub(super) use std::{vec, vec::Vec};

    #[cfg(not(feature = "std"))]
    pub(super) use alloc::{vec, vec::Vec};
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn guard_depth_accepts_up_to_the_limit() {
        assert!(guard_depth(0, "test").is_ok());
        assert!(guard_depth(MAX_EXPRESSION_DEPTH, "test").is_ok());
    }

    #[test]
    fn guard_depth_rejects_beyond_the_limit() {
        let err = guard_depth(MAX_EXPRESSION_DEPTH + 1, "test")
            .expect_err("depth past the limit must be rejected");
        assert!(matches!(
            err,
            AlgorithmError::NestingTooDeep {
                context: "test",
                max: MAX_EXPRESSION_DEPTH,
                ..
            }
        ));
        // The limit must be discoverable from the message.
        assert!(err.to_string().contains(&MAX_EXPRESSION_DEPTH.to_string()));
    }

    #[cfg(feature = "dsl")]
    #[test]
    fn scanner_counts_bracket_nesting() {
        assert_eq!(source_nesting_depth("x"), 0);
        assert_eq!(source_nesting_depth("(x)"), 1);
        assert_eq!(source_nesting_depth("((x))"), 2);
        assert_eq!(source_nesting_depth("(B1 - B2) / (B1 + B2)"), 1);
        // Function-call parentheses nest just like grouping parentheses.
        assert_eq!(source_nesting_depth("sqrt(abs(B1))"), 2);
        // Siblings do not accumulate.
        assert_eq!(source_nesting_depth("(a) + (b) + (c)"), 1);
    }

    #[cfg(feature = "dsl")]
    #[test]
    fn scanner_counts_if_chains_without_brackets() {
        assert_eq!(source_nesting_depth("if a then 1 else 0"), 1);
        assert_eq!(
            source_nesting_depth("if a then 1 else if b then 1 else 0"),
            2
        );
        // Separate statements reset at the `;` boundary.
        assert_eq!(
            source_nesting_depth("if a then 1 else 0; if b then 1 else 0;"),
            1
        );
    }

    #[cfg(feature = "dsl")]
    #[test]
    fn scanner_counts_unary_runs_but_not_binary_operators() {
        assert_eq!(source_nesting_depth("---x"), 3);
        assert_eq!(source_nesting_depth("2 * -3"), 1);
        // A long flat sum is depth 0: these minus signs are binary.
        let flat = (0..200)
            .map(|i| i.to_string())
            .collect::<std::vec::Vec<_>>()
            .join(" - ");
        assert_eq!(source_nesting_depth(&flat), 0);
    }

    #[cfg(feature = "dsl")]
    #[test]
    fn scanner_ignores_comments_and_exponents() {
        // Parentheses inside comments must not count.
        assert_eq!(source_nesting_depth("x // ((((((\n"), 0);
        assert_eq!(source_nesting_depth("x /* (((((( */ + 1"), 0);
        // The `-` of an exponent is not a unary operator.
        assert_eq!(source_nesting_depth("1e-3 + 2.5e+4"), 0);
    }

    #[cfg(feature = "dsl")]
    #[test]
    fn scanner_never_underestimates_deep_input() {
        let mut expr = std::string::String::from("x");
        for _ in 0..1000 {
            expr = std::format!("({expr} + 1)");
        }
        assert!(source_nesting_depth(&expr) > MAX_EXPRESSION_DEPTH);
        assert!(check_source_nesting_depth(&expr, "dsl").is_err());
    }

    #[cfg(feature = "dsl")]
    #[test]
    fn check_accepts_exactly_the_limit() {
        let mut expr = std::string::String::from("x");
        for _ in 0..MAX_EXPRESSION_DEPTH {
            expr = std::format!("({expr})");
        }
        assert_eq!(source_nesting_depth(&expr), MAX_EXPRESSION_DEPTH);
        assert!(check_source_nesting_depth(&expr, "dsl").is_ok());
    }
}
