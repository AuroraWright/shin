//! Real NXA requests/decoder failures and same-player retries on the default audio output.
use super::*;
use crate::audio::{BgmPlayer, SePlayer, VoicePlayFlags, VoicePlayer};
use shin_audio::{AudioFile, AudioManager};
use shin_core::{
    format::scenario::{
        Scenario,
        info::{BgmId, SeId},
        instruction_elements::FromNumber,
    },
    time::{Ticks, Tween},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};

enum Player {
    Bgm(BgmPlayer),
    Se(SePlayer, Option<i32>),
    Voice(VoicePlayer),
}
impl Player {
    fn new(kind: &str, audio: Arc<AudioManager>, server: Arc<AssetServer>) -> Self {
        match kind {
            "BGM" => Self::Bgm(BgmPlayer::new(audio)),
            "SE" => Self::Se(SePlayer::new(audio), None),
            "once" => Self::Se(SePlayer::new(audio), Some(0)),
            "Voice" => Self::Voice(VoicePlayer::new(audio, server)),
            _ => unreachable!(),
        }
    }
    fn request(&mut self, server: Arc<AssetServer>, scenario: &Scenario, path: &str) {
        match self {
            Self::Bgm(p) => {
                assert!(p.request(
                    server,
                    path.into(),
                    BgmId::from_number(0),
                    false,
                    Volume(0.5),
                    Tween::IMMEDIATE
                ));
            }
            Self::Se(p, slot) => {
                if let Some(slot) = slot {
                    *slot = p.request_once(
                        server,
                        path.into(),
                        SeId::from_number(0),
                        Volume(0.5),
                        Pan::default(),
                    );
                } else {
                    p.request(
                        server,
                        path.into(),
                        0,
                        SeId::from_number(0),
                        true,
                        Volume(0.5),
                        Pan::default(),
                        1.0,
                        Tween::IMMEDIATE,
                    );
                }
            }
            Self::Voice(p) => {
                assert!(p.play(
                    scenario,
                    "19/11900001",
                    0,
                    0,
                    VoicePlayFlags::empty(),
                    Volume(0.5)
                ));
            }
        }
    }
    fn play_sync(&mut self, file: Arc<AudioFile>) {
        match self {
            Self::Bgm(p) => {
                p.play(
                    BgmId::from_number(0),
                    file,
                    "",
                    false,
                    Volume(0.5),
                    Tween::IMMEDIATE,
                );
            }
            Self::Se(p, None) => p.play(
                0,
                SeId::from_number(0),
                file,
                true,
                Volume(0.5),
                Pan::default(),
                1.0,
                Tween::IMMEDIATE,
            ),
            _ => unreachable!(),
        }
    }
    fn update(&mut self) {
        match self {
            Self::Bgm(p) => p.update(),
            Self::Se(p, _) => p.update(),
            Self::Voice(p) => p.update(Ticks::ZERO),
        }
    }
    fn status(&self) -> AudioWaitStatus {
        match self {
            Self::Bgm(p) => p.get_wait_status(),
            Self::Se(p, None) => p.get_wait_status(0),
            Self::Se(p, Some(slot)) => p.get_once_wait_status(*slot),
            Self::Voice(p) => p.get_wait_status(),
        }
    }
    fn clock(&self) -> Option<u64> {
        match self {
            Self::Bgm(p) => Some(p.position_micros()),
            Self::Se(p, None) => Some(u64::from(p.reference_source(0).1)),
            Self::Voice(p) => Some(p.position_millis()),
            Self::Se(_, Some(_)) => None,
        }
    }
    fn source_id_cleared(&self) -> bool {
        match self {
            Self::Bgm(p) => p.reference_source().0.is_none(),
            Self::Se(p, None) => p.reference_source(0).0.is_none(),
            _ => self.status().is_empty(),
        }
    }
    fn stop(&mut self) {
        match self {
            Self::Bgm(p) => p.stop(Tween::IMMEDIATE),
            Self::Se(p, _) => p.stop_all(Tween::IMMEDIATE),
            Self::Voice(p) => p.stop(),
        }
    }
}
fn corrupt(bytes: &[u8], profile: &str) -> Vec<u8> {
    let mut bad = bytes.to_vec();
    match profile {
        "declared-size" => bad[8..12].copy_from_slice(&(bytes.len() as u32 + 1).to_le_bytes()),
        "channels" => bad[16..18].copy_from_slice(&3u16.to_le_bytes()),
        "sample-rate" => bad[12..16].copy_from_slice(&12345u32.to_le_bytes()),
        "zero-frame-samples" => bad[20..22].copy_from_slice(&0u16.to_le_bytes()),
        _ => unreachable!(),
    }
    bad
}
fn finish_loading(player: &mut Player) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while player.status().contains(AudioWaitStatus::LOADING) {
        player.update();
        assert!(
            std::time::Instant::now() < deadline,
            "Failed NXA request remained loading"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
fn valid_playback(player: &mut Player) -> Option<u64> {
    finish_loading(player);
    assert!(player.status().contains(AudioWaitStatus::PLAYING));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while player.clock().is_some_and(|clock| clock == 0) {
        player.update();
        assert!(
            std::time::Instant::now() < deadline,
            "Retry did not advance on the default audio output"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    player.clock()
}
fn assert_no_live_file(server: &AssetServer) {
    let maps = server.loaded_assets.read().unwrap();
    assert!(
        maps.get::<AssetMap<AudioFile>>()
            .is_none_or(|map| map.values().all(|asset| asset.upgrade().is_none())),
        "Failed request retained a loaded audio asset"
    );
}

#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output; temporary NXA files only"]
fn reference_nxa_player_failure_recovery() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_NXA_RECOVERY_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(directory.join("voice/19")).unwrap();
    let supplied = |path| super::accessor_io_reference_tests::supplied_payload(path).2;
    let scenario = Scenario::new(supplied("/main.snr").into()).unwrap();
    let bgm = supplied("/bgm/umib_055.nxa");
    let se = supplied("/se/umise_028.nxa");
    let voice = supplied("/voice/19/11900001.nxa");
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let audio = Arc::new(AudioManager::new_for_tests());
    let make_server = || {
        Arc::new(AssetServer::new(
            AssetIo::new_dir(&directory).unwrap(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: crate::asset::system::cache::AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ))
    };
    let mut cases = Vec::new();
    for profile in [
        "declared-size",
        "channels",
        "sample-rate",
        "zero-frame-samples",
    ] {
        for kind in ["BGM", "SE", "once", "Voice"] {
            let (path, bytes) = match kind {
                "BGM" => ("/probe.nxa", &bgm),
                "Voice" => ("/voice/19/11900001.nxa", &voice),
                _ => ("/probe.nxa", &se),
            };
            let file = directory.join(path.trim_start_matches('/'));
            std::fs::write(&file, corrupt(bytes, profile)).unwrap();
            let server = make_server();
            let mut player = Player::new(kind, audio.clone(), server.clone());
            player.request(server.clone(), &scenario, path);
            finish_loading(&mut player);
            assert!(
                player.status().is_empty(),
                "Failed {kind}/{profile} retained wait bits"
            );
            assert!(player.source_id_cleared());
            assert_no_live_file(&server);
            std::fs::write(&file, bytes).unwrap();
            player.request(server, &scenario, path);
            let clock = valid_playback(&mut player);
            player.stop();
            cases.push(serde_json::json!({"kind":kind,"profile":profile,"entry":"async request/update","retry_clock":clock,"failed_wait_bits":0,"failed_source_id_cleared":true,"failed_live_asset_owners":0}));
        }
        if profile == "declared-size" {
            continue;
        }
        for kind in ["BGM", "SE"] {
            let bytes = if kind == "BGM" { &bgm } else { &se };
            let file = directory.join("probe.nxa");
            std::fs::write(&file, corrupt(bytes, profile)).unwrap();
            let server = make_server();
            let mut player = Player::new(kind, audio.clone(), server.clone());
            player.play_sync(server.load_sync("/probe.nxa").unwrap());
            assert!(
                player.status().is_empty(),
                "Failed direct {kind}/{profile} retained wait bits"
            );
            assert!(player.source_id_cleared());
            assert_no_live_file(&server);
            std::fs::write(&file, bytes).unwrap();
            player.play_sync(server.load_sync("/probe.nxa").unwrap());
            let clock = valid_playback(&mut player);
            player.stop();
            cases.push(serde_json::json!({"kind":kind,"profile":profile,"entry":"direct play","retry_clock":clock,"failed_wait_bits":0,"failed_source_id_cleared":true,"failed_live_asset_owners":0}));
        }
    }
    assert_eq!(cases.len(), 22);
    std::fs::write(output.join("nxa-recovery.json"), serde_json::to_vec_pretty(&serde_json::json!({"cases_passed":cases.len(),"cases":cases,"patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),"audio_device":"default output","window_created":false,"archives_modified":false,"working_progress_written":false})).unwrap()).unwrap();
    println!(
        "{} malformed NXA request/decoder profiles clear wait bits and retry on the same players; BGM/SE/Voice clocks advance on the default audio output",
        cases.len()
    );
}
