//! Overview (pyramid) generation for GeoTIFF
//!
//! This module handles generating reduced resolution overview images.

use oxigdal_core::error::{OxiGdalError, Result};

/// Resampling method for overview generation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewResampling {
    /// Nearest neighbor (fastest)
    Nearest,
    /// Average (good for continuous data)
    Average,
    /// Bilinear interpolation
    Bilinear,
    /// Mode (good for categorical data)
    Mode,
}

/// Overview level data
#[derive(Debug, Clone)]
pub struct OverviewLevel {
    /// Scaling factor (2, 4, 8, etc.)
    pub factor: u32,
    /// Overview width
    pub width: u64,
    /// Overview height
    pub height: u64,
    /// Overview data
    pub data: Vec<u8>,
}

/// Generator for overview pyramids
pub struct OverviewGenerator {
    /// Original image width
    width: u64,
    /// Original image height
    height: u64,
    /// Bytes per sample
    bytes_per_sample: usize,
    /// Samples per pixel (bands)
    samples_per_pixel: usize,
    /// Resampling method
    resampling: OverviewResampling,
    /// Data type (for proper floating-point handling)
    data_type: oxigdal_core::types::RasterDataType,
}

impl OverviewGenerator {
    /// Creates a new overview generator
    #[must_use]
    pub const fn new(
        width: u64,
        height: u64,
        bytes_per_sample: usize,
        samples_per_pixel: usize,
        resampling: OverviewResampling,
        data_type: oxigdal_core::types::RasterDataType,
    ) -> Self {
        Self {
            width,
            height,
            bytes_per_sample,
            samples_per_pixel,
            resampling,
            data_type,
        }
    }

    /// Generates overview levels
    ///
    /// # Arguments
    /// * `data` - Original image data
    /// * `levels` - Overview factors (e.g., [2, 4, 8, 16])
    ///
    /// # Errors
    /// Returns an error if overview generation fails
    pub fn generate_overviews(&self, data: &[u8], levels: &[u32]) -> Result<Vec<OverviewLevel>> {
        let mut overviews = Vec::with_capacity(levels.len());

        for &factor in levels {
            if factor < 2 {
                return Err(OxiGdalError::InvalidParameter {
                    parameter: "overview_factor",
                    message: format!("Overview factor must be >= 2, got {}", factor),
                });
            }

            let overview = self.generate_overview(data, factor)?;
            overviews.push(overview);
        }

        Ok(overviews)
    }

    /// Generates a single overview level
    fn generate_overview(&self, data: &[u8], factor: u32) -> Result<OverviewLevel> {
        let overview_width = self.width.div_ceil(u64::from(factor));
        let overview_height = self.height.div_ceil(u64::from(factor));

        let pixel_bytes = self.bytes_per_sample * self.samples_per_pixel;
        let overview_size = overview_width as usize * overview_height as usize * pixel_bytes;
        let mut overview_data = vec![0u8; overview_size];

        match self.resampling {
            OverviewResampling::Nearest => {
                self.resample_nearest(
                    data,
                    &mut overview_data,
                    factor,
                    overview_width,
                    overview_height,
                );
            }
            OverviewResampling::Average => {
                self.resample_average(
                    data,
                    &mut overview_data,
                    factor,
                    overview_width,
                    overview_height,
                )?;
            }
            OverviewResampling::Bilinear => {
                self.resample_bilinear(
                    data,
                    &mut overview_data,
                    factor,
                    overview_width,
                    overview_height,
                )?;
            }
            OverviewResampling::Mode => {
                self.resample_mode(
                    data,
                    &mut overview_data,
                    factor,
                    overview_width,
                    overview_height,
                );
            }
        }

        Ok(OverviewLevel {
            factor,
            width: overview_width,
            height: overview_height,
            data: overview_data,
        })
    }

    /// Nearest neighbor resampling
    fn resample_nearest(
        &self,
        src: &[u8],
        dst: &mut [u8],
        factor: u32,
        dst_width: u64,
        dst_height: u64,
    ) {
        let pixel_bytes = self.bytes_per_sample * self.samples_per_pixel;

        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                let src_x = dst_x * u64::from(factor);
                let src_y = dst_y * u64::from(factor);

                let src_offset = (src_y * self.width + src_x) as usize * pixel_bytes;
                let dst_offset = (dst_y * dst_width + dst_x) as usize * pixel_bytes;

                if src_offset + pixel_bytes <= src.len() && dst_offset + pixel_bytes <= dst.len() {
                    dst[dst_offset..dst_offset + pixel_bytes]
                        .copy_from_slice(&src[src_offset..src_offset + pixel_bytes]);
                }
            }
        }
    }

    /// Average resampling (box filter)
    fn resample_average(
        &self,
        src: &[u8],
        dst: &mut [u8],
        factor: u32,
        dst_width: u64,
        dst_height: u64,
    ) -> Result<()> {
        let pixel_bytes = self.bytes_per_sample * self.samples_per_pixel;
        let is_float = self.is_float_type();

        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                let src_x_start = dst_x * u64::from(factor);
                let src_y_start = dst_y * u64::from(factor);
                let src_x_end = ((src_x_start + u64::from(factor)).min(self.width)) as usize;
                let src_y_end = ((src_y_start + u64::from(factor)).min(self.height)) as usize;

                let dst_offset = (dst_y * dst_width + dst_x) as usize * pixel_bytes;

                // Average pixels in the block
                for sample_idx in 0..self.samples_per_pixel {
                    if is_float {
                        // Use f64 for floating-point types
                        let mut sum = 0.0_f64;
                        let mut count = 0_u64;

                        for src_y in src_y_start as usize..src_y_end {
                            for src_x in src_x_start as usize..src_x_end {
                                let src_offset = (src_y * self.width as usize + src_x)
                                    * pixel_bytes
                                    + sample_idx * self.bytes_per_sample;

                                if src_offset + self.bytes_per_sample <= src.len() {
                                    let value = self.read_sample_float(&src[src_offset..]);
                                    sum += value;
                                    count += 1;
                                }
                            }
                        }

                        if count > 0 {
                            let avg = sum / count as f64;
                            self.write_sample_float(
                                &mut dst[dst_offset + sample_idx * self.bytes_per_sample..],
                                avg,
                            );
                        }
                    } else {
                        // Use u64 for integer types with saturating arithmetic
                        let mut sum = 0_u64;
                        let mut count = 0_u64;

                        for src_y in src_y_start as usize..src_y_end {
                            for src_x in src_x_start as usize..src_x_end {
                                let src_offset = (src_y * self.width as usize + src_x)
                                    * pixel_bytes
                                    + sample_idx * self.bytes_per_sample;

                                if src_offset + self.bytes_per_sample <= src.len() {
                                    let value = self.read_sample(&src[src_offset..]);
                                    sum = sum.saturating_add(value);
                                    count += 1;
                                }
                            }
                        }

                        if count > 0 {
                            let avg = sum / count;
                            self.write_sample(
                                &mut dst[dst_offset + sample_idx * self.bytes_per_sample..],
                                avg,
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Bilinear resampling
    fn resample_bilinear(
        &self,
        src: &[u8],
        dst: &mut [u8],
        factor: u32,
        dst_width: u64,
        dst_height: u64,
    ) -> Result<()> {
        let pixel_bytes = self.bytes_per_sample * self.samples_per_pixel;
        let factor_f64 = f64::from(factor);
        let is_float = self.is_float_type();

        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                let src_x_f = (dst_x as f64 + 0.5) * factor_f64 - 0.5;
                let src_y_f = (dst_y as f64 + 0.5) * factor_f64 - 0.5;

                let src_x0 = src_x_f.floor() as i64;
                let src_y0 = src_y_f.floor() as i64;
                let src_x1 = src_x0 + 1;
                let src_y1 = src_y0 + 1;

                let dx = src_x_f - src_x0 as f64;
                let dy = src_y_f - src_y0 as f64;

                let dst_offset = (dst_y * dst_width + dst_x) as usize * pixel_bytes;

                for sample_idx in 0..self.samples_per_pixel {
                    let mut val = 0.0;

                    // Sample four corners
                    for (sy, wy) in [(src_y0, 1.0 - dy), (src_y1, dy)] {
                        for (sx, wx) in [(src_x0, 1.0 - dx), (src_x1, dx)] {
                            if sx >= 0
                                && sy >= 0
                                && (sx as u64) < self.width
                                && (sy as u64) < self.height
                            {
                                let src_offset = (sy as usize * self.width as usize + sx as usize)
                                    * pixel_bytes
                                    + sample_idx * self.bytes_per_sample;

                                if src_offset + self.bytes_per_sample <= src.len() {
                                    let sample = if is_float {
                                        self.read_sample_float(&src[src_offset..])
                                    } else {
                                        self.read_sample(&src[src_offset..]) as f64
                                    };
                                    val += sample * wx * wy;
                                }
                            }
                        }
                    }

                    if is_float {
                        self.write_sample_float(
                            &mut dst[dst_offset + sample_idx * self.bytes_per_sample..],
                            val,
                        );
                    } else {
                        self.write_sample(
                            &mut dst[dst_offset + sample_idx * self.bytes_per_sample..],
                            val.round() as u64,
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Mode resampling (most common value)
    fn resample_mode(
        &self,
        src: &[u8],
        dst: &mut [u8],
        factor: u32,
        dst_width: u64,
        dst_height: u64,
    ) {
        let pixel_bytes = self.bytes_per_sample * self.samples_per_pixel;

        for dst_y in 0..dst_height {
            for dst_x in 0..dst_width {
                let src_x_start = dst_x * u64::from(factor);
                let src_y_start = dst_y * u64::from(factor);
                let src_x_end = ((src_x_start + u64::from(factor)).min(self.width)) as usize;
                let src_y_end = ((src_y_start + u64::from(factor)).min(self.height)) as usize;

                let dst_offset = (dst_y * dst_width + dst_x) as usize * pixel_bytes;

                // Find mode for each sample
                for sample_idx in 0..self.samples_per_pixel {
                    let mut values = Vec::new();

                    for src_y in src_y_start as usize..src_y_end {
                        for src_x in src_x_start as usize..src_x_end {
                            let src_offset = (src_y * self.width as usize + src_x) * pixel_bytes
                                + sample_idx * self.bytes_per_sample;

                            if src_offset + self.bytes_per_sample <= src.len() {
                                let value = self.read_sample(&src[src_offset..]);
                                values.push(value);
                            }
                        }
                    }

                    if !values.is_empty() {
                        values.sort_unstable();
                        let mode = self.find_mode(&values);
                        self.write_sample(
                            &mut dst[dst_offset + sample_idx * self.bytes_per_sample..],
                            mode,
                        );
                    }
                }
            }
        }
    }

    /// Finds the mode (most common value) in a sorted array
    fn find_mode(&self, values: &[u64]) -> u64 {
        if values.is_empty() {
            return 0;
        }

        let mut max_count = 1;
        let mut mode = values[0];
        let mut current_count = 1;

        for i in 1..values.len() {
            if values[i] == values[i - 1] {
                current_count += 1;
                if current_count > max_count {
                    max_count = current_count;
                    mode = values[i];
                }
            } else {
                current_count = 1;
            }
        }

        mode
    }

    /// Reads a sample value based on bytes_per_sample
    fn read_sample(&self, data: &[u8]) -> u64 {
        match self.bytes_per_sample {
            1 => u64::from(data[0]),
            2 => u64::from(u16::from_le_bytes([data[0], data[1]])),
            4 => u64::from(u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
            8 => u64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]),
            _ => 0,
        }
    }

    /// Writes a sample value based on bytes_per_sample
    fn write_sample(&self, data: &mut [u8], value: u64) {
        match self.bytes_per_sample {
            1 => data[0] = value as u8,
            2 => {
                let bytes = (value as u16).to_le_bytes();
                data[0..2].copy_from_slice(&bytes);
            }
            4 => {
                let bytes = (value as u32).to_le_bytes();
                data[0..4].copy_from_slice(&bytes);
            }
            8 => {
                let bytes = value.to_le_bytes();
                data[0..8].copy_from_slice(&bytes);
            }
            _ => {}
        }
    }

    /// Checks if the data type is floating point
    #[inline]
    const fn is_float_type(&self) -> bool {
        matches!(
            self.data_type,
            oxigdal_core::types::RasterDataType::Float32
                | oxigdal_core::types::RasterDataType::Float64
        )
    }

    /// Reads a floating-point sample value
    fn read_sample_float(&self, data: &[u8]) -> f64 {
        match self.bytes_per_sample {
            4 => {
                // Float32
                let bytes = [data[0], data[1], data[2], data[3]];
                f64::from(f32::from_le_bytes(bytes))
            }
            8 => {
                // Float64
                let bytes = [
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ];
                f64::from_le_bytes(bytes)
            }
            _ => 0.0,
        }
    }

    /// Writes a floating-point sample value
    fn write_sample_float(&self, data: &mut [u8], value: f64) {
        match self.bytes_per_sample {
            4 => {
                // Float32
                let bytes = (value as f32).to_le_bytes();
                data[0..4].copy_from_slice(&bytes);
            }
            8 => {
                // Float64
                let bytes = value.to_le_bytes();
                data[0..8].copy_from_slice(&bytes);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overview_dimensions() {
        let generator = OverviewGenerator::new(
            1024,
            1024,
            1,
            1,
            OverviewResampling::Nearest,
            oxigdal_core::types::RasterDataType::UInt8,
        );

        let data = vec![128u8; 1024 * 1024];
        let overviews = generator
            .generate_overviews(&data, &[2, 4, 8])
            .expect("Should generate overviews");

        assert_eq!(overviews.len(), 3);
        assert_eq!(overviews[0].width, 512);
        assert_eq!(overviews[0].height, 512);
        assert_eq!(overviews[1].width, 256);
        assert_eq!(overviews[2].width, 128);
    }

    #[test]
    fn test_nearest_resampling() {
        // 4x4 image
        let data = vec![
            1, 2, 3, 4, //
            5, 6, 7, 8, //
            9, 10, 11, 12, //
            13, 14, 15, 16, //
        ];

        let generator = OverviewGenerator::new(
            4,
            4,
            1,
            1,
            OverviewResampling::Nearest,
            oxigdal_core::types::RasterDataType::UInt8,
        );
        let overview = generator
            .generate_overview(&data, 2)
            .expect("Should generate overview");

        assert_eq!(overview.width, 2);
        assert_eq!(overview.height, 2);
        // Should sample: [1, 3, 9, 11]
        assert_eq!(overview.data[0], 1);
        assert_eq!(overview.data[1], 3);
        assert_eq!(overview.data[2], 9);
        assert_eq!(overview.data[3], 11);
    }

    #[test]
    fn test_mode_finding() {
        let generator = OverviewGenerator::new(
            1,
            1,
            1,
            1,
            OverviewResampling::Mode,
            oxigdal_core::types::RasterDataType::UInt8,
        );

        let values = vec![1, 2, 2, 3, 3, 3, 4];
        assert_eq!(generator.find_mode(&values), 3);

        let single = vec![42];
        assert_eq!(generator.find_mode(&single), 42);
    }

    #[test]
    fn test_invalid_factor() {
        let generator = OverviewGenerator::new(
            1024,
            1024,
            1,
            1,
            OverviewResampling::Nearest,
            oxigdal_core::types::RasterDataType::UInt8,
        );
        let data = vec![0u8; 1024 * 1024];

        let result = generator.generate_overviews(&data, &[1]); // Invalid: factor < 2
        assert!(result.is_err());
    }
}
