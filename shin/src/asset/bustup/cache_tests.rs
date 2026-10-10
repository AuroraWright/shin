use super::*;
use crate::asset::system::AssetLoadContext;
use std::{
    sync::{Barrier, mpsc},
    time::Duration,
};

fn fixture(ids: &[u32]) -> Vec<u8> {
    // Two different descriptor IDs can refer to the same payload offset.
    // Independent IDs must each publish a cache completion.
    let base_size = 4 + ids.len() as u32 * 12;
    let expression_offset = 52 + base_size;
    let payload_offset = expression_offset + 4;
    let mut bytes = b"BUP4".to_vec();
    for word in [
        4,
        payload_offset + 44,
        0,
        0x0001_0001,
        1,
        0x501,
        0,
        52,
        base_size,
        0,
        expression_offset,
        4,
        ids.len() as u32,
    ] {
        bytes.extend(word.to_le_bytes());
    }
    for &id in ids {
        for word in [payload_offset, 44, id] {
            bytes.extend(word.to_le_bytes());
        }
    }
    bytes.extend(0u32.to_le_bytes()); // no expressions
    for word in [1u16, 1, 0, 0, 0, 0, 1, 1] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(0u32.to_le_bytes()); // uncompressed RGBA
    for word in [0u16, 0, 1, 1] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend([100, 150, 200, 255]);
    bytes.extend([0; 12]); // native sixteen-byte differential stride
    assert_eq!(bytes.len(), (payload_offset + 44) as usize);
    bytes
}

fn context() -> Arc<AssetLoadContext> {
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .unwrap();
    Arc::new(AssetLoadContext {
        wgpu_device: device,
        wgpu_queue: queue,
        bustup_cache: AssetCache::new(),
        picture_cache: Default::default(),
        font_cache: Default::default(),
    })
}
fn args(context: &AssetLoadContext) -> GpuBustupBuilderArgs<'_> {
    GpuBustupBuilderArgs {
        context: GpuTextureBuilderContext {
            wgpu_device: &context.wgpu_device,
            wgpu_queue: &context.wgpu_queue,
        },
        cache: &context.bustup_cache,
        label: "cache-contract-fixture".into(),
        expression: String::new(),
        character_id: CharacterId::new(123),
        disable_animations: false,
        pending: Mutex::new(HashMap::default()),
    }
}

// Control only scheduling. Parsing, reservations, upload publication, failure
// retirement and final materialization delegate to the production builder.
struct Controlled<'b>(PhantomData<&'b ()>);
struct ControlledArgs<'b> {
    inner: GpuBustupBuilderArgs<'b>,
    split: Option<&'b Barrier>,
    reserved: Option<&'b mpsc::Sender<()>>,
    release: Option<&'b Mutex<mpsc::Receiver<()>>>,
}
impl<'b> BustupBuilder for Controlled<'b> {
    type Args = ControlledArgs<'b>;
    type Skeleton<'a> = GpuBustupSkeleton<'a>;
    type BlockType = Arc<GpuPictureBlock>;
    type Output = Bustup<CacheHandle<GpuPictureBlock>>;
    fn new<'a>(args: &Self::Args, mut skeleton: BustupSkeleton<'a>) -> Self::Skeleton<'a> {
        let first = args.split.map(|barrier| {
            let first = GpuBustupBlockPromise::new(&args.inner, skeleton.base_blocks.remove(0));
            barrier.wait();
            first
        });
        let mut result = GpuBustupBuilder::new(&args.inner, skeleton);
        if let Some(first) = first {
            result.base_blocks.insert(0, first);
        }
        if let Some(ready) = args.reserved {
            ready.send(()).unwrap();
        }
        if let Some(release) = args.release {
            release.lock().recv_timeout(Duration::from_secs(5)).unwrap();
        }
        result
    }
    fn new_block(args: &Self::Args, offset: u32, block: PicBlock) -> Result<Self::BlockType> {
        GpuBustupBuilder::new_block(&args.inner, offset, block)
    }
    fn new_block_with_id(
        args: &Self::Args,
        offset: u32,
        id: BustupBlockId,
        block: PicBlock,
    ) -> Result<Self::BlockType> {
        GpuBustupBuilder::new_block_with_id(&args.inner, offset, id, block)
    }
    fn on_error(args: &Self::Args, error: &anyhow::Error) {
        GpuBustupBuilder::on_error(&args.inner, error);
    }
    fn build(
        args: &Self::Args,
        skeleton: Self::Skeleton<'_>,
        token: BustupBlockPromiseToken<Self::BlockType>,
    ) -> Result<Self::Output> {
        GpuBustupBuilder::build(&args.inner, skeleton, token)
    }
}

#[test]
#[ignore = "requires Metal GPU; no audio, ROM or working saves"]
fn competing_bustup_workers_publish_shared_ids_before_assembling_portraits() {
    let context = context();
    let split = Arc::new(Barrier::new(2));
    let (done, received) = mpsc::channel();
    let mut threads = Vec::new();
    for ids in [[1, 2], [2, 1]] {
        let context = context.clone();
        let split = split.clone();
        let done = done.clone();
        threads.push(std::thread::spawn(move || {
            let result = shin_core::format::bustup::read_bustup::<Controlled>(
                &fixture(&ids),
                ControlledArgs {
                    inner: args(&context),
                    split: Some(&split),
                    reserved: None,
                    release: None,
                },
            )
            .and_then(|pending| shin_tasks::block_on(pending.resolve()));
            done.send(result).unwrap();
        }));
    }
    let first = received
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_or_else(|e| panic!("{e:#}"));
    let second = received
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_or_else(|e| panic!("{e:#}"));
    assert!(Arc::ptr_eq(&first.base_blocks[0], &second.base_blocks[1]));
    assert!(Arc::ptr_eq(&first.base_blocks[1], &second.base_blocks[0]));
    assert!(
        !Arc::ptr_eq(&first.base_blocks[0], &first.base_blocks[1]),
        "Distinct IDs sharing an offset still publish independently"
    );
    assert_eq!(first.character_id.raw(), 123);
    let weak = Arc::downgrade(&first.base_blocks[0]);
    drop(first);
    drop(second);
    assert!(
        weak.upgrade().is_none(),
        "Completed cache must retain only weak owners"
    );
    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
#[ignore = "requires Metal GPU; no audio, ROM or working saves"]
fn failed_bustup_block_wakes_aliases_preserves_completed_blocks_and_retries() {
    let context = context();
    let (ready, ready_recv) = mpsc::channel();
    let (release, release_recv) = mpsc::channel();
    let (done, done_recv) = mpsc::channel();
    let worker_context = context.clone();
    let worker = std::thread::spawn(move || {
        let mut bytes = fixture(&[1, 2]);
        bytes[72..76].copy_from_slice(&1u32.to_le_bytes()); // second block's header is truncated
        let release_recv = Mutex::new(release_recv);
        let result = shin_core::format::bustup::read_bustup::<Controlled>(
            &bytes,
            ControlledArgs {
                inner: args(&worker_context),
                split: None,
                reserved: Some(&ready),
                release: Some(&release_recv),
            },
        );
        done.send(result.err().expect("Truncated block must fail").to_string())
            .unwrap();
    });
    ready_recv.recv_timeout(Duration::from_secs(5)).unwrap();
    let success = context
        .bustup_cache
        .lookup(BustupBlockId::new(1))
        .try_into_cache_handle()
        .unwrap_or_else(|_| panic!("missing reservation"));
    let failure = context
        .bustup_cache
        .lookup(BustupBlockId::new(2))
        .try_into_cache_handle()
        .unwrap_or_else(|_| panic!("missing reservation"));
    release.send(()).unwrap();
    assert!(
        !done_recv
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .is_empty()
    );
    let success = shin_tasks::block_on(success.wait_async()).unwrap();
    let failure = shin_tasks::block_on(failure.wait_async())
        .err()
        .expect("failed ID must wake with an error");
    assert!(failure.to_string().contains("cache-contract-fixture"));
    let retry = shin_tasks::block_on(
        prepare_bustup(&fixture(&[1, 2]), args(&context))
            .unwrap_or_else(|e| panic!("{e:#}"))
            .resolve(),
    )
    .unwrap_or_else(|e| panic!("{e:#}"));
    assert!(Arc::ptr_eq(&success, &retry.base_blocks[0]));
    assert_eq!(retry.base_blocks.len(), 2);
    worker.join().unwrap();
}

#[test]
#[ignore = "requires Metal GPU; no audio, ROM or working saves"]
fn pending_bustup_does_not_starve_a_single_compute_worker() {
    let context = context();
    let producer = match context.bustup_cache.lookup(BustupBlockId::new(1)) {
        CacheLookupResult::LoadRequired(producer) => producer,
        _ => panic!("fresh cache"),
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let (pending_send, pending_recv) = mpsc::channel();
    let waiter_context = context.clone();
    pool.spawn_fifo(move || {
        let pending = prepare_bustup(&fixture(&[1]), args(&waiter_context));
        pending_send.send(pending).unwrap();
    });
    let (published, published_recv) = mpsc::channel();
    let producer_context = context.clone();
    pool.spawn_fifo(move || {
        let args = args(&producer_context);
        args.pending.lock().insert(BustupBlockId::new(1), producer);
        let bytes = fixture(&[1]);
        let offset = u32::from_le_bytes(bytes[56..60].try_into().unwrap());
        let block =
            shin_core::format::picture::read_picture_block(&bytes[offset as usize..]).unwrap();
        let block =
            GpuBustupBuilder::new_block_with_id(&args, offset, BustupBlockId::new(1), block)
                .unwrap();
        published.send(block).unwrap();
    });
    let pending = pending_recv
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_or_else(|e| panic!("{e:#}"));
    let block = published_recv.recv_timeout(Duration::from_secs(5)).unwrap();
    let resolved = shin_tasks::block_on(pending.resolve()).unwrap_or_else(|e| panic!("{e:#}"));
    assert!(Arc::ptr_eq(&block, &resolved.base_blocks[0]));
}

#[test]
#[ignore = "requires Metal GPU; no audio, ROM or working saves"]
fn invalid_bustup_ranges_and_decoder_panics_retire_reservations_for_retry() {
    let context = context();
    for kind in [0, 1, 2] {
        let mut bytes = fixture(&[1]);
        match kind {
            0 => bytes[56..60].copy_from_slice(&u32::MAX.to_le_bytes()), // overflowing offset+size
            1 => bytes[60..64].copy_from_slice(&1u32.to_le_bytes()),     // decoder Result::Err
            2 => bytes[60..64].copy_from_slice(&28u32.to_le_bytes()), // decoder assertion: no pixel data
            _ => unreachable!(),
        }
        let failure = prepare_bustup(&bytes, args(&context))
            .err()
            .expect("corrupt block must fail");
        if kind == 2 {
            assert!(failure.to_string().contains("Bustup worker failed"));
        }
        let pending =
            prepare_bustup(&fixture(&[1]), args(&context)).unwrap_or_else(|e| panic!("{e:#}"));
        let resolved = shin_tasks::block_on(pending.resolve()).unwrap_or_else(|e| panic!("{e:#}"));
        assert_eq!(resolved.base_blocks.len(), 1);
        drop(resolved);
    }
}
