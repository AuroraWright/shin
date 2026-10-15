//! Higurashi BUP4 v3 descriptors and CPU export, separate from v4 cache IDs.
//!
//! Updated native constructor 0x34b70 reads a 48-byte header, eight-byte base
//! locations, 12-byte auxiliary locations and 76-byte expression records.
//! The animation arrays have three fixed slots; null slots must not be removed.
use std::{collections::BTreeMap, io::Cursor};

use anyhow::{ensure, Context, Result};
use binrw::BinRead;
use image::RgbaImage;
use indexmap::IndexMap;

use super::{
    default_builder::{Bustup, BustupExpression},
    BustupHeaderInfo, BustupId,
};
use crate::format::{
    picture::{read_picture_block_with_format, PicBlock, PictureFormat},
    text::read_sjis_string,
};

#[derive(Debug, BinRead)]
#[br(little, magic = b"BUP4")]
#[br(assert(_version == 3))]
struct Header {
    _version: u32,
    file_size: u32,
    origin_x: i16,
    origin_y: i16,
    width: u16,
    height: u16,
    flags: u32,
    base_count: u32,
    expression_count: u32,
    auxiliary_count: u32,
    identifier: u32,
    reserved: u64,
}

#[derive(Debug, Copy, Clone, BinRead, PartialEq, Eq)]
#[br(little)]
pub struct FileLocation {
    pub offset: u32,
    pub size: u32,
}

impl FileLocation {
    pub fn is_null(self) -> bool {
        self.offset == 0
    }

    fn validate(self, source: &[u8], metadata_end: usize) -> Result<()> {
        if self.is_null() {
            ensure!(self.size == 0, "Nonzero size for null Higurashi BUP block");
        } else {
            ensure!(
                self.offset as usize >= metadata_end && self.size >= 20,
                "Higurashi BUP block overlaps metadata or is shorter than its header"
            );
            ensure!(
                (self.offset as u64 + self.size as u64) <= source.len() as u64,
                "Higurashi BUP block range outside file"
            );
        }
        Ok(())
    }
}

#[derive(Debug, BinRead)]
#[br(little)]
pub struct AuxiliaryDescriptor {
    /// Native auxiliary cache ID. Base fragments use the header identifier and
    /// their index instead; v4's per-fragment IDs cannot be inferred from offsets.
    pub identifier: u32,
    pub location: FileLocation,
}

#[derive(BinRead)]
#[br(little)]
struct RawExpression {
    name: [u8; 16],
    auxiliary_index: u32,
    face: FileLocation,
    eyes: [FileLocation; 3],
    mouths: [FileLocation; 3],
}

#[derive(Debug)]
pub struct ExpressionDescriptor {
    pub name: String,
    pub auxiliary_index: u32,
    pub face: FileLocation,
    pub eyes: [FileLocation; 3],
    pub mouths: [FileLocation; 3],
}

#[derive(Debug)]
pub struct HigurashiBustupMetadata {
    pub header: BustupHeaderInfo,
    pub flags: u32,
    pub reserved: u64,
    pub base_blocks: Vec<FileLocation>,
    pub auxiliary: Vec<AuxiliaryDescriptor>,
    pub expressions: Vec<ExpressionDescriptor>,
}

impl HigurashiBustupMetadata {
    /// Read only bounded metadata and descriptor ranges; do not decode textures.
    pub fn read(source: &[u8]) -> Result<Self> {
        let mut reader = Cursor::new(source);
        let header = Header::read(&mut reader).context("Reading Higurashi BUP v3 header")?;
        ensure!(
            header.file_size as usize == source.len(),
            "Higurashi BUP file size mismatch"
        );
        let metadata_end = 48u64
            + header.base_count as u64 * 8
            + header.auxiliary_count as u64 * 12
            + header.expression_count as u64 * 76;
        ensure!(
            metadata_end <= source.len() as u64,
            "Higurashi BUP descriptor tables outside file"
        );
        let metadata_end = metadata_end as usize;

        let mut base_blocks = Vec::new();
        for _ in 0..header.base_count {
            let location = FileLocation::read(&mut reader)?;
            location.validate(source, metadata_end)?;
            base_blocks.push(location);
        }
        let mut auxiliary = Vec::new();
        for _ in 0..header.auxiliary_count {
            let descriptor = AuxiliaryDescriptor::read(&mut reader)?;
            descriptor.location.validate(source, metadata_end)?;
            auxiliary.push(descriptor);
        }
        let mut expressions = Vec::new();
        for _ in 0..header.expression_count {
            let raw = RawExpression::read(&mut reader)?;
            let name_end = raw
                .name
                .iter()
                .position(|&byte| byte == 0)
                .context("Unterminated Higurashi BUP expression name")?;
            let name = read_sjis_string(&mut Cursor::new(&raw.name[..name_end]), Some(name_end))?;
            ensure!(!name.is_empty(), "Empty Higurashi BUP expression name");
            ensure!(
                (raw.auxiliary_index as usize) < auxiliary.len(),
                "Higurashi BUP auxiliary index outside table"
            );
            for location in std::iter::once(&raw.face)
                .chain(&raw.eyes)
                .chain(&raw.mouths)
            {
                location.validate(source, metadata_end)?;
            }
            ensure!(
                !expressions
                    .iter()
                    .any(|entry: &ExpressionDescriptor| entry.name == name),
                "Duplicate Higurashi BUP expression name: {name}"
            );
            expressions.push(ExpressionDescriptor {
                name,
                auxiliary_index: raw.auxiliary_index,
                face: raw.face,
                eyes: raw.eyes,
                mouths: raw.mouths,
            });
        }
        Ok(Self {
            header: BustupHeaderInfo {
                origin_x: header.origin_x,
                origin_y: header.origin_y,
                effective_width: header.width,
                effective_height: header.height,
                bustup_id: BustupId::new(header.identifier),
            },
            flags: header.flags,
            reserved: header.reserved,
            base_blocks,
            auxiliary,
            expressions,
        })
    }
}

/// CPU conversion, reusing pixel codecs and export types. GPU/cache behavior is
/// deliberately not routed through Umineko's v4 descriptor/promise reader.
pub fn read_higurashi_bustup(source: &[u8]) -> Result<Bustup> {
    let metadata = HigurashiBustupMetadata::read(source)?;
    let mut blocks = BTreeMap::new();
    let mut decode = |location: FileLocation| -> Result<PicBlock> {
        if location.is_null() {
            return Ok(PicBlock::empty());
        }
        if let Some((size, block)) = blocks.get(&location.offset) {
            ensure!(
                *size == location.size,
                "Aliased Higurashi BUP block sizes differ"
            );
            return Ok(PicBlock::clone(block));
        }
        let data =
            &source[location.offset as usize..(location.offset as usize + location.size as usize)];
        let mut block = read_picture_block_with_format(data, PictureFormat::HigurashiV2)?;
        mask_unused_areas(&mut block)?;
        blocks.insert(location.offset, (location.size, block.clone()));
        Ok(block)
    };
    let mut base_image = RgbaImage::new(
        metadata.header.effective_width as u32,
        metadata.header.effective_height as u32,
    );
    for location in metadata.base_blocks {
        let block = decode(location)?;
        // The reference exporter copies fragments in authored order, then masks
        // their unused areas. This is CPU export, not native GPU blending parity.
        image::imageops::replace(
            &mut base_image,
            &block.data,
            block.offset_x as i64,
            block.offset_y as i64,
        );
    }
    let mut expressions = IndexMap::new();
    for descriptor in metadata.expressions {
        let auxiliary = metadata.auxiliary[descriptor.auxiliary_index as usize].location;
        let face1 = if auxiliary.is_null() {
            None
        } else {
            Some(decode(auxiliary)?)
        };
        let face2 = if descriptor.face.is_null() {
            None
        } else {
            Some(decode(descriptor.face)?)
        };
        let mouths = descriptor
            .mouths
            .into_iter()
            .map(&mut decode)
            .collect::<Result<Vec<_>>>()?;
        let eyes = descriptor
            .eyes
            .into_iter()
            .map(&mut decode)
            .collect::<Result<Vec<_>>>()?;
        expressions.insert(
            descriptor.name,
            BustupExpression {
                face1,
                face2,
                mouths,
                eyes,
            },
        );
    }
    Ok(Bustup {
        base_image,
        origin: (metadata.header.origin_x, metadata.header.origin_y),
        bustup_id: metadata.header.bustup_id,
        expressions,
    })
}

fn mask_unused_areas(block: &mut PicBlock) -> Result<()> {
    let width = block.data.width();
    let height = block.data.height();
    let mut mask = vec![false; width as usize * height as usize];
    for rectangle in block.opaque_rects.iter().chain(&block.transparent_rects) {
        ensure!(
            rectangle.from_x <= rectangle.to_x
                && rectangle.from_y <= rectangle.to_y
                && rectangle.to_x as u32 <= width
                && rectangle.to_y as u32 <= height,
            "Higurashi BUP rectangle outside fragment"
        );
        for y in rectangle.from_y as usize..rectangle.to_y as usize {
            mask[y * width as usize + rectangle.from_x as usize
                ..y * width as usize + rectangle.to_x as usize]
                .fill(true);
        }
    }
    for (pixel, used) in block.data.pixels_mut().zip(mask) {
        if !used {
            *pixel = image::Rgba([0, 0, 0, 0]);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "higurashi_tests.rs"]
mod tests;
