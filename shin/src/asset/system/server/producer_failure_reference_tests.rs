//! Public-loader recovery checks against temporary copies of supplied assets.
use super::*;
use crate::asset::{mask::MaskTexture, picture::Picture, texture_archive::TextureArchive};
use shin_render::gpu_texture::GpuTexture;

// Exercise the same generated builder used by ordinary typed UI atlases,
// including failures after decoding/uploading, rather than a synthetic panic.
#[derive(TextureArchive)]
struct DialogTextures {
    #[txa(name = "dialog_frame")]
    frame: GpuTexture,
    #[txa(name = "dialog_button")]
    button: GpuTexture,
    #[txa(name = "drag")]
    drag: GpuTexture,
    #[txa(name = "scroll")]
    scroll: GpuTexture,
}

fn u32_at(bytes: &[u8], offset: usize) -> usize {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}
fn put_u32(bytes: &mut [u8], offset: usize, value: usize) {
    bytes[offset..offset + 4].copy_from_slice(&u32::try_from(value).unwrap().to_le_bytes());
}

fn assert_unpublished<T: Asset>(server: &AssetServer) {
    assert!(
        server
            .loaded_assets
            .read()
            .unwrap()
            .get::<AssetMap<T>>()
            .is_none()
    );
    assert_eq!(
        server.context().picture_cache.reference_ownership_counts(),
        (0, 0)
    );
    assert_eq!(
        server.context().bustup_cache.reference_ownership_counts(),
        (0, 0)
    );
}

async fn failure_and_retry<T: Asset<Args = ()>>(
    server: &AssetServer,
    directory: &Path,
    path: &str,
    bad: &[u8],
    good: &[u8],
    kind: &str,
) -> Arc<T> {
    let file = directory.join(path.trim_start_matches('/'));
    std::fs::write(&file, bad).unwrap();
    let error = match server.load::<T, _>(path).await {
        Ok(_) => panic!("Corrupt {kind} unexpectedly loaded"),
        Err(error) => error,
    };
    let message = format!("{error:#}");
    assert!(message.contains(path), "Missing path: {message}");
    assert!(
        message.contains(&format!("{kind} worker failed:")),
        "Wrong boundary: {message}"
    );
    assert_unpublished::<T>(server);
    std::fs::write(&file, good).unwrap();
    let loaded = server.load::<T, _>(path).await.unwrap();
    let hit = server.load::<T, _>(path).await.unwrap();
    assert!(Arc::ptr_eq(&loaded, &hit));
    drop(hit);
    loaded
}

#[test]
#[ignore = "requires supplied ROM and Metal; no window, audio or progress writes"]
fn reference_asset_producer_panic_recovery() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_PRODUCER_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let context = || AssetLoadContext {
        game: crate::cli::Game::Umineko,
        higurashi_bustup_cache: Default::default(),
        wgpu_device: device.clone(),
        wgpu_queue: queue.clone(),
        bustup_cache: crate::asset::system::cache::AssetCache::new(),
        picture_cache: Default::default(),
        font_cache: Default::default(),
    };
    let make_server = || AssetServer::new(AssetIo::new_dir(&directory).unwrap(), context());
    let supplied = |path| super::accessor_io_reference_tests::supplied_payload(path).2;
    let txa = supplied("/systex.txa");
    assert_eq!(&txa[..4], b"TXA4");
    let scenario =
        shin_core::format::scenario::Scenario::new(supplied("/main.snr").into()).unwrap();
    let mask_path = scenario.info_tables().mask_info[0].path();
    let mask = supplied(&mask_path);
    let picture_path = scenario.info_tables().picture_info[96].path();
    let picture = supplied(&picture_path);
    let mut cases = Vec::new();
    shin_tasks::block_on(async {
        // Baseline mode stops in this first public load: without a producer
        // guard, this assertion escapes detached Rayon and aborts the process.
        for corruption in [
            "declared-size",
            "compressed-body",
            "unknown-name",
            "missing-field",
        ] {
            let mut bad = txa.clone();
            match corruption {
                "declared-size" => put_u32(&mut bad, 8, txa.len() + 1),
                "compressed-body" => {
                    let offset = u32_at(&bad, 0x28);
                    assert!(offset + 3 <= bad.len());
                    put_u32(&mut bad, 0x2c, 3);
                    bad[offset..offset + 3].copy_from_slice(&[1, 0, 0]);
                }
                "unknown-name" => bad[0x34] = b'?',
                "missing-field" => {
                    // Keep every remaining entry/texture valid but remove the
                    // last entry from the index consumed by the real decoder.
                    let count = u32_at(&bad, 16);
                    assert_eq!(count, 4);
                    put_u32(&mut bad, 16, count - 1);
                }
                _ => unreachable!(),
            }
            let server = make_server();
            let loaded = failure_and_retry::<DialogTextures>(
                &server,
                &directory,
                "/probe.txa",
                &bad,
                &txa,
                "TXA",
            )
            .await;
            assert!(loaded.frame.size().width > 0 && loaded.button.size().height > 0);
            assert!(loaded.drag.size().width > 0 && loaded.scroll.size().height > 0);
            let weak = Arc::downgrade(&loaded);
            drop(loaded);
            assert!(weak.upgrade().is_none());
            cases.push(format!("TXA {corruption}: error/path/no publication; same-server full GPU retry and retained cache hit; final owner retires"));
        }
        let mut bad = mask.clone();
        let offset = u32_at(&bad, 20);
        assert!(u32_at(&bad, 24) >= 7 && offset + 7 <= bad.len());
        put_u32(&mut bad, offset, 3);
        bad[offset + 4..offset + 7].copy_from_slice(&[1, 0, 0]);
        let server = make_server();
        let loaded = failure_and_retry::<MaskTexture>(
            &server,
            &directory,
            "/probe.msk",
            &bad,
            &mask,
            "Mask",
        )
        .await;
        assert_eq!(
            loaded.texture.size().width,
            u16::from_le_bytes(mask[16..18].try_into().unwrap()) as u32
        );
        assert_eq!(
            loaded.texture.size().height,
            u16::from_le_bytes(mask[18..20].try_into().unwrap()) as u32
        );
        let weak = Arc::downgrade(&loaded);
        drop(loaded);
        assert!(weak.upgrade().is_none());
        cases.push("Mask compressed-body: in-range bad history; error/path/no publication; same-server GPU retry/dimensions/cache hit; final owner retires".to_owned());

        let mut bad = picture.clone();
        let offset = u32_at(&bad, 40);
        let size = u32_at(&bad, 44);
        assert!(size > 20 && offset + size <= bad.len());
        put_u32(&mut bad, offset + 16, size);
        let server = make_server();
        let loaded = failure_and_retry::<Picture>(
            &server,
            &directory,
            "/probe.pic",
            &bad,
            &picture,
            "Picture",
        )
        .await;
        assert!(loaded.blocks.len() > 0);
        assert_eq!(
            server.context().picture_cache.reference_ownership_counts(),
            (1, 0)
        );
        let weak = Arc::downgrade(&loaded.blocks);
        drop(loaded);
        assert!(weak.upgrade().is_none());
        assert_eq!(
            server.context().picture_cache.reference_ownership_counts(),
            (0, 0)
        );
        cases.push("Picture compressed-body: valid descriptor with inconsistent block body; error/path/no publication or pending ID; same-server GPU retry/cache hit; final blocks retire".to_owned());
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work must complete before readback or retirement");
    let report = serde_json::json!({
        "cases_passed": cases.len(), "cases": cases,
        "adapter": format!("{:?}", adapter.get_info()),
        "source_paths": {"txa": "/systex.txa", "mask": mask_path, "picture": picture_path},
        "bytes": {"txa": txa.len(), "mask": mask.len(), "picture": picture.len()},
        "patch_mounted": std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
        "audio_initialized": false, "window_created": false, "archives_modified": false,
        "progress_files_written": false
    });
    std::fs::write(
        output.join("producer-recovery.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "{} producer failure/retry cases pass on Metal; no window/audio/progress writes",
        cases.len()
    );
}
