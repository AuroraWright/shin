//! Real retained Owner and dialogs; original native scene-owner oracle remains
//! independent of Rust. Async resource/write readiness is an explicit boundary.
use super::super::*;
use crate::{
    adv::higurashi::{
        autosave::{ProgressEntry, SaveProgress},
        reference_gpu_assets, reference_render_to_texture,
    },
    app::higurashi::{
        load_game::{Source, Storage},
        runtime_saves::Slot,
    },
};
use shin_core::format::save::PersistData;
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Condvar, Mutex as StdMutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const NOW: SaveDate = SaveDate {
    year: 2026,
    month: 10,
    day: 13,
    hour: 23,
    minute: 58,
    second: 59,
};
const HASH: u64 = 0xcbf29ce484222325;
fn add(mut h: u64, word: u32) -> u64 {
    for b in word.to_le_bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    h
}
fn profile(i: u32, salt: u32) -> Slot {
    Slot {
        valid: (i + salt) % 3 != 0,
        data: RuntimeGameData::from_progress(
            SaveProgress::new(
                ((i + salt) & 1) as usize,
                0xfedc0000 + i * 17 + salt,
                std::array::from_fn(|j| {
                    let j = j as u32;
                    ProgressEntry {
                        scenario_id: (100 + i * 13 + j * 10000 + salt) as i32,
                        random_seed: 0x87650000 + i * 31 + j * 7 + salt,
                        save_position: 0,
                        replay_target: 0x234100 + i * 100 + j * 23 + salt,
                        response_data: (0..3 + (i + j + salt) % 7)
                            .map(|k| (i * 11 + j * 17 + k * 37 + salt) as u8)
                            .collect(),
                    }
                }),
            )
            .unwrap(),
            SaveDate {
                year: (2000 + i + salt) as u16,
                month: (1 + i % 12) as u8,
                day: (1 + i % 28) as u8,
                hour: (i % 24) as u8,
                minute: ((i * 3) % 60) as u8,
                second: ((i * 7) % 60) as u8,
            },
            (0..4 + (i + salt) % 5)
                .map(|k| (0xf800 + i * 31 + k * 977 + salt) as u16)
                .collect(),
            (0..7 + (i + salt) % 9)
                .map(|k| (i * 19 + k * 41 + salt) as u8)
                .collect(),
        ),
    }
}
fn data_hash(mut h: u64, slot: &Slot) -> u64 {
    let d = &slot.data;
    h = add(h, u32::from(slot.valid));
    let date = crate::app::higurashi::gameplay::bindings::packed_date(d.date);
    for w in [
        date as u32,
        (date >> 32) as u32,
        d.progress.active_index() as u32,
        d.progress.extra_word,
    ] {
        h = add(h, w);
    }
    for e in &d.progress.entries {
        for w in [
            e.scenario_id as u32,
            e.random_seed,
            e.save_position,
            e.replay_target,
        ] {
            h = add(h, w);
        }
        h = add(h, e.response_data.len() as u32);
        for &b in &e.response_data {
            h = add(h, u32::from(b));
        }
    }
    h = add(h, d.trailing_values.len() as u32);
    for &v in &d.trailing_values {
        h = add(h, u32::from(v));
    }
    h = add(h, d.byte_variables.len() as u32);
    for &b in &d.byte_variables {
        h = add(h, u32::from(b));
    }
    h
}
fn slots_hash(slots: &Slots, settings: &SaveData) -> u64 {
    std::iter::once(&slots.auto)
        .chain(&slots.manual)
        .fold(add(HASH, u32::from(settings.save_menu_position)), data_hash)
}
fn current_hash(owner: &Owner) -> u64 {
    match &owner.current {
        Some(data) => data_hash(
            HASH,
            &Slot {
                valid: true,
                data: data.clone(),
            },
        ),
        None => data_hash(HASH, &Slots::loaded(&SaveData::default()).auto),
    }
}

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    protocol: String,
    capture: String,
}
impl Native {
    fn new() -> Self {
        let mut child = Command::new(
            std::env::var("HIGURASHI_SAVE_MENU_OWNER_NATIVE")
                .unwrap_or_else(|_| "/tmp/higurashi-native-save-menu-owner".into()),
        )
        .args([
            "game-data/higurashi/uncompressed_executable_update",
            &std::env::var("HIGURASHI_NEWRODIN_FNT")
                .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into()),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
        Self {
            input: Some(child.stdin.take().unwrap()),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            protocol: String::new(),
            capture: String::new(),
        }
    }
    fn command(&mut self, line: &str) -> Vec<String> {
        self.protocol.push_str(line);
        self.protocol.push('\n');
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{line}").unwrap();
        input.flush().unwrap();
        let mut output = String::new();
        assert!(
            self.output.read_line(&mut output).unwrap() > 0,
            "Native owner ended after {line}"
        );
        self.capture.push_str(&output);
        output.split_whitespace().map(str::to_owned).collect()
    }
    fn finish(&mut self) {
        self.input.take();
        let status = self.child.wait().unwrap();
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(status.success(), "{stderr}");
        if let Ok(dir) = std::env::var("SHIN_HIGURASHI_SAVE_MENU_LIVE_OWNER_CAPTURE") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(PathBuf::from(&dir).join("native-input.txt"), &self.protocol).unwrap();
            std::fs::write(PathBuf::from(&dir).join("native-output.txt"), &self.capture).unwrap();
            std::fs::write(PathBuf::from(&dir).join("native-stderr.txt"), stderr).unwrap();
        }
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Write starts on the real SaveGame worker but is held until the test has
/// inspected the immutable payload. Only this unique temporary path is used.
struct Store {
    root: PathBuf,
    gate: StdMutex<bool>,
    wake: Condvar,
    bytes: StdMutex<Vec<Vec<u8>>>,
    started: AtomicU32,
    fail: bool,
}
impl Store {
    fn new(fail: bool) -> Arc<Self> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "shin-higurashi-owner-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Arc::new(Self {
            root,
            gate: StdMutex::new(false),
            wake: Condvar::new(),
            bytes: StdMutex::new(vec![]),
            started: AtomicU32::new(0),
            fail,
        })
    }
    fn release(&self) {
        *self.gate.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
impl Storage for Store {
    fn trial_token(&self) -> bool {
        false
    }
    fn read(&self, _: Source) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    fn write(&self, bytes: &[u8]) -> Result<()> {
        self.bytes.lock().unwrap().push(bytes.to_vec());
        self.started.fetch_add(1, Ordering::Release);
        let mut gate = self.gate.lock().unwrap();
        while !*gate {
            gate = self.wake.wait(gate).unwrap();
        }
        if self.fail {
            bail!("deliberate isolated Owner write failure");
        }
        crate::adv::persistence::write_atomic(&self.root.join("save.dat"), bytes)
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Fixture {
    owner: Owner,
    settings: SaveData,
    slots: Slots,
    source: RuntimeGameData,
    store: Arc<Store>,
    frame: u32,
    micros: u64,
    date_calls: u32,
    origin: u32,
    resumes: u32,
    tops: u32,
    loaded: Vec<(RuntimeGameData, u32)>,
    stages: Vec<u32>,
    observations: u32,
    frozen_hash: u64,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.store.release();
    }
}
impl Fixture {
    fn new(env: &Environment, origin: u32, salt: u32, fail: bool, native: &mut Native) -> Self {
        let mut settings = SaveData::default();
        settings.persist_data =
            PersistData((0..8).map(|i| (0x8000 + i * 11 + salt) as i16).collect());
        settings.save_vectors.seen_messages = vec![0xdeadbeef, 0x87654321];
        settings.save_vectors.select_seen_choices = vec![0x98765432];
        settings.save_vectors.select_last_choices = vec![255, 128, 7];
        let slots = Slots {
            auto: profile(0, salt),
            manual: std::array::from_fn(|i| profile(i as u32 + 1, salt)),
        };
        let source = profile(102, salt).data;
        let prepared = Prepared::new(
            (origin == 0).then_some(&source),
            &settings,
            &slots,
            true,
            (1280, 720),
        )
        .unwrap();
        let store = Store::new(fail);
        let owner = Owner::from_prepared(
            Environment {
                common: env.common.clone(),
                server: env.server.clone(),
                sounds: env.sounds.clone(),
                storage: Some(store.clone()),
            },
            prepared,
        )
        .unwrap();
        let mut f = Self {
            owner,
            settings,
            slots,
            source,
            store,
            frame: 0,
            micros: 0,
            date_calls: 0,
            origin,
            resumes: 0,
            tops: 0,
            loaded: vec![],
            stages: vec![],
            observations: 0,
            frozen_hash: 0,
        };
        let row = native.command(&format!("Z {origin} 0 1 {salt} 1"));
        f.compare(&row, &[]);
        // Wait only the real loader transport. No Owner/task/input poll occurs
        // before the same original native First readiness boundary.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f.owner.loader.poll().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        f.tick(native, 1.0, true, 0, 0).unwrap();
        f.tick(native, 10.0, true, 0, 0).unwrap();
        f.tick(native, 1.0, true, 0, 0).unwrap();
        f
    }
    fn question(&self) -> Option<(i32, i32)> {
        self.owner.scope.iter().find_map(|m| match m {
            Owned::Question(d) => d.ready.as_ref().map(|(q, _)| (q.phase(), q.selected())),
            _ => None,
        })
    }
    fn write_pending(&self) -> bool {
        self.owner.scope.iter().any(|m| {
            matches!(
                m,
                Owned::Save {
                    transaction: Some(_),
                    ..
                }
            )
        })
    }
    fn tick(
        &mut self,
        native: &mut Native,
        delta: f32,
        permission: bool,
        pressed: u32,
        repeat: u32,
    ) -> Result<()> {
        let before = self.question();
        let writing = self.write_pending();
        self.frame += 1;
        self.micros += 10000;
        let mut date = || {
            self.date_calls += 1;
            Ok(NOW)
        };
        let result = self.owner.poll(
            &mut self.settings,
            &mut self.slots,
            Ticks::from_f32(delta),
            permission,
            Buttons {
                pressed,
                repeat,
                ..Default::default()
            },
            &PointerFrame {
                number: self.frame,
                time_seconds: self.micros as f64 / 1_000_000.0,
                events: &[],
            },
            Some(&mut date),
        );
        let after = self.question();
        if let (Some((phase, answer)), Some((next, _))) = (before, after) {
            if phase == 3 && next == 4 {
                native.command(&format!("Q 0 {answer}"));
            }
            if phase == 4 && next < 0 {
                native.command(&format!("Q 1 {answer}"));
            }
        }
        if !writing && self.write_pending() {
            self.frozen_hash = slots_hash(&self.slots, &self.settings);
        }
        if writing && !self.write_pending() {
            native.command(&format!("W {}", u32::from(result.is_ok())));
        }
        let mut requests = Vec::new();
        for request in self.owner.take_requests() {
            match request {
                Request::ReturnSource(stage) => {
                    self.stages.push(stage);
                    requests.push(stage);
                    if stage == 1 {
                        if self.origin < 2 {
                            self.resumes += 1;
                        } else {
                            self.tops += 1;
                        }
                    }
                }
                Request::Load { data, flags } => self.loaded.push((data, flags)),
            }
        }
        let row = native.command(&format!(
            "F {} {} {} {pressed:x} {repeat:x} {delta} 0",
            self.frame,
            self.micros,
            u32::from(permission)
        ));
        self.compare(&row, &requests);
        // Native events5/6 are original borrowed Question/Write membership.
        // Rust draw must retain the same last snapshot and exclude new children.
        let mut drawn = Vec::new();
        self.owner.scope.draw(|m| match m {
            Owned::Question(_) => drawn.push(5),
            Owned::Save { .. } => drawn.push(6),
            _ => {}
        });
        drawn.reverse();
        let events = row[29..]
            .iter()
            .map(|v| v.parse::<u32>().unwrap())
            .filter(|v| *v == 5 || *v == 6)
            .collect::<Vec<_>>();
        assert_eq!(drawn, events, "mixed scope snapshot frame{}", self.frame);
        result
    }
    fn compare(&mut self, row: &[String], requests: &[u32]) {
        assert_eq!(row[0], "O");
        assert_eq!(row[5].parse::<u32>().unwrap(), self.resumes);
        assert_eq!(row[7].parse::<u32>().unwrap(), self.tops);
        assert_eq!(row[6].parse::<u32>().unwrap(), self.date_calls);
        assert_eq!(row[10], u32::from(self.owner.complete()).to_string());
        assert_eq!(row[11], u32::from(self.owner.model.loaded).to_string());
        assert_eq!(
            row[12].parse::<u32>().unwrap(),
            self.owner.model.progress.to_bits()
        );
        assert_eq!(row[13].parse::<u32>().unwrap(), self.owner.model.request);
        assert_eq!(row[14].parse::<i32>().unwrap(), self.owner.model.selected);
        assert_eq!(
            u64::from_str_radix(&row[15], 16).unwrap(),
            data_hash(
                HASH,
                &Slot {
                    valid: true,
                    data: self.source.clone()
                }
            )
        );
        assert_eq!(
            u64::from_str_radix(&row[16], 16).unwrap(),
            current_hash(&self.owner)
        );
        assert_eq!(
            u64::from_str_radix(&row[17], 16).unwrap(),
            slots_hash(&self.slots, &self.settings)
        );
        assert_eq!(
            u64::from_str_radix(&row[18], 16).unwrap(),
            self.frozen_hash,
            "native frozen writer snapshot boundary"
        );
        if let Some((data, flags)) = self.loaded.last() {
            assert_eq!(*flags, 0);
            assert_eq!(
                u64::from_str_radix(&row[19], 16).unwrap(),
                data_hash(
                    HASH,
                    &Slot {
                        valid: true,
                        data: data.clone()
                    }
                )
            );
        }
        let events = row[29..]
            .iter()
            .map(|v| v.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        if requests.contains(&1) {
            assert_eq!(
                events,
                if self.origin < 2 {
                    vec![2, 4, 3]
                } else {
                    vec![2, 8]
                }
            );
        }
        assert_eq!(
            self.settings.save_vectors.seen_messages,
            vec![0xdeadbeef, 0x87654321]
        );
        assert_eq!(
            self.settings.save_vectors.select_seen_choices,
            vec![0x98765432]
        );
        assert_eq!(
            self.settings.save_vectors.select_last_choices,
            vec![255, 128, 7]
        );
        self.observations += 1;
    }
    fn dialog(&mut self, native: &mut Native) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.question().is_none_or(|q| q.0 != 2) {
            self.tick(native, 4.0, true, 0, 0).unwrap();
            assert!(
                Instant::now() < deadline,
                "real manual Question did not become interactive"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(self.question().unwrap().1, 1, "native default answer is No");
    }
    fn answer(&mut self, native: &mut Native, yes: bool) {
        if yes {
            let repeat = self
                .owner
                .scope
                .iter()
                .find_map(|m| match m {
                    Owned::Question(d) => d.ready.as_ref().map(|(q, _)| {
                        if q.buttons()[0].rect.y == q.buttons()[1].rect.y {
                            1 << 7
                        } else {
                            1 << 4
                        }
                    }),
                    _ => None,
                })
                .unwrap();
            self.tick(native, 0.0, true, 0, repeat).unwrap();
            assert_eq!(self.question().unwrap().1, 0);
        }
        self.tick(native, 10.0, true, 1 << 24, 0).unwrap();
        self.tick(native, 0.0, true, 0, 0).unwrap();
    }
    fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32)) {
        let pixels = reference_render_to_texture(device, queue, size, |ctx, target| {
            self.owner.prepare(ctx).unwrap();
            let mut pass = ctx.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(ctx.depth_stencil),
                "native retained manual owner",
            );
            self.owner.render(&mut pass);
        });
        assert_eq!(pixels.len(), size.0 as usize * size.1 as usize * 4);
        assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
    }
}

#[test]
#[ignore = "whole retained SaveMenu with original native callback/snapshot oracle, real archive/dialog/preview resources, offscreen Metal/exact Loopback Audio and unique temporary gated save storage"]
fn higurashi_live_save_menu_owner_matches_native_dialogs_writes_snapshots_and_returns() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager)));
    let common = Arc::new(CommonAssets {
        scenario: server.load_sync("/main.snr").unwrap(),
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    let env = Environment {
        common,
        server,
        sounds: sounds.clone(),
        storage: None,
    };
    let mut native = Native::new();
    let mut observations = 0;
    let mut owners = 0;
    let mut frames = 0;
    // (origin,action,Yes,salt,failing writer,navigate one row). Load carries
    // both complete records; no scene execution is claimed by this Owner test.
    for (origin, action, yes, salt, fail, navigate) in [
        (0, 0, true, 0, false, true),
        (0, 0, false, 0, false, false),
        (0, 0, true, 2, false, false),
        (0, 0, true, 0, true, true),
        (0, 1, true, 0, false, true),
        (0, 1, false, 0, false, false),
        (1, 2, true, 0, false, true),
        (1, 2, false, 0, false, false),
        (2, 2, true, 0, false, true),
        (0, 3, false, 0, false, false),
        (1, 3, false, 0, false, false),
        (2, 3, false, 0, false, false),
    ] {
        let mut f = Fixture::new(&env, origin, salt, fail, &mut native);
        owners += 1;
        if navigate {
            f.tick(&mut native, 4.0, true, 0, 1 << 6).unwrap();
            assert_eq!(f.owner.model.selected, 1);
            assert_eq!(f.settings.save_menu_position, 0);
            // Native b102c advances scroll by one tick per owner frame, even
            // when the other clocks receive a larger delta. Confirm is gated
            // until this four-tick reveal finishes on both implementations.
            while !f.owner.model.scroll.is_idle() {
                f.tick(&mut native, 4.0, true, 0, 0).unwrap();
            }
        }
        let selected = f.owner.model.selected as usize;
        let before = slots_hash(&f.slots, &f.settings);
        let previous = f.slots.manual[selected].data.clone();
        let source = f.source.clone();
        f.render(&device, &queue, (1280, 720));
        frames += 1;
        if action == 3 {
            f.tick(&mut native, 10.0, true, 1 << 25, 0).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !f.owner.complete() {
                f.tick(&mut native, 10.0, true, 0, 0).unwrap();
                assert!(Instant::now() < deadline);
            }
            assert_eq!(f.stages, vec![0, 1]);
            assert_eq!(slots_hash(&f.slots, &f.settings), before);
            assert!(f.loaded.is_empty());
            assert_eq!(f.store.started.load(Ordering::Acquire), 0);
        } else {
            f.tick(
                &mut native,
                10.0,
                true,
                if action == 1 { 1 << 15 } else { 1 << 24 },
                0,
            )
            .unwrap();
            f.dialog(&mut native);
            f.render(&device, &queue, (1920, 1080));
            frames += 1;
            f.answer(&mut native, yes);
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut inspected = false;
            let mut failed = false;
            loop {
                if f.store.started.load(Ordering::Acquire) > 0 && !inspected {
                    let bytes = f.store.bytes.lock().unwrap()[0].clone();
                    assert_eq!(
                        bytes,
                        f.slots.disk(&f.settings).encode().unwrap(),
                        "real worker retains the frozen slot/settings encode boundary"
                    );
                    assert_eq!(
                        std::fs::read(f.store.root.join("save.dat")).ok(),
                        None,
                        "gated write must not publish early"
                    );
                    f.store.release();
                    inspected = true;
                }
                match f.tick(&mut native, 4.0, true, 0, 0) {
                    Ok(()) => {}
                    Err(error) => {
                        assert!(fail, "{error:#}");
                        assert!(
                            format!("{error:#}")
                                .contains("deliberate isolated Owner write failure")
                        );
                        failed = true;
                        break;
                    }
                }
                if action == 2 && yes && !f.loaded.is_empty() {
                    break;
                }
                if (!yes || action < 2)
                    && f.owner.model.request == 0
                    && f.owner
                        .scope
                        .iter()
                        .all(|m| !matches!(m, Owned::Question(_) | Owned::Save { .. }))
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "live menu callback/write did not complete"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            if action == 2 && yes {
                assert_eq!(f.loaded.len(), 1);
                assert_eq!(f.loaded[0], (previous, 0));
                assert!(f.stages.is_empty());
                assert!(!f.owner.complete());
                assert_eq!(f.settings.save_menu_position, selected as u8);
            } else if yes && action < 2 {
                assert!(inspected);
                assert_eq!(f.store.started.load(Ordering::Acquire), 1);
                assert_eq!(failed, fail);
                if fail {
                    assert_eq!(slots_hash(&f.slots, &f.settings), before);
                    assert!(f.owner.fault.is_some());
                } else {
                    assert!(f.store.root.join("save.dat").is_file());
                }
                if action == 0 {
                    assert_eq!(f.owner.current.as_ref().unwrap().date, NOW);
                    assert_eq!(f.date_calls, 1);
                    if !fail {
                        let mut expected = source.clone();
                        expected.date = NOW;
                        assert_eq!(f.slots.manual[selected].data, expected);
                        assert_eq!(f.settings.save_menu_position, selected as u8);
                    }
                } else {
                    assert!(!f.slots.manual[selected].valid);
                    assert_eq!(f.slots.manual[selected].data, previous);
                    assert_eq!(f.settings.save_menu_position, 0);
                    assert_eq!(f.date_calls, 0);
                }
            } else {
                assert_eq!(slots_hash(&f.slots, &f.settings), before);
                assert_eq!(f.date_calls, 0);
                assert_eq!(f.store.started.load(Ordering::Acquire), 0);
                assert!(f.loaded.is_empty());
            }
            assert_eq!(
                f.source, source,
                "opening/writing/return cannot mutate source current/date"
            );
        }
        observations += f.observations;
        assert_eq!(native.command("X"), vec!["X"]);
        drop(f);
    }
    assert!(sounds.lock().bank.is_some());
    native.finish();
    eprintln!(
        "Live SaveMenu Owner:{owners} native owners/{observations} exact source-menu-complete101-slot/clock/request/snapshot observations;{frames}720p/1080p real-resource frames;real No-default Questions,Save/empty/failed rollback/Delete/Load/Back,one deterministic date,unique temp gated persistence and source return stage ordering pass;scene-slot/ADV parent transfer is separately tested"
    );
}
