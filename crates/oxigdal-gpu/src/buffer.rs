//! GPU buffer management for OxiGDAL.
//!
//! This module provides efficient GPU buffer management for raster data,
//! including upload, download, and memory mapping operations.

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use bytemuck::{Pod, Zeroable};
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::{debug, trace};
use wgpu::{
    Buffer, BufferAsyncError, BufferDescriptor, BufferUsages, COPY_BUFFER_ALIGNMENT, MapMode,
};

/// GPU buffer wrapper with type safety.
///
/// This struct wraps a WGPU buffer and provides type-safe operations
/// for uploading and downloading data to/from the GPU.
pub struct GpuBuffer<T: Pod> {
    /// The underlying WGPU buffer.
    buffer: Arc<Buffer>,
    /// GPU context.
    context: GpuContext,
    /// Number of elements in the buffer.
    len: usize,
    /// Buffer usage flags.
    usage: BufferUsages,
    /// Phantom data for type parameter.
    _phantom: PhantomData<T>,
}

impl<T: Pod> GpuBuffer<T> {
    /// Create a new GPU buffer with the specified size and usage.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer creation fails or size is invalid.
    pub fn new(context: &GpuContext, len: usize, usage: BufferUsages) -> GpuResult<Self> {
        let size = Self::calculate_size(len)?;

        trace!("Creating GPU buffer: {} elements, {} bytes", len, size);

        let buffer = context.device().create_buffer(&BufferDescriptor {
            label: Some("GpuBuffer"),
            size,
            usage,
            mapped_at_creation: false,
        });

        Ok(Self {
            buffer: Arc::new(buffer),
            context: context.clone(),
            len,
            usage,
            _phantom: PhantomData,
        })
    }

    /// Create a GPU buffer from existing data.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer creation or upload fails.
    pub fn from_data(context: &GpuContext, data: &[T], usage: BufferUsages) -> GpuResult<Self> {
        let mut buffer = Self::new(context, data.len(), usage | BufferUsages::COPY_DST)?;
        buffer.write(data)?;
        Ok(buffer)
    }

    /// Create a staging buffer for CPU-GPU transfers.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer creation fails.
    pub fn staging(context: &GpuContext, len: usize) -> GpuResult<Self> {
        Self::new(
            context,
            len,
            BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        )
    }

    /// Calculate the aligned buffer size in bytes.
    fn calculate_size(len: usize) -> GpuResult<u64> {
        let element_size = std::mem::size_of::<T>();
        let size = len
            .checked_mul(element_size)
            .ok_or_else(|| GpuError::invalid_buffer("Buffer size overflow"))?;

        // Align to COPY_BUFFER_ALIGNMENT for efficient transfers
        let aligned_size = ((size as u64 + COPY_BUFFER_ALIGNMENT - 1) / COPY_BUFFER_ALIGNMENT)
            * COPY_BUFFER_ALIGNMENT;

        Ok(aligned_size)
    }

    /// Write data to the GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the buffer doesn't support writes or data size
    /// doesn't match buffer size.
    pub fn write(&mut self, data: &[T]) -> GpuResult<()> {
        if data.len() != self.len {
            return Err(GpuError::invalid_buffer(format!(
                "Data size mismatch: expected {}, got {}",
                self.len,
                data.len()
            )));
        }

        if !self.usage.contains(BufferUsages::COPY_DST) {
            return Err(GpuError::invalid_buffer(
                "Buffer not writable (missing COPY_DST usage)",
            ));
        }

        let bytes = bytemuck::cast_slice(data);
        self.context.queue().write_buffer(&self.buffer, 0, bytes);

        debug!("Wrote {} bytes to GPU buffer", bytes.len());
        Ok(())
    }

    /// Read data from the GPU buffer asynchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if the buffer doesn't support reads or mapping fails.
    pub async fn read(&self) -> GpuResult<Vec<T>> {
        if !self.usage.contains(BufferUsages::MAP_READ) {
            return Err(GpuError::invalid_buffer(
                "Buffer not readable (missing MAP_READ usage)",
            ));
        }

        let buffer_slice = self.buffer.slice(..);

        // Map the buffer for reading
        let (tx, rx) = futures::channel::oneshot::channel();
        buffer_slice.map_async(MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

        // Poll the device until the buffer is mapped
        self.context.poll(true);

        // Wait for mapping to complete
        rx.await
            .map_err(|_| GpuError::buffer_mapping("Channel closed"))?
            .map_err(|e| GpuError::buffer_mapping(Self::map_error_to_string(e)))?;

        // Read the data
        let data = buffer_slice.get_mapped_range();
        let result: Vec<T> = bytemuck::cast_slice(&data).to_vec();

        // Unmap the buffer
        drop(data);
        self.buffer.unmap();

        debug!("Read {} elements from GPU buffer", result.len());
        Ok(result)
    }

    /// Read data from the GPU buffer synchronously (blocking).
    ///
    /// # Errors
    ///
    /// Returns an error if the buffer doesn't support reads or mapping fails.
    pub fn read_blocking(&self) -> GpuResult<Vec<T>> {
        pollster::block_on(self.read())
    }

    /// Copy data from another GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer sizes don't match or copy is not supported.
    pub fn copy_from(&mut self, source: &GpuBuffer<T>) -> GpuResult<()> {
        if self.len != source.len {
            return Err(GpuError::invalid_buffer(format!(
                "Buffer size mismatch: {} != {}",
                self.len, source.len
            )));
        }

        if !source.usage.contains(BufferUsages::COPY_SRC) {
            return Err(GpuError::invalid_buffer(
                "Source buffer not copyable (missing COPY_SRC usage)",
            ));
        }

        if !self.usage.contains(BufferUsages::COPY_DST) {
            return Err(GpuError::invalid_buffer(
                "Destination buffer not copyable (missing COPY_DST usage)",
            ));
        }

        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Buffer Copy"),
                });

        let size = Self::calculate_size(self.len)?;
        encoder.copy_buffer_to_buffer(&source.buffer, 0, &self.buffer, 0, size);

        self.context.queue().submit(Some(encoder.finish()));

        debug!("Copied {} elements between GPU buffers", self.len);
        Ok(())
    }

    /// Get the number of elements in the buffer.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get the buffer size in bytes.
    pub fn size_bytes(&self) -> u64 {
        Self::calculate_size(self.len).unwrap_or(0)
    }

    /// Get the underlying WGPU buffer.
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Get buffer usage flags.
    pub fn usage(&self) -> BufferUsages {
        self.usage
    }

    /// Convert buffer mapping error to string.
    fn map_error_to_string(error: BufferAsyncError) -> String {
        error.to_string()
    }
}

impl<T: Pod> Clone for GpuBuffer<T> {
    fn clone(&self) -> Self {
        Self {
            buffer: Arc::clone(&self.buffer),
            context: self.context.clone(),
            len: self.len,
            usage: self.usage,
            _phantom: PhantomData,
        }
    }
}

impl<T: Pod> std::fmt::Debug for GpuBuffer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuBuffer")
            .field("len", &self.len)
            .field("size_bytes", &self.size_bytes())
            .field("usage", &self.usage)
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

/// GPU raster buffer for multi-band raster data.
///
/// This struct manages GPU buffers for multi-band raster data with
/// efficient interleaved or planar storage.
pub struct GpuRasterBuffer<T: Pod> {
    /// GPU buffers for each band.
    bands: Vec<GpuBuffer<T>>,
    /// Width of the raster.
    width: u32,
    /// Height of the raster.
    height: u32,
}

impl<T: Pod + Zeroable> GpuRasterBuffer<T> {
    /// Create a new GPU raster buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer creation fails.
    pub fn new(
        context: &GpuContext,
        width: u32,
        height: u32,
        num_bands: usize,
        usage: BufferUsages,
    ) -> GpuResult<Self> {
        let pixels_per_band = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| GpuError::invalid_buffer("Raster size overflow"))?;

        let bands = (0..num_bands)
            .map(|_| GpuBuffer::new(context, pixels_per_band, usage))
            .collect::<GpuResult<Vec<_>>>()?;

        debug!(
            "Created GPU raster buffer: {}x{} with {} bands",
            width, height, num_bands
        );

        Ok(Self {
            bands,
            width,
            height,
        })
    }

    /// Create a GPU raster buffer from data.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer creation or upload fails.
    pub fn from_bands(
        context: &GpuContext,
        width: u32,
        height: u32,
        bands_data: &[Vec<T>],
        usage: BufferUsages,
    ) -> GpuResult<Self> {
        let expected_size = (width as usize) * (height as usize);

        for (i, band) in bands_data.iter().enumerate() {
            if band.len() != expected_size {
                return Err(GpuError::invalid_buffer(format!(
                    "Band {} size mismatch: expected {}, got {}",
                    i,
                    expected_size,
                    band.len()
                )));
            }
        }

        let bands = bands_data
            .iter()
            .map(|data| GpuBuffer::from_data(context, data, usage))
            .collect::<GpuResult<Vec<_>>>()?;

        Ok(Self {
            bands,
            width,
            height,
        })
    }

    /// Get a specific band buffer.
    pub fn band(&self, index: usize) -> Option<&GpuBuffer<T>> {
        self.bands.get(index)
    }

    /// Get mutable reference to a specific band buffer.
    pub fn band_mut(&mut self, index: usize) -> Option<&mut GpuBuffer<T>> {
        self.bands.get_mut(index)
    }

    /// Get all band buffers.
    pub fn bands(&self) -> &[GpuBuffer<T>] {
        &self.bands
    }

    /// Get the number of bands.
    pub fn num_bands(&self) -> usize {
        self.bands.len()
    }

    /// Get raster dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Get raster width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get raster height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Read all bands from GPU asynchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub async fn read_all_bands(&self) -> GpuResult<Vec<Vec<T>>> {
        let mut results = Vec::with_capacity(self.bands.len());

        for band in &self.bands {
            results.push(band.read().await?);
        }

        Ok(results)
    }

    /// Read all bands from GPU synchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn read_all_bands_blocking(&self) -> GpuResult<Vec<Vec<T>>> {
        pollster::block_on(self.read_all_bands())
    }
}

impl<T: Pod> std::fmt::Debug for GpuRasterBuffer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuRasterBuffer")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("num_bands", &self.num_bands())
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gpu_buffer_creation() {
        if let Ok(context) = GpuContext::new().await {
            let buffer: GpuBuffer<f32> = GpuBuffer::new(&context, 1024, BufferUsages::STORAGE)
                .unwrap_or_else(|e| {
                    panic!("Failed to create buffer: {}", e);
                });

            assert_eq!(buffer.len(), 1024);
            assert!(!buffer.is_empty());
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_gpu_buffer_write_read() {
        if let Ok(context) = GpuContext::new().await {
            let data: Vec<f32> = (0..100).map(|i| i as f32).collect();

            let buffer = GpuBuffer::from_data(
                &context,
                &data,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            )
            .unwrap_or_else(|e| {
                panic!("Failed to create buffer: {}", e);
            });

            // Create staging buffer for reading
            let mut staging = GpuBuffer::staging(&context, 100).unwrap_or_else(|e| {
                panic!("Failed to create staging buffer: {}", e);
            });

            staging.copy_from(&buffer).unwrap_or_else(|e| {
                panic!("Failed to copy buffer: {}", e);
            });

            let result = staging.read().await.unwrap_or_else(|e| {
                panic!("Failed to read buffer: {}", e);
            });

            assert_eq!(result.len(), data.len());
            for (a, b) in result.iter().zip(data.iter()) {
                assert!((a - b).abs() < 1e-6);
            }
        }
    }

    #[tokio::test]
    async fn test_gpu_raster_buffer() {
        if let Ok(context) = GpuContext::new().await {
            let width = 64;
            let height = 64;
            let num_bands = 3;

            let raster: GpuRasterBuffer<f32> =
                GpuRasterBuffer::new(&context, width, height, num_bands, BufferUsages::STORAGE)
                    .unwrap_or_else(|e| {
                        panic!("Failed to create raster buffer: {}", e);
                    });

            assert_eq!(raster.width(), width);
            assert_eq!(raster.height(), height);
            assert_eq!(raster.num_bands(), num_bands);
        }
    }
}
