//! glTF 2.0 and GLB export
//!
//! Provides export functionality for glTF 2.0 format (JSON + binary) and GLB (binary container).

use crate::error::{Error, Result};
use crate::mesh::Mesh;
use byteorder::{LittleEndian, WriteBytesExt};
use serde_json::json;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// GLB magic number
const GLB_MAGIC: u32 = 0x46546C67; // "glTF"
const GLB_VERSION: u32 = 2;

/// GLB chunk types
const GLB_CHUNK_JSON: u32 = 0x4E4F534A; // "JSON"
const GLB_CHUNK_BIN: u32 = 0x004E4942; // "BIN\0"

/// glTF exporter
pub struct GltfExporter;

impl GltfExporter {
    /// Export mesh to glTF (JSON + .bin file)
    pub fn export_gltf<P: AsRef<Path>>(mesh: &Mesh, path: P) -> Result<()> {
        mesh.validate()?;

        let path = path.as_ref();

        // Create binary buffer file
        let bin_path = path.with_extension("bin");
        let (buffer_data, buffer_size) = Self::create_buffer(mesh)?;
        std::fs::write(&bin_path, &buffer_data)?;

        // Create glTF JSON
        let gltf_json = Self::create_gltf_json(
            mesh,
            bin_path
                .file_name()
                .ok_or_else(|| Error::Gltf("Invalid bin filename".to_string()))?
                .to_string_lossy()
                .to_string(),
            buffer_size,
        )?;

        // Write JSON file
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &gltf_json)?;

        Ok(())
    }

    /// Export mesh to GLB (binary glTF)
    pub fn export_glb<P: AsRef<Path>>(mesh: &Mesh, path: P) -> Result<()> {
        mesh.validate()?;

        // Create binary buffer
        let (buffer_data, buffer_size) = Self::create_buffer(mesh)?;

        // Create glTF JSON (without external buffer reference)
        let gltf_json = Self::create_gltf_json(mesh, String::new(), buffer_size)?;
        let json_string = serde_json::to_string(&gltf_json)?;
        let json_bytes = json_string.as_bytes();

        // Pad JSON to 4-byte alignment
        let json_padding = (4 - (json_bytes.len() % 4)) % 4;
        let json_length = json_bytes.len() + json_padding;

        // Pad binary buffer to 4-byte alignment
        let bin_padding = (4 - (buffer_data.len() % 4)) % 4;
        let bin_length = buffer_data.len() + bin_padding;

        // Calculate total file size
        let total_size = 12 + 8 + json_length + 8 + bin_length;

        // Write GLB file
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // GLB header
        writer.write_u32::<LittleEndian>(GLB_MAGIC)?;
        writer.write_u32::<LittleEndian>(GLB_VERSION)?;
        writer.write_u32::<LittleEndian>(total_size as u32)?;

        // JSON chunk header
        writer.write_u32::<LittleEndian>(json_length as u32)?;
        writer.write_u32::<LittleEndian>(GLB_CHUNK_JSON)?;

        // JSON chunk data
        writer.write_all(json_bytes)?;
        for _ in 0..json_padding {
            writer.write_u8(0x20)?; // Space character for padding
        }

        // Binary chunk header
        writer.write_u32::<LittleEndian>(bin_length as u32)?;
        writer.write_u32::<LittleEndian>(GLB_CHUNK_BIN)?;

        // Binary chunk data
        writer.write_all(&buffer_data)?;
        for _ in 0..bin_padding {
            writer.write_u8(0)?; // Null bytes for padding
        }

        writer.flush()?;
        Ok(())
    }

    /// Create binary buffer containing all mesh data
    fn create_buffer(mesh: &Mesh) -> Result<(Vec<u8>, usize)> {
        let mut buffer = Vec::new();

        // Write positions (vec3 float)
        for vertex in &mesh.vertices {
            buffer.write_f32::<LittleEndian>(vertex.position[0])?;
            buffer.write_f32::<LittleEndian>(vertex.position[1])?;
            buffer.write_f32::<LittleEndian>(vertex.position[2])?;
        }

        // Write normals (vec3 float)
        for vertex in &mesh.vertices {
            buffer.write_f32::<LittleEndian>(vertex.normal[0])?;
            buffer.write_f32::<LittleEndian>(vertex.normal[1])?;
            buffer.write_f32::<LittleEndian>(vertex.normal[2])?;
        }

        // Write texture coordinates (vec2 float)
        for vertex in &mesh.vertices {
            buffer.write_f32::<LittleEndian>(vertex.tex_coords[0])?;
            buffer.write_f32::<LittleEndian>(vertex.tex_coords[1])?;
        }

        // Write indices (uint32)
        for triangle in &mesh.triangles {
            buffer.write_u32::<LittleEndian>(triangle.indices[0])?;
            buffer.write_u32::<LittleEndian>(triangle.indices[1])?;
            buffer.write_u32::<LittleEndian>(triangle.indices[2])?;
        }

        let size = buffer.len();
        Ok((buffer, size))
    }

    /// Create glTF JSON structure
    fn create_gltf_json(
        mesh: &Mesh,
        buffer_uri: String,
        buffer_size: usize,
    ) -> Result<serde_json::Value> {
        let vertex_count = mesh.vertex_count();
        let triangle_count = mesh.triangle_count();

        // Calculate buffer offsets
        let positions_offset = 0;
        let positions_size = vertex_count * 12; // 3 floats * 4 bytes

        let normals_offset = positions_size;
        let normals_size = vertex_count * 12;

        let texcoords_offset = normals_offset + normals_size;
        let texcoords_size = vertex_count * 8; // 2 floats * 4 bytes

        let indices_offset = texcoords_offset + texcoords_size;
        let indices_size = triangle_count * 12; // 3 uint32 * 4 bytes

        // Calculate bounding box for positions
        let bbox = mesh.bounding_box().unwrap_or(([0.0; 3], [0.0; 3]));

        // Build glTF JSON
        let gltf = json!({
            "asset": {
                "version": "2.0",
                "generator": "OxiGDAL 3D",
            },
            "scene": 0,
            "scenes": [
                {
                    "nodes": [0]
                }
            ],
            "nodes": [
                {
                    "mesh": 0
                }
            ],
            "meshes": [
                {
                    "primitives": [
                        {
                            "attributes": {
                                "POSITION": 0,
                                "NORMAL": 1,
                                "TEXCOORD_0": 2,
                            },
                            "indices": 3,
                            "mode": 4, // TRIANGLES
                            "material": 0,
                        }
                    ]
                }
            ],
            "materials": [
                {
                    "name": mesh.material.name,
                    "pbrMetallicRoughness": {
                        "baseColorFactor": mesh.material.base_color,
                        "metallicFactor": mesh.material.metallic,
                        "roughnessFactor": mesh.material.roughness,
                    }
                }
            ],
            "accessors": [
                // 0: POSITION
                {
                    "bufferView": 0,
                    "componentType": 5126, // FLOAT
                    "count": vertex_count,
                    "type": "VEC3",
                    "min": bbox.0,
                    "max": bbox.1,
                },
                // 1: NORMAL
                {
                    "bufferView": 1,
                    "componentType": 5126, // FLOAT
                    "count": vertex_count,
                    "type": "VEC3",
                },
                // 2: TEXCOORD_0
                {
                    "bufferView": 2,
                    "componentType": 5126, // FLOAT
                    "count": vertex_count,
                    "type": "VEC2",
                },
                // 3: INDICES
                {
                    "bufferView": 3,
                    "componentType": 5125, // UNSIGNED_INT
                    "count": triangle_count * 3,
                    "type": "SCALAR",
                },
            ],
            "bufferViews": [
                // 0: Positions
                {
                    "buffer": 0,
                    "byteOffset": positions_offset,
                    "byteLength": positions_size,
                    "target": 34962, // ARRAY_BUFFER
                },
                // 1: Normals
                {
                    "buffer": 0,
                    "byteOffset": normals_offset,
                    "byteLength": normals_size,
                    "target": 34962, // ARRAY_BUFFER
                },
                // 2: Texture coordinates
                {
                    "buffer": 0,
                    "byteOffset": texcoords_offset,
                    "byteLength": texcoords_size,
                    "target": 34962, // ARRAY_BUFFER
                },
                // 3: Indices
                {
                    "buffer": 0,
                    "byteOffset": indices_offset,
                    "byteLength": indices_size,
                    "target": 34963, // ELEMENT_ARRAY_BUFFER
                },
            ],
            "buffers": [
                if buffer_uri.is_empty() {
                    json!({
                        "byteLength": buffer_size,
                    })
                } else {
                    json!({
                        "uri": buffer_uri,
                        "byteLength": buffer_size,
                    })
                }
            ],
        });

        Ok(gltf)
    }
}

/// Export mesh to glTF (convenience function)
pub fn export_gltf<P: AsRef<Path>>(mesh: &Mesh, path: P) -> Result<()> {
    GltfExporter::export_gltf(mesh, path)
}

/// Export mesh to GLB (convenience function)
pub fn export_glb<P: AsRef<Path>>(mesh: &Mesh, path: P) -> Result<()> {
    GltfExporter::export_glb(mesh, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Material, Vertex};
    use std::env;

    #[test]
    fn test_gltf_export() {
        let mut mesh = Mesh::new();

        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
        mesh.add_triangle(0, 1, 2);

        mesh.calculate_normals();

        let temp_dir = env::temp_dir();
        let output_path = temp_dir.join("test_mesh.gltf");

        let result = export_gltf(&mesh, &output_path);
        assert!(result.is_ok());

        // Check files exist
        assert!(output_path.exists());
        let bin_path = output_path.with_extension("bin");
        assert!(bin_path.exists());

        // Clean up
        let _ = std::fs::remove_file(&output_path);
        let _ = std::fs::remove_file(&bin_path);
    }

    #[test]
    fn test_glb_export() {
        let mut mesh = Mesh::new();

        mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
        mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
        mesh.add_triangle(0, 1, 2);

        mesh.calculate_normals();
        mesh.material = Material::new("test").with_color(0.8, 0.8, 0.8, 1.0);

        let temp_dir = env::temp_dir();
        let output_path = temp_dir.join("test_mesh.glb");

        let result = export_glb(&mesh, &output_path);
        assert!(result.is_ok());

        // Check file exists
        assert!(output_path.exists());

        // Check file has correct magic number
        let data = std::fs::read(&output_path).expect("Should be able to read generated GLB file");
        assert_eq!(&data[0..4], b"glTF");

        // Clean up
        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn test_buffer_creation() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new([1.0, 2.0, 3.0]));
        mesh.add_vertex(Vertex::new([4.0, 5.0, 6.0]));
        mesh.add_triangle(0, 1, 0);

        let (buffer, size) = GltfExporter::create_buffer(&mesh)
            .expect("Buffer creation should succeed for valid mesh");

        // Expected size: 2 vertices * (3 floats * 3 attributes) * 4 bytes + 1 triangle * 3 indices * 4 bytes
        // = 2 * 3 * (3 + 3 + 2) * 4 + 3 * 4 = 2 * 8 * 4 + 12 = 64 + 12 = 76
        // Actually: positions(2*3*4=24) + normals(2*3*4=24) + texcoords(2*2*4=16) + indices(3*4=12) = 76
        assert_eq!(size, 76);
        assert_eq!(buffer.len(), 76);
    }
}
