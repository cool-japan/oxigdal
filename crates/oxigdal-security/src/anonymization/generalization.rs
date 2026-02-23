//! K-anonymity and L-diversity.

use std::collections::{HashMap, HashSet};

/// K-anonymity checker.
pub struct KAnonymity {
    k: usize,
}

impl KAnonymity {
    /// Create new K-anonymity checker.
    pub fn new(k: usize) -> Self {
        Self { k }
    }

    /// Check if dataset satisfies k-anonymity.
    pub fn check(&self, records: &[Vec<String>], quasi_identifiers: &[usize]) -> bool {
        let mut groups: HashMap<Vec<String>, usize> = HashMap::new();

        for record in records {
            let qi: Vec<String> = quasi_identifiers
                .iter()
                .filter_map(|&i| record.get(i).cloned())
                .collect();
            *groups.entry(qi).or_insert(0) += 1;
        }

        groups.values().all(|&count| count >= self.k)
    }

    /// Get minimum group size.
    pub fn min_group_size(&self, records: &[Vec<String>], quasi_identifiers: &[usize]) -> usize {
        let mut groups: HashMap<Vec<String>, usize> = HashMap::new();

        for record in records {
            let qi: Vec<String> = quasi_identifiers
                .iter()
                .filter_map(|&i| record.get(i).cloned())
                .collect();
            *groups.entry(qi).or_insert(0) += 1;
        }

        groups.values().copied().min().unwrap_or(0)
    }
}

/// L-diversity checker.
pub struct LDiversity {
    l: usize,
}

impl LDiversity {
    /// Create new L-diversity checker.
    pub fn new(l: usize) -> Self {
        Self { l }
    }

    /// Check if dataset satisfies l-diversity.
    pub fn check(
        &self,
        records: &[Vec<String>],
        quasi_identifiers: &[usize],
        sensitive_attr: usize,
    ) -> bool {
        let mut groups: HashMap<Vec<String>, HashSet<String>> = HashMap::new();

        for record in records {
            let qi: Vec<String> = quasi_identifiers
                .iter()
                .filter_map(|&i| record.get(i).cloned())
                .collect();

            if let Some(sensitive) = record.get(sensitive_attr) {
                groups.entry(qi).or_default().insert(sensitive.clone());
            }
        }

        groups.values().all(|values| values.len() >= self.l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_k_anonymity() {
        let checker = KAnonymity::new(2);

        let records = vec![
            vec![
                "Alice".to_string(),
                "30".to_string(),
                "Engineer".to_string(),
            ],
            vec!["Bob".to_string(), "30".to_string(), "Doctor".to_string()],
            vec![
                "Charlie".to_string(),
                "40".to_string(),
                "Teacher".to_string(),
            ],
            vec!["David".to_string(), "40".to_string(), "Lawyer".to_string()],
        ];

        // Age is quasi-identifier (index 1)
        assert!(checker.check(&records, &[1]));
    }
}
