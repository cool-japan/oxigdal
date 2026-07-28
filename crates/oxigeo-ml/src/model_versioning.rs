//! Model versioning and A/B testing for geospatial ML
//!
//! # Overview
//!
//! This module provides:
//!
//! - [`ModelVersion`] — semantic versioning for ML models
//! - [`ModelMetadata`] — rich metadata stored alongside a registered model
//! - [`ModelRegistry`] — in-memory registry that keeps every version of every
//!   named model and can return the latest or a specific version
//! - [`AbTestConfig`] — deterministic per-request traffic splitting for online
//!   A/B experiments

use std::collections::HashMap;
use std::time::SystemTime;

use crate::error::MlError;

/// Semantic version for a model.
///
/// Ordering follows [semver](https://semver.org) precedence rules:
///
/// 1. The numeric triple `(major, minor, patch)` is compared first.
/// 2. When the triples are equal, a version **without** a tag (a final release)
///    has *higher* precedence than one carrying a pre-release/build `tag`. This
///    matches semver — `1.0.0 > 1.0.0-canary` — and means [`ModelRegistry::
///    get_latest`] never prefers an experimental tagged build over an
///    untagged release with the same numeric version.
/// 3. When both versions carry a tag, the tags are compared lexicographically
///    (ASCII order), again matching semver pre-release ordering.
///
/// The manual [`Ord`]/[`PartialOrd`] impls are kept consistent with the derived
/// [`Eq`]/[`Hash`]: `a.cmp(b) == Ordering::Equal` iff every field (including the
/// tag) is equal, so distinct tags remain distinct registry entries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelVersion {
    /// Breaking changes
    pub major: u32,
    /// Backward-compatible additions
    pub minor: u32,
    /// Backward-compatible bug-fixes
    pub patch: u32,
    /// Optional pre-release or build tag (e.g. `"alpha"`, `"prod"`)
    pub tag: Option<String>,
}

impl Ord for ModelVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        // Numeric triple first.
        match (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)) {
            Ordering::Equal => {}
            non_eq => return non_eq,
        }
        // Equal triple: apply semver pre-release precedence. A missing tag
        // (final release) outranks any present tag; two present tags compare
        // lexicographically.
        match (&self.tag, &other.tag) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for ModelVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ModelVersion {
    /// Create a release version with no tag.
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            tag: None,
        }
    }

    /// Attach a tag to this version (builder pattern).
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }
}

impl std::fmt::Display for ModelVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(tag) = &self.tag {
            write!(f, "-{tag}")?;
        }
        Ok(())
    }
}

/// Rich metadata for a registered model.
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    /// Human-readable model name (used as registry key)
    pub name: String,
    /// Version of this model entry
    pub version: ModelVersion,
    /// Short description of what the model does
    pub description: String,
    /// Expected shape of each input tensor
    pub input_shapes: Vec<Vec<usize>>,
    /// Expected shape of each output tensor
    pub output_shapes: Vec<Vec<usize>>,
    /// Wall-clock registration time
    pub created_at: SystemTime,
    /// Arbitrary key-value annotations (e.g. `"framework" => "onnx"`)
    pub tags: HashMap<String, String>,
}

impl ModelMetadata {
    /// Create a minimal metadata entry.
    pub fn new(
        name: impl Into<String>,
        version: ModelVersion,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version,
            description: description.into(),
            input_shapes: Vec::new(),
            output_shapes: Vec::new(),
            created_at: SystemTime::now(),
            tags: HashMap::new(),
        }
    }
}

/// A/B test traffic split configuration.
///
/// Routes individual requests to one of two models based on a deterministic
/// hash of the request ID.  The fraction `split` controls how much traffic
/// goes to `model_a`; the remainder goes to `model_b`.
#[derive(Debug, Clone)]
pub struct AbTestConfig {
    /// (model_a_id, fraction of traffic to model_a)
    pub model_a: (String, f64),
    /// (model_b_id, 1 − fraction)
    pub model_b: (String, f64),
    /// Seed mixed into the routing hash for experiment isolation
    pub routing_seed: u64,
}

impl AbTestConfig {
    /// Create a new A/B test where `split` fraction of traffic goes to
    /// `model_a` and `1 - split` goes to `model_b`.
    ///
    /// Returns an error if `split` is outside `[0.0, 1.0]`.
    pub fn new(
        model_a: impl Into<String>,
        model_b: impl Into<String>,
        split: f64,
    ) -> Result<Self, MlError> {
        if !(0.0..=1.0).contains(&split) {
            return Err(MlError::InvalidConfig("split must be in [0.0, 1.0]".into()));
        }
        Ok(Self {
            model_a: (model_a.into(), split),
            model_b: (model_b.into(), 1.0 - split),
            routing_seed: 42,
        })
    }

    /// Override the routing seed (useful for experiment isolation).
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.routing_seed = seed;
        self
    }

    /// Route a request to either `model_a` or `model_b` based on a
    /// deterministic hash of `request_id` XOR `routing_seed`.
    ///
    /// The returned `&str` is the model identifier chosen for this request.
    pub fn route(&self, request_id: u64) -> &str {
        let hash = fnv1a_u64(request_id ^ self.routing_seed);
        // Map to [0.0, 1.0)
        let fraction = (hash as f64) / (u64::MAX as f64 + 1.0);
        if fraction < self.model_a.1 {
            &self.model_a.0
        } else {
            &self.model_b.0
        }
    }
}

/// Simple FNV-1a hash of a single `u64` value.
///
/// Used for deterministic request routing without pulling in external crates.
fn fnv1a_u64(value: u64) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let bytes = value.to_le_bytes();
    let mut hash = OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// In-memory model registry.
///
/// Stores all registered versions of every named model.  The registry does
/// not manage model files on disk; it is purely a metadata store.
pub struct ModelRegistry {
    /// name → list of metadata entries sorted by version (ascending)
    models: HashMap<String, Vec<ModelMetadata>>,
}

impl ModelRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            models: HashMap::new(),
        }
    }

    /// Register a new model version.
    ///
    /// If a version with the same `(name, version)` pair already exists it is
    /// replaced.
    pub fn register(&mut self, metadata: ModelMetadata) {
        let versions = self.models.entry(metadata.name.clone()).or_default();

        // Replace existing entry with the same version if present
        if let Some(pos) = versions.iter().position(|m| m.version == metadata.version) {
            versions[pos] = metadata;
        } else {
            versions.push(metadata);
            // Keep list sorted ascending so `last()` is always the latest
            versions.sort_by(|a, b| a.version.cmp(&b.version));
        }
    }

    /// Return the latest (highest-version) entry for `name`, or `None` if
    /// the name is unknown.
    pub fn get_latest(&self, name: &str) -> Option<&ModelMetadata> {
        self.models.get(name)?.last()
    }

    /// Return the entry for a specific `(name, version)` pair.
    pub fn get_version(&self, name: &str, version: &ModelVersion) -> Option<&ModelMetadata> {
        self.models
            .get(name)?
            .iter()
            .find(|m| &m.version == version)
    }

    /// List all registered versions for `name`, sorted ascending.
    ///
    /// Returns an empty `Vec` if the name is unknown.
    pub fn list_versions(&self, name: &str) -> Vec<&ModelVersion> {
        match self.models.get(name) {
            Some(versions) => versions.iter().map(|m| &m.version).collect(),
            None => Vec::new(),
        }
    }

    /// List all model names currently in the registry.
    pub fn list_models(&self) -> Vec<&str> {
        self.models.keys().map(String::as_str).collect()
    }

    /// Remove a specific version of a model from the registry.
    ///
    /// Returns `true` if the entry was found and removed.
    pub fn unregister_version(&mut self, name: &str, version: &ModelVersion) -> bool {
        if let Some(versions) = self.models.get_mut(name) {
            if let Some(pos) = versions.iter().position(|m| &m.version == version) {
                versions.remove(pos);
                if versions.is_empty() {
                    self.models.remove(name);
                }
                return true;
            }
        }
        false
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> ModelVersion {
        ModelVersion::new(major, minor, patch)
    }

    fn meta(name: &str, version: ModelVersion) -> ModelMetadata {
        ModelMetadata::new(name, version, "test model")
    }

    // ── ModelVersion ──────────────────────────────────────────────────────────

    #[test]
    fn test_version_ordering_major() {
        assert!(v(1, 0, 0) < v(2, 0, 0));
    }

    #[test]
    fn test_version_ordering_minor() {
        assert!(v(1, 0, 0) < v(1, 1, 0));
    }

    #[test]
    fn test_version_ordering_patch() {
        assert!(v(1, 1, 0) < v(1, 1, 1));
    }

    #[test]
    fn test_version_ordering_chain() {
        let v100 = v(1, 0, 0);
        let v110 = v(1, 1, 0);
        let v200 = v(2, 0, 0);
        assert!(v100 < v110);
        assert!(v110 < v200);
        assert!(v100 < v200);
    }

    #[test]
    fn test_version_equality() {
        assert_eq!(v(1, 2, 3), v(1, 2, 3));
    }

    #[test]
    fn test_version_display_no_tag() {
        assert_eq!(v(1, 2, 3).to_string(), "1.2.3");
    }

    #[test]
    fn test_version_display_with_tag() {
        let ver = v(1, 0, 0).with_tag("alpha");
        assert_eq!(ver.to_string(), "1.0.0-alpha");
    }

    #[test]
    fn test_version_with_tag_stores_tag() {
        let ver = v(2, 0, 0).with_tag("prod");
        assert_eq!(ver.tag.as_deref(), Some("prod"));
    }

    // ── ModelRegistry ─────────────────────────────────────────────────────────

    #[test]
    fn test_registry_register_and_get_latest() {
        let mut reg = ModelRegistry::new();
        reg.register(meta("unet", v(1, 0, 0)));
        reg.register(meta("unet", v(1, 1, 0)));

        let latest = reg.get_latest("unet").expect("should exist");
        assert_eq!(latest.version, v(1, 1, 0));
    }

    #[test]
    fn test_registry_get_version() {
        let mut reg = ModelRegistry::new();
        reg.register(meta("unet", v(1, 0, 0)));
        reg.register(meta("unet", v(2, 0, 0)));

        let found = reg.get_version("unet", &v(1, 0, 0)).expect("v1");
        assert_eq!(found.version, v(1, 0, 0));
    }

    #[test]
    fn test_registry_get_version_missing() {
        let reg = ModelRegistry::new();
        assert!(reg.get_version("unet", &v(1, 0, 0)).is_none());
    }

    #[test]
    fn test_registry_list_versions_sorted() {
        let mut reg = ModelRegistry::new();
        reg.register(meta("seg", v(2, 0, 0)));
        reg.register(meta("seg", v(1, 0, 0)));
        reg.register(meta("seg", v(1, 5, 0)));

        let versions = reg.list_versions("seg");
        assert_eq!(versions.len(), 3);
        assert!(versions[0] < versions[1]);
        assert!(versions[1] < versions[2]);
    }

    #[test]
    fn test_registry_list_versions_unknown_model() {
        let reg = ModelRegistry::new();
        assert!(reg.list_versions("does_not_exist").is_empty());
    }

    #[test]
    fn test_registry_list_models() {
        let mut reg = ModelRegistry::new();
        reg.register(meta("unet", v(1, 0, 0)));
        reg.register(meta("resnet", v(1, 0, 0)));

        let mut names = reg.list_models();
        names.sort();
        assert_eq!(names, vec!["resnet", "unet"]);
    }

    #[test]
    fn test_registry_unregister_version() {
        let mut reg = ModelRegistry::new();
        reg.register(meta("m", v(1, 0, 0)));
        reg.register(meta("m", v(2, 0, 0)));

        let removed = reg.unregister_version("m", &v(1, 0, 0));
        assert!(removed);
        assert!(reg.get_version("m", &v(1, 0, 0)).is_none());
        assert!(reg.get_version("m", &v(2, 0, 0)).is_some());
    }

    #[test]
    fn test_registry_unregister_nonexistent() {
        let mut reg = ModelRegistry::new();
        assert!(!reg.unregister_version("ghost", &v(1, 0, 0)));
    }

    #[test]
    fn test_registry_unregister_all_versions_removes_model() {
        let mut reg = ModelRegistry::new();
        reg.register(meta("tmp", v(1, 0, 0)));
        reg.unregister_version("tmp", &v(1, 0, 0));
        assert!(reg.list_models().is_empty());
    }

    #[test]
    fn test_registry_default() {
        let reg = ModelRegistry::default();
        assert!(reg.list_models().is_empty());
    }

    // ── AbTestConfig ──────────────────────────────────────────────────────────

    #[test]
    fn test_ab_routing_determinism() {
        let ab = AbTestConfig::new("a", "b", 0.5).expect("ok");
        let r1 = ab.route(12345).to_string();
        let r2 = ab.route(12345).to_string();
        assert_eq!(r1, r2, "routing must be deterministic");
    }

    #[test]
    fn test_ab_invalid_split_negative() {
        assert!(AbTestConfig::new("a", "b", -0.1).is_err());
    }

    #[test]
    fn test_ab_invalid_split_over_one() {
        assert!(AbTestConfig::new("a", "b", 1.1).is_err());
    }

    #[test]
    fn test_ab_split_one_all_to_a() {
        let ab = AbTestConfig::new("model_a", "model_b", 1.0).expect("ok");
        for id in 0..100_u64 {
            assert_eq!(ab.route(id), "model_a", "split=1.0 should route all to a");
        }
    }

    #[test]
    fn test_ab_split_zero_all_to_b() {
        let ab = AbTestConfig::new("model_a", "model_b", 0.0).expect("ok");
        for id in 0..100_u64 {
            assert_eq!(ab.route(id), "model_b", "split=0.0 should route all to b");
        }
    }

    #[test]
    fn test_ab_split_half_roughly_balanced() {
        let ab = AbTestConfig::new("a", "b", 0.5).expect("ok");
        let a_count = (0_u64..10_000).filter(|id| ab.route(*id) == "a").count();
        // With a decent hash, should be close to 5000 ± 300 (3σ)
        assert!(
            a_count > 4500 && a_count < 5500,
            "expected ~50% to a, got {a_count}/10000"
        );
    }

    #[test]
    fn test_ab_different_seeds_produce_different_routing() {
        let ab1 = AbTestConfig::new("a", "b", 0.5).expect("ok");
        let ab2 = ab1.clone().with_seed(999);

        // With different seeds, at least some requests should route differently
        let differs = (0_u64..100).any(|id| ab1.route(id) != ab2.route(id));
        assert!(
            differs,
            "different seeds should produce at least some different routes"
        );
    }

    #[test]
    fn test_ab_config_stores_fractions() {
        let ab = AbTestConfig::new("alpha", "beta", 0.3).expect("ok");
        assert!((ab.model_a.1 - 0.3).abs() < 1e-9);
        assert!((ab.model_b.1 - 0.7).abs() < 1e-9);
    }

    #[test]
    fn test_numeric_triple_dominates_tag() {
        // A larger numeric triple always wins regardless of tags.
        assert!(v(1, 0, 0).with_tag("zzz") < v(1, 0, 1));
        assert!(v(1, 0, 0) < v(1, 0, 1).with_tag("aaa"));
    }

    #[test]
    fn test_untagged_release_outranks_tagged_same_triple() {
        // semver: 1.0.0 > 1.0.0-canary (a final release beats a pre-release).
        let release = v(1, 0, 0);
        let canary = v(1, 0, 0).with_tag("canary");
        assert!(
            release > canary,
            "untagged release must outrank tagged build"
        );
        assert!(canary < release);
        // Ordering must be consistent with equality: distinct tags are never Equal.
        assert_ne!(release.cmp(&canary), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_two_tags_compare_lexicographically() {
        let alpha = v(1, 0, 0).with_tag("alpha");
        let beta = v(1, 0, 0).with_tag("beta");
        assert!(alpha < beta, "alpha < beta by ASCII tag order");
    }

    #[test]
    fn test_ord_consistent_with_eq() {
        let a = v(1, 0, 0).with_tag("x");
        let b = v(1, 0, 0).with_tag("x");
        assert_eq!(a, b);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_get_latest_prefers_untagged_release_over_canary() {
        // Registering a canary build and then the final release must make the
        // final release the "latest", not the experimental tagged build.
        let mut reg = ModelRegistry::new();
        reg.register(meta("unet", v(1, 0, 0).with_tag("canary")));
        reg.register(meta("unet", v(1, 0, 0)));

        let latest = reg.get_latest("unet").expect("should exist");
        assert_eq!(latest.version, v(1, 0, 0));
        assert!(
            latest.version.tag.is_none(),
            "latest must be the untagged release"
        );
    }
}
