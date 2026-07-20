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
    like_match_recursive(&text_chars, 0, &pattern_chars, 0, case_insensitive)
}

fn like_match_recursive(
    text: &[char],
    ti: usize,
    pattern: &[char],
    pi: usize,
    case_insensitive: bool,
) -> bool {
    // Pattern exhausted: match iff the text is also fully consumed.
    let Some(&pattern_char) = pattern.get(pi) else {
        return ti >= text.len();
    };

    match pattern_char {
        '%' => {
            // Match zero or more characters.
            for i in ti..=text.len() {
                if like_match_recursive(text, i, pattern, pi + 1, case_insensitive) {
                    return true;
                }
            }
            false
        }
        '_' => {
            // Match exactly one character.
            if ti < text.len() {
                like_match_recursive(text, ti + 1, pattern, pi + 1, case_insensitive)
            } else {
                false
            }
        }
        c => {
            // Match an exact literal character.
            let matched = match text.get(ti) {
                Some(&t) if case_insensitive => t.eq_ignore_ascii_case(&c),
                Some(&t) => t == c,
                None => false,
            };
            if matched {
                like_match_recursive(text, ti + 1, pattern, pi + 1, case_insensitive)
            } else {
                false
            }
        }
    }
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
}
