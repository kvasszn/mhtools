use std::{collections::HashMap, fmt::Debug, fs::File, io::{BufWriter, Cursor, Read, Seek, SeekFrom, Write}};

use anyhow::{Context, Result, anyhow, bail};
use bytemuck::{Pod, Zeroable};
use byteorder::{LE, ReadBytesExt};
use num_enum::TryFromPrimitive;

use crate::{cast_data, cast_data_vec, read_pod};

// ty https://gist.github.com/Fothsid/67e46afea60d06f5feee7ffd241468d8

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
pub enum AmoMaterialType {
    MaterialType0 = 0,
    MaterialType1 = 1,
    MaterialType2 = 2,
    MaterialType5 = 5,
}


#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
pub enum AmoTextureType {
    TextureType0 = 0,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
pub enum AmoType {
    IndexedSubData=-1,
    //Unknown=0, // Shouldn't ever happen if properly handling Materials and Textures
    Start=1,
    Meshes=2,
    IndicesData=3,
    Mesh=4,
    IndexList=5,
    MaterialIds=6,
    PositionData=7,
    NormalData=8,
    MaterialHead=9, // This skips the first 0xc bytes (looks like a head, but it's not)
    TextureHead=0xa,

    // These don't have children (leafs)
    ModelAttributeUnk0x10000=0x10000,
    VersionOrSomething=0x20000, // I think this is actually kinda called Meshes in code, miight be
                                // some flag
    Indices1=0x30000,
    Indices2=0x40000,
    MaterialList=0x50000,
    MaterialIndex=0x60000,
    Vertex=0x70000,
    Normal=0x80000,
    TexCoords=0xa0000,
    Color=0xb0000,
    Weight=0xc0000,
    UnknownModelSomething0xe0000=0xe0000,
    Attribute=0xf0000,
    MatrixList=0x100000
}

impl AmoType {
    pub fn try_as_string(val: u32) -> String{
        AmoType::try_from_primitive(val as i32)
            .map(|t| format!("{t:?}"))
            .unwrap_or(val.to_string())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct AmoHead {
    ty: u32,
    count: u32,
    size: u32,
}

impl Debug for AmoHead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ty = AmoType::try_from_primitive(self.ty as i32)
            .map(|t| format!("{t:?}"))
            .unwrap_or(self.ty.to_string());
        f.debug_struct("AmoHead")
            .field("ty", &ty)
            .field("count", &self.count)
            .field("size", &self.size)
            .finish()
    }
}

pub fn enumerate_amo<R: Read + Seek>(r: &mut R, level: usize, parent: Option<u32>) -> Result<()> {
    let spacing = "    ".repeat(level);
    let pos = r.stream_position()?;
    let head = read_pod::<AmoHead, _>(r)?;

    let is_mat_or_tex_child = parent == Some(AmoType::MaterialHead as u32) || parent == Some(AmoType::TextureHead as u32);
    if !is_mat_or_tex_child && let Ok(ty) = AmoType::try_from_primitive(head.ty as i32) {
        println!("{spacing}{ty:?}: count={:#x}, size={:#x} @ {pos:#x}", head.count, head.size);
    } else {
        println!("{spacing}type={:#x}: count={:#x}, size={:#x} @ {pos:#x}", head.ty, head.count, head.size);
        r.seek(SeekFrom::Start(pos + head.size as u64))?;
        return Ok(())
    }

    if head.ty & 0xffff != 0 && !is_mat_or_tex_child {
        for _ in 0..head.count {
            enumerate_amo(r, level + 1, Some(head.ty))?;
        }
    }

    r.seek(SeekFrom::Start(pos + head.size as u64))?;
    Ok(())
}

#[derive(Debug)]
pub struct AmoHeadRef<'a> {
    pub head: &'a AmoHead,
    pub data: &'a [u8],
}

pub struct AmoNode<'a> {
    pub head: &'a AmoHead,
    pub children: Vec<AmoNode<'a>>,
    pub data: &'a [u8],
}

impl<'a> Debug for AmoNode<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AmoNode")
            .field("head", self.head)
            .field("children", &self.children)
            .finish()
    }
}

impl<'a> AmoNode<'a> {
    pub fn from_buf(data: &'a [u8], offset: &mut usize, parent: Option<u32>, level: usize) -> Result<Self> {
        let head: &AmoHead = bytemuck::try_from_bytes(&data[*offset..*offset+size_of::<AmoHead>()]).unwrap();

        let payload_size = head.size as usize - size_of::<AmoHead>();
        let payload_start = *offset + size_of::<AmoHead>();
        let payload_end = payload_start + payload_size;
        let payload_bytes = &data[payload_start..payload_end];

        let mut node = AmoNode {
            head,
            children: Vec::new(),
            data: payload_bytes,
        };

        let spacing = "    ".repeat(level);
        let ty_str = if parent == Some(AmoType::MaterialHead as u32) || parent == Some(AmoType::TextureHead as u32) {
            format!("type={}", head.ty)
        }
        else {
            AmoType::try_from_primitive(head.ty as i32)
                .map(|t| format!("{t:?}"))
                .unwrap_or(format!("unk_type={}", head.ty))
        };
        println!("{spacing}{ty_str}: count={:#x}, size={:#x} @ {offset:#x}", head.count, head.size);

        if head.ty & 0xffff != 0 && parent != Some(AmoType::MaterialHead as u32) {
            let mut child_offset = payload_start;
            for _ in 0..head.count {
                if child_offset >= payload_end { break; }
                node.children.push(AmoNode::from_buf(data, &mut child_offset, Some(head.ty), level + 1)?);
            }
        }

        *offset = payload_end;

        Ok(node)
    }

    pub fn find_node(&self, target_ty: AmoType) -> Option<&AmoNode<'a>> {
        if self.head.ty == target_ty as u32 {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find_node(target_ty) {
                return Some(found);
            }
        }
        None
    }

    // borked rn?
    pub fn find_node_idx(&self, target_ty: AmoType, idx: usize) -> Option<&AmoNode<'a>> {
        if self.head.ty == target_ty as u32 && idx == 0 {
            return Some(self);
        }
        for (i, child) in self.children.iter().enumerate() {
            if let Some(found) = child.find_node(target_ty) && i == idx {
                return Some(found);
            }
        }
        None
    }

    pub fn cast_node<T: Pod>(&self, target_ty: AmoType) -> Option<&T> {
        self.find_node(target_ty)
            .and_then(|n| cast_data::<T>(n.data).ok())
    }

    pub fn cast_node_slice<T: Pod>(&self, target_ty: AmoType) -> &'a [T] {
        self.find_node(target_ty)
            .and_then(|n| cast_data_vec::<T>(n.data).ok())
            .unwrap_or(&[])
    }

    pub fn cast_node_sub_datas<T: Pod>(&self, target_ty: AmoType) -> Option<Vec<&'a T>> {
        let node = self.find_node(target_ty)?;
        let mut res = Vec::with_capacity(node.children.len());
        for child in node.children.iter() {
            let value: &T = cast_data::<T>(child.data).ok()?;
            res.push(value);
        }
        Some(res)
    }

    pub fn write_obj_model(&self, file: &str) -> Result<()> {
        let meshes_node = self.find_node(AmoType::VersionOrSomething)
            .with_context(|| "Could not find Meshes container")?;
        let val = u32::from_le_bytes(meshes_node.data[0..4].try_into().unwrap());
        println!("{val:x}");

        let models_node = self.find_node(AmoType::Meshes)
            .with_context(|| "Could not find Models container")?;

        let file = std::fs::File::create(file).unwrap();
        let mut writer = std::io::BufWriter::new(file);

        let mut global_vertex_offset = 0;
        let mut total_triangles = 0;
        let mut model_count = 0;

        for model in &models_node.children[..1] {
            if model.head.ty != AmoType::Mesh as u32 {
                continue;
            }
            model_count += 1;

            let vertex_node = model.find_node(AmoType::Vertex).unwrap();
            let uv_node = model.find_node(AmoType::TexCoords).unwrap();
            let vertices: &[Vec3] = cast_data_vec::<Vec3>(vertex_node.data).unwrap();
            let uvs: &[Vec2] = cast_data_vec::<Vec2>(uv_node.data).unwrap();

            let mut all_indices = Vec::new();
            if let Some(index_list) = model.find_node(AmoType::IndexList) {
                for child in &index_list.children {
                    if child.head.ty == AmoType::Indices1 as u32 || child.head.ty == AmoType::Indices2 as u32 {
                        let mut decoded = decode_amo_indices(child.data, child.head.count);
                        all_indices.append(&mut decoded);
                    }
                }
            }

            for v in vertices {
                writeln!(writer, "v {} {} {}", v.x, v.y, v.z).unwrap();
            }

            for uv in uvs {
                writeln!(writer, "vt {} {}", uv.u, 1.0 - uv.v).unwrap();
            }

            for chunk in all_indices.chunks(3) {
                let i1 = chunk[0] + global_vertex_offset + 1;
                let i2 = chunk[1] + global_vertex_offset + 1;
                let i3 = chunk[2] + global_vertex_offset + 1;

                writeln!(writer, "f {}/{} {}/{} {}/{}", i1, i1, i2, i2, i3, i3).unwrap();
            }

            global_vertex_offset += vertices.len() as u32;
            total_triangles += all_indices.len() / 3;
        }

        /*println!(
            "Exported {} vertices and {} triangles across {} sub-models.", 
            global_vertex_offset, total_triangles, model_count
        );*/
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vec2 {
    pub u: f32,
    pub v: f32,
}

pub fn decode_amo_indices_grouped(payload: &[u8], strip_count: u32) -> Vec<Vec<u32>> {
    let mut cursor = Cursor::new(payload);
    let mut strips = Vec::with_capacity(strip_count as usize);

    for _ in 0..strip_count {
        let raw_count = cursor.read_u32::<LE>().unwrap();
        let index_count = raw_count & 0x7FFFFFFF; 

        let mut strip = Vec::with_capacity(index_count as usize);
        for _ in 0..index_count {
            let raw_idx = cursor.read_u32::<LE>().unwrap();
            strip.push(raw_idx & 0x00FFFFFF);
        }

        let mut flat_triangles = Vec::new();
        for i in 0..(strip.len().saturating_sub(2)) {
            let v1 = strip[i];
            let v2 = strip[i + 1];
            let v3 = strip[i + 2];

            if v1 == v2 || v2 == v3 || v1 == v3 {
                continue;
            }

            if i % 2 == 0 {
                flat_triangles.extend_from_slice(&[v1, v2, v3]);
            } else {
                flat_triangles.extend_from_slice(&[v1, v3, v2]);
            }
        }
        strips.push(flat_triangles);
    }

    strips
}

pub fn decode_amo_indices(payload: &[u8], strip_count: u32) -> Vec<u32> {
    let mut cursor = Cursor::new(payload);
    let mut flat_triangles = Vec::new();

    for _ in 0..strip_count {
        let raw_count = cursor.read_u32::<LE>().unwrap();
        let index_count = raw_count & 0x7FFFFFFF; 

        let mut strip = Vec::with_capacity(index_count as usize);
        for _ in 0..index_count {
            let raw_idx = cursor.read_u32::<LE>().unwrap();
            strip.push(raw_idx & 0x00FFFFFF);
        }

        for i in 0..(strip.len().saturating_sub(2)) {
            let v1 = strip[i];
            let v2 = strip[i + 1];
            let v3 = strip[i + 2];

            if v1 == v2 || v2 == v3 || v1 == v3 {
                continue;
            }

            if i % 2 == 0 {
                flat_triangles.push(v1);
                flat_triangles.push(v2);
                flat_triangles.push(v3);
            } else {
                flat_triangles.push(v1);
                flat_triangles.push(v3);
                flat_triangles.push(v2);
            }
        }
    }

    flat_triangles
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MaterialData {
    pub ambient: [f32; 4],
    pub diffuse: [f32; 4],
    pub specular: [f32; 4],
    pub shininess: f32,
    pub illumination: i32,
    pub _pad0: [u32; 50],
    pub texture_info_id: u32,
}
const _: () = assert!(size_of::<MaterialData>() == 0x104);

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TextureData {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub _pad0: [u32; 0xf4/4]
}
const _: () = assert!(size_of::<TextureData>() == 0x100);

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Joint {
    pub id: i32,
    pub weight: f32,
}
const _: () = assert!(size_of::<Joint>() == 0x8);

pub type MaterialId = i32;
pub type MatrixId = u32; // ?
pub type Vertex = Vec3;
pub type TexCoord = Vec2;
pub type Index = u32;

pub type MaterialListRef<'a> = &'a [i32];
pub type MatrixListRef<'a> = &'a [u32];
pub type JointsRef<'a> = &'a [Joint];
pub type VerticesRef<'a> = &'a [Vec3];
pub type TexCoordsRef<'a> = &'a [Vec2];
pub type IndicesRef<'a> = &'a [u32];

pub struct AmoPrimitive {
    pub material_index: u32,
    pub indices: Vec<Index>,
}

pub struct AmoMesh {
    pub vertices: Vec<Vertex>,
    pub tex_coords: Vec<TexCoord>,
    pub normals: Vec<Vertex>,
    pub colors: Vec<Color>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub primitives: Vec<AmoPrimitive>,
}

#[derive(Debug)]
pub struct AmoMeshView<'a> {
    pub indices1_count: u32,
    pub indices1_data: &'a [u8], // stored as raw data since they use triangle strips, cast them
                                 // when owning
    pub indices2_count: u32,
    pub indices2_data: &'a [u8],
    pub vertices: &'a [Vertex],
    pub normals: &'a [Vertex],
    pub tex_coords: &'a [TexCoord],
    pub colors: &'a [Color],
    pub weights: Vec<&'a [Joint]>,
    pub material_list: &'a [u32],
    pub material_indices: &'a [u32],
    pub matrix_list: &'a [u32],
}

impl<'a> TryFrom<&AmoNode<'a>> for AmoMeshView<'a> {
    type Error = anyhow::Error;
    fn try_from(node: &AmoNode<'a>) -> std::result::Result<Self, Self::Error> {
        if node.head.ty != AmoType::Mesh as u32 {
            bail!("Node {} is not a Mesh Node", AmoType::try_as_string(node.head.ty))
        }


        let indices1_data = node.find_node(AmoType::Indices1).map(|n| n.data).unwrap_or(&[]);
        let indices1_count = node.find_node(AmoType::Indices1).map(|n| n.head.count).unwrap_or(0);
        let indices2_data = node.find_node(AmoType::Indices2).map(|n| n.data).unwrap_or(&[]);
        let indices2_count = node.find_node(AmoType::Indices2).map(|n| n.head.count).unwrap_or(0);
        
        let vertices = node.cast_node_slice::<Vertex>(AmoType::Vertex);
        let normals = node.cast_node_slice::<Vertex>(AmoType::Normal);
        let tex_coords = node.cast_node_slice::<TexCoord>(AmoType::TexCoords);
        let colors = node.cast_node_slice::<Color>(AmoType::Color);
        let mut weights = Vec::new();
        if let Some(weight_node) = node.find_node(AmoType::Weight) {
            let mut weight_data = Cursor::new(&weight_node.data);
            for _ in 0..vertices.len() {
                let count = weight_data.read_u32::<LE>()?;
                let pos = weight_data.position() as usize;
                let size = count as usize * size_of::<Joint>();
                let data = &weight_data.get_ref()[pos..pos+size];
                weights.push(cast_data_vec(data)?);
                weight_data.seek_relative(size as i64)?;
            }
        }

        let material_list: &[u32] = node.cast_node_slice(AmoType::MaterialList);
        let material_indices: &[u32] = node.cast_node_slice(AmoType::MaterialIndex);
        let matrix_list: &[u32] = node.cast_node_slice(AmoType::MatrixList);

        Ok(Self {
            vertices,
            tex_coords,
            normals,
            indices1_count,
            indices1_data,
            indices2_count,
            indices2_data,
            material_list,
            material_indices,
            colors,
            weights,
            matrix_list,
        })
    }
}

impl<'a> AmoMeshView<'a> {
    pub fn to_owned(&self) -> AmoMesh {
        let mut primitives_map: HashMap<u32, Vec<Index>> = HashMap::new();

        let mut mat_idx_iter = self.material_indices.iter().copied();

        let mut process_strips = |data: &[u8], count: u32| {
            if data.is_empty() { return; }
            let strips = decode_amo_indices_grouped(data, count);

            for strip in strips {
                let local_mat_idx = mat_idx_iter.next().expect("not enough strips for mats");//.unwrap_or(0);

                let global_mat_id = self.material_list
                    .get(local_mat_idx as usize)
                    .copied()
                    .unwrap_or(0);

                primitives_map.entry(global_mat_id).or_default().extend(strip);
            }
        };

        process_strips(self.indices1_data, self.indices1_count);
        process_strips(self.indices2_data, self.indices2_count);

        let primitives = primitives_map
            .into_iter()
            .map(|(mat_id, indices)| AmoPrimitive {
                material_index: mat_id,
                indices,
            })
            .collect();

        let mut out_joints = Vec::with_capacity(self.vertices.len());
        let mut out_weights = Vec::with_capacity(self.vertices.len());

        for vertex_joints in &self.weights {
            let mut joints = [0u16; 4];
            let mut wts = [0f32; 4];

            let mut sorted: Vec<&Joint> = vertex_joints.iter().collect();
            sorted.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));

            for (i, j) in sorted.iter().take(4).enumerate() {
                joints[i] = j.id as u16;
                wts[i] = j.weight;
            }

            let sum: f32 = wts.iter().sum();
            if sum > 0.0 {
                for w in wts.iter_mut() { *w /= sum; }
            } else {
                wts[0] = 1.0;
            }

            out_joints.push(joints);
            out_weights.push(wts);
        }

        AmoMesh {
            vertices: self.vertices.to_vec(),
            tex_coords: self.tex_coords.to_vec(),
            normals: self.normals.to_vec(),
            colors: self.colors.to_vec(),
            weights: out_weights,
            joints: out_joints,

            primitives,
        }
    }
}

pub struct AmoView<'a> {
    version: u32,
    meshes: Vec<AmoMeshView<'a>>,
    materials: Vec<&'a MaterialData>,
    textures: Vec<&'a TextureData>,
}

impl<'a> AmoView<'a> {

}

impl<'a> TryFrom<AmoNode<'a>> for AmoView<'a> {
    type Error = anyhow::Error;
    fn try_from(node: AmoNode<'a>) -> std::result::Result<Self, Self::Error> {
        if node.head.ty != AmoType::Start as u32 {
            bail!("Node {} is not a Start Node", AmoType::try_as_string(node.head.ty))
        }

        let version = *node.cast_node::<u32>(AmoType::VersionOrSomething)
            .ok_or(anyhow!("Amo does not contain version or something <- wtf is this thing even"))?;

        let mut meshes = Vec::new();
        let meshes_node = node.find_node(AmoType::Meshes)
            .ok_or(anyhow!("Model does not contain any meshes node"))?;
        for i in 0..meshes_node.head.count {
            let mesh_node = meshes_node.find_node_idx(AmoType::Mesh, i as usize)
                .ok_or(anyhow!("Could not find mesh at index {i}"))?;
            let mesh_view = AmoMeshView::try_from(mesh_node)?;
            meshes.push(mesh_view);
        }

        let materials = node.cast_node_sub_datas::<MaterialData>(AmoType::MaterialHead)
            .ok_or(anyhow!("Model does not contain Material Datas"))?;
        let textures = node.cast_node_sub_datas::<TextureData>(AmoType::TextureHead)
            .ok_or(anyhow!("Model does not contain Texture Datas"))?;

        Ok(Self {
            version,
            meshes,
            materials,
            textures
        })
    }
}



// THIS IS LIKE ALL SLOP
use gltf_json::{self as json, validation::USize64};
use gltf_json::validation::Checked::Valid;

pub fn export_to_gltf(view: &AmoView, output_name: &str) -> Result<()> {
    let file_name = output_name.split('/').next_back()
        .unwrap_or(output_name);
    let clean_name = file_name.replace(".bin", "");
    let base_file_name = clean_name.replace("_amh", "");
    let mut buffer_data: Vec<u8> = Vec::new();
    let mut accessors = vec![];
    let mut buffer_views = vec![];
    let mut meshes = vec![];
    let mut nodes = vec![];

    let mut push_buffer_view = |bytes: &[u8], target: Option<json::buffer::Target>| -> u32 {
        let byte_offset = buffer_data.len() as u32;
        let byte_length = bytes.len() as u32;
        buffer_data.extend_from_slice(bytes);

        while buffer_data.len() % 4 != 0 {
            buffer_data.push(0);
        }

        let view_idx = buffer_views.len() as u32;
        buffer_views.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64(byte_length as u64),
            byte_offset: Some(USize64(byte_offset as u64)),
            byte_stride: None,
            extensions: Default::default(),
            extras: Default::default(),
            target: target.map(Valid),
            name: None,
        });
        view_idx
    };

    for (i, mesh_view) in view.meshes.iter().enumerate() {
        let owned_mesh = mesh_view.to_owned();

        if owned_mesh.vertices.is_empty() || owned_mesh.primitives.is_empty() {
            continue;
        }

        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for v in &owned_mesh.vertices {
            min = [min[0].min(v.x), min[1].min(v.y), min[2].min(v.z)];
            max = [max[0].max(v.x), max[1].max(v.y), max[2].max(v.z)];
        }

        let pos_bytes = bytemuck::cast_slice(&owned_mesh.vertices);
        let pos_view = push_buffer_view(pos_bytes, Some(json::buffer::Target::ArrayBuffer));
        let pos_accessor = accessors.len() as u32;
        accessors.push(json::Accessor {
            buffer_view: Some(json::Index::new(pos_view)),
            byte_offset: Some(USize64(0)),
            count: USize64(owned_mesh.vertices.len() as u64),
            component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
            extensions: Default::default(),
            extras: Default::default(),
            type_: Valid(json::accessor::Type::Vec3),
            min: Some(json::Value::Array(max.iter().map(|&v| json::Value::Number(serde_json::Number::from_f64(v as f64).unwrap())).collect())),
            max: Some(json::Value::Array(max.iter().map(|&v| json::Value::Number(serde_json::Number::from_f64(v as f64).unwrap())).collect())),
            normalized: false,
            sparse: None,
            name: None,
        });

        let norm_accessor = if !owned_mesh.normals.is_empty() {
            let norm_bytes = bytemuck::cast_slice(&owned_mesh.normals);
            let norm_view = push_buffer_view(norm_bytes, Some(json::buffer::Target::ArrayBuffer));
            let acc_idx = accessors.len() as u32;
            accessors.push(json::Accessor {
                buffer_view: Some(json::Index::new(norm_view)),
                byte_offset: Some(USize64(0)),
                count: USize64(owned_mesh.normals.len() as u64),
                component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Valid(json::accessor::Type::Vec3),
                min: None, max: None, normalized: false, sparse: None,
                name: None
            });
            Some(acc_idx)
        } else { None };

        let uv_accessor = if !owned_mesh.tex_coords.is_empty() {
            let uv_bytes = bytemuck::cast_slice(&owned_mesh.tex_coords);
            let uv_view = push_buffer_view(uv_bytes, Some(json::buffer::Target::ArrayBuffer));
            let acc_idx = accessors.len() as u32;
            accessors.push(json::Accessor {
                buffer_view: Some(json::Index::new(uv_view)),
                byte_offset: Some(USize64(0)),
                count: USize64(owned_mesh.tex_coords.len() as u64),
                component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Valid(json::accessor::Type::Vec2),
                min: None, max: None, normalized: false, sparse: None,
                name: None
            });
            Some(acc_idx)
        } else { None };

        let joint_accessor = if !owned_mesh.joints.is_empty() {
            let joint_bytes = bytemuck::cast_slice(&owned_mesh.joints);
            let joint_view = push_buffer_view(joint_bytes, Some(json::buffer::Target::ArrayBuffer));
            let acc_idx = accessors.len() as u32;
            accessors.push(json::Accessor {
                buffer_view: Some(json::Index::new(joint_view)),
                byte_offset: Some(USize64(0)),
                count: USize64(owned_mesh.joints.len() as u64),
                component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::U16)),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Valid(json::accessor::Type::Vec4),
                min: None, max: None, normalized: false, sparse: None, name: None
            });
            Some(acc_idx)
        } else { None };

        let weight_accessor = if !owned_mesh.weights.is_empty() {
            let weight_bytes = bytemuck::cast_slice(&owned_mesh.weights);
            let weight_view = push_buffer_view(weight_bytes, Some(json::buffer::Target::ArrayBuffer));
            let acc_idx = accessors.len() as u32;
            accessors.push(json::Accessor {
                buffer_view: Some(json::Index::new(weight_view)),
                byte_offset: Some(USize64(0)),
                count: USize64(owned_mesh.weights.len() as u64),
                component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Valid(json::accessor::Type::Vec4),
                min: None, max: None, normalized: false, sparse: None, name: None
            });
            Some(acc_idx)
        } else { None };

        let color_accessor = if !owned_mesh.colors.is_empty() && owned_mesh.colors.len() == owned_mesh.vertices.len() {
            let color_bytes = bytemuck::cast_slice(&owned_mesh.colors);
            let color_view = push_buffer_view(color_bytes, Some(json::buffer::Target::ArrayBuffer));
            let acc_idx = accessors.len() as u32;
            accessors.push(json::Accessor {
                buffer_view: Some(json::Index::new(color_view)),
                byte_offset: Some(USize64(0)),
                count: USize64(owned_mesh.colors.len() as u64),
                component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::U8)),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Valid(json::accessor::Type::Vec4),
                min: None, max: None, normalized: true, sparse: None,
                name: None
            });
            Some(acc_idx)
        } else { 
            if !owned_mesh.colors.is_empty() {
                println!("Warning: Mesh {} has {} vertices but {} colors. Skipping colors to prevent glTF corruption.", i, owned_mesh.vertices.len(), owned_mesh.colors.len());
            }
            None 
        };

        let mut attributes = std::collections::BTreeMap::new();
        attributes.insert(Valid(json::mesh::Semantic::Positions), json::Index::new(pos_accessor));
        if let Some(n) = norm_accessor { attributes.insert(Valid(json::mesh::Semantic::Normals), json::Index::new(n)); }
        if let Some(uv) = uv_accessor { attributes.insert(Valid(json::mesh::Semantic::TexCoords(0)), json::Index::new(uv)); }
        //if let Some(c) = color_accessor { attributes.insert(Valid(json::mesh::Semantic::Colors(0)), json::Index::new(c)); }
        if let Some(j) = joint_accessor { attributes.insert(Valid(json::mesh::Semantic::Joints(0)), json::Index::new(j)); }
        if let Some(w) = weight_accessor { attributes.insert(Valid(json::mesh::Semantic::Weights(0)), json::Index::new(w)); }

        // Primitives
        let mut gltf_primitives = vec![];

        for prim in &owned_mesh.primitives {
            if prim.indices.is_empty() { continue; }

            let idx_bytes = bytemuck::cast_slice(&prim.indices);
            let idx_view = push_buffer_view(idx_bytes, Some(json::buffer::Target::ElementArrayBuffer));
            let idx_accessor = accessors.len() as u32;

            accessors.push(json::Accessor {
                buffer_view: Some(json::Index::new(idx_view)),
                byte_offset: Some(USize64(0)),
                count: USize64(prim.indices.len() as u64),
                component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::U32)),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Valid(json::accessor::Type::Scalar),
                min: None, max: None, normalized: false, sparse: None,
                name: None
            });

            gltf_primitives.push(json::mesh::Primitive {
                attributes: attributes.clone(),
                extensions: Default::default(),
                extras: Default::default(),
                indices: Some(json::Index::new(idx_accessor)),
                material: Some(json::Index::new(prim.material_index)),
                mode: Valid(json::mesh::Mode::Triangles),
                targets: None,
            });
        }

        meshes.push(json::Mesh {
            extensions: Default::default(),
            extras: Default::default(),
            name: Some(format!("AmoMesh_{}", i)),
            primitives: gltf_primitives,
            weights: None,
        });

        // Add to Scene graph
        nodes.push(json::Node {
            camera: None,
            children: None,
            extensions: Default::default(),
            extras: Default::default(),
            matrix: None,
            mesh: Some(json::Index::new(i as u32)),
            name: None,
            //skin: Some(json::Index::new(0)),
            skin: None,
            rotation: None,
            scale: None,
            translation: None,
            weights: None,
        });
    }

    let child_node_indices: Vec<json::Index<json::Node>> = (0..nodes.len() as u32)
        .map(json::Index::new)
        .collect();

    let parent_node_idx = nodes.len() as u32;


    nodes.push(json::Node {
        camera: None, 
        children: Some(child_node_indices), // Tell it to hold all the meshes!
        extensions: Default::default(), 
        extras: Default::default(),
        matrix: None, 
        mesh: None, // The parent doesn't draw anything itself
        name: Some(clean_name), 
        rotation: None, 
        scale: None, 
        translation: None, 
        skin: None, 
        weights: None,
    });

    let mut gltf_textures = vec![];
    let mut gltf_images = vec![];

    for (idx, tex) in view.textures.iter().enumerate() {
        let texture_index = gltf_textures.len() as u32;
        let image_index = gltf_images.len() as u32;

        let texture_filename = format!("{}_tex.{}.0.png", base_file_name, tex.id);
        gltf_images.push(json::Image {
            uri: Some(texture_filename.clone()),
            buffer_view: None,
            mime_type: None,
            extensions: Default::default(),
            extras: Default::default(),
            name: Some(texture_filename.clone()),
        });

        gltf_textures.push(json::Texture {
            sampler: None,
            source: json::Index::new(image_index),
            extensions: Default::default(),
            extras: Default::default(),
            name: Some(format!("AmoTexture_{}", tex.id)),
        });
    }


    let mut gltf_materials = vec![];
    for (i, mat) in view.materials.iter().enumerate() {
        let tex_idx = mat.texture_info_id as usize;

        let roughness = (2.0 / (mat.shininess + 2.0)).powf(0.25).clamp(0.0, 1.0);
        let final_roughness = if mat.illumination == 1 { 1.0 } else { roughness };

        let base_color_texture = view.textures.get(tex_idx)
            .map(|_| {
                json::texture::Info {
                    index: json::Index::new(tex_idx as u32),
                    tex_coord: 0,
                    extensions: Default::default(),
                    extras: Default::default(),
                }
            });

        let has_texture = base_color_texture.is_some();

        let emissive_factor = [
            mat.ambient[0],
            mat.ambient[1],
            mat.ambient[2]
        ];

        let pbr = json::material::PbrMetallicRoughness {
            base_color_factor: json::material::PbrBaseColorFactor(mat.diffuse),
            base_color_texture,
            metallic_factor: json::material::StrengthFactor(0.0),
            roughness_factor: json::material::StrengthFactor(final_roughness),
            extensions: Default::default(),
            extras: Default::default(),
            ..Default::default()
        };

        let alpha_mode = if has_texture {
            Valid(json::material::AlphaMode::Mask) 
        } else {
            Valid(json::material::AlphaMode::Opaque)
        };

        gltf_materials.push(json::Material {
            name: Some(format!("AmoMaterial_{}", i)),
            alpha_mode,
            alpha_cutoff: if has_texture { Some(json::material::AlphaCutoff(0.1)) } else { None },
            double_sided: has_texture,
            pbr_metallic_roughness: pbr,
            emissive_factor: json::material::EmissiveFactor(emissive_factor),
            extensions: Default::default(),
            extras: Default::default(),
            ..Default::default()
        });
    }

    let root = json::Root {
        accessors,
        buffer_views,
        buffers: vec![json::Buffer {
            byte_length: USize64(buffer_data.len() as u64),
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            uri: None,
        }],
        meshes,
        materials: gltf_materials,
        textures: gltf_textures,
        images: gltf_images,
        nodes,
        scenes: vec![json::Scene {
            extensions: Default::default(),
            extras: Default::default(),
            name: Some(file_name.to_string()),
            nodes: vec![json::Index::new(parent_node_idx)],
        }],
        scene: Some(json::Index::new(0)),
        asset: json::Asset {
            generator: None,
            version: "2.0".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };



    let mut json_str = json::serialize::to_string(&root).unwrap();

    while json_str.len() % 4 != 0 {
        json_str.push(' ');
    }
    let json_bytes = json_str.as_bytes();
    let bin_bytes = &buffer_data;

    let json_chunk_len = json_bytes.len() as u32;
    let bin_chunk_len = bin_bytes.len() as u32;
    let total_length = 12 + 8 + json_chunk_len + 8 + bin_chunk_len;

    let final_name = output_name.replace(".bin", "");
    let mut file = std::fs::File::create(format!("{}.glb", final_name))?;

    file.write_all(b"glTF")?;
    file.write_all(&2u32.to_le_bytes())?;    
    file.write_all(&total_length.to_le_bytes())?; 

    file.write_all(&json_chunk_len.to_le_bytes())?;
    file.write_all(b"JSON")?;
    file.write_all(json_bytes)?;

    file.write_all(&bin_chunk_len.to_le_bytes())?;
    file.write_all(b"BIN\0")?;
    file.write_all(bin_bytes)?;

    println!("Successfully exported {}.glb", final_name);
    Ok(())
}
