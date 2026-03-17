# TODO: oxigdal-ml-foundation

## High Priority
- [ ] Implement Pure Rust forward pass for UNet architecture with actual tensor operations
- [ ] Add gradient computation and backpropagation engine using SciRS2-Core
- [ ] Implement Adam/AdamW optimizer with weight decay and gradient clipping
- [ ] Add data pipeline with parallel prefetching and on-the-fly augmentation
- [ ] Implement checkpoint saving/loading for training resumption
- [ ] Connect training loop to actual loss computation (CrossEntropy, Dice, Focal)

## Medium Priority
- [ ] Implement ResNet backbone with pretrained weight loading from ONNX
- [ ] Add mixed-precision training support (FP16 forward, FP32 gradients)
- [ ] Implement learning rate schedulers (cosine annealing, warmup, step decay)
- [ ] Add distributed data-parallel training across multiple devices
- [ ] Implement feature pyramid network (FPN) for multi-scale detection
- [ ] Add evaluation metrics computation (mIoU, F1, precision, recall) with confusion matrix
- [ ] Implement early stopping with patience and model selection by validation metric
- [ ] Add geospatial-specific augmentation (random CRS jitter, spectral band dropout)

## Low Priority / Future
- [ ] Implement attention mechanisms (self-attention, cross-attention) for transformer models
- [ ] Add model export to ONNX format from trained Pure Rust models
- [ ] Implement Vision Transformer (ViT) architecture for scene classification
- [ ] Add curriculum learning scheduler for progressive difficulty training
- [ ] Implement Segment Anything Model (SAM) adapter for interactive segmentation
- [ ] Add neural architecture search (NAS) for geospatial model optimization
- [ ] Implement contrastive learning pretraining for satellite image representations
