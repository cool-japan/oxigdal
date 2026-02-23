//! Advanced canvas rendering and viewport management
//!
//! This module provides comprehensive canvas context management, double buffering,
//! progressive rendering, viewport transformations, and zoom/pan optimizations for
//! high-performance geospatial data visualization in the browser.
//!
//! # Overview
//!
//! The rendering module is the core visualization engine, providing:
//!
//! - **Double Buffering**: Eliminates flickering during updates
//! - **Progressive Rendering**: Loads low-res first, then high-res
//! - **Viewport Management**: Pan, zoom, fit-to-bounds operations
//! - **History System**: Undo/redo for viewport changes
//! - **Transform System**: Affine transformations for efficient rendering
//! - **Canvas Buffers**: Off-screen rendering for compositing
//! - **Animation Management**: Frame rate monitoring and adaptive rendering
//! - **Quality Levels**: Dynamic quality adjustment based on performance
//!
//! # Double Buffering
//!
//! Double buffering prevents screen tearing and flickering:
//!
//! ```text
//! Frame N:
//!   Front Buffer: Currently displayed
//!   Back Buffer:  Being updated with new tiles
//!
//! Frame N+1:
//!   Swap buffers
//!   Front Buffer: New frame (was back buffer)
//!   Back Buffer:  Ready for next update
//! ```
//!
//! Benefits:
//! - No partial updates visible to user
//! - Smooth transitions between frames
//! - Allows complex compositing operations
//!
//! # Progressive Rendering
//!
//! Progressive rendering improves perceived performance:
//!
//! 1. **Initial Load**: Show lowest resolution immediately
//! 2. **Progressive Enhancement**: Load higher resolutions incrementally
//! 3. **Final Quality**: Display full resolution when available
//!
//! ```text
//! Time    Level  Resolution  Tiles  Status
//! ────────────────────────────────────────
//! 0ms     4      128x128     1      ▓ Loaded
//! 50ms    3      256x256     4      ▓ Loaded
//! 150ms   2      512x512     16     ▓ Loading...
//! 400ms   1      1024x1024   64     ░ Pending
//! 1000ms  0      2048x2048   256    ░ Pending
//! ```
//!
//! # Viewport Transformations
//!
//! The viewport system handles all coordinate transformations:
//!
//! ## World to Screen
//! Converts world coordinates (tile space) to screen coordinates (pixels):
//! ```text
//! world_x, world_y  →  [Transform Matrix]  →  screen_x, screen_y
//! ```
//!
//! ## Screen to World
//! Converts screen coordinates back to world coordinates:
//! ```text
//! screen_x, screen_y  →  [Inverse Transform]  →  world_x, world_y
//! ```
//!
//! # Transform Matrix
//!
//! The transform is represented as:
//! ```text
//! [ sx  0  tx ]   [ x ]
//! [ 0  sy  ty ] * [ y ]
//! [ 0   0   1 ]   [ 1 ]
//! ```
//!
//! Where:
//! - `sx, sy`: Scale factors (zoom level)
//! - `tx, ty`: Translation (pan offset)
//! - `rotation`: Rotation angle (optional)
//!
//! # Example Usage
//!
//! ```ignore
//! use oxigdal_wasm::rendering::{CanvasRenderer, ViewportState, ViewportTransform};
//!
//! // Create renderer for 800x600 canvas
//! let mut renderer = CanvasRenderer::new(800, 600, 4)?;
//!
//! // Setup viewport to show entire image
//! let mut viewport = ViewportState::new(800, 600);
//! viewport.fit_to_bounds((0.0, 0.0, 4096.0, 4096.0));
//!
//! // Begin rendering frame
//! renderer.begin_frame();
//!
//! // Draw tiles
//! for coord in visible_tiles {
//!     let tile_data = cache.get(&coord)?;
//!     renderer.draw_tile(coord, tile_data, 256, 256)?;
//! }
//!
//! // Swap buffers to display
//! renderer.swap_buffers();
//!
//! // Get as ImageData for canvas
//! let image_data = renderer.front_buffer_image_data()?;
//! ctx.putImageData(&image_data, 0, 0)?;
//! ```
//!
//! # Viewport Operations
//!
//! ## Pan
//! ```rust
//! use oxigdal_wasm::CanvasRenderer;
//! let mut renderer = CanvasRenderer::new(800, 600, 4).expect("Create failed");
//! renderer.pan(100.0, 50.0); // Move right 100px, down 50px
//! ```
//!
//! ## Zoom
//! ```rust
//! use oxigdal_wasm::CanvasRenderer;
//! let mut renderer = CanvasRenderer::new(800, 600, 4).expect("Create failed");
//! renderer.zoom(2.0, 400.0, 300.0); // 2x zoom at center
//! ```
//!
//! ## Fit to Bounds
//! ```rust
//! use oxigdal_wasm::ViewportState;
//! let mut viewport = ViewportState::new(800, 600);
//! viewport.fit_to_bounds((0.0, 0.0, 4096.0, 4096.0));
//! ```
//!
//! ## Undo/Redo
//! ```rust
//! use oxigdal_wasm::CanvasRenderer;
//! let mut renderer = CanvasRenderer::new(800, 600, 4).expect("Create failed");
//! if renderer.undo() {
//!     println!("Undid last viewport change");
//! }
//!
//! if renderer.redo() {
//!     println!("Redid viewport change");
//! }
//! ```
//!
//! # Performance Optimization
//!
//! ## Quality Levels
//! Adjust quality based on frame rate:
//! ```rust
//! use oxigdal_wasm::{CanvasRenderer, RenderQuality};
//!
//! let mut renderer = CanvasRenderer::new(800, 600, 4).expect("Create failed");
//! renderer.set_quality(RenderQuality::Low);    // Fast
//! renderer.set_quality(RenderQuality::Medium); // Balanced
//! renderer.set_quality(RenderQuality::High);   // Slow
//! ```
//!
//! ## Animation Management
//! Monitor frame rate and adapt:
//! ```ignore
//! use oxigdal_wasm::rendering::AnimationManager;
//!
//! let mut animator = AnimationManager::new(60.0);
//!
//! requestAnimationFrame(|timestamp| {
//!     animator.record_frame(timestamp);
//!
//!     if animator.is_below_target() {
//!         // Reduce quality or skip tiles
//!         renderer.set_quality(RenderQuality::Low);
//!     }
//! });
//! ```
//!
//! # Memory Management
//!
//! Buffers are automatically sized based on canvas dimensions:
//! ```text
//! Buffer Size = width * height * 4 bytes (RGBA)
//! Example: 800x600 = 1,920,000 bytes ≈ 1.8 MB per buffer
//! Total (double buffered): ~3.6 MB
//! ```
//!
//! # Best Practices
//!
//! 1. **Initialize Once**: Create renderer once, reuse across frames
//! 2. **Double Buffer**: Always use swap_buffers() for smooth updates
//! 3. **Progressive Load**: Start with low-res, enhance progressively
//! 4. **Monitor FPS**: Adjust quality if frame rate drops
//! 5. **Batch Updates**: Draw multiple tiles before swapping
//! 6. **Clear Appropriately**: Clear with background color, not transparent
//! 7. **History Limits**: Cap undo history to prevent memory growth

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use wasm_bindgen::prelude::*;
use web_sys::ImageData;

use crate::error::{CanvasError, WasmError, WasmResult};
use crate::tile::TileCoord;

/// Default canvas buffer size in MB
#[allow(dead_code)]
pub const DEFAULT_CANVAS_BUFFER_SIZE_MB: usize = 50;

/// Maximum viewport history size
pub const MAX_VIEWPORT_HISTORY: usize = 50;

/// Rendering quality levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderQuality {
    /// Low quality (fast)
    Low,
    /// Medium quality (balanced)
    Medium,
    /// High quality (slow)
    High,
    /// Ultra quality (very slow)
    Ultra,
}

impl RenderQuality {
    /// Returns the tile resolution multiplier for this quality level
    pub const fn resolution_multiplier(&self) -> f64 {
        match self {
            Self::Low => 0.5,
            Self::Medium => 1.0,
            Self::High => 1.5,
            Self::Ultra => 2.0,
        }
    }

    /// Returns the interpolation quality for this level
    pub const fn interpolation_quality(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Ultra => "high",
        }
    }
}

/// Viewport transformation matrix
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportTransform {
    /// Translation X
    pub tx: f64,
    /// Translation Y
    pub ty: f64,
    /// Scale X
    pub sx: f64,
    /// Scale Y
    pub sy: f64,
    /// Rotation angle in radians
    pub rotation: f64,
}

impl ViewportTransform {
    /// Creates a new transform with specified parameters (affine matrix form)
    /// Parameters: sx, shy, shx, sy, tx, ty (2D affine transform)
    /// For simpler cases, use identity(), translate(), scale(), or rotate()
    pub const fn new(sx: f64, _shy: f64, _shx: f64, sy: f64, tx: f64, ty: f64) -> Self {
        // Simplified: ignores shear components for now
        Self {
            tx,
            ty,
            sx,
            sy,
            rotation: 0.0,
        }
    }

    /// Creates a new identity transform
    pub const fn identity() -> Self {
        Self {
            tx: 0.0,
            ty: 0.0,
            sx: 1.0,
            sy: 1.0,
            rotation: 0.0,
        }
    }

    /// Creates a translation transform
    pub const fn translate(tx: f64, ty: f64) -> Self {
        Self {
            tx,
            ty,
            sx: 1.0,
            sy: 1.0,
            rotation: 0.0,
        }
    }

    /// Creates a scale transform
    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self {
            tx: 0.0,
            ty: 0.0,
            sx,
            sy,
            rotation: 0.0,
        }
    }

    /// Creates a uniform scale transform
    pub const fn uniform_scale(s: f64) -> Self {
        Self::scale(s, s)
    }

    /// Creates a rotation transform
    pub const fn rotate(rotation: f64) -> Self {
        Self {
            tx: 0.0,
            ty: 0.0,
            sx: 1.0,
            sy: 1.0,
            rotation,
        }
    }

    /// Transforms a point
    pub fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        let cos_r = self.rotation.cos();
        let sin_r = self.rotation.sin();

        let x_scaled = x * self.sx;
        let y_scaled = y * self.sy;

        let x_rotated = x_scaled * cos_r - y_scaled * sin_r;
        let y_rotated = x_scaled * sin_r + y_scaled * cos_r;

        (x_rotated + self.tx, y_rotated + self.ty)
    }

    /// Inverse transforms a point
    pub fn inverse_transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        let cos_r = self.rotation.cos();
        let sin_r = self.rotation.sin();

        let x_translated = x - self.tx;
        let y_translated = y - self.ty;

        let x_rotated = x_translated * cos_r + y_translated * sin_r;
        let y_rotated = -x_translated * sin_r + y_translated * cos_r;

        (x_rotated / self.sx, y_rotated / self.sy)
    }

    /// Composes two transforms
    pub fn compose(&self, other: &Self) -> Self {
        Self {
            tx: self.tx + other.tx * self.sx,
            ty: self.ty + other.ty * self.sy,
            sx: self.sx * other.sx,
            sy: self.sy * other.sy,
            rotation: self.rotation + other.rotation,
        }
    }
}

impl Default for ViewportTransform {
    fn default() -> Self {
        Self::identity()
    }
}

/// Viewport state with transformation and bounds
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportState {
    /// Canvas width in pixels
    pub canvas_width: u32,
    /// Canvas height in pixels
    pub canvas_height: u32,
    /// Viewport transformation
    pub transform: ViewportTransform,
    /// Visible bounds in world coordinates (min_x, min_y, max_x, max_y)
    pub visible_bounds: (f64, f64, f64, f64),
    /// Current zoom level
    pub zoom_level: u32,
}

impl ViewportState {
    /// Creates a new viewport state
    pub fn new(canvas_width: u32, canvas_height: u32) -> Self {
        Self {
            canvas_width,
            canvas_height,
            transform: ViewportTransform::identity(),
            visible_bounds: (0.0, 0.0, canvas_width as f64, canvas_height as f64),
            zoom_level: 0,
        }
    }

    /// Updates the viewport transform
    pub fn update_transform(&mut self, transform: ViewportTransform) {
        self.transform = transform;
        self.update_visible_bounds();
    }

    /// Updates visible bounds based on current transform
    fn update_visible_bounds(&mut self) {
        let (min_x, min_y) = self.transform.inverse_transform_point(0.0, 0.0);
        let (max_x, max_y) = self
            .transform
            .inverse_transform_point(self.canvas_width as f64, self.canvas_height as f64);

        self.visible_bounds = (
            min_x.min(max_x),
            min_y.min(max_y),
            min_x.max(max_x),
            min_y.max(max_y),
        );
    }

    /// Pans the viewport
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.transform.tx += dx;
        self.transform.ty += dy;
        self.update_visible_bounds();
    }

    /// Zooms the viewport
    pub fn zoom(&mut self, factor: f64, center_x: f64, center_y: f64) {
        // Ensure factor is positive to maintain valid transform
        let factor = factor.abs().max(0.01);

        // Transform center point to world coordinates
        let (world_x, world_y) = self.transform.inverse_transform_point(center_x, center_y);

        // Apply zoom
        self.transform.sx *= factor;
        self.transform.sy *= factor;

        // Adjust translation to keep center point fixed
        let (new_screen_x, new_screen_y) = self.transform.transform_point(world_x, world_y);
        self.transform.tx += center_x - new_screen_x;
        self.transform.ty += center_y - new_screen_y;

        self.update_visible_bounds();
    }

    /// Fits the viewport to bounds
    pub fn fit_to_bounds(&mut self, bounds: (f64, f64, f64, f64)) {
        let (min_x, min_y, max_x, max_y) = bounds;
        let width = max_x - min_x;
        let height = max_y - min_y;

        let scale_x = self.canvas_width as f64 / width;
        let scale_y = self.canvas_height as f64 / height;
        let scale = scale_x.min(scale_y);

        self.transform.sx = scale;
        self.transform.sy = scale;
        self.transform.tx = -min_x * scale;
        self.transform.ty = -min_y * scale;

        self.update_visible_bounds();
    }

    /// Returns the tiles visible in this viewport
    pub fn visible_tiles(&self, tile_size: u32) -> Vec<TileCoord> {
        let (min_x, min_y, max_x, max_y) = self.visible_bounds;

        let min_tile_x = (min_x / tile_size as f64).floor() as u32;
        let min_tile_y = (min_y / tile_size as f64).floor() as u32;
        let max_tile_x = (max_x / tile_size as f64).ceil() as u32;
        let max_tile_y = (max_y / tile_size as f64).ceil() as u32;

        let mut tiles = Vec::new();
        for y in min_tile_y..=max_tile_y {
            for x in min_tile_x..=max_tile_x {
                tiles.push(TileCoord::new(self.zoom_level, x, y));
            }
        }

        tiles
    }
}

/// Viewport history for undo/redo
pub struct ViewportHistory {
    /// History stack
    history: VecDeque<ViewportState>,
    /// Current position in history
    current_index: usize,
    /// Maximum history size
    max_size: usize,
}

impl ViewportHistory {
    /// Creates a new viewport history
    pub fn new(max_size: usize) -> Self {
        Self {
            history: VecDeque::new(),
            current_index: 0,
            max_size,
        }
    }

    /// Pushes a new state to history
    pub fn push(&mut self, state: ViewportState) {
        // Remove any states after current index (when we've undone some actions)
        while self.history.len() > self.current_index + 1 {
            self.history.pop_back();
        }

        // Add new state
        self.history.push_back(state);

        // Limit history size
        if self.history.len() > self.max_size {
            self.history.pop_front();
            // After popping from front, don't adjust current_index if we're at max
        } else {
            self.current_index = self.history.len() - 1;
        }
    }

    /// Undoes to the previous state
    pub fn undo(&mut self) -> Option<&ViewportState> {
        if self.current_index > 0 {
            self.current_index -= 1;
            self.history.get(self.current_index)
        } else {
            None
        }
    }

    /// Redoes to the next state
    pub fn redo(&mut self) -> Option<&ViewportState> {
        if self.current_index + 1 < self.history.len() {
            self.current_index += 1;
            self.history.get(self.current_index)
        } else {
            None
        }
    }

    /// Returns the current state
    pub fn current(&self) -> Option<&ViewportState> {
        self.history.get(self.current_index)
    }

    /// Checks if undo is available
    pub const fn can_undo(&self) -> bool {
        self.current_index > 0
    }

    /// Checks if redo is available
    pub fn can_redo(&self) -> bool {
        self.current_index + 1 < self.history.len()
    }

    /// Returns the current history index
    pub const fn current_index(&self) -> usize {
        self.current_index
    }

    /// Clears the history
    pub fn clear(&mut self) {
        self.history.clear();
        self.current_index = 0;
    }
}

/// Canvas buffer for double buffering
pub struct CanvasBuffer {
    /// Buffer data
    data: Vec<u8>,
    /// Buffer width
    width: u32,
    /// Buffer height
    height: u32,
    /// Whether the buffer is dirty
    dirty: bool,
}

impl CanvasBuffer {
    /// Creates a new canvas buffer
    pub fn new(width: u32, height: u32) -> WasmResult<Self> {
        if width == 0 || height == 0 {
            return Err(WasmError::InvalidOperation {
                operation: "CanvasBuffer::new".to_string(),
                reason: "Width and height must be greater than zero".to_string(),
            });
        }

        let size = (width as usize) * (height as usize) * 4;
        let data = vec![0u8; size];

        Ok(Self {
            data,
            width,
            height,
            dirty: true,
        })
    }

    /// Resizes the buffer
    pub fn resize(&mut self, width: u32, height: u32) -> WasmResult<()> {
        if width == 0 || height == 0 {
            return Err(WasmError::InvalidOperation {
                operation: "CanvasBuffer::resize".to_string(),
                reason: "Width and height must be greater than zero".to_string(),
            });
        }

        if width != self.width || height != self.height {
            let size = (width as usize) * (height as usize) * 4;
            self.data.resize(size, 0);
            self.width = width;
            self.height = height;
            self.dirty = true;
        }
        Ok(())
    }

    /// Clears the buffer
    pub fn clear(&mut self) {
        self.data.fill(0);
        self.dirty = true;
    }

    /// Clears the buffer with a color
    pub fn clear_with_color(&mut self, r: u8, g: u8, b: u8, a: u8) {
        for chunk in self.data.chunks_exact_mut(4) {
            chunk[0] = r;
            chunk[1] = g;
            chunk[2] = b;
            chunk[3] = a;
        }
        self.dirty = true;
    }

    /// Draws a tile to the buffer
    pub fn draw_tile(
        &mut self,
        tile_data: &[u8],
        tile_x: u32,
        tile_y: u32,
        tile_width: u32,
        tile_height: u32,
    ) -> WasmResult<()> {
        let expected_size = (tile_width * tile_height * 4) as usize;
        if tile_data.len() != expected_size {
            return Err(WasmError::Canvas(CanvasError::BufferSizeMismatch {
                expected: expected_size,
                actual: tile_data.len(),
            }));
        }

        // Check bounds
        if tile_x + tile_width > self.width || tile_y + tile_height > self.height {
            return Err(WasmError::Canvas(CanvasError::InvalidDimensions {
                width: tile_x + tile_width,
                height: tile_y + tile_height,
                reason: "Tile extends beyond buffer bounds".to_string(),
            }));
        }

        // Copy tile data to buffer
        for y in 0..tile_height {
            let src_offset = (y * tile_width * 4) as usize;
            let dst_offset = (((tile_y + y) * self.width + tile_x) * 4) as usize;
            let row_size = (tile_width * 4) as usize;

            self.data[dst_offset..dst_offset + row_size]
                .copy_from_slice(&tile_data[src_offset..src_offset + row_size]);
        }

        self.dirty = true;
        Ok(())
    }

    /// Composites a tile onto the buffer with alpha blending
    pub fn composite_tile(
        &mut self,
        tile_data: &[u8],
        tile_width: u32,
        tile_height: u32,
        dst_x: u32,
        dst_y: u32,
        opacity: f32,
    ) -> WasmResult<()> {
        let expected_size = (tile_width * tile_height * 4) as usize;
        if tile_data.len() != expected_size {
            return Err(WasmError::Canvas(CanvasError::BufferSizeMismatch {
                expected: expected_size,
                actual: tile_data.len(),
            }));
        }

        // Clamp opacity to valid range
        let opacity = opacity.clamp(0.0, 1.0);

        // Calculate intersection with buffer bounds
        let max_x = (dst_x + tile_width).min(self.width);
        let max_y = (dst_y + tile_height).min(self.height);

        if dst_x >= self.width || dst_y >= self.height {
            return Ok(()); // Tile is completely outside buffer
        }

        // Composite each pixel with alpha blending
        for y in dst_y..max_y {
            for x in dst_x..max_x {
                let src_idx = (((y - dst_y) * tile_width + (x - dst_x)) * 4) as usize;
                let dst_idx = ((y * self.width + x) * 4) as usize;

                if src_idx + 3 < tile_data.len() && dst_idx + 3 < self.data.len() {
                    let src_alpha = (f32::from(tile_data[src_idx + 3]) / 255.0) * opacity;
                    let dst_alpha = 1.0 - src_alpha;

                    // Alpha blending
                    for c in 0..3 {
                        let src_val = f32::from(tile_data[src_idx + c]);
                        let dst_val = f32::from(self.data[dst_idx + c]);
                        let blended = (src_val * src_alpha + dst_val * dst_alpha).clamp(0.0, 255.0);
                        self.data[dst_idx + c] = blended as u8;
                    }

                    // Composite alpha
                    let new_alpha = (src_alpha
                        + dst_alpha * f32::from(self.data[dst_idx + 3]) / 255.0)
                        .clamp(0.0, 1.0);
                    self.data[dst_idx + 3] = (new_alpha * 255.0) as u8;
                }
            }
        }

        self.dirty = true;
        Ok(())
    }

    /// Returns the buffer data
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Marks the buffer as clean
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Checks if the buffer is dirty
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Returns the buffer dimensions
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Creates ImageData from the buffer
    pub fn to_image_data(&self) -> Result<ImageData, JsValue> {
        let clamped = wasm_bindgen::Clamped(self.data.as_slice());
        ImageData::new_with_u8_clamped_array_and_sh(clamped, self.width, self.height)
    }
}

/// Progressive rendering manager
pub struct ProgressiveRenderer {
    /// Render queue (tiles sorted by priority)
    queue: Vec<(TileCoord, u32)>, // (coord, priority)
    /// Tiles currently being rendered
    rendering: Vec<TileCoord>,
    /// Completed tiles
    completed: Vec<TileCoord>,
    /// Maximum parallel renders
    max_parallel: usize,
}

impl ProgressiveRenderer {
    /// Creates a new progressive renderer
    pub fn new(max_parallel: usize) -> Self {
        Self {
            queue: Vec::new(),
            rendering: Vec::new(),
            completed: Vec::new(),
            max_parallel,
        }
    }

    /// Adds tiles to the render queue
    pub fn add_tiles(&mut self, tiles: Vec<TileCoord>, priority: u32) {
        for coord in tiles {
            if !self.is_completed(&coord) && !self.is_rendering(&coord) {
                self.queue.push((coord, priority));
            }
        }

        // Sort by priority (higher priority first)
        self.queue.sort_by(|a, b| b.1.cmp(&a.1));
    }

    /// Gets the next tiles to render
    pub fn next_batch(&mut self) -> Vec<TileCoord> {
        let available = self.max_parallel.saturating_sub(self.rendering.len());
        let mut batch = Vec::new();

        for _ in 0..available {
            if let Some((coord, _)) = self.queue.pop() {
                self.rendering.push(coord);
                batch.push(coord);
            } else {
                break;
            }
        }

        batch
    }

    /// Marks a tile as completed
    pub fn mark_completed(&mut self, coord: TileCoord) {
        if let Some(pos) = self.rendering.iter().position(|c| *c == coord) {
            self.rendering.remove(pos);
            self.completed.push(coord);
        }
    }

    /// Checks if a tile is completed
    pub fn is_completed(&self, coord: &TileCoord) -> bool {
        self.completed.contains(coord)
    }

    /// Checks if a tile is rendering
    pub fn is_rendering(&self, coord: &TileCoord) -> bool {
        self.rendering.contains(coord)
    }

    /// Clears all state
    pub fn clear(&mut self) {
        self.queue.clear();
        self.rendering.clear();
        self.completed.clear();
    }

    /// Returns rendering statistics
    pub fn stats(&self) -> ProgressiveRenderStats {
        ProgressiveRenderStats {
            queued: self.queue.len(),
            rendering: self.rendering.len(),
            completed: self.completed.len(),
        }
    }
}

/// Progressive rendering statistics
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProgressiveRenderStats {
    /// Number of tiles queued
    pub queued: usize,
    /// Number of tiles rendering
    pub rendering: usize,
    /// Number of tiles completed
    pub completed: usize,
}

impl ProgressiveRenderStats {
    /// Returns the total number of tiles
    pub const fn total(&self) -> usize {
        self.queued + self.rendering + self.completed
    }

    /// Returns the completion percentage
    pub fn completion_percentage(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            100.0
        } else {
            (self.completed as f64 / total as f64) * 100.0
        }
    }
}

/// Canvas renderer with double buffering and progressive rendering
pub struct CanvasRenderer {
    /// Front buffer
    front_buffer: CanvasBuffer,
    /// Back buffer
    back_buffer: CanvasBuffer,
    /// Progressive renderer
    progressive: ProgressiveRenderer,
    /// Viewport state
    viewport: ViewportState,
    /// Viewport history
    history: ViewportHistory,
    /// Render quality
    quality: RenderQuality,
}

impl CanvasRenderer {
    /// Creates a new canvas renderer
    pub fn new(width: u32, height: u32, max_parallel: usize) -> WasmResult<Self> {
        Ok(Self {
            front_buffer: CanvasBuffer::new(width, height)?,
            back_buffer: CanvasBuffer::new(width, height)?,
            progressive: ProgressiveRenderer::new(max_parallel),
            viewport: ViewportState::new(width, height),
            history: ViewportHistory::new(MAX_VIEWPORT_HISTORY),
            quality: RenderQuality::Medium,
        })
    }

    /// Resizes the renderer
    pub fn resize(&mut self, width: u32, height: u32) -> WasmResult<()> {
        self.front_buffer.resize(width, height)?;
        self.back_buffer.resize(width, height)?;
        self.viewport.canvas_width = width;
        self.viewport.canvas_height = height;
        Ok(())
    }

    /// Begins a new frame
    pub fn begin_frame(&mut self) {
        self.back_buffer.clear();
    }

    /// Draws a tile to the back buffer
    pub fn draw_tile(
        &mut self,
        coord: TileCoord,
        tile_data: &[u8],
        tile_width: u32,
        tile_height: u32,
    ) -> WasmResult<()> {
        // Transform tile coordinates to screen coordinates
        let world_x = f64::from(coord.x * tile_width);
        let world_y = f64::from(coord.y * tile_height);
        let (screen_x, screen_y) = self.viewport.transform.transform_point(world_x, world_y);

        self.back_buffer.draw_tile(
            tile_data,
            screen_x as u32,
            screen_y as u32,
            tile_width,
            tile_height,
        )?;

        self.progressive.mark_completed(coord);
        Ok(())
    }

    /// Swaps the buffers
    pub fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.front_buffer, &mut self.back_buffer);
        self.front_buffer.mark_clean();
    }

    /// Returns the front buffer as ImageData
    pub fn front_buffer_image_data(&self) -> Result<ImageData, JsValue> {
        self.front_buffer.to_image_data()
    }

    /// Updates the viewport
    pub fn update_viewport(&mut self, state: ViewportState) {
        self.history.push(self.viewport.clone());
        self.viewport = state;
    }

    /// Pans the viewport
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let old_state = self.viewport.clone();
        self.viewport.pan(dx, dy);
        if old_state != self.viewport {
            self.history.push(old_state);
        }
    }

    /// Zooms the viewport
    pub fn zoom(&mut self, factor: f64, center_x: f64, center_y: f64) {
        let old_state = self.viewport.clone();
        self.viewport.zoom(factor, center_x, center_y);
        if old_state != self.viewport {
            self.history.push(old_state);
        }
    }

    /// Undoes the last viewport change
    pub fn undo(&mut self) -> bool {
        if let Some(state) = self.history.undo() {
            self.viewport = state.clone();
            true
        } else {
            false
        }
    }

    /// Redoes the next viewport change
    pub fn redo(&mut self) -> bool {
        if let Some(state) = self.history.redo() {
            self.viewport = state.clone();
            true
        } else {
            false
        }
    }

    /// Sets the render quality
    pub fn set_quality(&mut self, quality: RenderQuality) {
        self.quality = quality;
    }

    /// Returns the current viewport state
    pub const fn viewport(&self) -> &ViewportState {
        &self.viewport
    }

    /// Returns progressive rendering statistics
    pub fn progressive_stats(&self) -> ProgressiveRenderStats {
        self.progressive.stats()
    }
}

/// Animation frame manager
pub struct AnimationManager {
    /// Frame times (in milliseconds)
    frame_times: VecDeque<f64>,
    /// Maximum frame time samples
    max_samples: usize,
    /// Last frame timestamp
    last_frame: Option<f64>,
    /// Target FPS
    target_fps: f64,
}

impl AnimationManager {
    /// Creates a new animation manager
    pub fn new(target_fps: f64) -> Self {
        Self {
            frame_times: VecDeque::new(),
            max_samples: 60,
            last_frame: None,
            target_fps,
        }
    }

    /// Records a frame
    pub fn record_frame(&mut self, timestamp: f64) {
        if let Some(last) = self.last_frame {
            let frame_time = timestamp - last;
            self.frame_times.push_back(frame_time);

            if self.frame_times.len() > self.max_samples {
                self.frame_times.pop_front();
            }
        }

        self.last_frame = Some(timestamp);
    }

    /// Returns the current FPS
    pub fn current_fps(&self) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }

        let avg_frame_time: f64 =
            self.frame_times.iter().sum::<f64>() / self.frame_times.len() as f64;
        if avg_frame_time > 0.0 {
            1000.0 / avg_frame_time
        } else {
            0.0
        }
    }

    /// Returns the average frame time in milliseconds
    pub fn average_frame_time(&self) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }

        self.frame_times.iter().sum::<f64>() / self.frame_times.len() as f64
    }

    /// Checks if the frame rate is below target
    pub fn is_below_target(&self) -> bool {
        self.current_fps() < self.target_fps
    }

    /// Returns frame statistics
    pub fn stats(&self) -> AnimationStats {
        AnimationStats {
            current_fps: self.current_fps(),
            average_frame_time_ms: self.average_frame_time(),
            target_fps: self.target_fps,
        }
    }
}

/// Animation statistics
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AnimationStats {
    /// Current FPS
    pub current_fps: f64,
    /// Average frame time in milliseconds
    pub average_frame_time_ms: f64,
    /// Target FPS
    pub target_fps: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_transform() {
        let transform = ViewportTransform::translate(10.0, 20.0);
        let (x, y) = transform.transform_point(0.0, 0.0);
        assert_eq!(x, 10.0);
        assert_eq!(y, 20.0);
    }

    #[test]
    fn test_viewport_transform_inverse() {
        let transform = ViewportTransform::translate(10.0, 20.0);
        let (x, y) = transform.inverse_transform_point(10.0, 20.0);
        assert!((x - 0.0).abs() < 0.001);
        assert!((y - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_viewport_state() {
        let mut state = ViewportState::new(800, 600);
        state.pan(10.0, 20.0);
        assert_eq!(state.transform.tx, 10.0);
        assert_eq!(state.transform.ty, 20.0);
    }

    #[test]
    fn test_viewport_history() {
        let mut history = ViewportHistory::new(10);
        let state1 = ViewportState::new(800, 600);
        let mut state2 = ViewportState::new(800, 600);
        state2.pan(10.0, 20.0);

        history.push(state1);
        history.push(state2);

        assert!(history.can_undo());
        history.undo();
        assert!(history.can_redo());
    }

    #[test]
    fn test_canvas_buffer() {
        let mut buffer = CanvasBuffer::new(256, 256).expect("Failed to create buffer");
        assert!(buffer.is_dirty());

        buffer.mark_clean();
        assert!(!buffer.is_dirty());

        buffer.clear();
        assert!(buffer.is_dirty());
    }

    #[test]
    fn test_progressive_renderer() {
        let mut renderer = ProgressiveRenderer::new(4);
        let tiles = vec![
            TileCoord::new(0, 0, 0),
            TileCoord::new(0, 1, 0),
            TileCoord::new(0, 0, 1),
        ];

        renderer.add_tiles(tiles, 10);
        let batch = renderer.next_batch();
        assert!(!batch.is_empty());
        assert!(batch.len() <= 4);
    }

    #[test]
    fn test_animation_manager() {
        let mut manager = AnimationManager::new(60.0);
        manager.record_frame(0.0);
        manager.record_frame(16.67); // ~60 FPS
        manager.record_frame(33.34);

        let fps = manager.current_fps();
        assert!(fps > 50.0 && fps < 70.0);
    }

    #[test]
    fn test_render_quality() {
        assert_eq!(RenderQuality::Low.resolution_multiplier(), 0.5);
        assert_eq!(RenderQuality::Medium.resolution_multiplier(), 1.0);
        assert_eq!(RenderQuality::High.resolution_multiplier(), 1.5);
        assert_eq!(RenderQuality::Ultra.resolution_multiplier(), 2.0);
    }
}
