use super::*;
use crate::{
    adv::{Adv, AdvAssets},
    asset::{
        picture::Picture,
        system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache},
    },
    layer::user::UserLayer,
    render::PreRenderContext,
    update::UpdateContext,
};
use shin_core::{
    format::{rom::RomReader, save::GameData},
    primitives::{stateless_reader::StatelessFile, update::FrameId},
    time::Ticks,
    vm::command::types::{LayerbankId, PlaneId},
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    process::Command,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

// Real FIFO reads and controlled seekable ROM syscall boundaries through the
// production asset accessors. Both use unchanged supplied BUP bytes/decoders.
#[test]
#[ignore = "requires supplied ROMs, macOS sample, Metal and exact Loopback Audio"]
fn reference_authored_blocked_read_retirement() {
    assert!(std::env::var_os("SHIN_REFERENCE_BLOCKED_READ_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-blocked-read-caller".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires supplied ROMs, syscall interposer, macOS sample, Metal and exact Loopback Audio"]
fn reference_authored_seekable_read_retirement() {
    assert!(std::env::var_os("SHIN_REFERENCE_SEEKABLE_READ_ONLY").is_some());
    assert!(std::env::var_os("SHIN_REFERENCE_BLOCKED_READ_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-seekable-read-caller".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

const DEADLINE: Duration = Duration::from_secs(20);

fn game(boundary: u32) -> GameData {
    let mut route = RouteRecorder::default();
    for _ in 0..512 {
        route.record_persistent(0);
    }
    GameData::from_entry(GameDataEntry {
        scenario_id: 0,
        random_seed: 0,
        save_position: boundary,
        route_data: route.snapshot(),
    })
}

fn finish(
    live: &mut Adv,
    context: &mut PreRenderContext,
    server: &Arc<AssetServer>,
    boundary: u32,
) {
    let deadline = Instant::now() + DEADLINE;
    let mut frame = FrameId::default();
    let mut pre_render = PreRenderContext {
        breakup_texture: context.breakup_texture,
        device: context.device,
        queue: context.queue,
        resize_source: context.resize_source,
        sampler_store: context.sampler_store,
        depth_stencil: context.depth_stencil,
        pipeline_storage: &mut *context.pipeline_storage,
        dynamic_buffer: &mut *context.dynamic_buffer,
        encoder: &mut *context.encoder,
    };
    while live.pending_restore.is_some() {
        assert!(Instant::now() < deadline, "Retry load timed out");
        frame.advance();
        live.update_saved_scene_load(&mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            pre_render: &mut pre_render,
        });
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        live.saved_scene_load_error().is_none(),
        "{:?}",
        live.saved_scene_load_error()
    );
    assert_eq!(live.vm_state.save_position, boundary);
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct SeekableReadHold {
    arm: unsafe extern "C" fn(*const std::ffi::c_char, u64) -> i32,
    state: unsafe extern "C" fn() -> i32,
    release: unsafe extern "C" fn(),
    completed: unsafe extern "C" fn() -> i32,
}

#[cfg(target_os = "macos")]
impl SeekableReadHold {
    fn load() -> Self {
        unsafe extern "C" {
            fn dlsym(
                handle: *mut std::ffi::c_void,
                name: *const std::ffi::c_char,
            ) -> *mut std::ffi::c_void;
        }
        unsafe fn symbol(name: &std::ffi::CStr) -> *mut std::ffi::c_void {
            // Darwin RTLD_DEFAULT. Loading the interposer is explicit in the
            // external runner; no engine reader or decoder is substituted.
            let value = unsafe { dlsym((-2isize) as *mut _, name.as_ptr()) };
            assert!(!value.is_null(), "Missing syscall observer {name:?}");
            value
        }
        unsafe {
            Self {
                arm: std::mem::transmute::<
                    *mut std::ffi::c_void,
                    unsafe extern "C" fn(*const std::ffi::c_char, u64) -> i32,
                >(symbol(c"shin_reference_pread_arm")),
                state: std::mem::transmute::<
                    *mut std::ffi::c_void,
                    unsafe extern "C" fn() -> i32,
                >(symbol(c"shin_reference_pread_state")),
                release: std::mem::transmute::<*mut std::ffi::c_void, unsafe extern "C" fn()>(
                    symbol(c"shin_reference_pread_release"),
                ),
                completed: std::mem::transmute::<
                    *mut std::ffi::c_void,
                    unsafe extern "C" fn() -> i32,
                >(symbol(c"shin_reference_pread_completed")),
            }
        }
    }
}

// Trace only the independent metadata probe to obtain the opaque ROM entry's
// real offset. The live loader below uses unmodified StatelessFile readers.
struct OffsetProbe {
    file: StatelessFile,
    offset: Arc<std::sync::atomic::AtomicU64>,
}
impl shin_core::primitives::stateless_reader::StatelessReader for OffsetProbe {
    fn size(&self) -> u64 {
        self.file.size()
    }
    fn read_at_exact(&self, offset: u64, bytes: &mut [u8]) -> std::io::Result<()> {
        self.offset
            .store(offset, std::sync::atomic::Ordering::SeqCst);
        self.file.read_at_exact(offset, bytes)
    }
}

pub(super) fn verify(
    context: &mut PreRenderContext,
    assets: &AdvAssets,
    scenario: &Arc<Scenario>,
) {
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_BLOCKED_READ_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    #[cfg(target_os = "macos")]
    let hold =
        std::env::var_os("SHIN_REFERENCE_SEEKABLE_READ_ONLY").map(|_| SeekableReadHold::load());
    #[cfg(not(target_os = "macos"))]
    let hold: Option<()> = None;
    let info = &scenario.info_tables().bustup_info[782];
    let portrait_path = info.path();
    let picture_path = scenario.info_tables().picture_info[96].path();
    let rom_path = PathBuf::from(std::env::var_os("SHIN_REFERENCE_ROM").unwrap());
    let mut rom =
        RomReader::new(StatelessFile::new(File::open(&rom_path).unwrap()).unwrap()).unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file(&portrait_path).unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(&bytes[..4], b"BUP4");
    std::fs::write(output.join("portrait.bup"), &bytes).unwrap();
    #[cfg(target_os = "macos")]
    let seekable_target = hold.map(|_| {
        let mut candidates = Vec::new();
        if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
            candidates.push(PathBuf::from(patch));
        }
        candidates.push(rom_path.clone());
        for path in candidates {
            let offset = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let mut reader = RomReader::new(OffsetProbe {
                file: StatelessFile::new(File::open(&path).unwrap()).unwrap(),
                offset: offset.clone(),
            })
            .unwrap();
            if let Ok(entry) = reader.find_file(&portrait_path) {
                let mut prefix = [0; 4];
                reader
                    .open_file(entry)
                    .unwrap()
                    .read_exact(&mut prefix)
                    .unwrap();
                assert_eq!(&prefix, b"BUP4");
                return (path, offset.load(std::sync::atomic::Ordering::SeqCst));
            }
        }
        panic!("No actual ROM contains authored portrait");
    });
    let mut authored_assets = assets.clone();
    authored_assets.scenario = scenario.clone();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut summaries = String::new();
    for action in ["replace", "shutdown"] {
        // Fresh caches bound retirement to this one loader. The temporary
        // directory shadows only the actual authored portrait path.
        let overlay = output.join(format!("overlay-{}-{action}", std::process::id()));
        if hold.is_none() {
            std::fs::create_dir(&overlay).unwrap();
        }
        let fifo = overlay.join(portrait_path.trim_start_matches('/'));
        let writer = if hold.is_none() {
            std::fs::create_dir_all(fifo.parent().unwrap()).unwrap();
            assert!(
                Command::new("mkfifo")
                    .arg(&fifo)
                    .status()
                    .unwrap()
                    .success()
            );
            // Opening both ends bounds setup; the production reader waits
            // for bytes and EOF in its real read_to_end system calls.
            Some(
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&fifo)
                    .unwrap(),
            )
        } else {
            None
        };
        let mut io = LayeredAssetIo::new();
        if hold.is_none() {
            io.try_with_dir(&overlay).unwrap();
        }
        if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
            io.try_with_rom(PathBuf::from(patch)).unwrap();
        }
        io.try_with_rom(&rom_path).unwrap();
        let server = Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: context.device.clone(),
                wgpu_queue: context.queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let mut live = Adv::new(
            audio.clone(),
            server.clone(),
            authored_assets.clone(),
            Scripter::new(scenario, 0, 0),
        );
        // Keep a genuine installed scene while the next load is blocked.
        live.load_game_data(&SaveData::default(), &game(56), server.clone())
            .unwrap();
        finish(&mut live, context, &server, 56);
        #[cfg(target_os = "macos")]
        if let Some(hold) = hold {
            use std::os::unix::ffi::OsStrExt;
            let (path, offset) = seekable_target.as_ref().unwrap();
            let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { (hold.arm)(path.as_ptr(), *offset) }, 0);
        }
        live.load_game_data(&SaveData::default(), &game(100), server.clone())
            .unwrap();
        let deadline = Instant::now() + DEADLINE;
        while !server.reference_requested_paths().contains(&portrait_path) || {
            #[cfg(target_os = "macos")]
            {
                hold.is_some_and(|hold| unsafe { (hold.state)() } != 2)
            }
            #[cfg(not(target_os = "macos"))]
            {
                false
            }
        } {
            assert!(
                Instant::now() < deadline,
                "Authored portrait request never started"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(live.vm_state.save_position, 56, "No early publication");
        assert!(live.pending_restore.as_ref().unwrap().ready.is_none());
        let retirement = live
            .pending_restore
            .as_ref()
            .unwrap()
            .task
            .as_ref()
            .unwrap()
            .retirement_handle()
            .unwrap();
        assert!(!retirement.is_finished());
        let partial = shin_tasks::block_on(server.load::<Picture, _>(&picture_path)).unwrap();
        let picture_owner = Arc::downgrade(&partial);
        let block_owner = Arc::downgrade(&partial.blocks);
        drop(partial);
        assert!(
            picture_owner.upgrade().is_some(),
            "Pending scene owns its already-loaded PIC"
        );
        let (entered, entry) = mpsc::channel();
        let (returned, caller_return) = mpsc::channel();
        let sample_path = output.join(format!("{action}.sample.txt"));
        let bytes = bytes.clone();
        let observer = retirement.clone();
        let partial_owner = picture_owner.clone();
        let helper = std::thread::spawn(move || {
            entry.recv_timeout(DEADLINE).unwrap();
            let sample = Command::new("/usr/bin/sample")
                .args([
                    std::process::id().to_string(),
                    "1".into(),
                    "1".into(),
                    "-file".into(),
                ])
                .arg(&sample_path)
                .output()
                .unwrap();
            let held = !observer.is_finished() && partial_owner.upgrade().is_some();
            let premature = caller_return.try_recv().is_ok();
            // Always release the real syscall before asserting sample data,
            // so a sampler failure cannot strand the loader/caller.
            if let Some(mut writer) = writer {
                writer.write_all(&bytes).unwrap();
                drop(writer);
            }
            #[cfg(target_os = "macos")]
            if let Some(hold) = hold {
                assert_eq!(unsafe { (hold.state)() }, 2);
                unsafe { (hold.release)() };
            }
            (sample.status.success(), sample_path, held, premature)
        });
        entered.send(()).unwrap();
        if action == "replace" {
            live.load_game_data(&SaveData::default(), &game(56), server.clone())
                .unwrap();
            let _ = returned.send(());
            assert!(retirement.is_finished());
            assert_eq!(live.vm_state.save_position, 56);
            finish(&mut live, context, &server, 56);
        } else {
            drop(live);
            let _ = returned.send(());
            assert!(retirement.is_finished());
            live = Adv::new(
                audio.clone(),
                server.clone(),
                authored_assets.clone(),
                Scripter::new(scenario, 0, 0),
            );
        }
        let (sample_ok, sample_path, held, premature) = helper.join().unwrap();
        assert!(sample_ok, "Sampling the actual blocked syscall failed");
        let sample = std::fs::read_to_string(&sample_path).unwrap();
        assert!(
            sample.contains("read_all_sync")
                && sample.contains("Bustup")
                && if hold.is_none() {
                    sample.contains("read  (in libsystem_kernel.dylib)")
                } else {
                    sample.contains("shin_reference_held_pread")
                        && sample.contains("StatelessFile")
                }
                && sample.contains("PendingSceneRestore")
                && sample.contains("_pthread_cond_wait"),
            "Sample must contain the real production Bustup file read: {}",
            sample_path.display()
        );
        #[cfg(target_os = "macos")]
        if let Some(hold) = hold {
            assert_eq!(unsafe { (hold.completed)() }, 1);
        }
        assert!(
            held,
            "Pending loader/resource remain owned while read is blocked"
        );
        assert!(
            !premature,
            "Cancellation caller waits for the active read and producer"
        );
        assert!(picture_owner.upgrade().is_none());
        assert!(block_owner.upgrade().is_none());
        assert_eq!(
            server.context().picture_cache.reference_ownership_counts(),
            (0, 0)
        );
        assert_eq!(
            server.context().bustup_cache.reference_ownership_counts(),
            (0, 0)
        );
        assert_eq!(
            shin_tasks::async_io::spawn(async { 42 }).poll_with_timeout(DEADLINE),
            Some(42)
        );
        // Remove the temporary shadow so the same asset/cache IDs retry via
        // normal ROM IO; no progress files are read, written or restored.
        if hold.is_none() {
            std::fs::remove_file(&fifo).unwrap();
        }
        live.load_game_data(&SaveData::default(), &game(100), server.clone())
            .unwrap();
        finish(&mut live, context, &server, 100);
        let group = live.adv_state.plane_layer_group(PlaneId::new(0));
        let picture_owner = group.get_layer(LayerbankId::new(0));
        let Some(UserLayer::Picture(picture)) = picture_owner.as_deref() else {
            panic!("Retry PIC missing")
        };
        let portrait_owner = group.get_layer(LayerbankId::new(3));
        let Some(UserLayer::Bustup(portrait)) = portrait_owner.as_deref() else {
            panic!("Retry BUP missing")
        };
        let retry_picture = picture.reference_resource_owner();
        let retry_portrait = portrait.reference_resource_owner();
        assert_eq!(
            portrait.inner_ref().character_id(),
            i32::from(info.lipsync_character_id as i16)
        );
        assert!(!live.is_finished());
        drop(live);
        assert!(retry_picture.upgrade().is_none());
        assert!(retry_portrait.upgrade().is_none());
        assert_eq!(
            server.context().picture_cache.reference_ownership_counts(),
            (0, 0)
        );
        assert_eq!(
            server.context().bustup_cache.reference_ownership_counts(),
            (0, 0)
        );
        if hold.is_none() {
            std::fs::remove_dir_all(&overlay).unwrap();
        }
        let summary = format!(
            "{action}: authored load56->100, held {} read of {portrait_path}; caller waits, partial PIC/BUP/cache producers retire before return, normal ROM retry100 publishes PIC96/BUP782 and all owners retire. sample={}\n",
            if hold.is_some() {
                "actual seekable ROM pread boundary"
            } else {
                "actual directory/FIFO"
            },
            sample_path.display()
        );
        #[cfg(target_os = "macos")]
        if let Some((path, offset)) = &seekable_target {
            summaries.push_str(&format!("seekable_target={}:offset={offset}; actual pread returned supplied BUP4 bytes; no in-kernel stall simulated.\n", path.display()));
        }
        eprint!("{summary}");
        summaries.push_str(&summary);
    }
    std::fs::write(output.join("desktop.txt"), summaries).unwrap();
}
