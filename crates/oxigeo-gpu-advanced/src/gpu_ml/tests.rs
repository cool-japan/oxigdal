//! Tests for GPU ML inference module.

use super::*;

#[test]
fn test_activation_type() {
    let relu = ActivationType::ReLU;
    assert!(matches!(relu, ActivationType::ReLU));

    let leaky = ActivationType::LeakyReLU(0.01);
    assert!(matches!(leaky, ActivationType::LeakyReLU(_)));

    let sigmoid = ActivationType::Sigmoid;
    assert!(matches!(sigmoid, ActivationType::Sigmoid));

    let tanh = ActivationType::Tanh;
    assert!(matches!(tanh, ActivationType::Tanh));
}

#[test]
fn test_pool_type() {
    let max_pool = PoolType::Max;
    assert!(matches!(max_pool, PoolType::Max));

    let avg_pool = PoolType::Average;
    assert!(matches!(avg_pool, PoolType::Average));
}

#[test]
fn test_inference_stats() {
    let stats = InferenceStats {
        total_inferences: 1000,
        total_batches: 50,
        avg_batch_size: 20.0,
        total_time_us: 100000,
        avg_time_per_sample_us: 100.0,
    };

    assert_eq!(stats.total_inferences, 1000);
    assert_eq!(stats.total_batches, 50);
    assert!((stats.avg_batch_size - 20.0).abs() < f64::EPSILON);
}

#[test]
fn test_layer_type_dense() {
    let layer = LayerType::Dense {
        input_features: 128,
        output_features: 64,
    };
    if let LayerType::Dense {
        input_features,
        output_features,
    } = layer
    {
        assert_eq!(input_features, 128);
        assert_eq!(output_features, 64);
    } else {
        panic!("Expected Dense layer");
    }
}

#[test]
fn test_layer_type_conv2d() {
    let layer = LayerType::Conv2d {
        input_channels: 3,
        output_channels: 32,
        kernel_size: 3,
    };
    if let LayerType::Conv2d {
        input_channels,
        output_channels,
        kernel_size,
    } = layer
    {
        assert_eq!(input_channels, 3);
        assert_eq!(output_channels, 32);
        assert_eq!(kernel_size, 3);
    } else {
        panic!("Expected Conv2d layer");
    }
}

#[test]
fn test_layer_type_batch_norm() {
    let layer = LayerType::BatchNorm {
        num_features: 64,
        epsilon: 1e-5,
    };
    if let LayerType::BatchNorm {
        num_features,
        epsilon,
    } = layer
    {
        assert_eq!(num_features, 64);
        assert!((epsilon - 1e-5).abs() < 1e-10);
    } else {
        panic!("Expected BatchNorm layer");
    }
}

#[test]
fn test_layer_type_pool2d() {
    let layer = LayerType::Pool2d {
        pool_type: PoolType::Max,
        pool_size: 2,
        stride: 2,
    };
    if let LayerType::Pool2d {
        pool_type,
        pool_size,
        stride,
    } = layer
    {
        assert!(matches!(pool_type, PoolType::Max));
        assert_eq!(pool_size, 2);
        assert_eq!(stride, 2);
    } else {
        panic!("Expected Pool2d layer");
    }
}

#[test]
fn test_layer_type_activation() {
    let layer = LayerType::Activation {
        activation: ActivationType::ReLU,
    };
    if let LayerType::Activation { activation } = layer {
        assert!(matches!(activation, ActivationType::ReLU));
    } else {
        panic!("Expected Activation layer");
    }
}

#[test]
fn test_layer_type_flatten() {
    let layer = LayerType::Flatten;
    assert!(matches!(layer, LayerType::Flatten));
}

#[test]
fn test_layer_type_dropout() {
    let layer = LayerType::Dropout { _rate: 0.5 };
    if let LayerType::Dropout { _rate } = layer {
        assert!((_rate - 0.5).abs() < f32::EPSILON);
    } else {
        panic!("Expected Dropout layer");
    }
}

#[test]
fn test_inference_stats_default() {
    let stats = InferenceStats::default();
    assert_eq!(stats.total_inferences, 0);
    assert_eq!(stats.total_batches, 0);
    assert!((stats.avg_batch_size - 0.0).abs() < f64::EPSILON);
    assert_eq!(stats.total_time_us, 0);
    assert!((stats.avg_time_per_sample_us - 0.0).abs() < f64::EPSILON);
}

/// CPU reference implementation for dense layer (for testing)
fn cpu_dense_layer(
    input: &[f32],
    weights: &[f32],
    bias: &[f32],
    batch_size: usize,
    input_features: usize,
    output_features: usize,
) -> Vec<f32> {
    let mut output = vec![0.0; batch_size * output_features];

    for b in 0..batch_size {
        for o in 0..output_features {
            let mut sum = 0.0;
            for i in 0..input_features {
                let input_idx = b * input_features + i;
                let weight_idx = i * output_features + o;
                sum += input[input_idx] * weights[weight_idx];
            }
            sum += bias[o];
            output[b * output_features + o] = sum;
        }
    }

    output
}

/// CPU reference implementation for ReLU
fn cpu_relu(input: &[f32]) -> Vec<f32> {
    input.iter().map(|&x| x.max(0.0)).collect()
}

/// CPU reference implementation for softmax
fn cpu_softmax(input: &[f32], batch_size: usize) -> Vec<f32> {
    let features = input.len() / batch_size;
    let mut output = vec![0.0; input.len()];

    for b in 0..batch_size {
        let offset = b * features;

        // Find max for numerical stability
        let max_val = input[offset..offset + features]
            .iter()
            .fold(f32::NEG_INFINITY, |a, &b| a.max(b));

        // Compute exp and sum
        let mut sum = 0.0;
        for i in 0..features {
            let exp_val = (input[offset + i] - max_val).exp();
            output[offset + i] = exp_val;
            sum += exp_val;
        }

        // Normalize
        for i in 0..features {
            output[offset + i] /= sum;
        }
    }

    output
}

/// CPU reference implementation for matrix multiplication
fn cpu_matmul(a: &[f32], b: &[f32], m: usize, k: usize, n: usize) -> Vec<f32> {
    let mut c = vec![0.0; m * n];

    for i in 0..m {
        for j in 0..n {
            let mut sum = 0.0;
            for l in 0..k {
                sum += a[i * k + l] * b[l * n + j];
            }
            c[i * n + j] = sum;
        }
    }

    c
}

#[test]
fn test_cpu_dense_layer_reference() {
    // Test CPU reference implementation
    let input = vec![1.0, 2.0, 3.0, 4.0]; // 2 batches, 2 features
    let weights = vec![0.5, 0.3, 0.2, 0.4]; // 2 input, 2 output
    let bias = vec![0.1, 0.2];

    let output = cpu_dense_layer(&input, &weights, &bias, 2, 2, 2);

    // Batch 0: [1, 2] * [[0.5, 0.3], [0.2, 0.4]] + [0.1, 0.2]
    // = [1*0.5 + 2*0.2 + 0.1, 1*0.3 + 2*0.4 + 0.2]
    // = [0.5 + 0.4 + 0.1, 0.3 + 0.8 + 0.2] = [1.0, 1.3]
    assert!((output[0] - 1.0).abs() < 1e-5);
    assert!((output[1] - 1.3).abs() < 1e-5);

    // Batch 1: [3, 4] * [[0.5, 0.3], [0.2, 0.4]] + [0.1, 0.2]
    // = [3*0.5 + 4*0.2 + 0.1, 3*0.3 + 4*0.4 + 0.2]
    // = [1.5 + 0.8 + 0.1, 0.9 + 1.6 + 0.2] = [2.4, 2.7]
    assert!((output[2] - 2.4).abs() < 1e-5);
    assert!((output[3] - 2.7).abs() < 1e-5);
}

#[test]
fn test_cpu_relu_reference() {
    let input = vec![-2.0, -1.0, 0.0, 1.0, 2.0];
    let output = cpu_relu(&input);

    assert!((output[0] - 0.0).abs() < f32::EPSILON);
    assert!((output[1] - 0.0).abs() < f32::EPSILON);
    assert!((output[2] - 0.0).abs() < f32::EPSILON);
    assert!((output[3] - 1.0).abs() < f32::EPSILON);
    assert!((output[4] - 2.0).abs() < f32::EPSILON);
}

#[test]
fn test_cpu_softmax_reference() {
    let input = vec![1.0, 2.0, 3.0]; // 1 batch, 3 features
    let output = cpu_softmax(&input, 1);

    // Verify sum to 1
    let sum: f32 = output.iter().sum();
    assert!((sum - 1.0).abs() < 1e-5);

    // Verify ordering
    assert!(output[0] < output[1]);
    assert!(output[1] < output[2]);
}

#[test]
fn test_cpu_matmul_reference() {
    // 2x3 @ 3x2 = 2x2
    let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let b = vec![7.0, 8.0, 9.0, 10.0, 11.0, 12.0];
    let c = cpu_matmul(&a, &b, 2, 3, 2);

    // Row 0: [1, 2, 3] @ [[7, 8], [9, 10], [11, 12]]
    // = [1*7 + 2*9 + 3*11, 1*8 + 2*10 + 3*12] = [58, 64]
    assert!((c[0] - 58.0).abs() < 1e-5);
    assert!((c[1] - 64.0).abs() < 1e-5);

    // Row 1: [4, 5, 6] @ [[7, 8], [9, 10], [11, 12]]
    // = [4*7 + 5*9 + 6*11, 4*8 + 5*10 + 6*12] = [139, 154]
    assert!((c[2] - 139.0).abs() < 1e-5);
    assert!((c[3] - 154.0).abs() < 1e-5);
}
