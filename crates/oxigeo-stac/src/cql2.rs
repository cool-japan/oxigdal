//! CQL2 (Common Query Language 2) Filter support.
//!
//! This module implements CQL2-JSON filtering for STAC API searches.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// CQL2 Filter expression.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op")]
pub enum Cql2Filter {
    /// Logical AND operation.
    #[serde(rename = "and")]
    And {
        /// Arguments to AND together.
        args: Vec<Cql2Filter>,
    },

    /// Logical OR operation.
    #[serde(rename = "or")]
    Or {
        /// Arguments to OR together.
        args: Vec<Cql2Filter>,
    },

    /// Logical NOT operation.
    #[serde(rename = "not")]
    Not {
        /// Arguments to negate.
        args: Vec<Cql2Filter>,
    },

    /// Equal comparison.
    #[serde(rename = "=")]
    Equal {
        /// Arguments to compare.
        args: Vec<Cql2Operand>,
    },

    /// Not equal comparison.
    #[serde(rename = "<>")]
    NotEqual {
        /// Arguments to compare.
        args: Vec<Cql2Operand>,
    },

    /// Less than comparison.
    #[serde(rename = "<")]
    LessThan {
        /// Arguments to compare.
        args: Vec<Cql2Operand>,
    },

    /// Less than or equal comparison.
    #[serde(rename = "<=")]
    LessThanOrEqual {
        /// Arguments to compare.
        args: Vec<Cql2Operand>,
    },

    /// Greater than comparison.
    #[serde(rename = ">")]
    GreaterThan {
        /// Arguments to compare.
        args: Vec<Cql2Operand>,
    },

    /// Greater than or equal comparison.
    #[serde(rename = ">=")]
    GreaterThanOrEqual {
        /// Arguments to compare.
        args: Vec<Cql2Operand>,
    },

    /// LIKE pattern matching.
    #[serde(rename = "like")]
    Like {
        /// Arguments for LIKE operation.
        args: Vec<Cql2Operand>,
    },

    /// IN membership test.
    #[serde(rename = "in")]
    In {
        /// Arguments for IN operation.
        args: Vec<Cql2Operand>,
    },

    /// IS NULL test.
    #[serde(rename = "isNull")]
    IsNull {
        /// Arguments for IS NULL test.
        args: Vec<Cql2Operand>,
    },

    /// Between range test.
    #[serde(rename = "between")]
    Between {
        /// Arguments for BETWEEN operation: [value, lower, upper].
        args: Vec<Cql2Operand>,
    },

    /// Spatial intersection.
    #[serde(rename = "s_intersects")]
    SIntersects {
        /// Arguments for spatial intersection.
        args: Vec<Cql2Operand>,
    },

    /// Spatial contains.
    #[serde(rename = "s_contains")]
    SContains {
        /// Arguments for spatial contains.
        args: Vec<Cql2Operand>,
    },

    /// Spatial within.
    #[serde(rename = "s_within")]
    SWithin {
        /// Arguments for spatial within.
        args: Vec<Cql2Operand>,
    },

    /// Temporal after.
    #[serde(rename = "t_after")]
    TAfter {
        /// Arguments for temporal after.
        args: Vec<Cql2Operand>,
    },

    /// Temporal before.
    #[serde(rename = "t_before")]
    TBefore {
        /// Arguments for temporal before.
        args: Vec<Cql2Operand>,
    },

    /// Temporal during.
    #[serde(rename = "t_during")]
    TDuring {
        /// Arguments for temporal during.
        args: Vec<Cql2Operand>,
    },
}

/// CQL2 operand (can be a property reference or a literal value).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Cql2Operand {
    /// Property reference.
    Property {
        /// Property name.
        property: String,
    },

    /// Literal value.
    Literal(Value),
}

impl Cql2Filter {
    /// Creates a new AND filter.
    pub fn and(filters: Vec<Cql2Filter>) -> Self {
        Self::And { args: filters }
    }

    /// Creates a new OR filter.
    pub fn or(filters: Vec<Cql2Filter>) -> Self {
        Self::Or { args: filters }
    }

    /// Creates a new NOT filter.
    pub fn not(filter: Cql2Filter) -> Self {
        Self::Not { args: vec![filter] }
    }

    /// Creates a new equality filter.
    pub fn equal(property: impl Into<String>, value: Value) -> Self {
        Self::Equal {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(value),
            ],
        }
    }

    /// Creates a new not equal filter.
    pub fn not_equal(property: impl Into<String>, value: Value) -> Self {
        Self::NotEqual {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(value),
            ],
        }
    }

    /// Creates a new less than filter.
    pub fn less_than(property: impl Into<String>, value: Value) -> Self {
        Self::LessThan {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(value),
            ],
        }
    }

    /// Creates a new less than or equal filter.
    pub fn less_than_or_equal(property: impl Into<String>, value: Value) -> Self {
        Self::LessThanOrEqual {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(value),
            ],
        }
    }

    /// Creates a new greater than filter.
    pub fn greater_than(property: impl Into<String>, value: Value) -> Self {
        Self::GreaterThan {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(value),
            ],
        }
    }

    /// Creates a new greater than or equal filter.
    pub fn greater_than_or_equal(property: impl Into<String>, value: Value) -> Self {
        Self::GreaterThanOrEqual {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(value),
            ],
        }
    }

    /// Creates a new LIKE filter.
    pub fn like(property: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self::Like {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(Value::String(pattern.into())),
            ],
        }
    }

    /// Creates a new IN filter.
    pub fn in_values(property: impl Into<String>, values: Vec<Value>) -> Self {
        let mut args = vec![Cql2Operand::Property {
            property: property.into(),
        }];
        args.extend(values.into_iter().map(Cql2Operand::Literal));

        Self::In { args }
    }

    /// Creates a new IS NULL filter.
    pub fn is_null(property: impl Into<String>) -> Self {
        Self::IsNull {
            args: vec![Cql2Operand::Property {
                property: property.into(),
            }],
        }
    }

    /// Creates a new BETWEEN filter.
    pub fn between(property: impl Into<String>, lower: Value, upper: Value) -> Self {
        Self::Between {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(lower),
                Cql2Operand::Literal(upper),
            ],
        }
    }

    /// Creates a new spatial intersects filter.
    pub fn s_intersects(property: impl Into<String>, geometry: Value) -> Self {
        Self::SIntersects {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(geometry),
            ],
        }
    }

    /// Creates a new spatial contains filter.
    pub fn s_contains(property: impl Into<String>, geometry: Value) -> Self {
        Self::SContains {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(geometry),
            ],
        }
    }

    /// Creates a new spatial within filter.
    pub fn s_within(property: impl Into<String>, geometry: Value) -> Self {
        Self::SWithin {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(geometry),
            ],
        }
    }

    /// Creates a new temporal after filter.
    pub fn t_after(property: impl Into<String>, timestamp: impl Into<String>) -> Self {
        Self::TAfter {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(Value::String(timestamp.into())),
            ],
        }
    }

    /// Creates a new temporal before filter.
    pub fn t_before(property: impl Into<String>, timestamp: impl Into<String>) -> Self {
        Self::TBefore {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(Value::String(timestamp.into())),
            ],
        }
    }

    /// Creates a new temporal during filter.
    pub fn t_during(property: impl Into<String>, interval: Value) -> Self {
        Self::TDuring {
            args: vec![
                Cql2Operand::Property {
                    property: property.into(),
                },
                Cql2Operand::Literal(interval),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_cql2_equal() {
        let filter = Cql2Filter::equal("platform", json!("Sentinel-2A"));

        let json = serde_json::to_string(&filter).expect("Failed to serialize");
        assert!(json.contains("\"op\":\"=\""));
        assert!(json.contains("platform"));
        assert!(json.contains("Sentinel-2A"));
    }

    #[test]
    fn test_cql2_and() {
        let filter = Cql2Filter::and(vec![
            Cql2Filter::equal("platform", json!("Sentinel-2A")),
            Cql2Filter::greater_than("eo:cloud_cover", json!(10)),
        ]);

        let json = serde_json::to_string(&filter).expect("Failed to serialize");
        assert!(json.contains("\"op\":\"and\""));
    }

    #[test]
    fn test_cql2_between() {
        let filter = Cql2Filter::between("eo:cloud_cover", json!(0), json!(20));

        let json = serde_json::to_string(&filter).expect("Failed to serialize");
        assert!(json.contains("\"op\":\"between\""));
    }

    #[test]
    fn test_cql2_like() {
        let filter = Cql2Filter::like("id", "S2%");

        let json = serde_json::to_string(&filter).expect("Failed to serialize");
        assert!(json.contains("\"op\":\"like\""));
        assert!(json.contains("S2%"));
    }

    #[test]
    fn test_cql2_in() {
        let filter =
            Cql2Filter::in_values("platform", vec![json!("Sentinel-2A"), json!("Sentinel-2B")]);

        let json = serde_json::to_string(&filter).expect("Failed to serialize");
        assert!(json.contains("\"op\":\"in\""));
    }

    #[test]
    fn test_cql2_spatial() {
        let geometry = json!({
            "type": "Point",
            "coordinates": [-122.0, 37.0]
        });

        let filter = Cql2Filter::s_intersects("geometry", geometry);

        let json = serde_json::to_string(&filter).expect("Failed to serialize");
        assert!(json.contains("\"op\":\"s_intersects\""));
    }

    #[test]
    fn test_cql2_temporal() {
        let filter = Cql2Filter::t_after("datetime", "2023-01-01T00:00:00Z");

        let json = serde_json::to_string(&filter).expect("Failed to serialize");
        assert!(json.contains("\"op\":\"t_after\""));
    }

    #[test]
    fn test_cql2_complex() {
        let filter = Cql2Filter::and(vec![
            Cql2Filter::equal("collection", json!("sentinel-2-l2a")),
            Cql2Filter::or(vec![
                Cql2Filter::less_than("eo:cloud_cover", json!(20)),
                Cql2Filter::is_null("eo:cloud_cover"),
            ]),
            Cql2Filter::between("view:off_nadir", json!(0), json!(15)),
        ]);

        let json = serde_json::to_string(&filter).expect("Failed to serialize");
        assert!(json.contains("\"op\":\"and\""));
        assert!(json.contains("\"op\":\"or\""));
        assert!(json.contains("\"op\":\"between\""));

        // Test round-trip
        let deserialized: Cql2Filter = serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(deserialized, filter);
    }
}
