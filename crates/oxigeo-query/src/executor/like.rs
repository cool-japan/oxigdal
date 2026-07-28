//! Shared SQL `LIKE` / `ILIKE` pattern matcher.
//!
//! This is the single implementation of the SQL wildcard matcher used by both
//! the WHERE-clause evaluator ([`crate::executor::filter`]) and the JOIN `ON`
//! evaluator ([`crate::executor::join`]). Keeping it in one place avoids the
//! two paths drifting apart (they historically did: one supported `LIKE`, the
//! other did not, and case-sensitivity differed).
//!
//! Supported wildcards:
//! - `%` matches zero or more characters.
//! - `_` matches exactly one character.
//!
//! When `case_insensitive` is `true` (SQL `ILIKE` semantics) literal characters
//! are compared using ASCII case-insensitive equality; otherwise the match is
//! case-sensitive (standard SQL `LIKE`).

/// Match `text` against a SQL `LIKE`/`ILIKE` `pattern`.
///
/// `%` matches any sequence (including empty), `_` matches a single character,
/// and every other character is matched literally. When `case_insensitive` is
/// `true`, literal characters are compared case-insensitively (ASCII).
pub(crate) fn like_match(text: &str, pattern: &str, case_insensitive: bool) -> bool {
    let text_chars: Vec<char> = text.chars().collect();
    let pattern_chars: Vec<char> = pattern.chars().collect();
    like_match_iterative(&text_chars, &pattern_chars, case_insensitive)
}

/// Compare a single literal pattern character against a text character.
#[inline]
fn char_matches(pattern_char: char, text_char: char, case_insensitive: bool) -> bool {
    if case_insensitive {
        text_char.eq_ignore_ascii_case(&pattern_char)
    } else {
        text_char == pattern_char
    }
}

/// Iterative wildcard matcher with `O(n * m)` worst-case behaviour.
///
/// This is the classic two-pointer / backtrack-on-star algorithm used by
/// production SQL and shell glob engines. Unlike a naive recursion that forks
/// on every position after a `%`, it remembers only the **most recent** `%`
/// position (`star_pi`) and the text position at which that star last resumed
/// (`star_ti`). On a mismatch it backtracks to just after that star and advances
/// the resume point by one character. Because each `(ti, pi)` pair is visited at
/// most once, adversarial patterns such as `"a%a%a%…%b"` can no longer trigger
/// the exponential blow-up of the previous recursive implementation.
fn like_match_iterative(text: &[char], pattern: &[char], case_insensitive: bool) -> bool {
    let n = text.len();
    let m = pattern.len();

    let mut ti = 0usize;
    let mut pi = 0usize;
    // Position of the last `%` seen in the pattern, and the text index it
    // should resume matching from on backtrack. `None` means no `%` yet.
    let mut star_pi: Option<usize> = None;
    let mut star_ti = 0usize;

    while ti < n {
        if pi < m && (pattern[pi] == '_' || char_matches(pattern[pi], text[ti], case_insensitive)) {
            // `_` matches any single char; a literal matches its counterpart.
            ti += 1;
            pi += 1;
        } else if pi < m && pattern[pi] == '%' {
            // Record this star as the backtrack point and skip it (matching the
            // empty string so far).
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            // Mismatch: let the most recent `%` absorb one more text char.
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            // Mismatch with no star to backtrack to.
            return false;
        }
    }

    // Text consumed: any trailing pattern must be all `%` to match.
    while pi < m && pattern[pi] == '%' {
        pi += 1;
    }
    pi == m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_sensitive_like() {
        assert!(like_match("hello", "hello", false));
        assert!(like_match("hello", "h%", false));
        assert!(like_match("hello", "%o", false));
        assert!(like_match("hello", "%ll%", false));
        assert!(like_match("hello", "h_llo", false));
        assert!(like_match("hello", "_____", false));
        assert!(!like_match("hello", "____", false));
        assert!(!like_match("hello", "world", false));
        assert!(like_match("hello", "%", false));
        assert!(like_match("", "%", false));
        assert!(like_match("", "", false));
        // Case-sensitive: uppercase pattern must NOT match lowercase text.
        assert!(!like_match("hello", "HELLO", false));
        assert!(!like_match("Hello", "hello", false));
        assert!(!like_match("hello", "H%", false));
    }

    #[test]
    fn case_insensitive_ilike() {
        assert!(like_match("hello", "HELLO", true));
        assert!(like_match("Hello", "hello", true));
        assert!(like_match("HELLO", "h%", true));
        assert!(like_match("HeLLo", "%LL%", true));
        assert!(!like_match("hello", "world", true));
    }

    #[test]
    fn consecutive_and_leading_trailing_stars() {
        // Collapsing multiple stars and edge stars must behave identically to a
        // single star.
        assert!(like_match("abc", "%%%", false));
        assert!(like_match("abc", "a%%c", false));
        assert!(like_match("abc", "%b%", false));
        assert!(!like_match("abc", "%d%", false));
        assert!(like_match("abc", "%a%b%c%", false));
    }

    #[test]
    fn pathological_pattern_terminates_quickly() {
        // Adversarial input for a naive backtracking matcher: a long run of the
        // same char with many `%a` groups and a final `b` that never matches.
        // The previous recursive matcher was exponential here; the iterative
        // matcher is O(n*m) and must return promptly.
        let text: String = "a".repeat(64);
        let pattern = "a%a%a%a%a%a%a%a%a%a%a%a%a%a%a%a%b";

        let start = std::time::Instant::now();
        assert!(!like_match(&text, pattern, false));
        // Generous bound; the exponential version would take effectively forever.
        assert!(
            start.elapsed().as_secs() < 2,
            "LIKE matcher took too long — possible exponential blow-up"
        );

        // A matching variant (ends in `a`) must still succeed just as fast.
        let pattern_ok = "a%a%a%a%a%a%a%a%a%a%a%a%a%a%a%a%a";
        assert!(like_match(&text, pattern_ok, false));
    }
}
