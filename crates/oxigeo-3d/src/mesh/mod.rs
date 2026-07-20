//! 3D mesh formats and operations
//!
//! This module provides support for various 3D mesh formats including:
//! - OBJ format export
//! - glTF 2.0 / GLB (binary) export
//! - Texture mapping and materials
//! - Normal calculation and optimization

pub mod gltf_export;
pub mod obj;

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// 3D vertex with position, normal, and texture coordinates
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    /// Position (x, y, z)
    pub position: [f32; 3],
    /// Normal vector (nx, ny, nz)
    pub normal: [f32; 3],
    /// Texture coordinates (u, v)
    pub tex_coords: [f32; 2],
}

impl Vertex {
    /// Create a new vertex
    pub fn new(position: [f32; 3]) -> Self {
        Self {
            position,
            normal: [0.0, 0.0, 1.0],
            tex_coords: [0.0, 0.0],
        }
    }

    /// Create a vertex with all attributes
    pub fn with_attributes(position: [f32; 3], normal: [f32; 3], tex_coords: [f32; 2]) -> Self {
        Self {
            position,
            normal,
            tex_coords,
        }
    }
}

/// Triangle face with vertex indices
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Triangle {
    /// Vertex indices (v0, v1, v2)
    pub indices: [u32; 3],
}

impl Triangle {
    /// Create a new triangle
    pub fn new(v0: u32, v1: u32, v2: u32) -> Self {
        Self {
            indices: [v0, v1, v2],
        }
    }

    /// Get vertices from a mesh
    pub fn vertices<'a>(&self, mesh: &'a Mesh) -> [&'a Vertex; 3] {
        [
            &mesh.vertices[self.indices[0] as usize],
            &mesh.vertices[self.indices[1] as usize],
            &mesh.vertices[self.indices[2] as usize],
        ]
    }
}

/// Material properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Material {
    /// Material name
    pub name: String,
    /// Base color (RGBA)
    pub base_color: [f32; 4],
    /// Metallic factor (0.0 = dielectric, 1.0 = metal)
    pub metallic: f32,
    /// Roughness factor (0.0 = smooth, 1.0 = rough)
    pub roughness: f32,
    /// Texture image path (optional)
    pub texture: Option<String>,
}

impl Material {
    /// Create a new material
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base_color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            texture: None,
        }
    }

    /// Set base color
    pub fn with_color(mut self, r: f32, g: f32, b: f32, a: f32) -> Self {
        self.base_color = [r, g, b, a];
        self
    }

    /// Set texture
    pub fn with_texture(mut self, path: impl Into<String>) -> Self {
        self.texture = Some(path.into());
        self
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::new("default")
    }
}

/// 3D mesh structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mesh {
    /// Vertices
    pub vertices: Vec<Vertex>,
    /// Triangles (indices into vertices)
    pub triangles: Vec<Triangle>,
    /// Material
    pub material: Material,
}

impl Mesh {
    /// Create a new empty mesh
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            triangles: Vec::new(),
            material: Material::default(),
        }
    }

    /// Create a mesh with vertices and triangles
    pub fn with_data(vertices: Vec<Vertex>, triangles: Vec<Triangle>) -> Self {
        Self {
            vertices,
            triangles,
            material: Material::default(),
        }
    }

    /// Add a vertex
    pub fn add_vertex(&mut self, vertex: Vertex) -> u32 {
        let index = self.vertices.len() as u32;
        self.vertices.push(vertex);
        index
    }

    /// Add a triangle
    pub fn add_triangle(&mut self, v0: u32, v1: u32, v2: u32) {
        self.triangles.push(Triangle::new(v0, v1, v2));
    }

    /// Get number of vertices
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Get number of triangles
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Calculate normals for all vertices (average of adjacent face normals)
    pub fn calculate_normals(&mut self) {
        // Reset all normals
        for vertex in &mut self.vertices {
            vertex.normal = [0.0, 0.0, 0.0];
        }

        // Accumulate face normals
        for triangle in &self.triangles {
            let v0 = &self.vertices[triangle.indices[0] as usize].position;
            let v1 = &self.vertices[triangle.indices[1] as usize].position;
            let v2 = &self.vertices[triangle.indices[2] as usize].position;

            // Calculate face normal
            let edge1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
            let edge2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];

            let normal = [
                edge1[1] * edge2[2] - edge1[2] * edge2[1],
                edge1[2] * edge2[0] - edge1[0] * edge2[2],
                edge1[0] * edge2[1] - edge1[1] * edge2[0],
            ];

            // Add to each vertex of the triangle
            for &idx in &triangle.indices {
                let v = &mut self.vertices[idx as usize].normal;
                v[0] += normal[0];
                v[1] += normal[1];
                v[2] += normal[2];
            }
        }

        // Normalize all normals
        for vertex in &mut self.vertices {
            let n = &mut vertex.normal;
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if length > 0.0 {
                n[0] /= length;
                n[1] /= length;
                n[2] /= length;
            }
        }
    }

    /// Calculate bounding box
    pub fn bounding_box(&self) -> Option<([f32; 3], [f32; 3])> {
        if self.vertices.is_empty() {
            return None;
        }

        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];

        for vertex in &self.vertices {
            for i in 0..3 {
                min[i] = min[i].min(vertex.position[i]);
                max[i] = max[i].max(vertex.position[i]);
            }
        }

        Some((min, max))
    }

    /// Validate mesh (check for invalid indices, degenerate triangles)
    pub fn validate(&self) -> Result<()> {
        let vertex_count = self.vertices.len();

        for (i, triangle) in self.triangles.iter().enumerate() {
            // Check indices are valid
            for &idx in &triangle.indices {
                if idx as usize >= vertex_count {
                    return Err(Error::InvalidMesh(format!(
                        "Triangle {} has invalid vertex index: {}",
                        i, idx
                    )));
                }
            }

            // Check for degenerate triangles (duplicate indices)
            if triangle.indices[0] == triangle.indices[1]
                || triangle.indices[1] == triangle.indices[2]
                || triangle.indices[0] == triangle.indices[2]
            {
                return Err(Error::InvalidMesh(format!(
                    "Triangle {} is degenerate: {:?}",
                    i, triangle.indices
                )));
            }
        }

        Ok(())
    }
}

impl Default for Mesh {
    fn default() -> Self {
        Self::new()
    }
}

// Re-exports
pub use gltf_export::{GltfExporter, export_glb, export_gltf};
pub use obj::{ObjExporter, export_obj};

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_vertex_creation() {
        let v = Vertex::new([1.0, 2.0, 3.0]);
        assert_eq!(v.position, [1.0, 2.0, 3.0]);
        assert_eq!(v.normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_triangle_creation() {
        let t = Triangle::new(0, 1, 2);
        assert_eq!(t.indices, [0, 1, 2]);
    }

    #[test]
    fn test_mesh_creation() {
        let mut mesh = Mesh::new();
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.triangle_count(), 0);

        let v0 = mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        let v1 = mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        let v2 = mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));

        mesh.add_triangle(v0, v1, v2);

        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
    }

    #[test]
    fn test_calculate_normals() {
        let mut mesh = Mesh::new();

        // Create a simple triangle
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
        mesh.add_triangle(0, 1, 2);

        mesh.calculate_normals();

        // Normal should point in +Z direction
        let normal = mesh.vertices[0].normal;
        assert_relative_eq!(normal[0], 0.0, epsilon = 0.0001);
        assert_relative_eq!(normal[1], 0.0, epsilon = 0.0001);
        assert_relative_eq!(normal[2], 1.0, epsilon = 0.0001);
    }

    #[test]
    fn test_bounding_box() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([10.0, 10.0, 10.0]));

        let bbox = mesh
            .bounding_box()
            .expect("Bounding box should exist for mesh with vertices");
        assert_eq!(bbox.0, [0.0, 0.0, 0.0]);
        assert_eq!(bbox.1, [10.0, 10.0, 10.0]);
    }

    #[test]
    fn test_validate_valid_mesh() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
        mesh.add_triangle(0, 1, 2);

        assert!(mesh.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_index() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_triangle(0, 1, 2); // Indices 1 and 2 are invalid

        assert!(mesh.validate().is_err());
    }

    #[test]
    fn test_validate_degenerate() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        mesh.add_triangle(0, 0, 1); // Degenerate (0 appears twice)

        assert!(mesh.validate().is_err());
    }
}
