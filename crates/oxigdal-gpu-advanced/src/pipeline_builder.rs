//! GPU compute pipeline builder with fluent API.
//!
//! This module provides a fluent, composable API for building complex GPU compute
//! pipelines with automatic optimization and validation.

use crate::error::{GpuAdvancedError, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::{BindGroupLayout, ComputePipeline, Device};

/// Fluent builder for GPU compute pipelines
pub struct PipelineBuilder {
    device: Arc<Device>,
    stages: Vec<PipelineStage>,
    bind_group_layouts: Vec<BindGroupLayout>,
    cache: Arc<RwLock<PipelineCache>>,
    config: PipelineConfig,
}

impl PipelineBuilder {
    /// Create a new pipeline builder
    pub fn new(device: Arc<Device>) -> Self {
        Self {
            device,
            stages: Vec::new(),
            bind_group_layouts: Vec::new(),
            cache: Arc::new(RwLock::new(PipelineCache::new())),
            config: PipelineConfig::default(),
        }
    }

    /// Add a compute stage to the pipeline
    pub fn add_stage(mut self, stage: PipelineStage) -> Self {
        self.stages.push(stage);
        self
    }

    /// Add a bind group layout
    pub fn add_bind_group_layout(mut self, layout: BindGroupLayout) -> Self {
        self.bind_group_layouts.push(layout);
        self
    }

    /// Set pipeline configuration
    pub fn with_config(mut self, config: PipelineConfig) -> Self {
        self.config = config;
        self
    }

    /// Enable pipeline caching
    pub fn with_cache(mut self, cache: Arc<RwLock<PipelineCache>>) -> Self {
        self.cache = cache;
        self
    }

    /// Build the pipeline
    pub fn build(mut self) -> Result<Pipeline> {
        if self.stages.is_empty() {
            return Err(GpuAdvancedError::ConfigError(
                "Pipeline must have at least one stage".to_string(),
            ));
        }

        // Validate pipeline
        self.validate()?;

        // Optimize if enabled (do this before moving bind_group_layouts)
        if self.config.optimize {
            let optimized_stages = self.optimize_stages_immutable(&self.stages)?;
            self.stages = optimized_stages;
        }

        // Extract owned values from self after optimization
        let device = self.device;
        let cache = self.cache;
        let config = self.config;
        let bind_group_layouts = self.bind_group_layouts;
        let stages = self.stages;

        // Build compute pipelines for each stage
        let mut compute_pipelines = Vec::new();
        for stage in &stages {
            // Recreate necessary context for building pipelines
            let layout_refs: Vec<&BindGroupLayout> = bind_group_layouts
                .iter()
                .take(stage.bind_group_count)
                .collect();

            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&format!("shader_{}", stage.label)),
                source: wgpu::ShaderSource::Wgsl(stage.shader_source.as_str().into()),
            });

            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(&format!("layout_{}", stage.label)),
                bind_group_layouts: &layout_refs,
                immediate_size: 0,
            });

            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(&stage.label),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(&stage.entry_point),
                compilation_options: Default::default(),
                cache: None,
            });

            compute_pipelines.push(Arc::new(pipeline));
        }

        Ok(Pipeline {
            device,
            stages,
            compute_pipelines,
            cache,
            config,
        })
    }

    /// Validate pipeline configuration
    fn validate(&self) -> Result<()> {
        // Check for circular dependencies
        for (i, stage) in self.stages.iter().enumerate() {
            for dep in &stage.dependencies {
                if *dep >= i {
                    return Err(GpuAdvancedError::ConfigError(format!(
                        "Stage {} has forward or circular dependency on stage {}",
                        i, dep
                    )));
                }
            }
        }

        // Validate bind group layouts
        for stage in &self.stages {
            if stage.bind_group_count > self.bind_group_layouts.len() {
                return Err(GpuAdvancedError::ConfigError(format!(
                    "Stage '{}' requires {} bind groups but only {} layouts provided",
                    stage.label,
                    stage.bind_group_count,
                    self.bind_group_layouts.len()
                )));
            }
        }

        Ok(())
    }

    /// Optimize pipeline stages (immutable version)
    fn optimize_stages_immutable(&self, stages: &[PipelineStage]) -> Result<Vec<PipelineStage>> {
        // Simple optimization: merge compatible stages
        let mut optimized = Vec::new();
        let mut skip_next = false;

        for i in 0..stages.len() {
            if skip_next {
                skip_next = false;
                continue;
            }

            let stage = &stages[i];

            // Check if we can merge with next stage
            if i + 1 < stages.len() && self.can_merge_stages(stage, &stages[i + 1]) {
                let merged = self.merge_stages(stage, &stages[i + 1])?;
                optimized.push(merged);
                skip_next = true;
            } else {
                optimized.push(stage.clone());
            }
        }

        Ok(optimized)
    }

    /// Check if two stages can be merged
    fn can_merge_stages(&self, _stage1: &PipelineStage, _stage2: &PipelineStage) -> bool {
        // Simple heuristic: can merge if both stages use same shader
        // and have no dependencies
        false // Conservative: don't merge for now
    }

    /// Merge two stages
    fn merge_stages(
        &self,
        stage1: &PipelineStage,
        stage2: &PipelineStage,
    ) -> Result<PipelineStage> {
        // Create a new stage that combines both
        Ok(PipelineStage {
            label: format!("{}_merged_{}", stage1.label, stage2.label),
            shader_source: stage1.shader_source.clone(),
            entry_point: stage1.entry_point.clone(),
            workgroup_size: stage1.workgroup_size,
            bind_group_count: stage1.bind_group_count.max(stage2.bind_group_count),
            dependencies: stage1.dependencies.clone(),
        })
    }

    /// Build a compute pipeline for a stage
    #[allow(dead_code)]
    fn build_compute_pipeline(&self, stage: &PipelineStage) -> Result<Arc<ComputePipeline>> {
        // Check cache first
        let cache_key = self.compute_cache_key(stage);
        {
            let cache = self.cache.read();
            if let Some(pipeline) = cache.get(&cache_key) {
                return Ok(pipeline);
            }
        }

        // Create shader module
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&format!("shader_{}", stage.label)),
                source: wgpu::ShaderSource::Wgsl(stage.shader_source.as_str().into()),
            });

        // Create pipeline layout
        let layout_refs: Vec<&BindGroupLayout> = self
            .bind_group_layouts
            .iter()
            .take(stage.bind_group_count)
            .collect();

        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(&format!("layout_{}", stage.label)),
                bind_group_layouts: &layout_refs,
                immediate_size: 0,
            });

        // Create compute pipeline
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(&stage.label),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(&stage.entry_point),
                compilation_options: Default::default(),
                cache: None,
            });

        let pipeline = Arc::new(pipeline);

        // Cache the pipeline
        {
            let mut cache = self.cache.write();
            cache.insert(cache_key, pipeline.clone());
        }

        Ok(pipeline)
    }

    /// Compute cache key for a stage
    #[allow(dead_code)]
    fn compute_cache_key(&self, stage: &PipelineStage) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        stage.shader_source.hash(&mut hasher);
        stage.entry_point.hash(&mut hasher);
        stage.bind_group_count.hash(&mut hasher);

        format!("{}_{}", stage.label, hasher.finish())
    }
}

/// A stage in the compute pipeline
#[derive(Debug, Clone)]
pub struct PipelineStage {
    /// Stage label
    pub label: String,
    /// Shader source code
    pub shader_source: String,
    /// Entry point function name
    pub entry_point: String,
    /// Workgroup size (x, y, z)
    pub workgroup_size: (u32, u32, u32),
    /// Number of bind groups
    pub bind_group_count: usize,
    /// Dependencies (indices of stages that must execute before this one)
    pub dependencies: Vec<usize>,
}

impl PipelineStage {
    /// Create a new pipeline stage
    pub fn new(
        label: impl Into<String>,
        shader_source: impl Into<String>,
        entry_point: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            shader_source: shader_source.into(),
            entry_point: entry_point.into(),
            workgroup_size: (8, 8, 1),
            bind_group_count: 1,
            dependencies: Vec::new(),
        }
    }

    /// Set workgroup size
    pub fn with_workgroup_size(mut self, x: u32, y: u32, z: u32) -> Self {
        self.workgroup_size = (x, y, z);
        self
    }

    /// Set bind group count
    pub fn with_bind_groups(mut self, count: usize) -> Self {
        self.bind_group_count = count;
        self
    }

    /// Add a dependency
    pub fn depends_on(mut self, stage_index: usize) -> Self {
        self.dependencies.push(stage_index);
        self
    }
}

/// Pipeline configuration
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Enable pipeline optimization
    pub optimize: bool,
    /// Enable pipeline caching
    pub cache: bool,
    /// Maximum stages per pipeline
    pub max_stages: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            optimize: true,
            cache: true,
            max_stages: 16,
        }
    }
}

/// Built pipeline ready for execution
pub struct Pipeline {
    device: Arc<Device>,
    stages: Vec<PipelineStage>,
    compute_pipelines: Vec<Arc<ComputePipeline>>,
    /// Pipeline cache for compiled shaders (reserved for cache management)
    #[allow(dead_code)]
    cache: Arc<RwLock<PipelineCache>>,
    config: PipelineConfig,
}

impl Pipeline {
    /// Get the device
    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Get number of stages
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }

    /// Get a stage by index
    pub fn get_stage(&self, index: usize) -> Option<&PipelineStage> {
        self.stages.get(index)
    }

    /// Get compute pipeline for a stage
    pub fn get_compute_pipeline(&self, index: usize) -> Option<&ComputePipeline> {
        self.compute_pipelines.get(index).map(|p| p.as_ref())
    }

    /// Get all compute pipelines
    pub fn compute_pipelines(&self) -> &[Arc<ComputePipeline>] {
        &self.compute_pipelines
    }

    /// Get pipeline info
    pub fn info(&self) -> PipelineInfo {
        PipelineInfo {
            stage_count: self.stages.len(),
            total_bind_groups: self.stages.iter().map(|s| s.bind_group_count).sum(),
            optimized: self.config.optimize,
            cached: self.config.cache,
        }
    }

    /// Visualize pipeline structure
    pub fn visualize(&self) -> String {
        let mut output = String::from("Pipeline Structure:\n");

        for (i, stage) in self.stages.iter().enumerate() {
            output.push_str(&format!("  Stage {}: {}\n", i, stage.label));
            output.push_str(&format!("    Entry: {}\n", stage.entry_point));
            output.push_str(&format!(
                "    Workgroup: {}x{}x{}\n",
                stage.workgroup_size.0, stage.workgroup_size.1, stage.workgroup_size.2
            ));
            output.push_str(&format!("    Bind groups: {}\n", stage.bind_group_count));

            if !stage.dependencies.is_empty() {
                output.push_str(&format!("    Dependencies: {:?}\n", stage.dependencies));
            }
        }

        output
    }
}

/// Pipeline information
#[derive(Debug, Clone)]
pub struct PipelineInfo {
    /// Number of stages
    pub stage_count: usize,
    /// Total bind groups across all stages
    pub total_bind_groups: usize,
    /// Whether pipeline was optimized
    pub optimized: bool,
    /// Whether pipeline is cached
    pub cached: bool,
}

/// Pipeline cache for compiled pipelines
pub struct PipelineCache {
    pipelines: HashMap<String, Arc<ComputePipeline>>,
    max_size: usize,
}

impl PipelineCache {
    /// Create a new pipeline cache
    pub fn new() -> Self {
        Self {
            pipelines: HashMap::new(),
            max_size: 128,
        }
    }

    /// Get a cached pipeline by key
    pub fn get(&self, key: &str) -> Option<Arc<ComputePipeline>> {
        self.pipelines.get(key).cloned()
    }

    /// Insert a pipeline into the cache
    pub fn insert(&mut self, key: String, pipeline: Arc<ComputePipeline>) {
        if self.pipelines.len() >= self.max_size {
            // Simple eviction: remove first entry
            if let Some(first_key) = self.pipelines.keys().next().cloned() {
                self.pipelines.remove(&first_key);
            }
        }
        self.pipelines.insert(key, pipeline);
    }

    /// Clear all cached pipelines
    pub fn clear(&mut self) {
        self.pipelines.clear();
    }

    /// Get the number of cached pipelines
    pub fn size(&self) -> usize {
        self.pipelines.len()
    }
}

impl Default for PipelineCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_stage_creation() {
        let stage = PipelineStage::new("test", "shader code", "main")
            .with_workgroup_size(16, 16, 1)
            .with_bind_groups(2);

        assert_eq!(stage.label, "test");
        assert_eq!(stage.workgroup_size, (16, 16, 1));
        assert_eq!(stage.bind_group_count, 2);
    }

    #[test]
    fn test_pipeline_cache() {
        let cache = PipelineCache::new();
        assert_eq!(cache.size(), 0);

        // Cannot test actual pipeline insertion without a device
        // but we can test the cache logic
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_pipeline_config() {
        let config = PipelineConfig::default();
        assert!(config.optimize);
        assert!(config.cache);
        assert_eq!(config.max_stages, 16);
    }
}
