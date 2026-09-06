use std::io::Cursor;

use anyhow::Result;
use bytemuck::{Pod, Zeroable};

#[derive(Debug, Clone)]
pub struct Material {
    pub ambient: [f32; 4],
    pub diffuse: [f32; 4],
    pub specular: [f32; 4],
    pub shininess: f32,
    pub illumination: i32,
    pub tex_count: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct Attribute {
    // not sure what's here, i think it's actually a tagged union?, sized 0x54 in the engine
}
