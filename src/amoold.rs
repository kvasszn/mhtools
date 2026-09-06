use std::io::{Cursor, Read, Seek, SeekFrom};

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use byteorder::{LE, ReadBytesExt};

// ty https://gist.github.com/Fothsid/67e46afea60d06f5feee7ffd241468d8

// I think i should structure this parser so that i read the entry and data types, and then parse
// types into structs i need based on that, the game seems to just look for the stuff it wants and
// disregard everything else, so just parseing the whole thang would be nice

pub struct Amo {
    entry: Chunk
}

pub const MODEL_HEAD: u32 = 0x4;
pub const MATERIAL_HEAD: u32 = 0x9;
pub const MATERIAL_LIST: u32 = 0x50000; // material num, look for 0x9 and use count, else 0
pub const MODEL_ATTRIBUTE: u32 = 0xf0000;

#[repr(u16)]
#[derive(Debug, Clone, Copy)]
pub enum ChunkType {
    Start = 0x1,
    Meshes = 0x2,
    Mesh = 0x4,
    Indices = 0x5,
    Material = 0x9,
    Texture = 0xa,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy)]
pub enum SubDataType {
    TextureInfo = 0x0,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EntryHeader {
    pub entry_type: ChunkType,
    pub data_type: SubDataType,
    pub count: u32,
    pub size: u32,
}

#[derive(Debug, Clone)]
pub enum Chunk {
    SubData(Box<SubData>),
    Start(Vec<Chunk>, u32),
    MaterialBegin(Vec<SubData>),
    TextureBegin(Vec<Chunk>),
    UnknownChunkType(u16, u16, u32, u32),
}

impl Chunk {
    pub const CHUNK_START: u16 = 0x1;
    pub const CHUNK_MESHES_BEGIN: u16 = 0x2;
    pub const CHUNK_MESH: u16 = 0x4;
    pub const CHUNK_INDICES_BEGIN: u16 = 0x5;
    pub const CHUNK_MATERIAL_BEGIN: u16 = 0x9;
    pub const CHUNK_TEXTURE_BEGIN: u16 = 0xa;

    pub fn read<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let pos = reader.stream_position()?;
        println!("start={:#x}", reader.stream_position()?);
        let chunk_type = reader.read_u16::<LE>()?;
        let data_type = reader.read_u16::<LE>()?;
        let count = reader.read_u32::<LE>()?;
        let size = reader.read_u32::<LE>()?;
        println!("{chunk_type:#x}, {data_type:#x}, {count:#x}, {size:#x}");
        
        anyhow::ensure!(size > 12, "AMO chunk size is smaller than header size");
        let mut data = vec![0u8; (size - 12) as usize];
        //reader.read_exact(&mut data)?;
        //let mut data_reader = Cursor::new(&data);

        let chunk = match (chunk_type, data_type) {
            (Self::CHUNK_START, 0) => {
                let mut chunks = Vec::new();
                for _ in 0..count {
                    //chunks.push(Chunk::read(&mut data_reader)?);
                    chunks.push(Chunk::read(reader)?);
                }
                Self::Start(chunks, size)
            },
            (Self::CHUNK_MATERIAL_BEGIN, 0) => {
                // This one is weird, the first chunk inside it is a Start? have to just treat it
                // seperately
                // It might use it's own type values for stuff, so like chunk_type=1 here might just mean MaterialInfo1
                let mut chunks = Vec::new();
                for i in 0..count {
                    let chunk_type = reader.read_u16::<LE>()?;
                    let data_type = reader.read_u16::<LE>()?;
                    let _count = reader.read_u32::<LE>()?;
                    let size = reader.read_u32::<LE>()?;
                    println!("matinfo chunk {i}: {chunk_type:#x}, {data_type:#x}, {count:#x}, {size:#x}");
                    let mut data = vec![0u8; (size - 12) as usize];
                    reader.read_exact(&mut data)?;
                    let mat_info: MaterialInfo = *bytemuck::try_from_bytes(&data).unwrap();

                    chunks.push(SubData::MaterialInfo1(mat_info));
                }
                Self::MaterialBegin(chunks)
            },
            (Self::CHUNK_TEXTURE_BEGIN, 0) => {
                let mut chunks = Vec::new();
                for _ in 0..count {
                    chunks.push(Chunk::read(reader)?);
                }
                Self::TextureBegin(chunks)
            },
            (0, 0) => {
                let mut data = vec![0u8; (size - 12) as usize];
                reader.read_exact(&mut data)?;
                let tex_info: TextureInfo = *bytemuck::try_from_bytes(&data).unwrap();
                Chunk::SubData(Box::new(SubData::TextureInfo(tex_info)))
            }
            (0, 2) => {
                let version = reader.read_u32::<LE>()?;
                Chunk::SubData(Box::new(SubData::Version(version)))
            }
            (0, _) => Self::SubData(Box::new(SubData::UnknownSubDataType(chunk_type, data_type, count, size))),
            _ => Self::UnknownChunkType(chunk_type, data_type, count, size),
        };
        reader.seek(SeekFrom::Start(pos + size as u64))?;
        
        Ok(chunk)
    }
}

#[derive(Debug, Clone)]
pub enum SubData {
    Version(u32), // idk if this is version or id or something
    TextureInfo(TextureInfo),
    MaterialInfo1(MaterialInfo),
    MaterialInfo2(MaterialInfo),
    UnknownSubDataType(u16, u16, u32, u32),
}

#[derive(Debug, Clone)]
pub struct Dir {
    pub unk0: u32,
    pub count: u32,
    pub size: u32,
    pub values: Vec<EntryV1>,
}

impl Dir {
    pub fn read<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let unk0 = reader.read_u32::<LE>()?;
        let count = reader.read_u32::<LE>()?;
        let size = reader.read_u32::<LE>()?;
        let mut values = Vec::with_capacity(count as usize);
        for _ in 0..count {
            values.push(EntryV1::read(reader)?);
        }
        Ok(Self {
            unk0,
            count,
            size,
            values
        })
    }
}


#[derive(Debug, Clone)]
pub struct EntryV1 {
    pub ty: u16,
    pub data_type: u16,
    pub count: u32,
    pub size: u32,
    //pub data: Vec<u8>
}

impl EntryV1 {
    pub fn read<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let ty = reader.read_u16::<LE>()?;
        let data_type = reader.read_u16::<LE>()?;
        let count = reader.read_u32::<LE>()?;
        let size = reader.read_u32::<LE>()?;

        if size < 12 {
            anyhow::bail!("Corrupt archive: size is smaller than header size");
        }
        
        let payload_size = size - 12;
        let mut data = vec![0u8; payload_size as usize];
        reader.read_exact(&mut data)?;

        Ok(Self {
            ty,
            data_type,
            count,
            size,
            //data
        })
    }
}


#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MaterialInfo {
    pub ambient: [f32; 4],
    pub diffuse: [f32; 4],
    pub specular: [f32; 4],
    pub shininess: f32,
    pub illumination: i32,
    // i don't even see this referenced in the code, why is it here? future proofing?
    pub _pad0: [u32; 50],
    pub texture_info_id: u32,
}
const _: () = assert!(size_of::<MaterialInfo>() == 0x104);

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TextureInfo {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    // wtf?, maybe because the engine does some zerocopy stuff?
    pub _pad0: [u32; 0xf4/4]
}
const _: () = assert!(size_of::<TextureInfo>() == 0x100);
