//! Integration tests for model architectures.

use oxigdal_ml_foundation::models::{
    Activation, Model,
    layers::{Conv2dConfig, ConvBlock, ResidualBlock},
    resnet::ResNet,
    unet::UNet,
};

#[test]
fn test_conv2d_config() {
    let config = Conv2dConfig::new(3, 64, 3);
    assert!(config.validate().is_ok());
    assert_eq!(config.in_channels, 3);
    assert_eq!(config.out_channels, 64);

    let params = config.num_parameters();
    assert!(params > 0);

    let (out_h, out_w) = config.output_size(224, 224);
    assert_eq!(out_h, 224);
    assert_eq!(out_w, 224);
}

#[test]
fn test_conv_block() {
    let block = ConvBlock::new(3, 64, 3, Activation::ReLU);
    assert!(block.validate().is_ok());

    let params = block.num_parameters();
    assert!(params > 0);
}

#[test]
fn test_residual_block() {
    let block = ResidualBlock::new(64, 128, 2);
    assert!(block.validate().is_ok());
    assert!(block.downsample);

    let params = block.num_parameters();
    assert!(params > 0);
}

#[test]
fn test_unet_creation() {
    let unet = UNet::new(3, 10, 4).expect("Failed to create UNet");
    assert!(unet.validate().is_ok());
    assert_eq!(unet.config.in_channels, 3);
    assert_eq!(unet.config.num_classes, 10);
    assert_eq!(unet.config.depth, 4);

    let params = unet.num_parameters();
    assert!(params > 0);

    let metadata = unet.metadata();
    assert_eq!(metadata.name, "UNet");
    assert_eq!(metadata.in_channels, 3);
    assert_eq!(metadata.out_channels, 10);
}

#[test]
fn test_unet_variants() {
    let small = UNet::small(3, 5).expect("Failed to create small UNet");
    let standard = UNet::standard(3, 5).expect("Failed to create standard UNet");
    let deep = UNet::deep(3, 5).expect("Failed to create deep UNet");

    let small_params = small.num_parameters();
    let standard_params = standard.num_parameters();
    let deep_params = deep.num_parameters();

    assert!(small_params < standard_params);
    assert!(standard_params < deep_params);
}

#[test]
fn test_resnet_variants() {
    let resnet18 = ResNet::resnet18(3, 1000).expect("Failed to create ResNet-18");
    let resnet34 = ResNet::resnet34(3, 1000).expect("Failed to create ResNet-34");
    let resnet50 = ResNet::resnet50(3, 1000).expect("Failed to create ResNet-50");

    assert!(resnet18.validate().is_ok());
    assert!(resnet34.validate().is_ok());
    assert!(resnet50.validate().is_ok());

    let params18 = resnet18.num_parameters();
    let params34 = resnet34.num_parameters();
    let params50 = resnet50.num_parameters();

    assert!(params18 < params34);
    assert!(params34 < params50);
}

#[test]
fn test_resnet_metadata() {
    let resnet = ResNet::resnet18(3, 100).expect("Failed to create ResNet-18");
    let metadata = resnet.metadata();

    assert_eq!(metadata.name, "ResNet-18");
    assert_eq!(metadata.in_channels, 3);
    assert_eq!(metadata.out_channels, 100);
    assert!(metadata.num_parameters > 0);
}

#[test]
fn test_model_configuration() {
    // Test that models can be created with different configurations
    let unet_rgb = UNet::standard(3, 10).expect("Failed to create RGB UNet");
    let unet_multispectral = UNet::standard(10, 15).expect("Failed to create multispectral UNet");

    assert_eq!(unet_rgb.config.in_channels, 3);
    assert_eq!(unet_multispectral.config.in_channels, 10);
}

#[test]
fn test_invalid_model_configs() {
    let result = UNet::new(0, 10, 4);
    assert!(result.is_err());

    let result = UNet::new(3, 0, 4);
    assert!(result.is_err());

    let result = ResNet::resnet18(0, 100);
    assert!(result.is_err());
}
