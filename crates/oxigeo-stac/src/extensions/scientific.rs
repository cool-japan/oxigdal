//! Scientific Extension.
//!
//! This module implements the STAC Scientific Citation Extension for describing scientific data.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Scientific Extension for STAC Items and Collections.
///
/// This extension provides fields for citing scientific data and publications.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScientificExtension {
    /// Digital Object Identifier (DOI).
    #[serde(rename = "sci:doi", skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,

    /// Citation text for the data.
    #[serde(rename = "sci:citation", skip_serializing_if = "Option::is_none")]
    pub citation: Option<String>,

    /// Publications that describe or use the data.
    #[serde(rename = "sci:publications", skip_serializing_if = "Option::is_none")]
    pub publications: Option<Vec<Publication>>,

    /// Additional properties.
    #[serde(flatten)]
    pub additional_properties: HashMap<String, serde_json::Value>,
}

/// Scientific publication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Publication {
    /// Digital Object Identifier (DOI) of the publication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,

    /// Citation text for the publication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citation: Option<String>,

    /// URL to the publication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl ScientificExtension {
    /// Creates a new Scientific extension.
    pub fn new() -> Self {
        Self {
            doi: None,
            citation: None,
            publications: None,
            additional_properties: HashMap::new(),
        }
    }

    /// Sets the DOI.
    pub fn with_doi(mut self, doi: impl Into<String>) -> Self {
        self.doi = Some(doi.into());
        self
    }

    /// Sets the citation.
    pub fn with_citation(mut self, citation: impl Into<String>) -> Self {
        self.citation = Some(citation.into());
        self
    }

    /// Adds a publication.
    pub fn add_publication(mut self, publication: Publication) -> Self {
        match &mut self.publications {
            Some(pubs) => pubs.push(publication),
            None => self.publications = Some(vec![publication]),
        }
        self
    }

    /// Sets the publications list.
    pub fn with_publications(mut self, publications: Vec<Publication>) -> Self {
        self.publications = Some(publications);
        self
    }

    /// Validates the DOI format.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, otherwise an error message.
    pub fn validate(&self) -> Result<(), String> {
        // Basic DOI validation (should start with "10.")
        if let Some(doi) = &self.doi {
            if !doi.starts_with("10.") {
                return Err(format!("DOI should start with '10.', got: {}", doi));
            }
        }

        // Validate publications
        if let Some(publications) = &self.publications {
            for (i, pub_) in publications.iter().enumerate() {
                if let Some(doi) = &pub_.doi {
                    if !doi.starts_with("10.") {
                        return Err(format!(
                            "Publication[{}] DOI should start with '10.', got: {}",
                            i, doi
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}

impl Default for ScientificExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl Publication {
    /// Creates a new publication.
    pub fn new() -> Self {
        Self {
            doi: None,
            citation: None,
            url: None,
        }
    }

    /// Creates a publication with a DOI.
    pub fn with_doi(doi: impl Into<String>) -> Self {
        Self {
            doi: Some(doi.into()),
            citation: None,
            url: None,
        }
    }

    /// Sets the citation.
    pub fn with_citation(mut self, citation: impl Into<String>) -> Self {
        self.citation = Some(citation.into());
        self
    }

    /// Sets the URL.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }
}

impl Default for Publication {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scientific_extension_new() {
        let sci = ScientificExtension::new();
        assert!(sci.doi.is_none());
        assert!(sci.citation.is_none());
        assert!(sci.publications.is_none());
    }

    #[test]
    fn test_scientific_extension_builder() {
        let pub1 = Publication::with_doi("10.1000/example")
            .with_citation("Example et al. (2023)")
            .with_url("https://example.com/paper");

        let sci = ScientificExtension::new()
            .with_doi("10.1000/dataset")
            .with_citation("Dataset v1.0")
            .add_publication(pub1);

        assert_eq!(sci.doi, Some("10.1000/dataset".to_string()));
        assert_eq!(sci.citation, Some("Dataset v1.0".to_string()));
        assert_eq!(sci.publications.as_ref().map(|p| p.len()), Some(1));
    }

    #[test]
    fn test_scientific_extension_validation() {
        let valid = ScientificExtension::new().with_doi("10.1000/example");
        assert!(valid.validate().is_ok());

        let invalid = ScientificExtension::new().with_doi("invalid-doi");
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_publication_builder() {
        let pub_ = Publication::with_doi("10.1000/example")
            .with_citation("Example et al. (2023)")
            .with_url("https://example.com");

        assert_eq!(pub_.doi, Some("10.1000/example".to_string()));
        assert_eq!(pub_.citation, Some("Example et al. (2023)".to_string()));
        assert_eq!(pub_.url, Some("https://example.com".to_string()));
    }

    #[test]
    fn test_scientific_extension_serialization() {
        let sci = ScientificExtension::new()
            .with_doi("10.1000/dataset")
            .with_citation("Dataset v1.0");

        let json = serde_json::to_string(&sci).expect("Failed to serialize");
        assert!(json.contains("sci:doi"));
        assert!(json.contains("sci:citation"));

        let deserialized: ScientificExtension =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(deserialized, sci);
    }
}
