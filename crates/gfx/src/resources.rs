use std::io;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};

use cgmath::{InnerSpace, Vector2, Vector3, Vector4, Zero};
use image::Rgb;
use thiserror::Error;
use tracing::warn;
use wgpu::util::DeviceExt;
use wgpu::{Device, Queue};

use crate::model::{Material, Mesh, Model, ModelVertex};
use crate::texture;
use crate::texture::Texture;

const ASSETS_PATH: &str = "assets";

#[derive(Error, Debug)]
pub enum LoadError {
    #[error("failed to load string from path `{1}`")]
    String(#[source] io::Error, PathBuf),
    #[error("failed to load texture at path `{1}`")]
    Texture(#[source] texture::TextureError, PathBuf),
    #[error("failed to load bytes from binary file at path `{1}`")]
    Binary(#[source] io::Error, PathBuf),
    #[error("failed to load meshes from OBJ buffer")]
    Mesh(#[source] tobj::LoadError),
    #[error("failed to load materials from OBJ buffer")]
    Materials(#[source] tobj::LoadError),
}

type Result<T, E = LoadError> = std::result::Result<T, E>;

impl LoadError {
    fn into_tobj_error(self) -> Option<tobj::LoadError> {
        match self {
            LoadError::Mesh(error) => Some(error),
            LoadError::Materials(error) => Some(error),
            _ => None,
        }
    }
}

pub fn load_string(file_path: &Path) -> Result<String> {
    let path = Path::new(env!("OUT_DIR")).join(ASSETS_PATH).join(file_path);
    let txt =
        std::fs::read_to_string(path).map_err(|e| LoadError::String(e, file_path.to_owned()))?;
    Ok(txt)
}

pub fn load_binary(file_path: &Path) -> Result<Vec<u8>> {
    let path = Path::new(env!("OUT_DIR")).join(ASSETS_PATH).join(file_path);
    let data = std::fs::read(path).map_err(|e| LoadError::Binary(e, file_path.to_owned()))?;
    Ok(data)
}

pub fn load_texture(
    file_path: &Path,
    is_normal_map: bool,
    device: &Device,
    queue: &Queue,
) -> Result<Texture> {
    let data = load_binary(file_path)?;
    let file_name = file_path.to_string_lossy();
    Texture::from_bytes(
        device,
        queue,
        &data,
        Some(file_name.as_ref()),
        is_normal_map,
    )
    .map_err(|e| LoadError::Texture(e, file_path.to_owned()))
}

pub fn load_model(
    file_path: &Path,
    device: &Device,
    queue: &Queue,
    layout: &wgpu::BindGroupLayout,
) -> Result<Model> {
    let obj_text = load_string(file_path)?;
    let obj_cursor = Cursor::new(obj_text);
    let mut obj_reader = BufReader::new(obj_cursor);

    let (models, obj_materials) =
        tobj::load_obj_buf(&mut obj_reader, &tobj::GPU_LOAD_OPTIONS, |material_name| {
            let mat_text = load_string(material_name).map_err(|e| {
                e.into_tobj_error()
                    .expect("LoadError::String can always be converted into tobj::Error")
            })?;
            tobj::load_mtl_buf(&mut BufReader::new(Cursor::new(mat_text)))
        })
        .map_err(LoadError::Mesh)?;

    let mut materials = Vec::new();
    for m in obj_materials.map_err(LoadError::Materials)? {
        let diffuse_texture = if m.diffuse_texture.is_empty() {
            None
        } else {
            Some(load_texture(
                Path::new(&m.diffuse_texture),
                false,
                device,
                queue,
            )?)
        };
        let normal_texture = if m.normal_texture.is_empty() {
            // If no custom normal map is present, use a single-pixel normal map, as this is the
            // same as performing no normal-mapping at all (uses per-vertex normals instead).
            // (128, 128, 255) maps to (0, 0, 1) since normal coordinates are [-1, 1]
            let orthogonal_normal = Rgb([128, 128, 255]);
            let file_name = file_path.to_string_lossy();
            Texture::from_pixel(device, queue, orthogonal_normal, Some(file_name.as_ref()))
                .map_err(|e| LoadError::Texture(e, file_path.to_owned()))
        } else {
            Ok(load_texture(
                Path::new(&m.normal_texture),
                true,
                device,
                queue,
            )?)
        }?;

        materials.push(Material::new(
            device,
            queue,
            &m.name,
            m.diffuse,
            diffuse_texture,
            normal_texture,
            layout,
        ));
    }

    let meshes = models
        .into_iter()
        .map(|m| {
            // Some meshes don't have normals, so automatically generate some based on vertices.
            let auto_normals: Vec<Vector3<f32>> = generate_normals(&m.mesh);

            if m.mesh.normals.is_empty() {
                warn!(
                    "mesh in model {0} has no normals, using auto-generated ones",
                    m.name
                );
            }

            let mut vertices = (0..m.mesh.positions.len() / 3)
                .map(|i| {
                    let normal = if m.mesh.normals.len() >= 3 {
                        [
                            m.mesh.normals[i * 3],
                            m.mesh.normals[i * 3 + 1],
                            m.mesh.normals[i * 3 + 2],
                        ]
                    } else {
                        auto_normals[i].into()
                    };
                    ModelVertex {
                        position: [
                            m.mesh.positions[i * 3],
                            m.mesh.positions[i * 3 + 1],
                            m.mesh.positions[i * 3 + 2],
                        ],
                        tex_coords: [m.mesh.texcoords[i * 2], m.mesh.texcoords[i * 2 + 1]],
                        normal,
                        // These are calculated later.
                        tangent: [0.0; 3],
                        bitangent: [0.0; 3],
                    }
                })
                .collect::<Vec<_>>();

            calculate_tangents_bitangents(&m, &mut vertices);

            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{file_path:?} Vertex Buffer")),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{file_path:?} Index Buffer")),
                contents: bytemuck::cast_slice(&m.mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });

            Mesh {
                name: file_path.to_string_lossy().to_string(),
                vertex_buffer,
                index_buffer,
                num_elements: m.mesh.indices.len() as u32,
                material_index: m.mesh.material_id.unwrap_or(0),
            }
        })
        .collect::<Vec<_>>();

    Ok(Model { meshes, materials })
}

fn generate_normals(mesh: &tobj::Mesh) -> Vec<Vector3<f32>> {
    let mut normals: Vec<Vector4<f32>> = (0..mesh.positions.len() / 3)
        .map(|_| Vector4::zero())
        .collect();
    for face in 0..mesh.indices.len() / 3 {
        let vertex0_index = mesh.indices[face * 3] as usize;
        let vertex0: Vector3<_> = [
            mesh.positions[vertex0_index * 3],
            mesh.positions[vertex0_index * 3 + 1],
            mesh.positions[vertex0_index * 3 + 2],
        ]
        .into();
        let vertex1_index = mesh.indices[face * 3 + 1] as usize;
        let vertex1: Vector3<_> = [
            mesh.positions[vertex1_index * 3],
            mesh.positions[vertex1_index * 3 + 1],
            mesh.positions[vertex1_index * 3 + 2],
        ]
        .into();
        let vertex2_index = mesh.indices[face * 3 + 2] as usize;
        let vertex2: Vector3<_> = [
            mesh.positions[vertex2_index * 3],
            mesh.positions[vertex2_index * 3 + 1],
            mesh.positions[vertex2_index * 3 + 2],
        ]
        .into();

        let edge0 = (vertex1 - vertex0).normalize();
        let edge1 = (vertex2 - vertex0).normalize();
        let face_normal = edge0.cross(edge1);

        // w = 1 so it can be used to normalize vertex normals, because a single vertex
        // may be present in multiple triangles.
        normals[vertex0_index] += face_normal.extend(1.0);
        normals[vertex1_index] += face_normal.extend(1.0);
        normals[vertex2_index] += face_normal.extend(1.0);
    }

    for normal in normals.iter_mut() {
        *normal = (1.0 / normal.w) * *normal;
    }

    normals
        .into_iter()
        .map(|normal| normal.truncate() / normal.w)
        .collect()
}

fn calculate_tangents_bitangents(m: &tobj::Model, vertices: &mut Vec<ModelVertex>) {
    let indices = &m.mesh.indices;
    let mut triangles_included = vec![0; vertices.len()];

    // Calculate tangents and bitangents.
    for triangle in indices.chunks(3) {
        let v0 = vertices[triangle[0] as usize];
        let v1 = vertices[triangle[1] as usize];
        let v2 = vertices[triangle[2] as usize];

        let pos0: Vector3<_> = v0.position.into();
        let pos1: Vector3<_> = v1.position.into();
        let pos2: Vector3<_> = v2.position.into();

        let uv0: Vector2<_> = v0.tex_coords.into();
        let uv1: Vector2<_> = v1.tex_coords.into();
        let uv2: Vector2<_> = v2.tex_coords.into();

        let edge1 = pos1 - pos0;
        let edge2 = pos2 - pos0;

        // This will give us a direction to calculate the tangent and bitangent.
        let delta_uv1 = uv1 - uv0;
        let delta_uv2 = uv2 - uv0;

        // Solving the following system of equations will give us the tangent and bitangent:
        //     edge1 = delta_uv1.x * T + delta_u.y * B
        //     edge2 = delta_uv2.x * T + delta_uv2.y * B
        let r = 1.0 / (delta_uv1.x * delta_uv2.y - delta_uv1.y * delta_uv2.x);
        let tangent = (edge1 * delta_uv2.y - edge2 * delta_uv1.y) * r;
        // We flip the bitangent to enable right-handed normal
        // maps with wgpu texture coordinate system
        let bitangent = (edge2 * delta_uv1.x - edge1 * delta_uv2.x) * -r;

        // We'll use the same tangent/bitangent for each vertex in the triangle
        for vertex in 0..3 {
            vertices[triangle[vertex] as usize].tangent =
                (tangent + Vector3::from(vertices[triangle[vertex] as usize].tangent)).into();
            vertices[triangle[vertex] as usize].bitangent =
                (bitangent + Vector3::from(vertices[triangle[vertex] as usize].bitangent)).into();
            // Used to average the tangents/bitangents
            triangles_included[triangle[vertex] as usize] += 1;
        }
    }

    // Average the tangents/bitangents
    for (i, n) in triangles_included.into_iter().enumerate() {
        let denom = 1.0 / n as f32;
        let mut v = &mut vertices[i];
        v.tangent = (cgmath::Vector3::from(v.tangent) * denom).into();
        v.bitangent = (cgmath::Vector3::from(v.bitangent) * denom).into();
    }
}
