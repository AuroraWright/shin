//! Later-engine UTF-8 expression selection with the shared BUP GPU/cache loader.
use super::*;

/// A separate asset type keeps name decoding explicit and the legacy asset
/// registry unchanged. Both dialects reuse block promises, uploads and owners.
pub struct Gerokasu2Bustup(pub Arc<Bustup>);

impl Asset for Gerokasu2Bustup {
    type Args = BustupArgs;

    async fn load(
        context: &Arc<AssetLoadContext>,
        args: BustupArgs,
        name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        if context.game == crate::cli::Game::Gerokasu {
            return load_gerokasu(context, args, name, data)
                .await
                .map(|bustup| Self(Arc::new(bustup)));
        }
        load_bustup(context, args, name, data, TextEncoding::Utf8)
            .await
            .map(|bustup| Self(Arc::new(bustup)))
    }
}

/// Gerokasu ships the Umineko-era BUP4 version 3 container (the same layout
/// Higurashi uses: base fragments, auxiliary faces and 76-byte expressions).
/// Its fragments use the shared picture block encoding. The supplied files
/// carry no eye or mouth frames; a face number absent from a file (the
/// single-expression red-eye costumes) draws the body alone.
async fn load_gerokasu(
    context: &Arc<AssetLoadContext>,
    args: BustupArgs,
    name: &str,
    data: AssetDataAccessor,
) -> Result<Bustup> {
    use shin_core::format::bustup::higurashi::HigurashiBustupMetadata;

    use super::higurashi::{HigurashiBody, upload};

    let data = Arc::new(data.read_all().await?);
    let metadata = HigurashiBustupMetadata::read(&data)?;
    let label = format!("Bup3[{name}]/{}", args.expression);
    let expression = metadata
        .expressions
        .iter()
        .find(|entry| entry.name == args.expression);
    if expression.is_none() {
        tracing::debug!("{name}: no expression {:?}, drawing body only", args.expression);
    }
    let auxiliary = expression.map(|e| metadata.auxiliary[e.auxiliary_index as usize].location);
    let face = expression.map(|e| e.face);
    let eyes = expression.map(|e| e.eyes);
    let mouths = expression.map(|e| e.mouths);

    let body_context = context.clone();
    let body_data = data.clone();
    let body_label = label.clone();
    let body_locations = metadata.base_blocks.clone();
    let body = context
        .higurashi_bustup_cache
        .body
        .get_or_load(metadata.header.bustup_id.raw(), || async move {
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

    let expression_context = context.clone();
    let (face1, face2, eye_blocks, mouth_blocks) =
        shin_tasks::compute::spawn(move || -> Result<_> {
            let one = |location| match location {
                Some(location) => upload(&expression_context, &data, location, &label),
                None => Ok(None),
            };
            let many = |locations: Option<[_; 3]>| -> Result<Vec<_>> {
                Ok(locations
                    .into_iter()
                    .flatten()
                    .map(|location| upload(&expression_context, &data, location, &label))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect())
            };
            Ok((one(auxiliary)?, one(face)?, many(eyes)?, many(mouths)?))
        })
        .await?;

    Ok(Bustup {
        character_id: args.character_id,
        origin_x: metadata.header.origin_x,
        origin_y: metadata.header.origin_y,
        effective_width: metadata.header.effective_width,
        effective_height: metadata.header.effective_height,
        bustup_id: metadata.header.bustup_id,
        base_blocks: body.blocks.iter().flatten().cloned().collect(),
        face1,
        face2,
        mouth_blocks,
        eye_blocks,
    })
}
