//! # Trainable Neural Backend (Pure Rust, scirs2-neural)
//!
//! This module provides a concrete [`MLBackend`] implementation
//! built from real [`scirs2_neural`] layers whose `backward`/`update` methods
//! perform genuine gradient computation and parameter updates. Composing these
//! leaf layers into a small module tree makes the entire
//! [`Trainer`](crate::training::training_loop::Trainer) machinery
//! (checkpointing, schedulers, early stopping, losses) reachable and functional:
//! `forward` produces predictions, `backward` propagates gradients through every
//! learnable layer, and `optimizer_step` applies them, so training actually
//! reduces the loss.
//!
//! ## Why a hand-written module tree
//!
//! scirs2-neural 0.6.1 ships `UNet`/`ResNet` *architecture* types, but their
//! `Layer::backward` implementations are gradient pass-throughs
//! (`Ok(grad_output.clone())`) — they do not backpropagate into their internal
//! layers, so wrapping them would yield a model that never learns. The leaf
//! layers ([`Conv2D`], [`Dense`], [`MaxPool2D`], [`GlobalAvgPool2D`]) *do*
//! implement real backward passes and cache their own inputs on `forward`.
//! We therefore compose those leaves ourselves and route gradients (including
//! residual sums and U-Net skip concatenations) explicitly, which is genuine
//! end-to-end backpropagation.
//!
//! ## Known limitations (documented, not silent)
//!
//! - Batch normalization is omitted: scirs2-neural 0.6.1's `BatchNorm`
//!   `Layer::forward` is an identity pass-through, so including it would add a
//!   non-functional layer. Convolutional blocks use Conv → ReLU.
//! - Bottleneck ResNet variants (50/101/152) are constructed from basic
//!   residual blocks (the block schedule and channel widths still follow the
//!   requested variant). This is a documented approximation, not fabricated
//!   output.
//! - ONNX export of live weights is not wired here; [`MLBackend::export_onnx`]
//!   returns an explicit `NotImplemented` error rather than writing a placeholder file.

use crate::error::{Error, Result};
use scirs2_core::ndarray::{Array, Axis, IxDyn, s};
use scirs2_core::random::{RngExt, SeedableRng};
use scirs2_neural::layers::conv::PaddingMode;
use scirs2_neural::layers::{Conv2D, Dense, GlobalAvgPool2D, Layer, MaxPool2D};

use super::{BackendConfig, MLBackend};

/// Dense f32 tensor used throughout the backend.
type Tensor = Array<f32, IxDyn>;

/// A composable, trainable network module.
///
/// Every implementor performs a real forward pass (caching whatever it needs
/// for the backward pass) and a real backward pass that both returns the
/// gradient w.r.t. its input and stores parameter gradients for [`Module::update`].
trait Module: Send + Sync {
    /// Forward pass.
    fn forward(&self, x: &Tensor) -> Result<Tensor>;

    /// Backward pass. Returns the gradient w.r.t. this module's input and
    /// stores parameter gradients internally for the next [`Module::update`].
    fn backward(&self, grad_output: &Tensor) -> Result<Tensor>;

    /// Apply the last-computed gradients with the given learning rate.
    fn update(&mut self, learning_rate: f32) -> Result<()>;

    /// Append this module's parameter tensors (in a stable order) to `out`.
    fn collect_params(&self, out: &mut Vec<Tensor>);

    /// Consume parameter tensors from `iter` (same order as [`collect_params`]).
    fn load_params(&mut self, iter: &mut std::vec::IntoIter<Tensor>) -> Result<()>;

    /// Enable or disable training mode.
    fn set_training(&mut self, training: bool);

    /// Number of scalar learnable parameters.
    fn num_parameters(&self) -> usize;

    /// Number of independently addressable top-level layers. A leaf or a
    /// composite that is treated as a single unit reports `1`; a [`Sequential`]
    /// reports its child count. This is the granularity at which layer-wise
    /// learning rates / freezing are applied.
    fn num_top_layers(&self) -> usize {
        1
    }

    /// Apply the last-computed gradients to a single top-level layer with a
    /// per-layer learning rate. A learning rate of `0.0` freezes the layer (its
    /// parameters are left unchanged for this step).
    ///
    /// The default treats the whole module as one layer (`layer_idx` must be 0).
    fn update_top_layer(&mut self, layer_idx: usize, learning_rate: f32) -> Result<()> {
        if layer_idx != 0 {
            return Err(Error::invalid_parameter(
                "layer_idx",
                layer_idx,
                "module exposes a single top-level layer",
            ));
        }
        self.update(learning_rate)
    }
}

/// Leaf module wrapping a single scirs2-neural [`Layer`].
struct LeafModule {
    layer: Box<dyn Layer<f32> + Send + Sync>,
}

impl LeafModule {
    fn new(layer: Box<dyn Layer<f32> + Send + Sync>) -> Self {
        Self { layer }
    }
}

impl Module for LeafModule {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        self.layer
            .forward(x)
            .map_err(|e| Error::Backend(format!("layer forward failed: {}", e)))
    }

    fn backward(&self, grad_output: &Tensor) -> Result<Tensor> {
        // Every leaf layer used here caches its own forward input and ignores
        // the `input` argument, so a zero-sized placeholder is sufficient.
        let placeholder = Array::zeros(IxDyn(&[1]));
        self.layer
            .backward(&placeholder, grad_output)
            .map_err(|e| Error::Backend(format!("layer backward failed: {}", e)))
    }

    fn update(&mut self, learning_rate: f32) -> Result<()> {
        self.layer
            .update(learning_rate)
            .map_err(|e| Error::Backend(format!("layer update failed: {}", e)))
    }

    fn collect_params(&self, out: &mut Vec<Tensor>) {
        out.extend(self.layer.params());
    }

    fn load_params(&mut self, iter: &mut std::vec::IntoIter<Tensor>) -> Result<()> {
        let n = self.layer.params().len();
        if n == 0 {
            return Ok(());
        }
        let mut buf = Vec::with_capacity(n);
        for _ in 0..n {
            let t = iter.next().ok_or_else(|| {
                Error::Backend("weight file has fewer tensors than the model".to_string())
            })?;
            buf.push(t);
        }
        self.layer
            .set_params(&buf)
            .map_err(|e| Error::Backend(format!("layer set_params failed: {}", e)))
    }

    fn set_training(&mut self, training: bool) {
        self.layer.set_training(training);
    }

    fn num_parameters(&self) -> usize {
        self.layer.parameter_count()
    }
}

/// ReLU activation with a cached positivity mask for the backward pass.
struct Relu {
    mask: std::sync::RwLock<Option<Tensor>>,
}

impl Relu {
    fn new() -> Self {
        Self {
            mask: std::sync::RwLock::new(None),
        }
    }
}

impl Module for Relu {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let mask = x.mapv(|v| if v > 0.0 { 1.0f32 } else { 0.0 });
        let out = x.mapv(|v| v.max(0.0));
        *self
            .mask
            .write()
            .map_err(|_| Error::Backend("relu mask lock poisoned".to_string()))? = Some(mask);
        Ok(out)
    }

    fn backward(&self, grad_output: &Tensor) -> Result<Tensor> {
        let guard = self
            .mask
            .read()
            .map_err(|_| Error::Backend("relu mask lock poisoned".to_string()))?;
        let mask = guard
            .as_ref()
            .ok_or_else(|| Error::Backend("relu backward before forward".to_string()))?;
        if mask.shape() != grad_output.shape() {
            return Err(Error::invalid_dimensions(
                format!("{:?}", mask.shape()),
                format!("{:?}", grad_output.shape()),
            ));
        }
        Ok(grad_output * mask)
    }

    fn update(&mut self, _learning_rate: f32) -> Result<()> {
        Ok(())
    }

    fn collect_params(&self, _out: &mut Vec<Tensor>) {}

    fn load_params(&mut self, _iter: &mut std::vec::IntoIter<Tensor>) -> Result<()> {
        Ok(())
    }

    fn set_training(&mut self, _training: bool) {}

    fn num_parameters(&self) -> usize {
        0
    }
}

/// Flatten `[N, C, 1, 1]` (or `[N, C, H, W]`) to `[N, C*H*W]` for a dense head.
struct Flatten {
    input_shape: std::sync::RwLock<Option<Vec<usize>>>,
}

impl Flatten {
    fn new() -> Self {
        Self {
            input_shape: std::sync::RwLock::new(None),
        }
    }
}

impl Module for Flatten {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let shape = x.shape().to_vec();
        if shape.is_empty() {
            return Err(Error::invalid_dimensions("non-empty tensor", "scalar"));
        }
        let batch = shape[0];
        let features: usize = shape[1..].iter().product();
        *self
            .input_shape
            .write()
            .map_err(|_| Error::Backend("flatten lock poisoned".to_string()))? = Some(shape);
        x.clone()
            .into_shape_with_order(IxDyn(&[batch, features]))
            .map_err(|e| Error::Backend(format!("flatten reshape failed: {}", e)))
    }

    fn backward(&self, grad_output: &Tensor) -> Result<Tensor> {
        let guard = self
            .input_shape
            .read()
            .map_err(|_| Error::Backend("flatten lock poisoned".to_string()))?;
        let shape = guard
            .as_ref()
            .ok_or_else(|| Error::Backend("flatten backward before forward".to_string()))?;
        grad_output
            .clone()
            .into_shape_with_order(IxDyn(shape))
            .map_err(|e| Error::Backend(format!("flatten backward reshape failed: {}", e)))
    }

    fn update(&mut self, _learning_rate: f32) -> Result<()> {
        Ok(())
    }

    fn collect_params(&self, _out: &mut Vec<Tensor>) {}

    fn load_params(&mut self, _iter: &mut std::vec::IntoIter<Tensor>) -> Result<()> {
        Ok(())
    }

    fn set_training(&mut self, _training: bool) {}

    fn num_parameters(&self) -> usize {
        0
    }
}

/// Nearest-neighbour spatial upsampling by an integer factor (NCHW).
struct Upsample {
    scale: usize,
}

impl Upsample {
    fn new(scale: usize) -> Self {
        Self { scale }
    }
}

impl Module for Upsample {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        scirs2_neural::tensor_ops::upsample_nearest(x, self.scale)
            .map_err(|e| Error::Backend(format!("upsample failed: {}", e)))
    }

    fn backward(&self, grad_output: &Tensor) -> Result<Tensor> {
        // Gradient of nearest upsampling: each source pixel receives the sum of
        // the gradients of the `scale x scale` output pixels it produced.
        let shape = grad_output.shape();
        if shape.len() != 4 {
            return Err(Error::invalid_dimensions(
                "4D [N, C, H, W]",
                format!("{:?}", shape),
            ));
        }
        let (n, c, out_h, out_w) = (shape[0], shape[1], shape[2], shape[3]);
        let in_h = out_h / self.scale;
        let in_w = out_w / self.scale;
        let mut grad_input = Array::<f32, _>::zeros(IxDyn(&[n, c, in_h, in_w]));
        for b in 0..n {
            for ch in 0..c {
                for oh in 0..out_h {
                    for ow in 0..out_w {
                        let ih = oh / self.scale;
                        let iw = ow / self.scale;
                        grad_input[[b, ch, ih, iw]] += grad_output[[b, ch, oh, ow]];
                    }
                }
            }
        }
        Ok(grad_input)
    }

    fn update(&mut self, _learning_rate: f32) -> Result<()> {
        Ok(())
    }

    fn collect_params(&self, _out: &mut Vec<Tensor>) {}

    fn load_params(&mut self, _iter: &mut std::vec::IntoIter<Tensor>) -> Result<()> {
        Ok(())
    }

    fn set_training(&mut self, _training: bool) {}

    fn num_parameters(&self) -> usize {
        0
    }
}

/// Sequential container: forward chains, backward reverse-chains.
struct Sequential {
    modules: Vec<Box<dyn Module>>,
}

impl Sequential {
    fn new(modules: Vec<Box<dyn Module>>) -> Self {
        Self { modules }
    }
}

impl Module for Sequential {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let mut out = x.clone();
        for m in &self.modules {
            out = m.forward(&out)?;
        }
        Ok(out)
    }

    fn backward(&self, grad_output: &Tensor) -> Result<Tensor> {
        let mut grad = grad_output.clone();
        for m in self.modules.iter().rev() {
            grad = m.backward(&grad)?;
        }
        Ok(grad)
    }

    fn update(&mut self, learning_rate: f32) -> Result<()> {
        for m in &mut self.modules {
            m.update(learning_rate)?;
        }
        Ok(())
    }

    fn collect_params(&self, out: &mut Vec<Tensor>) {
        for m in &self.modules {
            m.collect_params(out);
        }
    }

    fn load_params(&mut self, iter: &mut std::vec::IntoIter<Tensor>) -> Result<()> {
        for m in &mut self.modules {
            m.load_params(iter)?;
        }
        Ok(())
    }

    fn set_training(&mut self, training: bool) {
        for m in &mut self.modules {
            m.set_training(training);
        }
    }

    fn num_parameters(&self) -> usize {
        self.modules.iter().map(|m| m.num_parameters()).sum()
    }

    fn num_top_layers(&self) -> usize {
        self.modules.len()
    }

    fn update_top_layer(&mut self, layer_idx: usize, learning_rate: f32) -> Result<()> {
        let len = self.modules.len();
        let module = self.modules.get_mut(layer_idx).ok_or_else(|| {
            Error::invalid_parameter(
                "layer_idx",
                layer_idx,
                format!("out of range for {} top-level layers", len),
            )
        })?;
        module.update(learning_rate)
    }
}

/// Residual block: `out = body(x) + shortcut(x)`.
struct ResidualBlock {
    body: Box<dyn Module>,
    /// Optional projection for when the body changes shape (channels/stride).
    /// `None` means an identity shortcut.
    shortcut: Option<Box<dyn Module>>,
}

impl Module for ResidualBlock {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let body = self.body.forward(x)?;
        let short = match &self.shortcut {
            Some(sc) => sc.forward(x)?,
            None => x.clone(),
        };
        if body.shape() != short.shape() {
            return Err(Error::invalid_dimensions(
                format!("residual body {:?}", body.shape()),
                format!("shortcut {:?}", short.shape()),
            ));
        }
        Ok(body + short)
    }

    fn backward(&self, grad_output: &Tensor) -> Result<Tensor> {
        // out = body(x) + shortcut(x)  =>  dL/dx = body'(g) + shortcut'(g)
        let grad_body = self.body.backward(grad_output)?;
        let grad_short = match &self.shortcut {
            Some(sc) => sc.backward(grad_output)?,
            None => grad_output.clone(),
        };
        Ok(grad_body + grad_short)
    }

    fn update(&mut self, learning_rate: f32) -> Result<()> {
        self.body.update(learning_rate)?;
        if let Some(sc) = &mut self.shortcut {
            sc.update(learning_rate)?;
        }
        Ok(())
    }

    fn collect_params(&self, out: &mut Vec<Tensor>) {
        self.body.collect_params(out);
        if let Some(sc) = &self.shortcut {
            sc.collect_params(out);
        }
    }

    fn load_params(&mut self, iter: &mut std::vec::IntoIter<Tensor>) -> Result<()> {
        self.body.load_params(iter)?;
        if let Some(sc) = &mut self.shortcut {
            sc.load_params(iter)?;
        }
        Ok(())
    }

    fn set_training(&mut self, training: bool) {
        self.body.set_training(training);
        if let Some(sc) = &mut self.shortcut {
            sc.set_training(training);
        }
    }

    fn num_parameters(&self) -> usize {
        self.body.num_parameters()
            + self
                .shortcut
                .as_ref()
                .map(|s| s.num_parameters())
                .unwrap_or(0)
    }
}

/// One U-Net level: encode → pool → (deeper level) → upsample → concat(skip) → decode.
///
/// Backward routing splits the concatenated gradient back into the skip branch
/// (added to the encoder-output gradient) and the upsampled branch.
struct UNetLevel {
    /// Encoder double-conv producing the skip features.
    encode: Box<dyn Module>,
    /// Downsampling pool.
    pool: Box<dyn Module>,
    /// Deeper level (a nested `UNetLevel` or the bottleneck).
    inner: Box<dyn Module>,
    /// Upsampling of the deeper-level output.
    up: Upsample,
    /// Decoder double-conv operating on the concatenated features.
    decode: Box<dyn Module>,
    /// Channel count of the skip branch (for the concat split in backward).
    skip_channels: usize,
}

impl Module for UNetLevel {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let skip = self.encode.forward(x)?;
        let pooled = self.pool.forward(&skip)?;
        let deep = self.inner.forward(&pooled)?;
        let upsampled = self.up.forward(&deep)?;
        if skip.shape()[0] != upsampled.shape()[0]
            || skip.shape()[2] != upsampled.shape()[2]
            || skip.shape()[3] != upsampled.shape()[3]
        {
            return Err(Error::invalid_dimensions(
                format!("skip {:?}", skip.shape()),
                format!("upsampled {:?}", upsampled.shape()),
            ));
        }
        let concat = scirs2_core::ndarray::concatenate(Axis(1), &[skip.view(), upsampled.view()])
            .map_err(|e| Error::Backend(format!("u-net concat failed: {}", e)))?
            .into_dyn();
        self.decode.forward(&concat)
    }

    fn backward(&self, grad_output: &Tensor) -> Result<Tensor> {
        let grad_concat = self.decode.backward(grad_output)?;
        let total_channels = grad_concat.shape()[1];
        if self.skip_channels > total_channels {
            return Err(Error::invalid_dimensions(
                format!("<= {} channels", total_channels),
                format!("skip_channels {}", self.skip_channels),
            ));
        }
        let grad_skip_from_concat = grad_concat
            .slice(s![.., 0..self.skip_channels, .., ..])
            .to_owned()
            .into_dyn();
        let grad_up = grad_concat
            .slice(s![.., self.skip_channels.., .., ..])
            .to_owned()
            .into_dyn();

        let grad_deep = self.up.backward(&grad_up)?;
        let grad_pooled = self.inner.backward(&grad_deep)?;
        let grad_skip_from_pool = self.pool.backward(&grad_pooled)?;

        if grad_skip_from_concat.shape() != grad_skip_from_pool.shape() {
            return Err(Error::invalid_dimensions(
                format!("{:?}", grad_skip_from_concat.shape()),
                format!("{:?}", grad_skip_from_pool.shape()),
            ));
        }
        let grad_skip = grad_skip_from_concat + grad_skip_from_pool;
        self.encode.backward(&grad_skip)
    }

    fn update(&mut self, learning_rate: f32) -> Result<()> {
        self.encode.update(learning_rate)?;
        self.pool.update(learning_rate)?;
        self.inner.update(learning_rate)?;
        self.up.update(learning_rate)?;
        self.decode.update(learning_rate)?;
        Ok(())
    }

    fn collect_params(&self, out: &mut Vec<Tensor>) {
        self.encode.collect_params(out);
        self.inner.collect_params(out);
        self.decode.collect_params(out);
    }

    fn load_params(&mut self, iter: &mut std::vec::IntoIter<Tensor>) -> Result<()> {
        self.encode.load_params(iter)?;
        self.inner.load_params(iter)?;
        self.decode.load_params(iter)?;
        Ok(())
    }

    fn set_training(&mut self, training: bool) {
        self.encode.set_training(training);
        self.inner.set_training(training);
        self.decode.set_training(training);
    }

    fn num_parameters(&self) -> usize {
        self.encode.num_parameters() + self.inner.num_parameters() + self.decode.num_parameters()
    }
}

// ---------------------------------------------------------------------------
// Layer construction helpers
// ---------------------------------------------------------------------------

/// He-initialised Conv2D (`same` padding) wrapped as a leaf module.
///
/// scirs2-neural's `Conv2D::new` initialises every weight to the same constant
/// (breaking gradient symmetry is impossible), so we overwrite the weights with
/// He-distributed random values via `set_params`.
fn conv_same<R: RngExt>(
    in_ch: usize,
    out_ch: usize,
    kernel: usize,
    stride: usize,
    rng: &mut R,
) -> Result<Box<dyn Module>> {
    let mut conv = Conv2D::<f32>::new(in_ch, out_ch, (kernel, kernel), (stride, stride), None)
        .map_err(|e| Error::Backend(format!("Conv2D::new failed: {}", e)))?
        .with_padding(PaddingMode::Same);

    let fan_in = in_ch * kernel * kernel;
    let scale = (2.0f32 / fan_in as f32).sqrt();
    let weights = Array::from_shape_fn(IxDyn(&[out_ch, in_ch, kernel, kernel]), |_| {
        rng.random_range(-scale..scale)
    });
    let bias = Array::<f32, _>::zeros(IxDyn(&[out_ch]));
    conv.set_params(&[weights, bias])
        .map_err(|e| Error::Backend(format!("Conv2D set_params failed: {}", e)))?;

    Ok(Box::new(LeafModule::new(Box::new(conv))))
}

/// Double convolution block: Conv → ReLU → Conv → ReLU.
fn double_conv<R: RngExt>(
    in_ch: usize,
    out_ch: usize,
    kernel: usize,
    rng: &mut R,
) -> Result<Box<dyn Module>> {
    Ok(Box::new(Sequential::new(vec![
        conv_same(in_ch, out_ch, kernel, 1, rng)?,
        Box::new(Relu::new()),
        conv_same(out_ch, out_ch, kernel, 1, rng)?,
        Box::new(Relu::new()),
    ])))
}

/// A 2x2 stride-2 max-pool leaf module (spatial downsampling by a factor of 2).
fn max_pool_2x2() -> Result<Box<dyn Module>> {
    Ok(Box::new(LeafModule::new(Box::new(
        MaxPool2D::<f32>::new((2, 2), (2, 2), None)
            .map_err(|e| Error::Backend(format!("MaxPool2D::new failed: {}", e)))?,
    ))))
}

/// Basic residual block (two 3x3 convs) with an optional projection shortcut.
///
/// scirs2-neural's `Same` padding is *symmetric*, so a strided 3x3 convolution
/// and a strided 1x1 convolution disagree on their output size (the 3x3 path
/// loses the pixel that only asymmetric padding could keep). Striding the
/// convolutions would therefore make the residual sum `body(x) + shortcut(x)`
/// shape-inconsistent. To keep the two branches aligned we never stride the
/// convolutions: spatial downsampling is performed by a `MaxPool2D` applied
/// identically to both branches, and every convolution keeps stride 1 (which
/// `Same` padding sizes exactly).
fn residual_block<R: RngExt>(
    in_ch: usize,
    out_ch: usize,
    stride: usize,
    rng: &mut R,
) -> Result<Box<dyn Module>> {
    let downsample = stride != 1;

    let mut body_mods: Vec<Box<dyn Module>> = Vec::new();
    if downsample {
        body_mods.push(max_pool_2x2()?);
    }
    body_mods.push(conv_same(in_ch, out_ch, 3, 1, rng)?);
    body_mods.push(Box::new(Relu::new()));
    body_mods.push(conv_same(out_ch, out_ch, 3, 1, rng)?);
    let body = Sequential::new(body_mods);

    // A projection shortcut is required whenever the channel count or spatial
    // resolution changes; otherwise the identity is used. The shortcut pools
    // with the same factor as the body so the two branches stay aligned.
    let shortcut: Option<Box<dyn Module>> = if in_ch != out_ch || downsample {
        let mut sc: Vec<Box<dyn Module>> = Vec::new();
        if downsample {
            sc.push(max_pool_2x2()?);
        }
        sc.push(conv_same(in_ch, out_ch, 1, 1, rng)?);
        Some(Box::new(Sequential::new(sc)))
    } else {
        None
    };
    Ok(Box::new(ResidualBlock {
        body: Box::new(body),
        shortcut,
    }))
}

/// Build a real, trainable U-Net module tree.
fn build_unet(
    in_channels: usize,
    num_classes: usize,
    depth: usize,
    base_filters: usize,
    kernel: usize,
    rng: &mut impl RngExt,
) -> Result<Box<dyn Module>> {
    // Recursively build from the deepest level up.
    fn build_level(
        level: usize,
        depth: usize,
        in_ch: usize,
        base_filters: usize,
        kernel: usize,
        rng: &mut impl RngExt,
    ) -> Result<(Box<dyn Module>, usize)> {
        let ch = base_filters * (1 << level);
        let encode = double_conv(in_ch, ch, kernel, rng)?;
        let pool: Box<dyn Module> = Box::new(LeafModule::new(Box::new(
            MaxPool2D::<f32>::new((2, 2), (2, 2), None)
                .map_err(|e| Error::Backend(format!("MaxPool2D::new failed: {}", e)))?,
        )));

        let (inner, inner_out) = if level + 1 == depth {
            // Bottleneck: a double conv at the coarsest resolution.
            let bottleneck_ch = base_filters * (1 << depth);
            (double_conv(ch, bottleneck_ch, kernel, rng)?, bottleneck_ch)
        } else {
            build_level(level + 1, depth, ch, base_filters, kernel, rng)?
        };

        let up = Upsample::new(2);
        let decode = double_conv(ch + inner_out, ch, kernel, rng)?;

        let module: Box<dyn Module> = Box::new(UNetLevel {
            encode,
            pool,
            inner,
            up,
            decode,
            skip_channels: ch,
        });
        Ok((module, ch))
    }

    if depth == 0 {
        return Err(Error::invalid_parameter("depth", depth, "must be >= 1"));
    }

    let (body, out_ch) = build_level(0, depth, in_channels, base_filters, kernel, rng)?;
    // Final 1x1 conv mapping to the class channels.
    let final_conv = conv_same(out_ch, num_classes, 1, 1, rng)?;
    Ok(Box::new(Sequential::new(vec![body, final_conv])))
}

/// Build a real, trainable ResNet-style classifier module tree.
fn build_resnet(
    in_channels: usize,
    num_classes: usize,
    base_filters: usize,
    blocks_per_stage: &[usize],
    rng: &mut impl RngExt,
) -> Result<Box<dyn Module>> {
    let mut modules: Vec<Box<dyn Module>> = Vec::new();

    // Stem.
    modules.push(conv_same(in_channels, base_filters, 3, 1, rng)?);
    modules.push(Box::new(Relu::new()));

    let mut prev_ch = base_filters;
    for (stage_idx, &num_blocks) in blocks_per_stage.iter().enumerate() {
        let out_ch = base_filters * (1 << stage_idx);
        for block_idx in 0..num_blocks {
            // Downsample at the start of every stage except the first.
            let stride = if block_idx == 0 && stage_idx > 0 {
                2
            } else {
                1
            };
            modules.push(residual_block(prev_ch, out_ch, stride, rng)?);
            modules.push(Box::new(Relu::new()));
            prev_ch = out_ch;
        }
    }

    // Classification head: global average pool → flatten → dense.
    modules.push(Box::new(LeafModule::new(Box::new(
        GlobalAvgPool2D::<f32>::new(None),
    ))));
    modules.push(Box::new(Flatten::new()));
    let dense = Dense::<f32>::new(prev_ch, num_classes, None, rng)
        .map_err(|e| Error::Backend(format!("Dense::new failed: {}", e)))?;
    modules.push(Box::new(LeafModule::new(Box::new(dense))));

    Ok(Box::new(Sequential::new(modules)))
}

// ---------------------------------------------------------------------------
// NeuralBackend
// ---------------------------------------------------------------------------

/// A trainable backend that owns a module tree and implements [`MLBackend`].
pub struct NeuralBackend {
    module: Box<dyn Module>,
    kind: &'static str,
    /// Spatial divisibility requirement of the input (`2^depth` for U-Net, `1`
    /// otherwise). Validated on `forward`.
    spatial_divisor: usize,
    num_params: usize,
    training: bool,
    /// Shape produced by the most recent `forward`, used to reshape the flat
    /// gradient in `backward`.
    last_output_shape: std::sync::RwLock<Option<Vec<usize>>>,
}

impl NeuralBackend {
    /// Build a trainable U-Net backend from a [`UNetConfig`](crate::models::unet::UNetConfig).
    pub fn unet(
        config: &crate::models::unet::UNetConfig,
        _backend_config: &BackendConfig,
    ) -> Result<Self> {
        config.validate()?;
        let mut rng = scirs2_core::random::rngs::StdRng::seed_from_u64(u_net_seed(config));
        let module = build_unet(
            config.in_channels,
            config.num_classes,
            config.depth,
            config.base_filters,
            3,
            &mut rng,
        )?;
        let num_params = module.num_parameters();
        Ok(Self {
            module,
            kind: "UNet",
            spatial_divisor: 1 << config.depth,
            num_params,
            training: true,
            last_output_shape: std::sync::RwLock::new(None),
        })
    }

    /// Build a trainable ResNet backend from a [`ResNetConfig`](crate::models::resnet::ResNetConfig).
    pub fn resnet(
        config: &crate::models::resnet::ResNetConfig,
        _backend_config: &BackendConfig,
    ) -> Result<Self> {
        config.validate()?;
        let blocks = config.variant.blocks_per_stage();
        let mut rng = scirs2_core::random::rngs::StdRng::seed_from_u64(0x5EED_5151);
        let module = build_resnet(
            config.in_channels,
            config.num_classes,
            config.base_filters,
            &blocks,
            &mut rng,
        )?;
        let num_params = module.num_parameters();
        Ok(Self {
            module,
            kind: "ResNet",
            spatial_divisor: 1,
            num_params,
            training: true,
            last_output_shape: std::sync::RwLock::new(None),
        })
    }

    /// Reshape a flat NCHW input into a 4D array, inferring the batch dimension.
    fn to_nchw(&self, input: &[f32], input_shape: &[usize]) -> Result<Tensor> {
        if input_shape.len() != 4 {
            return Err(Error::invalid_dimensions(
                "4D input shape [N, C, H, W]",
                format!("{:?}", input_shape),
            ));
        }
        let (c, h, w) = (input_shape[1], input_shape[2], input_shape[3]);
        let per_sample = c * h * w;
        if per_sample == 0 {
            return Err(Error::invalid_parameter(
                "input_shape",
                format!("{:?}", input_shape),
                "channels/height/width must be > 0",
            ));
        }
        if !input.len().is_multiple_of(per_sample) {
            return Err(Error::invalid_dimensions(
                format!("multiple of {}", per_sample),
                format!("{} elements", input.len()),
            ));
        }
        let n = input.len() / per_sample;
        if self.spatial_divisor > 1
            && (!h.is_multiple_of(self.spatial_divisor) || !w.is_multiple_of(self.spatial_divisor))
        {
            return Err(Error::invalid_parameter(
                "input_spatial",
                format!("{}x{}", h, w),
                format!(
                    "{} requires height/width divisible by {}",
                    self.kind, self.spatial_divisor
                ),
            ));
        }
        Array::from_shape_vec(IxDyn(&[n, c, h, w]), input.to_vec())
            .map_err(|e| Error::Backend(format!("input reshape failed: {}", e)))
    }
}

impl MLBackend for NeuralBackend {
    fn forward(&self, input: &[f32], input_shape: &[usize]) -> Result<Vec<f32>> {
        let x = self.to_nchw(input, input_shape)?;
        let out = self.module.forward(&x)?;
        *self
            .last_output_shape
            .write()
            .map_err(|_| Error::Backend("output-shape lock poisoned".to_string()))? =
            Some(out.shape().to_vec());
        // `iter()` yields elements in row-major logical order regardless of the
        // underlying memory layout, matching the NCHW flattening expected by the
        // training loop.
        Ok(out.iter().copied().collect())
    }

    fn backward(&self, grad_output: &[f32], _grad_shape: &[usize]) -> Result<Vec<f32>> {
        // Reshape the flat gradient to the exact shape produced by `forward`
        // (which may be 2D for the classifier head or 4D for segmentation).
        let shape = {
            let guard = self
                .last_output_shape
                .read()
                .map_err(|_| Error::Backend("output-shape lock poisoned".to_string()))?;
            guard
                .clone()
                .ok_or_else(|| Error::Backend("backward called before forward".to_string()))?
        };
        let expected: usize = shape.iter().product();
        if grad_output.len() != expected {
            return Err(Error::invalid_dimensions(
                format!("{} gradient elements", expected),
                format!("{} elements", grad_output.len()),
            ));
        }
        let grad = Array::from_shape_vec(IxDyn(&shape), grad_output.to_vec())
            .map_err(|e| Error::Backend(format!("gradient reshape failed: {}", e)))?;
        let grad_input = self.module.backward(&grad)?;
        Ok(grad_input.iter().copied().collect())
    }

    fn optimizer_step(&mut self, learning_rate: f32) -> Result<()> {
        self.module.update(learning_rate)
    }

    fn num_layers(&self) -> usize {
        self.module.num_top_layers()
    }

    fn optimizer_step_layerwise(&mut self, layer_lrs: &[f32]) -> Result<()> {
        let expected = self.module.num_top_layers();
        if layer_lrs.len() != expected {
            return Err(Error::invalid_parameter(
                "layer_lrs",
                layer_lrs.len(),
                format!("expected one learning rate per layer ({})", expected),
            ));
        }
        for (idx, &lr) in layer_lrs.iter().enumerate() {
            // A learning rate of 0.0 leaves the layer's parameters unchanged,
            // which realises freezing / gradient masking for that layer.
            self.module.update_top_layer(idx, lr)?;
        }
        Ok(())
    }

    fn zero_grad(&mut self) -> Result<()> {
        // Gradients are freshly recomputed (overwritten) on every `backward`
        // rather than accumulated, so there is nothing to clear here.
        Ok(())
    }

    fn num_parameters(&self) -> usize {
        self.num_params
    }

    fn last_loss(&self) -> Option<f32> {
        // Loss is computed by the training loop's loss function, not the
        // backend, so no value is available here.
        None
    }

    fn set_train_mode(&mut self, train: bool) {
        self.training = train;
        self.module.set_training(train);
    }

    fn save_weights(&self, path: &std::path::Path) -> Result<()> {
        use byteorder::{LittleEndian, WriteBytesExt};
        use std::io::Write;

        let mut params = Vec::new();
        self.module.collect_params(&mut params);

        let mut buf: Vec<u8> = Vec::new();
        // Magic + version + tensor count.
        buf.write_all(b"OGMLW1\0\0")
            .map_err(|e| Error::Checkpoint(format!("write header failed: {}", e)))?;
        buf.write_u64::<LittleEndian>(params.len() as u64)
            .map_err(|e| Error::Checkpoint(format!("write count failed: {}", e)))?;
        for tensor in &params {
            let shape = tensor.shape();
            buf.write_u32::<LittleEndian>(shape.len() as u32)
                .map_err(|e| Error::Checkpoint(format!("write ndim failed: {}", e)))?;
            for &dim in shape {
                buf.write_u64::<LittleEndian>(dim as u64)
                    .map_err(|e| Error::Checkpoint(format!("write dim failed: {}", e)))?;
            }
            for v in tensor.iter() {
                buf.write_f32::<LittleEndian>(*v)
                    .map_err(|e| Error::Checkpoint(format!("write value failed: {}", e)))?;
            }
        }

        std::fs::write(path, &buf)
            .map_err(|e| Error::Checkpoint(format!("failed to write {}: {}", path.display(), e)))
    }

    fn load_weights(&mut self, path: &std::path::Path) -> Result<()> {
        use byteorder::{LittleEndian, ReadBytesExt};
        use std::io::Read;

        let bytes = std::fs::read(path)
            .map_err(|e| Error::Checkpoint(format!("failed to read {}: {}", path.display(), e)))?;
        let mut cursor = std::io::Cursor::new(bytes);

        let mut magic = [0u8; 8];
        cursor
            .read_exact(&mut magic)
            .map_err(|e| Error::Checkpoint(format!("read header failed: {}", e)))?;
        if &magic != b"OGMLW1\0\0" {
            return Err(Error::Checkpoint("invalid weight-file magic".to_string()));
        }
        let count = cursor
            .read_u64::<LittleEndian>()
            .map_err(|e| Error::Checkpoint(format!("read count failed: {}", e)))?;

        let mut tensors: Vec<Tensor> = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let ndim = cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| Error::Checkpoint(format!("read ndim failed: {}", e)))?;
            let mut dims = Vec::with_capacity(ndim as usize);
            for _ in 0..ndim {
                let dim = cursor
                    .read_u64::<LittleEndian>()
                    .map_err(|e| Error::Checkpoint(format!("read dim failed: {}", e)))?;
                dims.push(dim as usize);
            }
            let numel: usize = dims.iter().product();
            let mut data = Vec::with_capacity(numel);
            for _ in 0..numel {
                data.push(
                    cursor
                        .read_f32::<LittleEndian>()
                        .map_err(|e| Error::Checkpoint(format!("read value failed: {}", e)))?,
                );
            }
            let tensor = Array::from_shape_vec(IxDyn(&dims), data)
                .map_err(|e| Error::Checkpoint(format!("tensor reshape failed: {}", e)))?;
            tensors.push(tensor);
        }

        let mut iter = tensors.into_iter();
        self.module.load_params(&mut iter)?;
        Ok(())
    }

    #[cfg(feature = "onnx")]
    fn export_onnx(&self, _path: &std::path::Path) -> Result<()> {
        Err(Error::NotImplemented(
            "ONNX export of live trained weights is not wired for NeuralBackend; \
             use backend::onnx_export to serialize a model architecture from its config"
                .to_string(),
        ))
    }
}

/// Deterministic per-config seed so that two backends built from identical
/// configs are reproducible while different configs differ.
fn u_net_seed(config: &crate::models::unet::UNetConfig) -> u64 {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    for v in [
        config.in_channels,
        config.num_classes,
        config.depth,
        config.base_filters,
    ] {
        seed = seed.wrapping_mul(0x0100_0000_01B3).wrapping_add(v as u64);
    }
    seed
}

#[cfg(test)]
mod tests {
    // NCHW dimensions are written out in full (e.g. `n * 1 * h * w`) to make the
    // tensor layout self-documenting, so the `* 1` factors are intentional.
    #![allow(clippy::identity_op)]
    use super::*;
    use crate::models::resnet::{ResNetConfig, ResNetVariant};
    use crate::models::unet::UNetConfig;

    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn mse_grad(pred: &[f32], target: &[f32]) -> (f64, Vec<f32>) {
        let n = pred.len() as f64;
        let mut loss = 0.0;
        let mut grad = vec![0.0f32; pred.len()];
        for i in 0..pred.len() {
            let d = pred[i] - target[i];
            loss += (d * d) as f64;
            grad[i] = (2.0 / n) as f32 * d;
        }
        (loss / n, grad)
    }

    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn test_resnet_backend_trains() {
        // Tiny ResNet-18 with narrow channels on a fixed toy batch. Real
        // backprop must drive the loss down over a few steps.
        let config = ResNetConfig {
            variant: ResNetVariant::ResNet18,
            in_channels: 1,
            num_classes: 2,
            base_filters: 2,
            ..Default::default()
        };
        let mut backend = NeuralBackend::resnet(&config, &BackendConfig::default())
            .expect("failed to build resnet backend");
        assert!(backend.num_parameters() > 0);

        let n = 2usize;
        let (c, h, w) = (1usize, 8usize, 8usize);
        let input: Vec<f32> = (0..n * c * h * w)
            .map(|i| ((i % 7) as f32) / 7.0 - 0.5)
            .collect();
        let input_shape = vec![1, c, h, w];
        // One-hot-ish targets: sample 0 -> class 0, sample 1 -> class 1.
        let target = vec![1.0f32, 0.0, 0.0, 1.0];

        let out = backend
            .forward(&input, &input_shape)
            .expect("forward failed");
        assert_eq!(out.len(), n * 2);
        let (loss0, _) = mse_grad(&out, &target);

        let mut last_loss = loss0;
        for _ in 0..25 {
            let out = backend
                .forward(&input, &input_shape)
                .expect("forward failed");
            let (loss, grad) = mse_grad(&out, &target);
            backend.backward(&grad, &[]).expect("backward failed");
            backend.optimizer_step(0.05).expect("step failed");
            backend.zero_grad().expect("zero_grad failed");
            last_loss = loss;
        }
        assert!(
            last_loss < loss0,
            "resnet loss did not decrease: {} -> {}",
            loss0,
            last_loss
        );
    }

    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn test_unet_backend_trains() {
        // Tiny U-Net (depth 2) on a toy segmentation batch.
        let config = UNetConfig {
            in_channels: 1,
            num_classes: 1,
            base_filters: 2,
            depth: 2,
            ..Default::default()
        };
        let mut backend = NeuralBackend::unet(&config, &BackendConfig::default())
            .expect("failed to build unet backend");
        assert!(backend.num_parameters() > 0);

        let n = 1usize;
        let (c, h, w) = (1usize, 8usize, 8usize);
        let input: Vec<f32> = (0..n * c * h * w).map(|i| (i as f32 % 5.0) / 5.0).collect();
        let input_shape = vec![1, c, h, w];
        // Target segmentation: a constant map the network can fit.
        let target = vec![0.7f32; n * 1 * h * w];

        let out = backend
            .forward(&input, &input_shape)
            .expect("forward failed");
        assert_eq!(out.len(), n * 1 * h * w);
        let (loss0, _) = mse_grad(&out, &target);

        let mut last_loss = loss0;
        for _ in 0..20 {
            let out = backend
                .forward(&input, &input_shape)
                .expect("forward failed");
            let (loss, grad) = mse_grad(&out, &target);
            backend.backward(&grad, &[]).expect("backward failed");
            backend.optimizer_step(0.02).expect("step failed");
            last_loss = loss;
        }
        assert!(
            last_loss < loss0,
            "unet loss did not decrease: {} -> {}",
            loss0,
            last_loss
        );
    }

    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn test_unet_requires_divisible_spatial() {
        let config = UNetConfig {
            in_channels: 1,
            num_classes: 1,
            base_filters: 2,
            depth: 2,
            ..Default::default()
        };
        let backend = NeuralBackend::unet(&config, &BackendConfig::default())
            .expect("failed to build unet backend");
        // 8 is not divisible by 2^2=4? 8 % 4 == 0, so use 6 which is not.
        let input = vec![0.0f32; 1 * 6 * 6];
        let result = backend.forward(&input, &[1, 1, 6, 6]);
        assert!(result.is_err(), "non-divisible spatial dims must error");
    }

    /// Flatten every parameter of a backend's module tree into one vector.
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn all_params(backend: &NeuralBackend) -> Vec<f32> {
        let mut params = Vec::new();
        backend.module.collect_params(&mut params);
        params
            .into_iter()
            .flat_map(|t| t.into_iter().collect::<Vec<_>>())
            .collect()
    }

    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn test_layerwise_zero_lr_freezes_all() {
        // A layer-wise step with every learning rate 0.0 must leave the model
        // completely unchanged (full freeze / gradient masking).
        let config = ResNetConfig {
            variant: ResNetVariant::ResNet18,
            in_channels: 1,
            num_classes: 2,
            base_filters: 2,
            ..Default::default()
        };
        let mut backend = NeuralBackend::resnet(&config, &BackendConfig::default())
            .expect("failed to build resnet backend");
        let num_layers = backend.num_layers();
        assert!(
            num_layers > 1,
            "resnet must expose several top-level layers"
        );

        let input: Vec<f32> = (0..1 * 8 * 8)
            .map(|i| (i as f32 % 7.0) / 7.0 - 0.5)
            .collect();
        let input_shape = vec![1, 1, 8, 8];
        let target = vec![1.0f32, 0.0];

        let before = all_params(&backend);

        let out = backend
            .forward(&input, &input_shape)
            .expect("forward failed");
        let (_, grad) = mse_grad(&out, &target);
        backend.backward(&grad, &[]).expect("backward failed");
        // All layers frozen.
        backend
            .optimizer_step_layerwise(&vec![0.0f32; num_layers])
            .expect("layerwise step failed");

        let after_frozen = all_params(&backend);
        assert_eq!(
            before, after_frozen,
            "zero-LR step must not change any weight"
        );

        // Now a non-zero layer-wise step must change the weights.
        let out = backend
            .forward(&input, &input_shape)
            .expect("forward failed");
        let (_, grad) = mse_grad(&out, &target);
        backend.backward(&grad, &[]).expect("backward failed");
        backend
            .optimizer_step_layerwise(&vec![0.1f32; num_layers])
            .expect("layerwise step failed");
        let after_trained = all_params(&backend);
        assert_ne!(
            before, after_trained,
            "non-zero layer-wise step must update the weights"
        );
    }

    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn test_layerwise_wrong_length_errors() {
        let config = ResNetConfig {
            variant: ResNetVariant::ResNet18,
            in_channels: 1,
            num_classes: 2,
            base_filters: 2,
            ..Default::default()
        };
        let mut backend = NeuralBackend::resnet(&config, &BackendConfig::default())
            .expect("failed to build resnet backend");
        // One fewer LR than layers must error rather than silently skip a layer.
        let short = vec![0.1f32; backend.num_layers() - 1];
        assert!(backend.optimizer_step_layerwise(&short).is_err());
    }

    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn test_finetuning_scheduler_drives_backend() {
        // The transfer-learning scheduler must be able to drive a real model:
        // its per-layer learning rates feed the backend's layer-wise step and,
        // while all layers are frozen, the weights do not move.
        use crate::transfer::finetuning::FineTuningScheduler;
        use crate::transfer::{FineTuningConfig, TransferStrategy};

        let config = ResNetConfig {
            variant: ResNetVariant::ResNet18,
            in_channels: 1,
            num_classes: 2,
            base_filters: 2,
            ..Default::default()
        };
        let mut backend = NeuralBackend::resnet(&config, &BackendConfig::default())
            .expect("failed to build resnet backend");
        let num_layers = backend.num_layers();

        // Gradual unfreezing that keeps everything frozen for the first epochs.
        let ft_config = FineTuningConfig {
            strategy: TransferStrategy::GradualUnfreezing,
            base_learning_rate: 0.1,
            epochs_before_unfreeze: 5,
            ..Default::default()
        };
        let scheduler =
            FineTuningScheduler::new(ft_config, num_layers).expect("failed to build scheduler");

        // Epoch 0: all layers frozen -> every learning rate is 0.
        let lrs = scheduler
            .layer_learning_rates(num_layers)
            .expect("layer lrs failed");
        assert_eq!(lrs.len(), num_layers);
        assert!(
            lrs.iter().all(|&lr| lr == 0.0),
            "epoch 0 must freeze all layers"
        );

        let input: Vec<f32> = (0..1 * 8 * 8)
            .map(|i| (i as f32 % 7.0) / 7.0 - 0.5)
            .collect();
        let input_shape = vec![1, 1, 8, 8];
        let target = vec![1.0f32, 0.0];

        let before = all_params(&backend);
        let out = backend
            .forward(&input, &input_shape)
            .expect("forward failed");
        let (_, grad) = mse_grad(&out, &target);
        backend.backward(&grad, &[]).expect("backward failed");
        backend
            .optimizer_step_layerwise(&lrs)
            .expect("layerwise step failed");
        assert_eq!(
            before,
            all_params(&backend),
            "frozen schedule must not move any weight"
        );
    }

    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn test_weight_roundtrip() {
        let config = ResNetConfig {
            variant: ResNetVariant::ResNet18,
            in_channels: 1,
            num_classes: 2,
            base_filters: 2,
            ..Default::default()
        };
        let backend = NeuralBackend::resnet(&config, &BackendConfig::default())
            .expect("failed to build resnet backend");

        let input = vec![0.1f32; 1 * 8 * 8];
        let input_shape = vec![1, 1, 8, 8];
        let before = backend
            .forward(&input, &input_shape)
            .expect("forward failed");

        let path =
            std::env::temp_dir().join(format!("oxigeo_ml_weights_{}.bin", std::process::id()));
        backend.save_weights(&path).expect("save failed");

        let mut restored = NeuralBackend::resnet(&config, &BackendConfig::default())
            .expect("failed to build second backend");
        restored.load_weights(&path).expect("load failed");
        let after = restored
            .forward(&input, &input_shape)
            .expect("forward failed");

        for (a, b) in before.iter().zip(after.iter()) {
            assert!(
                (a - b).abs() < 1e-5,
                "weights did not round-trip: {} vs {}",
                a,
                b
            );
        }
        let _ = std::fs::remove_file(&path);
    }
}
