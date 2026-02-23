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
    pub fn apply(&self, input: &str) -> String {
        match self {
            MaskingStrategy::Full(mask_char) => mask_char.to_string().repeat(input.len()),
            MaskingStrategy::Partial {
                keep_first,
                keep_last,
                mask_char,
            } => {
                if input.len() <= keep_first + keep_last {
                    return input.to_string();
                }
                let first = &input[..*keep_first];
                let last = &input[input.len() - keep_last..];
                let mask_len = input.len() - keep_first - keep_last;
                format!(
                    "{}{}{}",
                    first,
                    mask_char.to_string().repeat(mask_len),
                    last
                )
            }
            MaskingStrategy::Email => {
                if let Some(at_pos) = input.find('@') {
                    let username = &input[..at_pos];
                    let domain = &input[at_pos..];
                    if username.len() <= 2 {
                        format!("**{}", domain)
                    } else {
                        format!("{}***{}", &username[..1], domain)
                    }
                } else {
                    MaskingStrategy::Full('*').apply(input)
                }
            }
            MaskingStrategy::CreditCard => {
                if input.len() >= 4 {
                    format!("****-****-****-{}", &input[input.len() - 4..])
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
}
