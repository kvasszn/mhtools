pub mod apx;
pub mod model;
pub mod amo;

use std::{fs::File, io::{Cursor, Read, Seek, SeekFrom, Write}, iter::repeat_n};
use anyhow::{anyhow, Result};
use bytemuck::Pod;
use byteorder::{LE, ReadBytesExt, WriteBytesExt};

use crate::apx::Texture;

pub fn read_afs_file<R: Read + Seek>(reader: &mut R) -> Result<Vec<u8>> {
    let _t = reader.read_u32::<LE>()?;
    let file_offset = reader.read_u32::<LE>()?;
    reader.seek(SeekFrom::Start(file_offset as u64))?;
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn meltw(compressed: &[u8]) -> Result<Vec<u8>> {
    let mut buf = Cursor::new(compressed);
    let mut out: Vec<u8> = Vec::with_capacity(1024 * 1024);
    let mut flag = 0;
    let mut val = 0;

    loop {
        if flag == 0 {
            val = buf.read_u16::<LE>()?;
            flag = 0x8000;
        }
       if (val & flag) == 0 {
            out.write_u16::<LE>(buf.read_u16::<LE>()?)?;
        } else {
            let mut lower = buf.read_u16::<LE>()?;
            let mut upper = (lower >> 0xb) as u32;
            if upper == 0 {
                upper = buf.read_u16::<LE>()? as u32;
            } else {
                lower &= 0x7ff;
            }
            if lower == 0 {
                if upper == 0 {
                    return Ok(out);
                }
                out.extend(repeat_n(0u8, upper as usize * 2));
            } else {
                let distance = lower as usize * 2;
                if distance == 0 || distance > out.len() {
                    return Err(anyhow!("invalid backreference"));
                }
                for _ in 0..upper {
                    let current_len = out.len();
                    let src_idx = current_len - (lower as usize * 2);
                    let b1 = out[src_idx];
                    let b2 = out[src_idx + 1];
                    out.extend_from_slice(&[b1, b2]);
                }
            }
        }
        flag >>= 1;
    }
}


/* Links look like this
 * But texlists are handled differently, they dont really care about size
 * amo and ahi do care about size though, though only have 2 entries

struct Links {
    u32 count;
    Link links[count];
}

struct Link {
    u32 offset;
    u32 size;
}

*/

pub fn get_link_file_num(buf: &[u8]) -> Option<u32> {
    let bytes = buf.get(..4)?;
    let num = u32::from_le_bytes(bytes.try_into().ok()?);
    Some(num)
}

pub fn get_link_file_address(buf: &[u8], idx: usize) -> Option<u32> {
    let loc = idx * 8 + 4;
    let bytes = buf.get(loc .. loc + 4)?;
    let addr = u32::from_le_bytes(bytes.try_into().ok()?);
    Some(addr)
}

pub fn get_link_file_slice(buf: &[u8], idx: usize) -> Option<&[u8]> {
    let addr = get_link_file_address(buf, idx)?;
    buf.get(addr as usize..)
}

pub fn get_link_file_size(buf: &[u8], idx: usize) -> Option<u32> {
    let loc = idx * 8 + 8;

    let bytes = buf.get(loc .. loc + 4)?;
    let size = u32::from_le_bytes(bytes.try_into().ok()?);

    Some(size)
}

pub fn load_texlist(data: &[u8]) -> Option<Vec<Texture>> {
    let count = get_link_file_num(data)?;

    println!("{count} textures in tex list");
    let mut textures = Vec::with_capacity(count as usize);

    for i in 0..count {
        let data = get_link_file_slice(data, i as usize)?;
        let tex = Texture::parse(data, false).ok()?;
        println!("tex_{i}: w={}, h={}", tex.width, tex.height);
        textures.push(tex);
    }

    Some(textures)
}


// return (amo, ahi)
// i might want to also wrap the data to include addr and size?
pub fn amo_ahi_expand(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    let count = get_link_file_num(buf)?;
    if count != 2 {
        eprintln!("[WARNING] Count for amo or ahi should be 2");
        //return None
    }

    let amo_addr = get_link_file_address(buf, 0)? as usize;
    let amo_size = get_link_file_size(buf, 0)? as usize;
    let ahi_addr = get_link_file_address(buf, 1)? as usize;
    let ahi_size = get_link_file_size(buf, 1)? as usize;
    println!("amo: addr={amo_addr:x}, size={amo_size:x}");
    println!("ahi: addr={ahi_addr:x}, size={ahi_size:x}");
    let amo = buf.get(amo_addr..amo_addr + amo_size)?;
    let ahi = buf.get(ahi_addr..ahi_addr + ahi_size)?;

    Some((amo, ahi))
}

#[derive(Debug, Clone, Copy)]
pub struct Link {
    pub offset: u32,
    pub size: u32
}

pub fn read_all_links(buf: &[u8]) -> Option<Vec<Link>> {
    let mut res = Vec::new();
    let count = get_link_file_num(buf)?;
    for i in 0..count {
        let offset = get_link_file_address(buf, i as usize)?;
        let size = get_link_file_size(buf, i as usize)?;
        res.push(Link {offset, size})
    }
    Some(res)
}


pub fn read_pod<T: Pod, R: Read>(r: &mut R) -> std::io::Result<T> {
    let mut buf = vec![0u8; std::mem::size_of::<T>()];
    r.read_exact(&mut buf)?;
    Ok(bytemuck::cast_slice(&buf)[0])
}

pub fn cast_data_vec<T: Pod>(data: &[u8]) -> Result<&[T]> {
    anyhow::ensure!(
        data.len().is_multiple_of(std::mem::size_of::<T>()),
        "Data size is not a multiple of the target struct size"
    );
    Ok(bytemuck::cast_slice(data))
}

pub fn cast_data<T: Pod>(data: &[u8]) -> Result<&T> {
    anyhow::ensure!(
        data.len() == std::mem::size_of::<T>(),
        "Data size is not equal to the target struct size"
    );
    Ok(bytemuck::from_bytes(data))
}


pub fn cast_data_n<T: Pod>(data: &[u8], count: usize) -> Result<&[T]> {
    anyhow::ensure!(
        data.len() / std::mem::size_of::<T>() == count,
        "Not enough data to fill {count} elements"
    );
    Ok(bytemuck::cast_slice(data))
}
