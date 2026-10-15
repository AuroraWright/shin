use std::{marker::PhantomData, sync::Arc};

pub mod higurashi;

use anyhow::{Context, Result};
use glam::{Vec2, vec2};
use indexmap::IndexMap;
use parking_lot::Mutex;
use shin_core::format::{
    bustup::{
        BustupBlockId, BustupBlockPromise, BustupBlockPromiseToken, BustupBuilder, BustupId,
        BustupSkeleton,
    },
    picture::PicBlock,
    text::TextEncoding,
};
use std::collections::HashMap;

use crate::asset::{
    picture::{GpuPictureBlock, GpuTextureBuilderContext},
    system::{
        Asset, AssetDataAccessor, AssetLoadContext,
        cache::{AssetCache, CacheHandle, CacheLoadHandle, CacheLoaderHandle, CacheLookupResult},
    },
};

type BlockCache = AssetCache<BustupBlockId, GpuPictureBlock>;
type BlockCacheLoadHandle = CacheLoadHandle<GpuPictureBlock>;
type BlockCacheLoaderHandle = CacheLoaderHandle<BustupBlockId, GpuPictureBlock>;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CharacterId(i32);

impl CharacterId {
    /// Native metadata factories20804/3adf4 sign-extend the stored16-bit ID.
    pub fn from_metadata(id: u16) -> Self {
        Self(i32::from(id as i16))
    }
    pub fn raw(self) -> i32 {
        self.0
    }

    pub fn new(id: i32) -> Self {
        Self(id)
    }
}

/// Native357d8/358dc reserves descriptor0 for the non-animated portrait.
/// Select promises before materializing them so unused frames are not decoded.
fn animation_frames<T>(frames: Vec<T>, disable_animations: bool) -> impl Iterator<Item = T> {
    frames
        .into_iter()
        .skip(usize::from(!disable_animations))
        .take(if disable_animations { 1 } else { usize::MAX })
}

struct GpuBustupBuilder<'b> {
    phantom: PhantomData<&'b ()>,
}

struct GpuBustupBuilderArgs<'a> {
    context: GpuTextureBuilderContext<'a>,
    cache: &'a BlockCache,
    label: String,
    expression: String,
    character_id: CharacterId,
    disable_animations: bool,
    pending: Mutex<HashMap<BustupBlockId, BlockCacheLoaderHandle>>,
}

struct GpuBustupSkeleton<'a> {
    pub origin_x: i16,
    pub origin_y: i16,
    pub effective_width: u16,
    pub effective_height: u16,
    pub bustup_id: BustupId,

    pub base_blocks: Vec<GpuBustupBlockPromise<'a>>,
    pub face1: Option<GpuBustupBlockPromise<'a>>,
    pub face2: Option<GpuBustupBlockPromise<'a>>,
    pub mouth_blocks: Vec<GpuBustupBlockPromise<'a>>,
    pub eye_blocks: Vec<GpuBustupBlockPromise<'a>>,
}

enum GpuBustupBlockPromise<'a> {
    Loaded(Arc<GpuPictureBlock>),
    Loading(BlockCacheLoadHandle),
    LoadRequired(BustupBlockPromise<'a>),
}

pub struct Bustup<T = Arc<GpuPictureBlock>> {
    pub character_id: CharacterId,
    pub origin_x: i16,
    pub origin_y: i16,
    pub effective_width: u16,
    pub effective_height: u16,
    pub bustup_id: BustupId,

    pub base_blocks: Vec<T>,
    pub face1: Option<T>,
    pub face2: Option<T>,
    pub mouth_blocks: Vec<T>,
    pub eye_blocks: Vec<T>,
}

impl<'a> GpuBustupBlockPromise<'a> {
    pub fn new(args: &GpuBustupBuilderArgs, promise: BustupBlockPromise<'a>) -> Self {
        match args.cache.lookup(promise.get_id()) {
            CacheLookupResult::Loaded(block) => Self::Loaded(block),
            CacheLookupResult::Loading(load_handle) => Self::Loading(load_handle),
            CacheLookupResult::LoadRequired(loader_handle) => {
                args.pending.lock().insert(promise.get_id(), loader_handle);
                Self::LoadRequired(promise)
            }
        }
    }

    pub fn materialize(
        self,
        token: &BustupBlockPromiseToken<Arc<GpuPictureBlock>>,
    ) -> CacheHandle<GpuPictureBlock> {
        match self {
            GpuBustupBlockPromise::Loaded(block) => CacheHandle::Loaded(block),
            GpuBustupBlockPromise::Loading(handle) => CacheHandle::Loading(handle),
            GpuBustupBlockPromise::LoadRequired(promise) => {
                CacheHandle::Loaded(promise.get(token).clone())
            }
        }
    }
}

impl<'b> BustupBuilder for GpuBustupBuilder<'b> {
    type Args = GpuBustupBuilderArgs<'b>;
    type Skeleton<'a> = GpuBustupSkeleton<'a>;
    type BlockType = Arc<GpuPictureBlock>;
    type Output = Bustup<CacheHandle<GpuPictureBlock>>;

    fn new<'a>(args: &Self::Args, skeleton: BustupSkeleton<'a>) -> Self::Skeleton<'a> {
        let BustupSkeleton {
            origin_x,
            origin_y,
            effective_width,
            effective_height,
            bustup_id,
            base_blocks,
            mut expressions,
        } = skeleton;

        let lower_block = |block: BustupBlockPromise<'a>| GpuBustupBlockPromise::new(args, block);

        let base_blocks = base_blocks.into_iter().map(lower_block).collect::<Vec<_>>();

        let mut face1 = None;
        let mut face2 = None;
        let mut mouth_blocks = Vec::new();
        let mut eye_blocks = Vec::new();

        if let Some(expression) = expressions.swap_remove(&args.expression) {
            face1 = expression.face1.map(lower_block);
            face2 = expression.face2.map(lower_block);
            mouth_blocks = animation_frames(expression.mouth_blocks, args.disable_animations)
                .map(lower_block)
                .collect();
            eye_blocks = animation_frames(expression.eye_blocks, args.disable_animations)
                .map(lower_block)
                .collect();
        }

        GpuBustupSkeleton {
            origin_x,
            origin_y,
            effective_width,
            effective_height,
            bustup_id,
            base_blocks,
            face1,
            face2,
            mouth_blocks,
            eye_blocks,
        }
    }

    fn new_block(args: &Self::Args, data_offset: u32, block: PicBlock) -> Result<Self::BlockType> {
        Ok(Arc::new(GpuPictureBlock::new(
            args.context,
            block,
            &format!("{}/{}", args.label, data_offset),
        )))
    }

    fn new_block_with_id(
        args: &Self::Args,
        data_offset: u32,
        id: BustupBlockId,
        block: PicBlock,
    ) -> Result<Self::BlockType> {
        let block = Self::new_block(args, data_offset, block)?;
        // Publish each job before any portrait assembles its ordered layers.
        // Other workers may own remaining IDs in the same portrait.
        let handle = args.pending.lock().remove(&id);
        if let Some(handle) = handle {
            args.cache.finish_load(handle, block.clone());
        }
        Ok(block)
    }

    fn on_error(args: &Self::Args, error: &anyhow::Error) {
        let error = Arc::new(anyhow::anyhow!("Decoding {}: {error:#}", args.label));
        let pending = std::mem::take(&mut *args.pending.lock());
        for handle in pending.into_values() {
            args.cache.fail_load(handle, error.clone());
        }
    }

    fn build(
        args: &Self::Args,
        skeleton: Self::Skeleton<'_>,
        token: BustupBlockPromiseToken<Self::BlockType>,
    ) -> Result<Self::Output> {
        let GpuBustupSkeleton {
            origin_x,
            origin_y,
            effective_width,
            effective_height,
            bustup_id,
            base_blocks,
            face1,
            face2,
            mouth_blocks,
            eye_blocks,
        } = skeleton;

        let lower_block = |block: GpuBustupBlockPromise| block.materialize(&token);

        let base_blocks = base_blocks.into_iter().map(lower_block).collect::<Vec<_>>();
        let face1 = face1.map(|block| lower_block(block));
        let face2 = face2.map(|block| lower_block(block));
        let mouth_blocks = mouth_blocks
            .into_iter()
            .map(lower_block)
            .collect::<Vec<_>>();
        let eye_blocks = eye_blocks.into_iter().map(lower_block).collect::<Vec<_>>();

        Ok(Bustup {
            character_id: args.character_id,
            origin_x,
            origin_y,
            effective_width,
            effective_height,
            bustup_id,
            base_blocks,
            face1,
            face2,
            mouth_blocks,
            eye_blocks,
        })
    }
}

impl Bustup<CacheHandle<GpuPictureBlock>> {
    async fn resolve(self) -> Result<Bustup> {
        let resolve_blocks = |blocks: Vec<CacheHandle<GpuPictureBlock>>| {
            futures::future::try_join_all(blocks.into_iter().map(CacheHandle::wait_async))
        };
        // Pending cache IDs await outside the compute pool. Even a one-thread
        // pool can finish a queued producer while this portrait is waiting.
        Ok(Bustup {
            character_id: self.character_id,
            origin_x: self.origin_x,
            origin_y: self.origin_y,
            effective_width: self.effective_width,
            effective_height: self.effective_height,
            bustup_id: self.bustup_id,
            base_blocks: resolve_blocks(self.base_blocks).await?,
            face1: match self.face1 {
                Some(block) => Some(block.wait_async().await?),
                None => None,
            },
            face2: match self.face2 {
                Some(block) => Some(block.wait_async().await?),
                None => None,
            },
            mouth_blocks: resolve_blocks(self.mouth_blocks).await?,
            eye_blocks: resolve_blocks(self.eye_blocks).await?,
        })
    }
}

fn prepare_bustup(
    data: &[u8],
    args: GpuBustupBuilderArgs,
) -> Result<Bustup<CacheHandle<GpuPictureBlock>>> {
    prepare_bustup_with_encoding(data, args, TextEncoding::ShiftJis)
}

fn prepare_bustup_with_encoding(
    data: &[u8],
    args: GpuBustupBuilderArgs,
    encoding: TextEncoding,
) -> Result<Bustup<CacheHandle<GpuPictureBlock>>> {
    // Decoder/upload panics must not escape a detached Rayon job and abort the
    // process. Producer guards cancel and wake pending IDs while unwinding.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        shin_core::format::bustup::read_bustup_with_encoding::<GpuBustupBuilder>(
            data, args, encoding,
        )
    }))
    .map_err(|panic| {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        anyhow::anyhow!("Bustup worker failed: {message}")
    })?
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BustupArgs {
    pub expression: String,
    pub character_id: CharacterId,
    /// Select static descriptor0 rather than animated descriptors1..N.
    pub disable_animations: bool,
}

pub mod gerokasu2;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod native_load_tests;

#[cfg(test)]
mod cache_tests;

impl Asset for Bustup {
    type Args = BustupArgs;

    async fn load(
        context: &Arc<AssetLoadContext>,
        args: BustupArgs,
        name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        load_bustup(context, args, name, data, TextEncoding::ShiftJis).await
    }
}

async fn load_bustup(
    context: &Arc<AssetLoadContext>,
    args: BustupArgs,
    name: &str,
    data: AssetDataAccessor,
    encoding: TextEncoding,
) -> Result<Bustup> {
    let label = format!("{}[{}]", name, args.expression);
    let data = data.read_all().await?;
    let context = context.clone();

    shin_tasks::compute::spawn(move || {
        let args = GpuBustupBuilderArgs {
            context: GpuTextureBuilderContext {
                wgpu_device: &context.wgpu_device,
                wgpu_queue: &context.wgpu_queue,
            },
            cache: &context.bustup_cache,
            label,
            expression: args.expression,
            character_id: args.character_id,
            disable_animations: args.disable_animations,
            pending: Mutex::new(HashMap::default()),
        };

        match encoding {
            TextEncoding::ShiftJis => prepare_bustup(&data, args),
            TextEncoding::Utf8 => prepare_bustup_with_encoding(&data, args, encoding),
        }
    })
    .await?
    .resolve()
    .await
}
