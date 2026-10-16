//! Older body-group/auxiliary owners and fixed animation slots. The v4
//! fragment-ID promise/cache machinery stays in the Umineko asset loader.
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use shin_core::format::{
    bustup::{
        BustupId,
        higurashi::{FileLocation, HigurashiBustupMetadata},
    },
    picture::{PictureFormat, read_picture_block_with_format},
};

use super::{BustupArgs, CharacterId};
use crate::asset::{
    picture::{GpuPictureBlock, GpuTextureBuilderContext},
    system::{Asset, AssetDataAccessor, AssetLoadContext, resource_cache::ResourceCache},
};

/// Native body owner is keyed by the header identifier and holds its complete
/// descriptor vector, including null slots. Assets retain this owner together.
pub struct HigurashiBody {
    pub blocks: Vec<Option<Arc<GpuPictureBlock>>>,
}

pub struct HigurashiAuxiliary {
    pub block: Option<Arc<GpuPictureBlock>>,
}

#[derive(Default)]
pub struct HigurashiBustupCache {
    pub body: ResourceCache<HigurashiBody>,
    pub auxiliary: ResourceCache<HigurashiAuxiliary>,
}

pub struct HigurashiBustup {
    pub character_id: CharacterId,
    pub origin_x: i16,
    pub origin_y: i16,
    pub effective_width: u16,
    pub effective_height: u16,
    pub bustup_id: BustupId,
    pub body: Arc<HigurashiBody>,
    pub auxiliary: Option<Arc<HigurashiAuxiliary>>,
    pub face: Option<Arc<GpuPictureBlock>>,
    /// The updated native draw uses only first-array slot zero.
    pub eye: Option<Arc<GpuPictureBlock>>,
    /// Direct native indexes 0..2, with absent frames preserved.
    pub mouths: [Option<Arc<GpuPictureBlock>>; 3],
    pub disable_animations: bool,
}

pub(super) fn upload(
    context: &AssetLoadContext,
    data: &[u8],
    location: FileLocation,
    label: &str,
) -> Result<Option<Arc<GpuPictureBlock>>> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        upload_block(context, data, location, label)
    }))
    .map_err(|_| anyhow::anyhow!("Higurashi bustup upload worker failed for {label}"))?
}

fn upload_block(
    context: &AssetLoadContext,
    data: &[u8],
    location: FileLocation,
    label: &str,
) -> Result<Option<Arc<GpuPictureBlock>>> {
    if location.is_null() {
        return Ok(None);
    }
    let block = read_picture_block_with_format(
        &data[location.offset as usize..location.offset as usize + location.size as usize],
        PictureFormat::HigurashiV2,
    )?;
    // Same no-geometry convention as the picture and v4 GPU loaders.
    if block.opaque_rects.is_empty() && block.transparent_rects.is_empty() {
        return Ok(None);
    }
    Ok(Some(Arc::new(GpuPictureBlock::new(
        GpuTextureBuilderContext {
            wgpu_device: &context.wgpu_device,
            wgpu_queue: &context.wgpu_queue,
        },
        block,
        &format!("{label}/{}", location.offset),
    ))))
}

impl Asset for HigurashiBustup {
    type Args = BustupArgs;

    async fn load(
        context: &Arc<AssetLoadContext>,
        args: Self::Args,
        name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        ensure!(
            context.game == crate::cli::Game::Higurashi,
            "Higurashi bustup requires a Higurashi asset server"
        );
        let data = Arc::new(data.read_all().await?);
        let metadata = HigurashiBustupMetadata::read(&data)?;
        let expression = metadata
            .expressions
            .iter()
            .find(|entry| entry.name == args.expression)
            .with_context(|| {
                format!(
                    "Missing Higurashi expression {:?} in {name}",
                    args.expression
                )
            })?;
        let face = expression.face;
        let eye = expression.eyes[0];
        let mouths = expression.mouths;
        let auxiliary = &metadata.auxiliary[expression.auxiliary_index as usize];
        let auxiliary_id = auxiliary.identifier;
        let auxiliary_location = auxiliary.location;
        let label = format!("Bup3[{name}]/{}", args.expression);
        let id = metadata.header.bustup_id;
        let body_context = context.clone();
        let body_data = data.clone();
        let body_label = label.clone();
        let body_locations = metadata.base_blocks;
        let body = context
            .higurashi_bustup_cache
            .body
            .get_or_load(id.raw(), || async move {
                shin_tasks::compute::spawn(move || {
                    let blocks = body_locations
                        .into_iter()
                        .map(|location| upload(&body_context, &body_data, location, &body_label))
                        .collect::<Result<Vec<_>>>()?;
                    Ok(HigurashiBody { blocks })
                })
                .await
            })
            .await?;
        let auxiliary = if auxiliary_location.is_null() {
            None
        } else {
            let auxiliary_context = context.clone();
            let auxiliary_data = data.clone();
            let auxiliary_label = label.clone();
            Some(
                context
                    .higurashi_bustup_cache
                    .auxiliary
                    .get_or_load(auxiliary_id, || async move {
                        shin_tasks::compute::spawn(move || {
                            Ok(HigurashiAuxiliary {
                                block: upload(
                                    &auxiliary_context,
                                    &auxiliary_data,
                                    auxiliary_location,
                                    &auxiliary_label,
                                )?,
                            })
                        })
                        .await
                    })
                    .await?,
            )
        };
        let expression_context = context.clone();
        let (face, eye, mouths) = shin_tasks::compute::spawn(move || -> Result<_> {
            let face = upload(&expression_context, &data, face, &label)?;
            let eye = upload(&expression_context, &data, eye, &label)?;
            // Do not compact sparse slots or use v4's skip-descriptor-zero rule.
            let mouths = [
                upload(&expression_context, &data, mouths[0], &label)?,
                upload(&expression_context, &data, mouths[1], &label)?,
                upload(&expression_context, &data, mouths[2], &label)?,
            ];
            Ok((face, eye, mouths))
        })
        .await?;
        Ok(Self {
            character_id: args.character_id,
            origin_x: metadata.header.origin_x,
            origin_y: metadata.header.origin_y,
            effective_width: metadata.header.effective_width,
            effective_height: metadata.header.effective_height,
            bustup_id: id,
            body,
            auxiliary,
            face,
            eye,
            mouths,
            disable_animations: args.disable_animations,
        })
    }
}

#[cfg(test)]
mod tests;
