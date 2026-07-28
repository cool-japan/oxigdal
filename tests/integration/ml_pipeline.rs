//! End-to-end ML pipeline integration tests.
//!
//! These tests describe complete ML workflows (image classification, object
//! detection, semantic segmentation, feature extraction, time-series
//! prediction, training, batch inference, model export/reload) that must run
//! against the real `oxigeo-ml` inference stack.
//!
//! `oxigeo-ml` is NOT a dependency of this test target (`oxigeo-dev-tools`), so
//! real inference cannot be exercised here. An earlier revision of this file
//! faked every stage — `classify_patches` returned `vec![0; n]`, `detect_objects`
//! returned one hardcoded box, `segment_image` returned a fixed 256x256 zero
//! mask — and asserted only the shapes of those fabricated outputs, so the tests
//! passed unconditionally while validating nothing.
//!
//! Rather than keep that false coverage, every pipeline test below is
//! `#[ignore]`d and returns an honest typed error. Wire `oxigeo-ml` in as a
//! dev-dependency of `oxigeo-dev-tools` and route these through the real
//! `Model`/inference API to make them real (see the crate follow-up note).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Honest error: the ML crate needed to run this pipeline for real is not wired
/// into this test target.
fn ml_unavailable(stage: &str) -> Box<dyn std::error::Error> {
    format!(
        "{stage} requires the oxigeo-ml inference stack, which is not a \
         dependency of oxigeo-dev-tools; this pipeline cannot be validated from \
         this test target"
    )
    .into()
}

#[test]
#[ignore = "requires oxigeo-ml dev-dependency for real classification inference"]
fn test_image_classification_pipeline() -> Result<()> {
    Err(ml_unavailable("image classification"))
}

#[test]
#[ignore = "requires oxigeo-ml dev-dependency for real object-detection inference"]
fn test_object_detection_pipeline() -> Result<()> {
    Err(ml_unavailable("object detection"))
}

#[test]
#[ignore = "requires oxigeo-ml dev-dependency for real segmentation inference"]
fn test_semantic_segmentation_pipeline() -> Result<()> {
    Err(ml_unavailable("semantic segmentation"))
}

#[test]
#[ignore = "requires oxigeo-ml dev-dependency for real feature extraction"]
fn test_feature_extraction_pipeline() -> Result<()> {
    Err(ml_unavailable("feature extraction"))
}

#[test]
#[ignore = "requires oxigeo-ml dev-dependency for real time-series prediction"]
fn test_time_series_prediction() -> Result<()> {
    Err(ml_unavailable("time-series prediction"))
}

#[test]
#[ignore = "requires oxigeo-ml dev-dependency and significant compute for training"]
fn test_model_training() -> Result<()> {
    Err(ml_unavailable("model training"))
}

#[test]
#[ignore = "requires oxigeo-ml dev-dependency for real batch inference"]
fn test_batch_inference() -> Result<()> {
    Err(ml_unavailable("batch inference"))
}

#[test]
#[ignore = "requires oxigeo-ml dev-dependency for real model export/reload"]
fn test_model_export_reload() -> Result<()> {
    Err(ml_unavailable("model export/reload"))
}
