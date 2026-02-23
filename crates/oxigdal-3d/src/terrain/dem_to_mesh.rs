//! DEM to 3D mesh conversion
//!
//! Converts Digital Elevation Models (raster grids) to 3D meshes with texture support.

use crate::error::{Error, Result};
use crate::mesh::{Material, Mesh, Vertex};
use serde::{Deserialize, Serialize};

/// DEM data structure (simplified raster representation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dem {
    /// Width in pixels
    pub width: usize,
    /// Height in pixels
    pub height: usize,
    /// Elevation values (row-major order)
    pub elevations: Vec<f32>,
    /// Geographic bounds (min_x, min_y, max_x, max_y)
    pub bounds: [f64; 4],
    /// No-data value
    pub nodata: Option<f32>,
}

impl Dem {
    /// Create a new DEM
    pub fn new(
        width: usize,
        height: usize,
        elevations: Vec<f32>,
        bounds: [f64; 4],
    ) -> Result<Self> {
        if elevations.len() != width * height {
            return Err(Error::DemToMesh(format!(
                "Elevation array size mismatch: expected {}, got {}",
                width * height,
                elevations.len()
            )));
        }

        Ok(Self {
            width,
            height,
            elevations,
            bounds,
            nodata: None,
        })
    }

    /// Get elevation at pixel coordinates
    pub fn get_elevation(&self, x: usize, y: usize) -> Option<f32> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let idx = y * self.width + x;
        let elev = self.elevations[idx];

        // Check for no-data
        if let Some(nodata) = self.nodata {
            if (elev - nodata).abs() < 1e-6 {
                return None;
            }
        }

        Some(elev)
    }

    /// Get geographic coordinates for pixel
    pub fn pixel_to_geo(&self, x: usize, y: usize) -> (f64, f64) {
        let [min_x, min_y, max_x, max_y] = self.bounds;

        let cell_width = (max_x - min_x) / self.width as f64;
        let cell_height = (max_y - min_y) / self.height as f64;

        let geo_x = min_x + (x as f64 + 0.5) * cell_width;
        let geo_y = max_y - (y as f64 + 0.5) * cell_height; // Y axis is inverted in rasters

        (geo_x, geo_y)
    }

    /// Calculate minimum elevation
    pub fn min_elevation(&self) -> f32 {
        self.elevations
            .iter()
            .filter(|&&e| {
                if let Some(nodata) = self.nodata {
                    (e - nodata).abs() > 1e-6
                } else {
                    true
                }
            })
            .copied()
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0)
    }

    /// Calculate maximum elevation
    pub fn max_elevation(&self) -> f32 {
        self.elevations
            .iter()
            .filter(|&&e| {
                if let Some(nodata) = self.nodata {
                    (e - nodata).abs() > 1e-6
                } else {
                    true
                }
            })
            .copied()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0)
    }
}

/// Options for DEM to mesh conversion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemMeshOptions {
    /// Vertical exaggeration factor
    pub vertical_exaggeration: f32,
    /// Simplification level (1 = no simplification, 2 = half resolution, etc.)
    pub simplification: usize,
    /// Whether to flip Y axis
    pub flip_y: bool,
    /// Whether to center the mesh at origin
    pub center_at_origin: bool,
    /// Texture image path (optional)
    pub texture: Option<String>,
    /// Generate texture coordinates
    pub generate_uvs: bool,
}

impl Default for DemMeshOptions {
    fn default() -> Self {
        Self {
            vertical_exaggeration: 1.0,
            simplification: 1,
            flip_y: false,
            center_at_origin: false,
            texture: None,
            generate_uvs: true,
        }
    }
}

impl DemMeshOptions {
    /// Create with vertical exaggeration
    pub fn with_exaggeration(mut self, factor: f32) -> Self {
        self.vertical_exaggeration = factor;
        self
    }

    /// Create with simplification
    pub fn with_simplification(mut self, level: usize) -> Self {
        self.simplification = level.max(1);
        self
    }

    /// Create with texture
    pub fn with_texture(mut self, path: impl Into<String>) -> Self {
        self.texture = Some(path.into());
        self
    }

    /// Enable centering at origin
    pub fn centered(mut self) -> Self {
        self.center_at_origin = true;
        self
    }
}

/// Convert DEM to 3D mesh
pub fn dem_to_mesh(dem: &Dem, options: &DemMeshOptions) -> Result<Mesh> {
    if dem.width < 2 || dem.height < 2 {
        return Err(Error::DemToMesh(
            "DEM must be at least 2x2 pixels".to_string(),
        ));
    }

    let mut mesh = Mesh::new();

    let step = options.simplification;
    let effective_width = dem.width.div_ceil(step);
    let effective_height = dem.height.div_ceil(step);

    // Calculate offset for centering
    let (offset_x, offset_y) = if options.center_at_origin {
        let [min_x, min_y, max_x, max_y] = dem.bounds;
        let center_x = (min_x + max_x) / 2.0;
        let center_y = (min_y + max_y) / 2.0;
        (center_x, center_y)
    } else {
        (0.0, 0.0)
    };

    // Create vertex grid
    let mut vertex_map = vec![vec![None; effective_width]; effective_height];

    for (y, row) in vertex_map.iter_mut().enumerate().take(effective_height) {
        for (x, cell) in row.iter_mut().enumerate().take(effective_width) {
            let src_x = (x * step).min(dem.width - 1);
            let src_y = (y * step).min(dem.height - 1);

            if let Some(elev) = dem.get_elevation(src_x, src_y) {
                let (geo_x, geo_y) = dem.pixel_to_geo(src_x, src_y);

                let mut position = [
                    (geo_x - offset_x) as f32,
                    (geo_y - offset_y) as f32,
                    elev * options.vertical_exaggeration,
                ];

                if options.flip_y {
                    position[1] = -position[1];
                }

                // Texture coordinates (normalized)
                let tex_coords = if options.generate_uvs {
                    [
                        x as f32 / (effective_width - 1).max(1) as f32,
                        y as f32 / (effective_height - 1).max(1) as f32,
                    ]
                } else {
                    [0.0, 0.0]
                };

                let vertex = Vertex::with_attributes(position, [0.0, 0.0, 1.0], tex_coords);
                let idx = mesh.add_vertex(vertex);
                *cell = Some(idx);
            }
        }
    }

    // Create triangles
    for y in 0..effective_height - 1 {
        for x in 0..effective_width - 1 {
            let v00 = vertex_map[y][x];
            let v10 = vertex_map[y][x + 1];
            let v01 = vertex_map[y + 1][x];
            let v11 = vertex_map[y + 1][x + 1];

            // Create two triangles per quad (if all vertices exist)
            match (v00, v10, v01, v11) {
                (Some(i00), Some(i10), Some(i01), Some(i11)) => {
                    // Triangle 1: (0,0) -> (1,0) -> (0,1)
                    mesh.add_triangle(i00, i10, i01);

                    // Triangle 2: (1,0) -> (1,1) -> (0,1)
                    mesh.add_triangle(i10, i11, i01);
                }
                _ => {
                    // Skip quads with missing vertices (no-data areas)
                }
            }
        }
    }

    // Calculate normals
    mesh.calculate_normals();

    // Set material
    if let Some(ref texture_path) = options.texture {
        mesh.material = Material::new("terrain").with_texture(texture_path);
    }

    Ok(mesh)
}

/// Create a tiled mesh from large DEM
pub fn dem_to_tiled_mesh(
    dem: &Dem,
    tile_size: usize,
    options: &DemMeshOptions,
) -> Result<Vec<Mesh>> {
    let mut tiles = Vec::new();

    let tiles_x = dem.width.div_ceil(tile_size);
    let tiles_y = dem.height.div_ceil(tile_size);

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let x_start = tx * tile_size;
            let y_start = ty * tile_size;
            let x_end = ((tx + 1) * tile_size).min(dem.width);
            let y_end = ((ty + 1) * tile_size).min(dem.height);

            let tile_width = x_end - x_start;
            let tile_height = y_end - y_start;

            // Extract tile elevations
            let mut tile_elevations = Vec::with_capacity(tile_width * tile_height);
            for y in y_start..y_end {
                for x in x_start..x_end {
                    let idx = y * dem.width + x;
                    tile_elevations.push(dem.elevations[idx]);
                }
            }

            // Calculate tile bounds
            let (min_x_geo, max_y_geo) = dem.pixel_to_geo(x_start, y_start);
            let (max_x_geo, min_y_geo) = dem.pixel_to_geo(x_end - 1, y_end - 1);

            let tile_bounds = [min_x_geo, min_y_geo, max_x_geo, max_y_geo];

            // Create tile DEM
            let tile_dem = Dem {
                width: tile_width,
                height: tile_height,
                elevations: tile_elevations,
                bounds: tile_bounds,
                nodata: dem.nodata,
            };

            // Convert tile to mesh
            let tile_mesh = dem_to_mesh(&tile_dem, options)?;
            tiles.push(tile_mesh);
        }
    }

    Ok(tiles)
}

/// Generate LOD (Level of Detail) meshes
pub fn dem_to_lod_meshes(
    dem: &Dem,
    num_levels: usize,
    options: &DemMeshOptions,
) -> Result<Vec<Mesh>> {
    let mut lods = Vec::with_capacity(num_levels);

    for level in 0..num_levels {
        let simplification = 1 << level; // 1, 2, 4, 8, ...
        let mut lod_options = options.clone();
        lod_options.simplification = simplification;

        let mesh = dem_to_mesh(dem, &lod_options)?;
        lods.push(mesh);
    }

    Ok(lods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_dem_creation() {
        let elevations = vec![1.0, 2.0, 3.0, 4.0];
        let bounds = [0.0, 0.0, 10.0, 10.0];

        let dem = Dem::new(2, 2, elevations, bounds);
        assert!(dem.is_ok());

        let dem = dem.expect("Failed to create 2x2 DEM with valid elevations");
        assert_eq!(dem.width, 2);
        assert_eq!(dem.height, 2);
    }

    #[test]
    fn test_dem_invalid_size() {
        let elevations = vec![1.0, 2.0, 3.0]; // Wrong size
        let bounds = [0.0, 0.0, 10.0, 10.0];

        let dem = Dem::new(2, 2, elevations, bounds);
        assert!(dem.is_err());
    }

    #[test]
    fn test_dem_get_elevation() {
        let elevations = vec![1.0, 2.0, 3.0, 4.0];
        let bounds = [0.0, 0.0, 10.0, 10.0];
        let dem = Dem::new(2, 2, elevations, bounds).expect("Failed to create 2x2 DEM");

        assert_relative_eq!(
            dem.get_elevation(0, 0)
                .expect("Elevation at (0,0) should exist"),
            1.0
        );
        assert_relative_eq!(
            dem.get_elevation(1, 0)
                .expect("Elevation at (1,0) should exist"),
            2.0
        );
        assert_relative_eq!(
            dem.get_elevation(0, 1)
                .expect("Elevation at (0,1) should exist"),
            3.0
        );
        assert_relative_eq!(
            dem.get_elevation(1, 1)
                .expect("Elevation at (1,1) should exist"),
            4.0
        );
    }

    #[test]
    fn test_dem_to_mesh() {
        let elevations = vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let bounds = [0.0, 0.0, 10.0, 10.0];
        let dem = Dem::new(3, 3, elevations, bounds).expect("Failed to create 3x3 DEM");

        let options = DemMeshOptions::default();
        let mesh = dem_to_mesh(&dem, &options);

        assert!(mesh.is_ok());
        let mesh = mesh.expect("Failed to convert 3x3 DEM to mesh");
        assert_eq!(mesh.vertex_count(), 9);
        assert!(mesh.triangle_count() > 0);
    }

    #[test]
    fn test_dem_to_mesh_with_exaggeration() {
        let elevations = vec![0.0, 1.0, 2.0, 3.0];
        let bounds = [0.0, 0.0, 10.0, 10.0];
        let dem = Dem::new(2, 2, elevations, bounds)
            .expect("Failed to create 2x2 DEM for exaggeration test");

        let options = DemMeshOptions::default().with_exaggeration(2.0);
        let mesh =
            dem_to_mesh(&dem, &options).expect("Failed to convert DEM to mesh with exaggeration");

        // Check that Z values are exaggerated
        assert_relative_eq!(mesh.vertices[1].position[2], 2.0);
    }

    #[test]
    fn test_dem_to_mesh_with_simplification() {
        let elevations = vec![
            0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];
        let bounds = [0.0, 0.0, 10.0, 10.0];
        let dem = Dem::new(4, 4, elevations, bounds)
            .expect("Failed to create 4x4 DEM for simplification test");

        let options = DemMeshOptions::default().with_simplification(2);
        let mesh = dem_to_mesh(&dem, &options).expect("Failed to convert DEM to simplified mesh");

        // Simplified mesh should have fewer vertices
        assert_eq!(mesh.vertex_count(), 4); // 2x2 grid
    }

    #[test]
    fn test_dem_min_max_elevation() {
        let elevations = vec![1.0, 5.0, 3.0, 10.0];
        let bounds = [0.0, 0.0, 10.0, 10.0];
        let dem = Dem::new(2, 2, elevations, bounds)
            .expect("Failed to create 2x2 DEM for min/max elevation test");

        assert_relative_eq!(dem.min_elevation(), 1.0);
        assert_relative_eq!(dem.max_elevation(), 10.0);
    }

    #[test]
    fn test_dem_to_lod_meshes() {
        let elevations = vec![
            0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let bounds = [0.0, 0.0, 10.0, 10.0];
        let dem =
            Dem::new(4, 4, elevations, bounds).expect("Failed to create 4x4 DEM for LOD test");

        let options = DemMeshOptions::default();
        let lods = dem_to_lod_meshes(&dem, 3, &options).expect("Failed to generate LOD meshes");

        assert_eq!(lods.len(), 3);
        // LOD 0 should have most vertices
        assert!(lods[0].vertex_count() >= lods[1].vertex_count());
        assert!(lods[1].vertex_count() >= lods[2].vertex_count());
    }
}
