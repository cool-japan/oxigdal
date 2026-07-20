//! Neural network components for knowledge distillation

/// Simple xorshift-based RNG for reproducibility
#[derive(Debug, Clone)]
pub struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    /// Creates a new RNG with given seed
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    /// Generates next u64
    pub fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// Generates a random f32 in [0, 1)
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() as f64 / u64::MAX as f64) as f32
    }

    /// Generates a normally distributed f32 using Box-Muller transform
    pub fn next_normal(&mut self) -> f32 {
        let u1 = self.next_f32().max(1e-10);
        let u2 = self.next_f32();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos()
    }

    /// Shuffles a slice in-place
    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        for i in (1..slice.len()).rev() {
            let j = (self.next_u64() as usize) % (i + 1);
            slice.swap(i, j);
        }
    }
}

/// A simple dense layer for demonstration purposes
#[derive(Debug, Clone)]
pub struct DenseLayer {
    /// Weight matrix (flattened: input_size * output_size)
    pub weights: Vec<f32>,
    /// Bias vector
    pub bias: Vec<f32>,
    /// Input size
    pub input_size: usize,
    /// Output size
    pub output_size: usize,
}

impl DenseLayer {
    /// Creates a new dense layer with Xavier initialization
    #[must_use]
    pub fn new(input_size: usize, output_size: usize, seed: u64) -> Self {
        let scale = (2.0 / (input_size + output_size) as f32).sqrt();
        let mut rng = SimpleRng::new(seed);

        let weights: Vec<f32> = (0..input_size * output_size)
            .map(|_| rng.next_normal() * scale)
            .collect();

        let bias = vec![0.0; output_size];

        Self {
            weights,
            bias,
            input_size,
            output_size,
        }
    }

    /// Forward pass
    #[must_use]
    pub fn forward(&self, input: &[f32]) -> Vec<f32> {
        let mut output = self.bias.clone();

        for (o_idx, out) in output.iter_mut().enumerate() {
            for (i_idx, &inp) in input.iter().enumerate() {
                let w_idx = o_idx * self.input_size + i_idx;
                if let Some(&w) = self.weights.get(w_idx) {
                    *out += inp * w;
                }
            }
        }

        output
    }

    /// Backward pass computing gradients w.r.t. weights, bias, and input
    #[must_use]
    pub fn backward(&self, input: &[f32], grad_output: &[f32]) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        // Gradient w.r.t. weights
        let mut grad_weights = vec![0.0; self.weights.len()];
        for (o_idx, &go) in grad_output.iter().enumerate() {
            for (i_idx, &inp) in input.iter().enumerate() {
                let w_idx = o_idx * self.input_size + i_idx;
                if w_idx < grad_weights.len() {
                    grad_weights[w_idx] += go * inp;
                }
            }
        }

        // Gradient w.r.t. bias
        let grad_bias = grad_output.to_vec();

        // Gradient w.r.t. input
        let mut grad_input = vec![0.0; self.input_size];
        for (o_idx, &go) in grad_output.iter().enumerate() {
            for (i_idx, gi) in grad_input.iter_mut().enumerate() {
                let w_idx = o_idx * self.input_size + i_idx;
                if let Some(&w) = self.weights.get(w_idx) {
                    *gi += go * w;
                }
            }
        }

        (grad_weights, grad_bias, grad_input)
    }

    /// Returns the total number of parameters
    #[must_use]
    pub fn num_params(&self) -> usize {
        self.weights.len() + self.bias.len()
    }

    /// Gets all parameters as a flat vector
    #[must_use]
    pub fn get_params(&self) -> Vec<f32> {
        let mut params = self.weights.clone();
        params.extend(&self.bias);
        params
    }

    /// Sets parameters from a flat vector
    pub fn set_params(&mut self, params: &[f32]) {
        let w_end = self.weights.len();
        let b_len = self.bias.len();
        if params.len() >= w_end + b_len {
            self.weights.copy_from_slice(&params[..w_end]);
            self.bias.copy_from_slice(&params[w_end..w_end + b_len]);
        }
    }
}

/// Cached activations for backpropagation
#[derive(Debug, Clone)]
pub struct ForwardCache {
    /// Input to the network
    pub input: Vec<f32>,
    /// Hidden layer pre-activation
    pub hidden_pre: Vec<f32>,
    /// Hidden layer post-activation (after ReLU)
    pub hidden_post: Vec<f32>,
}

/// Gradients for MLP
#[derive(Debug, Clone)]
pub struct MLPGradients {
    /// Hidden layer weight gradients
    pub hidden_weights: Vec<f32>,
    /// Hidden layer bias gradients
    pub hidden_bias: Vec<f32>,
    /// Output layer weight gradients
    pub output_weights: Vec<f32>,
    /// Output layer bias gradients
    pub output_bias: Vec<f32>,
}

impl MLPGradients {
    /// Flatten all gradients into a single vector
    #[must_use]
    pub fn flatten(&self) -> Vec<f32> {
        let mut flat = self.hidden_weights.clone();
        flat.extend(&self.hidden_bias);
        flat.extend(&self.output_weights);
        flat.extend(&self.output_bias);
        flat
    }
}

/// A simple two-layer MLP for student model
#[derive(Debug, Clone)]
pub struct SimpleMLP {
    /// Hidden layer
    pub hidden: DenseLayer,
    /// Output layer
    pub output: DenseLayer,
}

impl SimpleMLP {
    /// Creates a new simple MLP
    #[must_use]
    pub fn new(input_size: usize, hidden_size: usize, output_size: usize, seed: u64) -> Self {
        Self {
            hidden: DenseLayer::new(input_size, hidden_size, seed),
            output: DenseLayer::new(hidden_size, output_size, seed.wrapping_add(1)),
        }
    }

    /// Forward pass returning logits
    #[must_use]
    pub fn forward(&self, input: &[f32]) -> Vec<f32> {
        let hidden_out = self.hidden.forward(input);
        // ReLU activation
        let hidden_activated: Vec<f32> = hidden_out.iter().map(|&x| x.max(0.0)).collect();
        self.output.forward(&hidden_activated)
    }

    /// Forward pass with cached activations for backprop
    #[must_use]
    pub fn forward_with_cache(&self, input: &[f32]) -> (Vec<f32>, ForwardCache) {
        let hidden_pre = self.hidden.forward(input);
        let hidden_post: Vec<f32> = hidden_pre.iter().map(|&x| x.max(0.0)).collect();
        let output = self.output.forward(&hidden_post);

        let cache = ForwardCache {
            input: input.to_vec(),
            hidden_pre,
            hidden_post,
        };

        (output, cache)
    }

    /// Backward pass computing all gradients
    pub fn backward(&self, grad_output: &[f32], cache: &ForwardCache) -> MLPGradients {
        // Backward through output layer
        let (grad_out_weights, grad_out_bias, grad_hidden) =
            self.output.backward(&cache.hidden_post, grad_output);

        // Backward through ReLU
        let grad_hidden_pre: Vec<f32> = grad_hidden
            .iter()
            .zip(cache.hidden_pre.iter())
            .map(|(&g, &h)| if h > 0.0 { g } else { 0.0 })
            .collect();

        // Backward through hidden layer
        let (grad_hidden_weights, grad_hidden_bias, _) =
            self.hidden.backward(&cache.input, &grad_hidden_pre);

        MLPGradients {
            hidden_weights: grad_hidden_weights,
            hidden_bias: grad_hidden_bias,
            output_weights: grad_out_weights,
            output_bias: grad_out_bias,
        }
    }

    /// Total number of parameters
    #[must_use]
    pub fn num_params(&self) -> usize {
        self.hidden.num_params() + self.output.num_params()
    }

    /// Get all parameters as flat vector
    #[must_use]
    pub fn get_params(&self) -> Vec<f32> {
        let mut params = self.hidden.get_params();
        params.extend(self.output.get_params());
        params
    }

    /// Set parameters from flat vector
    pub fn set_params(&mut self, params: &[f32]) {
        let hidden_size = self.hidden.num_params();
        self.hidden.set_params(&params[..hidden_size]);
        self.output.set_params(&params[hidden_size..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_rng() {
        let mut rng = SimpleRng::new(42);

        let val1 = rng.next_u64();

        let mut rng2 = SimpleRng::new(42);
        let val2 = rng2.next_u64();

        assert_eq!(val1, val2);

        let mut rng3 = SimpleRng::new(123);
        for _ in 0..100 {
            let f = rng3.next_f32();
            assert!((0.0..1.0).contains(&f));
        }
    }

    #[test]
    fn test_dense_layer_forward() {
        let layer = DenseLayer::new(4, 3, 42);
        let input = vec![1.0, 2.0, 3.0, 4.0];
        let output = layer.forward(&input);

        assert_eq!(output.len(), 3);
        for &o in &output {
            assert!(o.is_finite());
        }
    }

    #[test]
    fn test_dense_layer_backward() {
        let layer = DenseLayer::new(4, 3, 42);
        let input = vec![1.0, 2.0, 3.0, 4.0];
        let grad_output = vec![0.1, 0.2, 0.3];

        let (grad_w, grad_b, grad_i) = layer.backward(&input, &grad_output);

        assert_eq!(grad_w.len(), 4 * 3);
        assert_eq!(grad_b.len(), 3);
        assert_eq!(grad_i.len(), 4);
    }

    #[test]
    fn test_simple_mlp_forward() {
        let mlp = SimpleMLP::new(10, 20, 5, 42);
        let input = vec![0.1; 10];
        let output = mlp.forward(&input);

        assert_eq!(output.len(), 5);
        for &o in &output {
            assert!(o.is_finite());
        }
    }

    #[test]
    fn test_simple_mlp_params() {
        let mlp = SimpleMLP::new(10, 20, 5, 42);
        let params = mlp.get_params();

        // Should have (10*20 + 20) + (20*5 + 5) = 220 + 105 = 325 params
        assert_eq!(params.len(), 325);
        assert_eq!(mlp.num_params(), 325);
    }
}
