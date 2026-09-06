use anyhow::{Result, bail, ensure};
use bytemuck::{Pod, Zeroable};
use byteorder::{LE, ReadBytesExt};
use image::{ColorType, save_buffer};

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ApxHeader {
    unk0: u32, // probably size
    palette_offset: u32, // 0x4
    unk1: u32,
    bit_depth: i16, // 0xc
    width: u16, // 0xe
    height: u16, // 0x10
    mip_count: u16, // 0x12
    palette_type: u16, // 0x14
    palette_num: u16, // 0x16
    pad: [u8; 0x8]
}
const _: () = assert!(std::mem::size_of::<ApxHeader>() == 0x20);

impl ApxHeader {
    pub fn level_size(bit_depth: i16, w: usize, h: usize) -> usize {
        match bit_depth {
            0x20 => w * h * 4,
            0x18 => w * h * 4,
            0x10 => w * h * 2,
            8 => w * h,
            4 => (w * h) >> 1,
            _ => 0,
        }
    }

    pub fn mip_offset(&self, level: usize) -> usize {
        let mut offset = 0x20;
        let mut current_w = self.width as usize;
        let mut current_h = self.height as usize;

        for _ in 0..level {
            let size = match self.bit_depth {
                0x20 => current_w * current_h * 4,
                0x18 => current_w * current_h * 3,
                0x10 => current_w * current_h * 2,
                8    => current_w * current_h,
                4    => (current_w * current_h + 1) >> 1,
                _    => 0,
            };
            offset += size;
            current_w >>= 1;
            current_h >>= 1;
        }
        offset
    }

    pub fn mip_palette_offset(&self, level: usize) -> usize {
        let mut offset = self.mip_offset(level) + self.palette_offset as usize;

        let palette_len = if self.bit_depth == 8 {
            0x100
        } else {
            0x10
        };

        for _ in 0..level {
            let size = match self.palette_type {
                0x20 => palette_len * 4,
                0x18 => palette_len * 3,
                0x10 => palette_len * 2,
                _    => 0,
            };
            offset += size;

        }
        offset
    }

    pub fn mip_dims(&self, level: usize) -> (usize, usize) {
        ((self.width as usize) >> level, (self.height as usize) >> level)
    }

    pub fn pixel_format(&self) -> Result<PixelFormat> {
        PixelFormat::from_bit_depth(self.bit_depth)
    }

}

#[derive(Debug, Clone, Copy)]
pub enum PixelFormat {
    Rgba32, // 4
    Rgb24, // 3
    Rgba5A1, // 2
    Indexed8, // 1
    Indexed4, // 0
}

impl PixelFormat {
    pub fn from_bit_depth(depth: i16) -> Result<Self> {
        Ok(match depth {
            0x20 => Self::Rgba32,
            0x18 => Self::Rgb24,
            0x10 => Self::Rgba5A1,
            8 => Self::Indexed8,
            4 => Self::Indexed4,
            other => bail!("unknown bit depth: {:#x}", other),
        })
    }
 
    pub fn is_indexed(&self) -> bool {
        matches!(self, Self::Indexed8 | Self::Indexed4)
    }

    pub fn get_buf_size(&self, w: usize, h: usize) -> usize {
        match self {
            Self::Rgba32 | Self::Rgb24 => w * h * 4,
            Self::Rgba5A1 => w * h * 2,
            Self::Indexed8 => w * h,
            Self::Indexed4 => (w * h) >> 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteFormat {
    Rgba8,
    Rgb8,
    Rgba5A1,
}

impl PaletteFormat {
    pub fn from_raw(raw: u16) -> Result<Self> {
        Ok(match raw {
            0x20 => Self::Rgba8,
            0x18 => Self::Rgb8,
            0x10 => Self::Rgba5A1,
            other => bail!("unknown palette type: {:#x}", other),
        })
    }
}


pub fn unswizzle_indices8(indices: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = indices.to_vec();
    let total = width * height;
    let mut i = 0;
    while i + 32 <= total {
        for j in 0..8 {
            out[i + 8 + j] = indices[i + 16 + j];
            out[i + 16 + j] = indices[i + 8 + j];
        }
        i += 32;
    }
    out
}

pub fn unswizzle_indices4(indices: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = indices.to_vec();
    
    let total_bytes = (width * height) / 2;
    let mut i = 0;
    
    while i + 32 <= total_bytes {
        for j in 0..8 {
            out[i + 8 + j] = indices[i + 16 + j];
            out[i + 16 + j] = indices[i + 8 + j];
        }
        i += 32;
    }
    
    out
}

pub fn unswizzle_ps2_clut(palette: &[u8]) -> Vec<u8> {
    let mut out = palette.to_vec();

    if palette.len() != 1024 {
        return out;
    }

    for block in 0..8 {
        let base = block * 32;
        for j in 0..8 {
            let idx1 = (base + 8 + j) * 4;
            let idx2 = (base + 16 + j) * 4;

            out[idx1..idx1 + 4].copy_from_slice(&palette[idx2..idx2 + 4]);
            out[idx2..idx2 + 4].copy_from_slice(&palette[idx1..idx1 + 4]);
        }
    }

    out
}


#[derive(Debug, Clone)]
pub struct Mipmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Texture {
    pub bit_depth: i16,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub mipmaps: Vec<Mipmap>,
    pub palette_type: Option<PaletteFormat>,
    pub palette: Option<Vec<u8>>,
}

impl Texture { 
    pub fn parse(buf: &[u8], apply_index_swizzle: bool) -> Result<Self> {
        ensure!(buf.len() >= 0x20, "buffer too small for ApxHeader");
        let header: ApxHeader = *bytemuck::from_bytes(&buf[0..0x20]);
        println!("header: {header:#?}");
        let fmt = header.pixel_format()?;
        println!("fmt: {fmt:?}");

        let mut mipmaps = Vec::with_capacity(header.mip_count as usize);
        for level in 0..header.mip_count as usize {
            let (w, h) = header.mip_dims(level);
            if w == 0 || h == 0 {
                break;
            }
            let off = header.mip_offset(level);
            let pixels = get_mipmap_pixels(buf, off, w, h, fmt, apply_index_swizzle)?;
            mipmaps.push(Mipmap { width: w as u32, height: h as u32, pixels });
        }

        let palette_type = PaletteFormat::from_raw(header.palette_type).ok();
        let palette = if fmt.is_indexed() && header.palette_num > 0 {
            let Some(palette_type) = &palette_type else {
                bail!("Invalid palette type {}", header.palette_type)
            };
            let off = header.mip_palette_offset(0);
            let palette_len = if header.bit_depth == 8 {
                0x100
            } else {
                0x10
            };
            let palette = match palette_type {
                PaletteFormat::Rgba8 => {
                    buf[off..off+palette_len * 4].to_vec()
                }
                PaletteFormat::Rgb8 => {
                    buf[off..off+palette_len * 3].to_vec()
                }
                PaletteFormat::Rgba5A1 => {
                    buf[off..off+palette_len * 2].to_vec()
                }
            };
            Some(palette)
        } else {
            None
        };

        Ok(Self {
            width: header.width as u32,
            height: header.height as u32,
            bit_depth: header.bit_depth,
            format: fmt,
            mipmaps, 
            palette_type,
            palette 
        })
    }


    pub fn save_all(&self, path: &str) {
        let levels: Vec<usize> = (0..self.mipmaps.len()).collect();
        self.save(path, &levels);
    }

    pub fn save(&self, path: &str, levels: &[usize]) {
        if levels.is_empty() {
            self.save_mip(&format!("{path}.0.png"), 0);
        } else {
            for level in levels {
                let path = format!("{path}.{level}.png");
                self.save_mip(&path, 0);
            }
        }
    }

    pub fn save_mip(&self, path: &str, level: usize) {
        let Ok((w, h, rgba)) = self.decode_rgba(level) else {
            return;
        };
        let _ = save_buffer(path, &rgba, w, h, ColorType::Rgba8)
            .inspect_err(|e| eprintln!("Failed to save image mip {level} to {path}: {e}"));
    }

    pub fn decode_rgba(&self, level: usize) -> Result<(u32, u32, Vec<u8>)> {
        let mip = &self.mipmaps[level];
        let w = mip.width as usize;
        let h = mip.height as usize;

        let palette = if let (Some(palette_type), Some(palette)) = (self.palette_type, self.palette.as_ref()) {
            let mut decoded = decode_palette_rgba(palette_type, palette)?;

            if self.format.is_indexed() && palette_type == PaletteFormat::Rgb8 {
                decoded = unswizzle_ps2_clut(&decoded);
            }

            Some(decoded)
        } else {
            None
        };

        //palette.as_ref().inspect(|p| {
        //    save_buffer("outputs/palette_dump.png", p, 0x10, 0x10, ColorType::Rgba8)
        //        .expect("Failed to save PNG image");
        //});

        use PixelFormat::*;
        let rgba = match &self.format {
            Rgba32 => mip.pixels.clone(),
            Rgb24 => {
                let mut buf = vec![0u8; w * h * 4];
                let src = &mip.pixels;
                for i in 0..w * h {
                    buf[i * 4] = src[i * 4];
                    buf[i * 4 + 1] = src[i * 4 + 1];
                    buf[i * 4 + 2] = src[i * 4 + 2];
                    buf[i * 4 + 3] = 0xff;
                }
                buf
            },
            Rgba5A1 => {
                let mut buf = vec![0u8; w * h * 4];
                let src = &mip.pixels;
                for i in 0..w * h {
                    let lo = src[i * 2];
                    let hi = src[i * 2 + 1];
                    let px = u16::from_le_bytes([lo, hi]);
                    let r5 = (px & 0x1f) as u8;
                    let g5 = ((px >> 5) & 0x1f) as u8;
                    let b5 = ((px >> 10) & 0x1f) as u8;
                    let a1 = (px >> 15) & 1;
                    let expand5 = |v: u8| (v << 3) | (v >> 2);
                    buf[i * 4] = expand5(r5);
                    buf[i * 4 + 1] = expand5(g5);
                    buf[i * 4 + 2] = expand5(b5);
                    buf[i * 4 + 3] = if a1 == 1 { 255 } else { 0 };
                }
                buf
            },
            Indexed8 => {
                let Some(palette) = &palette else {
                    bail!("Indexed8 pixel format but no palette")
                };
                let mut buf = vec![0u8; w * h * 4];
                let src = &mip.pixels;
                for i in 0..w * h {
                    let index = src[i];
                    let color = &palette[index as usize * 4..index as usize * 4 + 4];
                    buf[i*4..i*4+4].copy_from_slice(color);
                }
                buf
            }
            Indexed4 => {
                let Some(palette) = palette else {
                    bail!("Indexed8 pixel format but no palette")
                };
                let mut buf = vec![0u8; w * h * 4];
                let src = &mip.pixels;
                for i in 0..w * h / 2 {
                    let lo = src[i] & 0xf;
                    let color_lo = &palette[lo as usize * 4..lo as usize * 4 + 4];
                    let hi = src[i] >> 4;
                    let color_hi = &palette[hi as usize * 4..hi as usize * 4 + 4];
                    let px1 = i * 2;
                    let px2 = i * 2 + 1;
                    buf[px1 * 4..px1 * 4 + 4].copy_from_slice(color_lo);
                    buf[px2 * 4..px2 * 4 + 4].copy_from_slice(color_hi);
                }
                buf
            }
        };
        Ok((mip.width, mip.height, rgba))
    }
}

pub fn decode_palette_rgba(palette_format: PaletteFormat,  buf: &[u8]) -> Result<Vec<u8>> {
    use PaletteFormat::*;
    let res = match palette_format {
        Rgba8 => {
            buf.to_vec()
        },
        Rgb8 => {
            let len = buf.len() / 3;
            println!("rgb8 pal len={len}");
            let mut out = vec![0u8; len * 4];
            for i in 0..len {
                out[i * 4] = buf[i * 4];
                out[i * 4 + 1] = buf[i * 4 + 1];
                out[i * 4 + 2] = buf[i * 4 + 2];
                out[i * 4 + 3] = 0xff;
            }
            out
        }
        Rgba5A1 => {
            let len = buf.len() / 2;
            println!("rgba5a1 pal len={len}");
            let mut out = vec![0u8; len * 4];
            for i in 0..len {
                let lo = buf[i * 2];
                let hi = buf[i * 2 + 1];
                let px = u16::from_le_bytes([lo, hi]);
                let r5 = (px & 0x1f) as u8;
                let g5 = ((px >> 5) & 0x1f) as u8;
                let b5 = ((px >> 10) & 0x1f) as u8;
                let a1 = (px >> 15) & 1;
                let expand5 = |v: u8| (v << 3) | (v >> 2);
                out[i * 4] = expand5(r5);
                out[i * 4 + 1] = expand5(g5);
                out[i * 4 + 2] = expand5(b5);
                out[i * 4 + 3] = if a1 == 1 { 255 } else { 0 };
            }
            out
        }
    };
    Ok(res)
}

fn get_mipmap_pixels(
    buf: &[u8],
    off: usize,
    w: usize,
    h: usize,
    fmt: PixelFormat,
    apply_index_swizzle: bool,
) -> Result<Vec<u8>> {
    let need = fmt.get_buf_size(w, h);
    let mut out = vec![0u8; need];
    match fmt {
        PixelFormat::Rgba32 => {
            ensure!(buf.len() >= off + need, "buffer too small for RGBA32 level");
            let src = &buf[off..off + need];
            for i in 0..w * h {
                out[i * 4] = src[i * 4];
                out[i * 4 + 1] = src[i * 4 + 1];
                out[i * 4 + 2] = src[i * 4 + 2];
                out[i * 4 + 3] = src[i * 4 + 3];
            }
        }
        PixelFormat::Rgb24 => {
            ensure!(buf.len() >= off + need, "buffer too small for RGB32 level");
            out.copy_from_slice(&buf[off..off+need]);
        }
        PixelFormat::Rgba5A1 => {
            ensure!(buf.len() >= off + need, "buffer too small for 5551 level");
            out.copy_from_slice(&buf[off..off+need]);

        }
        PixelFormat::Indexed8 => {
            ensure!(buf.len() >= off + need, "buffer too small for 8-bit level");
            let raw = &buf[off..off + need];
            if apply_index_swizzle {
                out = unswizzle_indices8(raw, w, h);
            } else {
                out.copy_from_slice(raw);
            }
        }
        PixelFormat::Indexed4 => {
            ensure!(buf.len() >= off + need, "buffer too small for 4-bit level");
            let raw = &buf[off..off + need];

            if apply_index_swizzle {
                out = unswizzle_indices4(raw, w, h);
            } else {
                out.copy_from_slice(raw);
            }
        }
    };
    Ok(out)
}
