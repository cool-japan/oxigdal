//! Integration tests for data augmentation.

use ndarray::Array3;
use oxigdal_ml_foundation::augmentation::{
    Augmentation, AugmentationPipeline, Clip, Identity, Normalize, Standardize,
    color::{Brightness, Contrast, Gamma},
    geometric::{CenterCrop, HorizontalFlip, Rotate90, VerticalFlip},
    geospatial::{BandSelection, SpectralNormalization},
    noise::GaussianNoise,
};

fn create_test_image() -> Array3<f32> {
    Array3::from_shape_fn((3, 8, 8), |(c, h, w)| {
        (c as f32 + h as f32 + w as f32) * 10.0
    })
}

#[test]
fn test_basic_augmentations() {
    let image = create_test_image();

    let identity = Identity;
    let result = identity.apply(&image).expect("Failed to apply identity");
    assert_eq!(result, image);

    let normalize = Normalize::from_uint8();
    let result = normalize.apply(&image).expect("Failed to apply normalize");
    assert_eq!(result.shape(), image.shape());

    let clip = Clip::new(0.0, 100.0).expect("Failed to create clip");
    let result = clip.apply(&image).expect("Failed to apply clip");
    assert_eq!(result.shape(), image.shape());
}

#[test]
fn test_geometric_augmentations() {
    let image = create_test_image();

    let h_flip = HorizontalFlip;
    let result = h_flip.apply(&image).expect("Failed to apply h_flip");
    assert_eq!(result.shape(), image.shape());

    let v_flip = VerticalFlip;
    let result = v_flip.apply(&image).expect("Failed to apply v_flip");
    assert_eq!(result.shape(), image.shape());

    let rotate = Rotate90::new(1).expect("Failed to create rotate");
    let result = rotate.apply(&image).expect("Failed to apply rotate");
    assert_eq!(result.dim().0, image.dim().0);

    let crop = CenterCrop::square(4).expect("Failed to create crop");
    let result = crop.apply(&image).expect("Failed to apply crop");
    assert_eq!(result.dim(), (3, 4, 4));
}

#[test]
fn test_color_augmentations() {
    let image = Array3::from_elem((3, 4, 4), 0.5);

    let brightness = Brightness::new(2.0).expect("Failed to create brightness");
    let result = brightness
        .apply(&image)
        .expect("Failed to apply brightness");
    assert!((result[[0, 0, 0]] - 1.0).abs() < 1e-6);

    let contrast = Contrast::new(1.5).expect("Failed to create contrast");
    let result = contrast.apply(&image).expect("Failed to apply contrast");
    assert_eq!(result.shape(), image.shape());

    let gamma = Gamma::new(2.0).expect("Failed to create gamma");
    let result = gamma.apply(&image).expect("Failed to apply gamma");
    assert!((result[[0, 0, 0]] - 0.25).abs() < 1e-6);
}

#[test]
fn test_geospatial_augmentations() {
    let image = Array3::from_shape_fn((10, 4, 4), |(c, _, _)| c as f32);

    let band_sel = BandSelection::rgb();
    let result = band_sel
        .apply(&image)
        .expect("Failed to apply band selection");
    assert_eq!(result.dim(), (3, 4, 4));

    let spectral_norm = SpectralNormalization::new(vec![0.0; 10], vec![100.0; 10])
        .expect("Failed to create spectral normalization");
    let result = spectral_norm
        .apply(&image)
        .expect("Failed to apply spectral normalization");
    assert_eq!(result.shape(), image.shape());
}

#[test]
fn test_augmentation_pipeline() {
    let image = create_test_image();
    let mut pipeline = AugmentationPipeline::new();

    pipeline
        .add(Box::new(Normalize::from_uint8()))
        .add(Box::new(HorizontalFlip))
        .add(Box::new(
            Clip::new(0.0, 1.0).expect("Failed to create clip"),
        ));

    assert_eq!(pipeline.len(), 3);

    let result = pipeline.apply(&image).expect("Failed to apply pipeline");
    assert_eq!(result.shape(), image.shape());
}

#[test]
fn test_standardization() {
    let image = Array3::from_elem((3, 4, 4), 0.5);
    let standardize = Standardize::imagenet();

    let result = standardize
        .apply(&image)
        .expect("Failed to apply standardization");
    assert_eq!(result.shape(), image.shape());
}

#[test]
fn test_noise_augmentation() {
    let image = Array3::from_elem((3, 4, 4), 0.5);
    let noise = GaussianNoise::new(0.0, 0.1).expect("Failed to create noise");

    let result = noise.apply(&image).expect("Failed to apply noise");
    assert_eq!(result.shape(), image.shape());
}

#[test]
fn test_augmentation_errors() {
    let image = Array3::from_elem((3, 5, 5), 1.0);

    // Crop larger than image
    let crop = CenterCrop::square(10).expect("Failed to create crop");
    let result = crop.apply(&image);
    assert!(result.is_err());

    // Invalid normalization
    let result = Normalize::new(100.0, 50.0);
    assert!(result.is_err());

    // Invalid brightness
    let result = Brightness::new(-1.0);
    assert!(result.is_err());
}
