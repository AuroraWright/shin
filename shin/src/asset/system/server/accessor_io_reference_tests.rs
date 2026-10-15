use super::*;
use shin_core::primitives::stateless_reader::StatelessReader;
use std::io::Read;

pub(crate) struct IoFault {
    arm: unsafe extern "C" fn(*const std::ffi::c_char, u64, u64, i32) -> i32,
    report: unsafe extern "C" fn(u32) -> u64,
}

impl IoFault {
    pub(crate) fn load() -> Self {
        unsafe extern "C" {
            fn dlsym(
                handle: *mut std::ffi::c_void,
                name: *const std::ffi::c_char,
            ) -> *mut std::ffi::c_void;
        }
        unsafe fn symbol(name: &std::ffi::CStr) -> *mut std::ffi::c_void {
            let pointer = unsafe { dlsym((-2isize) as *mut _, name.as_ptr()) };
            assert!(
                !pointer.is_null(),
                "Missing explicit test interposer {name:?}"
            );
            pointer
        }
        unsafe {
            Self {
                arm: std::mem::transmute::<
                    *mut std::ffi::c_void,
                    unsafe extern "C" fn(*const std::ffi::c_char, u64, u64, i32) -> i32,
                >(symbol(c"shin_reference_io_arm")),
                report: std::mem::transmute::<
                    *mut std::ffi::c_void,
                    unsafe extern "C" fn(u32) -> u64,
                >(symbol(c"shin_reference_io_report")),
            }
        }
    }
    pub(crate) fn arm(&self, path: &Path, offset: u64, length: u64, mode: i32) {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(
            unsafe { (self.arm)(path.as_ptr(), offset, length, mode) },
            0
        );
    }
    pub(crate) fn report(&self) -> [u64; 3] {
        [0, 1, 2].map(|index| unsafe { (self.report)(index) })
    }
}

struct OffsetProbe {
    file: StatelessFile,
    offset: Arc<std::sync::atomic::AtomicU64>,
}
impl StatelessReader for OffsetProbe {
    fn size(&self) -> u64 {
        self.file.size()
    }
    fn read_at_exact(&self, offset: u64, bytes: &mut [u8]) -> std::io::Result<()> {
        self.offset
            .store(offset, std::sync::atomic::Ordering::SeqCst);
        self.file.read_at_exact(offset, bytes)
    }
}

pub(crate) fn supplied_portrait() -> (LayeredAssetIo, PathBuf, String, u64, Vec<u8>) {
    let base = PathBuf::from(std::env::var_os("SHIN_REFERENCE_ROM").unwrap());
    let mut paths = Vec::new();
    if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
        paths.push(PathBuf::from(patch));
    }
    paths.push(base);
    let mut io = LayeredAssetIo::new();
    for path in &paths {
        io.try_with_rom(path).unwrap();
    }
    // Keep this probe independent of the production asset accessor under test.
    let mut scenario_bytes = Vec::new();
    for path in &paths {
        let mut rom =
            RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
        if let Ok(entry) = rom.find_file("/main.snr") {
            rom.open_file(entry)
                .unwrap()
                .read_to_end(&mut scenario_bytes)
                .unwrap();
            break;
        }
    }
    let scenario = shin_core::format::scenario::Scenario::new(scenario_bytes.into()).unwrap();
    let portrait = scenario.info_tables().bustup_info[782].path();
    for path in paths {
        let offset = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mut rom = RomReader::new(OffsetProbe {
            file: StatelessFile::new(File::open(&path).unwrap()).unwrap(),
            offset: offset.clone(),
        })
        .unwrap();
        if let Ok(entry) = rom.find_file(&portrait) {
            let mut reader = rom.open_file(entry).unwrap();
            let mut prefix = [0; 4];
            reader.read_exact(&mut prefix).unwrap();
            let start = offset.load(std::sync::atomic::Ordering::SeqCst);
            let mut bytes = prefix.to_vec();
            reader.read_to_end(&mut bytes).unwrap();
            assert_eq!(&bytes[..4], b"BUP4");
            return (io, path, portrait, start, bytes);
        }
    }
    panic!("Supplied archives lack authored portrait");
}

pub(crate) fn supplied_payload(path: &str) -> (PathBuf, u64, Vec<u8>) {
    let mut paths = Vec::new();
    if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
        paths.push(PathBuf::from(patch));
    }
    paths.push(PathBuf::from(
        std::env::var_os("SHIN_REFERENCE_ROM").unwrap(),
    ));
    for archive in paths {
        let offset = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mut rom = RomReader::new(OffsetProbe {
            file: StatelessFile::new(File::open(&archive).unwrap()).unwrap(),
            offset: offset.clone(),
        })
        .unwrap();
        if let Ok(entry) = rom.find_file(path) {
            let mut reader = rom.open_file(entry).unwrap();
            let mut prefix = [0; 4];
            reader.read_exact(&mut prefix).unwrap();
            let start = offset.load(std::sync::atomic::Ordering::SeqCst);
            let mut bytes = prefix.to_vec();
            reader.read_to_end(&mut bytes).unwrap();
            return (archive, start, bytes);
        }
    }
    panic!("Supplied archives lack {path}");
}

#[test]
#[ignore = "requires supplied ROM and explicit Darwin syscall interposer; no GPU/audio/saves"]
fn reference_asset_accessor_io_failure_recovery() {
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_ASSET_IO_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let file = output.join("removed.asset");
    std::fs::write(&file, b"initial").unwrap();
    let directory = AssetIo::new_dir(&output).unwrap();
    // Resolve while the file exists, then exercise the real lookup/open race.
    let data = directory.read_file("/removed.asset").unwrap();
    std::fs::remove_file(&file).unwrap();
    assert_eq!(
        data.cursor().err().unwrap().kind(),
        std::io::ErrorKind::NotFound
    );
    assert_eq!(
        data.read_all_sync().unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    assert_eq!(
        shin_tasks::block_on(data.read_all()).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    let directory_data = directory.read_file("/").unwrap();
    assert_eq!(
        directory_data.read_all_sync().unwrap_err().kind(),
        std::io::ErrorKind::IsADirectory
    );
    assert_eq!(
        shin_tasks::block_on(directory_data.read_all())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::IsADirectory
    );
    let recovered = b"same accessor, replacement file";
    std::fs::write(&file, recovered).unwrap();
    assert_eq!(data.read_all_sync().unwrap(), recovered);
    assert_eq!(shin_tasks::block_on(data.read_all()).unwrap(), recovered);
    let mut cursor_bytes = Vec::new();
    data.cursor()
        .unwrap()
        .read_to_end(&mut cursor_bytes)
        .unwrap();
    assert_eq!(cursor_bytes, recovered);

    let fault = IoFault::load();
    let (io, archive, path, offset, bytes) = supplied_portrait();
    let data = io.read_file(&path).unwrap();
    let mut summary = String::new();
    for method in ["sync", "async", "cursor"] {
        for mode in 1..=4 {
            fault.arm(&archive, offset, bytes.len() as u64, mode);
            let result = match method {
                "sync" => data.read_all_sync(),
                "async" => shin_tasks::block_on(data.read_all()),
                "cursor" => {
                    let mut bytes = Vec::new();
                    data.cursor()
                        .unwrap()
                        .read_to_end(&mut bytes)
                        .map(|_| bytes)
                }
                _ => unreachable!(),
            };
            let report = fault.report();
            assert!(
                report[0] > 0,
                "The real selected archive payload must be read"
            );
            match mode {
                1 => {
                    assert_eq!(result.unwrap_err().raw_os_error(), Some(5));
                    assert_eq!(report[1], 1);
                }
                2 => {
                    assert_eq!(
                        result.unwrap_err().kind(),
                        std::io::ErrorKind::UnexpectedEof
                    );
                    assert_eq!(report[2], 4);
                }
                3 => {
                    assert_eq!(result.unwrap(), bytes);
                    assert_eq!(report[1], 1);
                }
                4 => {
                    assert_eq!(result.unwrap(), bytes);
                    assert!(report[0] > 1);
                    assert_eq!(report[1], 0);
                }
                _ => unreachable!(),
            }
            fault.arm(&archive, offset, bytes.len() as u64, 0);
            assert_eq!(
                data.read_all_sync().unwrap(),
                bytes,
                "Same-accessor retry must read the complete supplied payload"
            );
            summary.push_str(&format!("{method} mode{mode}: hits/faults/real bytes={report:?}; same-accessor full retry passes.\n"));
        }
    }
    std::fs::remove_file(file).unwrap();
    summary.push_str(&format!("5 filesystem errors,3 replacement-file reads,12 ROM fault/short-read/interruption cases with12 full retries pass; archive={} asset={path} offset={offset} size={}. No GPU/audio/user saves.\n", archive.display(), bytes.len()));
    eprint!("{summary}");
    std::fs::write(output.join("accessor.txt"), summary).unwrap();
}

pub(crate) fn verify_asset_load_failures(
    context: &AssetLoadContext,
    scenario: &shin_core::format::scenario::Scenario,
) {
    use crate::asset::{
        bustup::{Bustup, BustupArgs, CharacterId},
        picture::Picture,
    };
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_ASSET_IO_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let fault = IoFault::load();
    let mut summary = String::new();
    for picture in [false, true] {
        for mode in [1, 2] {
            let (io, mut archive, portrait, mut offset, mut bytes) = supplied_portrait();
            let path = if picture {
                scenario.info_tables().picture_info[96].path()
            } else {
                portrait
            };
            if picture {
                let mut paths = Vec::new();
                if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
                    paths.push(PathBuf::from(patch));
                }
                paths.push(PathBuf::from(
                    std::env::var_os("SHIN_REFERENCE_ROM").unwrap(),
                ));
                let mut found = false;
                for candidate in paths {
                    let observed = Arc::new(std::sync::atomic::AtomicU64::new(0));
                    let mut rom = RomReader::new(OffsetProbe {
                        file: StatelessFile::new(File::open(&candidate).unwrap()).unwrap(),
                        offset: observed.clone(),
                    })
                    .unwrap();
                    if let Ok(entry) = rom.find_file(&path) {
                        let mut cursor = rom.open_file(entry).unwrap();
                        let mut prefix = [0; 36];
                        cursor.read_exact(&mut prefix).unwrap();
                        offset = observed.load(std::sync::atomic::Ordering::SeqCst) + 36;
                        bytes.clear();
                        cursor.read_to_end(&mut bytes).unwrap();
                        archive = candidate;
                        found = true;
                        break;
                    }
                }
                assert!(found, "Supplied picture missing");
            }
            let server = AssetServer::new(
                io.into(),
                AssetLoadContext {
                    game: crate::cli::Game::Umineko,
                    higurashi_bustup_cache: Default::default(),
                    wgpu_device: context.wgpu_device.clone(),
                    wgpu_queue: context.wgpu_queue.clone(),
                    bustup_cache: crate::asset::system::cache::AssetCache::new(),
                    picture_cache: Default::default(),
                    font_cache: Default::default(),
                },
            );
            let load = || {
                shin_tasks::block_on(async {
                    if picture {
                        server.load::<Picture, _>(&path).await.map(|asset| {
                            assert!(asset.effective_width > 0 && asset.effective_height > 0);
                        })
                    } else {
                        let info = &scenario.info_tables().bustup_info[782];
                        server
                            .load_with_args::<Bustup, _>(
                                &path,
                                BustupArgs {
                                    expression: info.emotion.to_string(),
                                    character_id: CharacterId::from_metadata(
                                        info.lipsync_character_id,
                                    ),
                                    disable_animations: false,
                                },
                            )
                            .await
                            .map(|_| ())
                    }
                })
            };
            fault.arm(&archive, offset, bytes.len() as u64, mode);
            let error = load().unwrap_err();
            let report = fault.report();
            fault.arm(&archive, offset, bytes.len() as u64, 0);
            assert!(report[0] > 0 && report[1] > 0);
            // Picture cache waiters share an Arc error through its existing
            // formatted-error contract; uncached BUP retains the typed cause.
            let message = format!("{error:#}");
            if picture {
                assert!(message.contains(if mode == 1 {
                    "os error 5"
                } else {
                    "failed to read whole buffer"
                }));
            } else {
                let io_error = error.downcast_ref::<std::io::Error>().unwrap();
                if mode == 1 {
                    assert_eq!(io_error.raw_os_error(), Some(5));
                } else {
                    assert_eq!(io_error.kind(), std::io::ErrorKind::UnexpectedEof);
                }
            }
            if mode == 2 {
                assert_eq!(report[2], 4);
            }
            assert!(
                format!("{error:#}").contains(&path),
                "Asset error must identify the requested path"
            );
            assert_eq!(
                server.context().picture_cache.reference_ownership_counts(),
                (0, 0)
            );
            assert_eq!(
                server.context().bustup_cache.reference_ownership_counts(),
                (0, 0)
            );
            assert!(
                server
                    .loaded_assets
                    .read()
                    .unwrap()
                    .get::<AssetMap<Picture>>()
                    .is_none()
            );
            assert!(
                server
                    .loaded_assets
                    .read()
                    .unwrap()
                    .get::<AssetMap<Bustup>>()
                    .is_none()
            );
            load().unwrap();
            assert_eq!(
                server.context().picture_cache.reference_ownership_counts(),
                (0, 0)
            );
            assert_eq!(
                server.context().bustup_cache.reference_ownership_counts(),
                (0, 0)
            );
            summary.push_str(&format!("{} mode{mode}: actual read failure propagated with asset path; hits/faults/bytes={report:?}; no live/pending cache owners or failed asset publication; same-server supplied-asset GPU decode retry passes.\n", if picture { "PIC96 body after36-byte header" } else { "BUP782 whole read" }));
        }
    }
    eprint!("{summary}");
    std::fs::write(output.join("asset-loaders.txt"), summary).unwrap();
}
