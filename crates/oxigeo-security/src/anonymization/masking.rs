//! Data masking strategies.

/// Masking strategy.
pub enum MaskingStrategy {
    /// Full masking.
    Full(char),
    /// Partial masking (keep first/last N chars).
    Partial {
        /// Number of characters to keep at the beginning.
        keep_first: usize,
        /// Number of characters to keep at the end.
        keep_last: usize,
        /// Character to use for masking.
        mask_char: char,
    },
    /// Email masking.
    Email,
    /// Credit card masking.
    CreditCard,
}

impl MaskingStrategy {
    /// Apply masking to a string.
    ///
    /// All slicing is performed on Unicode scalar values (`char`s), not raw
    /// UTF-8 byte offsets, so multi-byte PII (internationalized names,
    /// addresses, email local-parts, etc.) is masked safely without risking
    /// a byte-index/char-boundary panic.
    pub fn apply(&self, input: &str) -> String {
        match self {
            MaskingStrategy::Full(mask_char) => mask_char.to_string().repeat(input.chars().count()),
            MaskingStrategy::Partial {
                keep_first,
                keep_last,
                mask_char,
            } => {
                let char_count = input.chars().count();
                if char_count <= keep_first + keep_last {
                    return input.to_string();
                }
                let chars: Vec<char> = input.chars().collect();
                let first: String = chars[..*keep_first].iter().collect();
                let last: String = chars[char_count - keep_last..].iter().collect();
                let mask_len = char_count - keep_first - keep_last;
                format!(
                    "{}{}{}",
                    first,
                    mask_char.to_string().repeat(mask_len),
                    last
                )
            }
            MaskingStrategy::Email => {
                if let Some(at_pos) = input.find('@') {
                    // `find` returns a byte offset that is guaranteed to be
                    // a valid char boundary (it points at the ASCII '@'),
                    // so slicing the string in two at `at_pos` is safe.
                    let username = &input[..at_pos];
                    let domain = &input[at_pos..];
                    let username_char_count = username.chars().count();
                    if username_char_count <= 2 {
                        format!("**{}", domain)
                    } else {
                        let first_char = username.chars().next().unwrap_or('*');
                        format!("{}***{}", first_char, domain)
                    }
                } else {
                    MaskingStrategy::Full('*').apply(input)
                }
            }
            MaskingStrategy::CreditCard => {
                let char_count = input.chars().count();
                if char_count >= 4 {
                    let last4: String = input.chars().skip(char_count - 4).collect();
                    format!("****-****-****-{}", last4)
                } else {
                    MaskingStrategy::Full('*').apply(input)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_masking() {
        let strategy = MaskingStrategy::Full('*');
        assert_eq!(strategy.apply("secret"), "******");
    }

    #[test]
    fn test_partial_masking() {
        let strategy = MaskingStrategy::Partial {
            keep_first: 2,
            keep_last: 2,
            mask_char: '*',
        };
        assert_eq!(strategy.apply("1234567890"), "12******90");
    }

    #[test]
    fn test_email_masking() {
        let strategy = MaskingStrategy::Email;
        assert_eq!(strategy.apply("user@example.com"), "u***@example.com");
    }

    #[test]
    fn test_credit_card_masking() {
        let strategy = MaskingStrategy::CreditCard;
        assert_eq!(strategy.apply("1234567812345678"), "****-****-****-5678");
    }

    #[test]
    fn test_full_masking_multibyte_does_not_panic() {
        let strategy = MaskingStrategy::Full('*');
        // 5 Unicode scalar values, each multi-byte in UTF-8.
        assert_eq!(strategy.apply("こんにちは"), "*****");
    }

    #[test]
    fn test_partial_masking_multibyte_does_not_panic() {
        let strategy = MaskingStrategy::Partial {
            keep_first: 2,
            keep_last: 2,
            mask_char: '*',
        };
        // "héllo wörld" is 11 chars but has multi-byte 'é'/'ö', which would
        // panic on naive byte-index slicing.
        let result = strategy.apply("héllo wörld");
        assert_eq!(result, "hé*******ld");
    }

    #[test]
    fn test_partial_masking_multibyte_short_input_returned_unchanged() {
        let strategy = MaskingStrategy::Partial {
            keep_first: 3,
            keep_last: 3,
            mask_char: '*',
        };
        // Only 3 chars total, at/near a multi-byte boundary; must not panic
        // and must return the input unchanged (char_count <= keep_first+keep_last).
        assert_eq!(strategy.apply("日本語"), "日本語");
    }

    #[test]
    fn test_email_masking_multibyte_local_part_does_not_panic() {
        let strategy = MaskingStrategy::Email;
        // Multi-byte local-part (Japanese name) must not panic on `[..1]`
        // byte slicing.
        let result = strategy.apply("田中太郎@example.com");
        assert_eq!(result, "田***@example.com");
    }

    #[test]
    fn test_email_masking_short_multibyte_local_part() {
        let strategy = MaskingStrategy::Email;
        // Local part has exactly 2 multi-byte chars -> falls into the
        // `username_char_count <= 2` branch.
        let result = strategy.apply("田中@example.com");
        assert_eq!(result, "**@example.com");
    }

    #[test]
    fn test_credit_card_masking_trailing_multibyte_does_not_panic() {
        let strategy = MaskingStrategy::CreditCard;
        // Ends with multi-byte characters; byte-index slicing at
        // `len() - 4` would land mid-character and panic.
        let result = strategy.apply("123456789012日本");
        assert_eq!(result, "****-****-****-12日本");
    }
}
