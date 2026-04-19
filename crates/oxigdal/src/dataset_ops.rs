//! Spatial operations on [`Dataset`]: clipping and reprojection.
//!
//! These methods produce a new `Dataset` whose metadata reflects the requested
//! spatial transformation.  They do not read or write pixel data — only the
//! `DatasetInfo` (geo-transform, CRS, dimensions) is updated.

use crate::{Dataset, DatasetInfo};
use oxigdal_core::error::OxiGdalError;
use oxigdal_core::error::Result;
use oxigdal_core::types::{BoundingBox, GeoTransform};

impl Dataset {
    /// Inner implementation for [`Dataset::clip`].
    pub(crate) fn clip_to_bbox(&self, bbox: BoundingBox) -> Result<Dataset> {
        match self.info().geotransform {
            Some(gt) => {
                // Raster path: convert world bbox → pixel window, clamp to extent.
                //
                // We transform all four corners and take min/max to build the pixel
                // window.  This handles both north-up (pixel_height < 0) and south-up
                // (pixel_height > 0) geo-transforms as well as rotated rasters.
                let corners = [
                    gt.world_to_pixel(bbox.min_x, bbox.min_y),
                    gt.world_to_pixel(bbox.max_x, bbox.min_y),
                    gt.world_to_pixel(bbox.min_x, bbox.max_y),
                    gt.world_to_pixel(bbox.max_x, bbox.max_y),
                ];

                // Propagate any inversion error.
                let mut px_min = f64::INFINITY;
                let mut px_max = f64::NEG_INFINITY;
                let mut py_min = f64::INFINITY;
                let mut py_max = f64::NEG_INFINITY;

                for corner in corners {
                    let (cx, cy) = corner.map_err(|e| OxiGdalError::InvalidParameter {
                        parameter: "bbox",
                        message: format!("geo-transform inversion failed: {e}"),
                    })?;
                    if cx < px_min {
                        px_min = cx;
                    }
                    if cx > px_max {
                        px_max = cx;
                    }
                    if cy < py_min {
                        py_min = cy;
                    }
                    if cy > py_max {
                        py_max = cy;
                    }
                }

                // Helper: clamp v ≥ 0; clamp v ≤ dim only when dim is known (> 0).
                let clamp_pixel = |v: f64, dim: Option<u32>| -> u32 {
                    let floored = v.max(0.0);
                    match dim {
                        Some(d) if d > 0 => floored.min(d as f64) as u32,
                        _ => floored as u32,
                    }
                };

                let col0 = clamp_pixel(px_min, self.info().width);
                let row0 = clamp_pixel(py_min, self.info().height);
                let col1 = clamp_pixel(px_max, self.info().width);
                let row1 = clamp_pixel(py_max, self.info().height);

                if col1 <= col0 || row1 <= row0 {
                    return Err(OxiGdalError::InvalidParameter {
                        parameter: "bbox",
                        message: format!(
                            "bounding box does not intersect the dataset extent (pixel window [{col0},{row0}]–[{col1},{row1}])"
                        ),
                    });
                }

                let new_width = col1 - col0;
                let new_height = row1 - row0;

                // Compute new geo-transform origin (upper-left of clipped region).
                let (new_origin_x, new_origin_y) = gt.pixel_to_world(col0 as f64, row0 as f64);
                let new_gt = GeoTransform::north_up(
                    new_origin_x,
                    new_origin_y,
                    gt.pixel_width,
                    gt.pixel_height,
                );

                let mut new_info: DatasetInfo = self.info().clone();
                new_info.width = Some(new_width);
                new_info.height = Some(new_height);
                new_info.geotransform = Some(new_gt);

                Ok(Dataset::from_info(self.path().to_string(), new_info))
            }
            None => {
                // Vector path: no geo-transform available — just record the intent.
                if self.info().width.is_none() && self.info().height.is_none() {
                    // Pure vector dataset: store clip bbox as a note in the CRS field
                    // (the clip is a logical annotation; no physical subsetting occurs).
                    let mut new_info = self.info().clone();
                    // Preserve existing CRS; append bbox annotation when none exists.
                    if new_info.crs.is_none() {
                        new_info.crs = Some(format!(
                            "clipped to [{},{},{},{}]",
                            bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y
                        ));
                    }
                    Ok(Dataset::from_info(self.path().to_string(), new_info))
                } else {
                    Err(OxiGdalError::InvalidParameter {
                        parameter: "bbox",
                        message: "clip() requires a geo-transform; dataset has none".to_string(),
                    })
                }
            }
        }
    }

    /// Inner implementation for [`Dataset::reproject`].
    pub(crate) fn reproject_to_epsg(&self, target_epsg: u32) -> Result<Dataset> {
        #[cfg(feature = "proj")]
        {
            self.reproject_with_proj(target_epsg)
        }
        #[cfg(not(feature = "proj"))]
        {
            let _ = target_epsg;
            Err(OxiGdalError::NotSupported {
                operation: "reproject() requires the 'proj' feature flag".to_string(),
            })
        }
    }

    /// `proj`-feature implementation of [`Dataset::reproject`].
    #[cfg(feature = "proj")]
    fn reproject_with_proj(&self, target_epsg: u32) -> Result<Dataset> {
        let gt = self
            .info()
            .geotransform
            .ok_or_else(|| OxiGdalError::InvalidParameter {
                parameter: "dataset",
                message: "reproject() requires a geo-transform; dataset has none".to_string(),
            })?;

        let width = self.info().width.unwrap_or(0) as u64;
        let height = self.info().height.unwrap_or(0) as u64;
        if width == 0 || height == 0 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "dataset",
                message: "reproject() requires non-zero raster dimensions".to_string(),
            });
        }

        // Derive source EPSG from the stored CRS string.
        // Convention: look for "EPSG:<n>" or fall back to 4326 (WGS 84).
        let source_epsg = self
            .info()
            .crs
            .as_deref()
            .and_then(crate::extract_epsg_from_crs_string)
            .unwrap_or(4326);

        // Build the bounding box of the current raster in world coordinates.
        let src_bbox = gt.compute_bounds(width, height);

        // Convert to the proj-crate BoundingBox type.
        let proj_bbox = oxigdal_proj::BoundingBox::new(
            src_bbox.min_x,
            src_bbox.min_y,
            src_bbox.max_x,
            src_bbox.max_y,
        )
        .map_err(|e| OxiGdalError::InvalidParameter {
            parameter: "bbox",
            message: format!("invalid dataset bounding box: {e}"),
        })?;

        // Transform the bbox corners.
        let transformer =
            oxigdal_proj::Transformer::from_epsg(source_epsg, target_epsg).map_err(|e| {
                OxiGdalError::Crs(oxigdal_core::error::CrsError::TransformationError {
                    source_crs: format!("EPSG:{source_epsg}"),
                    target_crs: format!("EPSG:{target_epsg}"),
                    message: format!("failed to build transformer: {e}"),
                })
            })?;

        let dst_bbox = transformer.transform_bbox(&proj_bbox).map_err(|e| {
            OxiGdalError::Crs(oxigdal_core::error::CrsError::TransformationError {
                source_crs: format!("EPSG:{source_epsg}"),
                target_crs: format!("EPSG:{target_epsg}"),
                message: format!("bbox transformation failed: {e}"),
            })
        })?;

        // Build new geo-transform from the reprojected bbox keeping the same pixel count.
        let core_dst_bbox = BoundingBox::new(
            dst_bbox.min_x,
            dst_bbox.min_y,
            dst_bbox.max_x,
            dst_bbox.max_y,
        )?;
        let new_gt = GeoTransform::from_bounds(&core_dst_bbox, width, height)?;

        let new_crs = format!("EPSG:{target_epsg}");

        let mut new_info = self.info().clone();
        new_info.geotransform = Some(new_gt);
        new_info.crs = Some(new_crs);

        Ok(Dataset::from_info(self.path().to_string(), new_info))
    }
}
