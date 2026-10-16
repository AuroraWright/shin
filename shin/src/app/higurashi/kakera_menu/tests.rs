use super::*;
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveDate};
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
fn bytes(length: usize, seed: u8) -> Vec<u8> {
    (0..length)
        .map(|i| seed.wrapping_add((i * 17) as u8))
        .collect()
}
pub(super) fn source(
    scenario: i32,
    active: usize,
    kind: usize,
    length: usize,
    count: usize,
) -> GameData {
    let entry = |index| GameDataEntry {
        scenario_id: if index == active {
            scenario
        } else {
            0x5a + 31 * index as i32
        },
        random_seed: 0x65 + 31 * index as u32,
        save_position: 0x70 + 31 * index as u32,
        replay_target: 0x7b + 31 * index as u32,
        response_data: bytes(7 + index, 0x5a + 19 * index as u8),
    };
    GameData {
        date: SaveDate::now(),
        extra_word: 0x12,
        entry: entry(active),
        primary_entry: (active == 1).then(|| entry(0)),
        trailing_values: vec![0x5a, 0x6b, 0x7c],
        byte_variables: (0..length)
            .map(|i| match kind {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => (i * 17 + 0x5a) as u8,
                4 => u8::from(i < count) * 2,
                _ => unreachable!(),
            })
            .collect(),
    }
}
pub(super) fn persistent(kind: usize, count: usize, completion: i16) -> PersistData {
    let mut data = (0..192)
        .map(|i| match kind {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => {
                if i % 2 == 0 {
                    i16::MIN
                } else {
                    i16::MAX
                }
            }
            4 => (i % 5) as i16 - 2,
            5 => i16::from(i >= 60 && i < 60 + count) * 2,
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    data[176] = completion;
    PersistData(data)
}
struct Profile {
    model: Model,
    input: String,
    expected: Vec<String>,
    moves: usize,
    disabled: usize,
    source_allocs: usize,
}
impl Profile {
    fn new(
        scenario: i32,
        active: usize,
        selected: i32,
        p_kind: usize,
        b_kind: usize,
        length: usize,
        count: usize,
        completion: i16,
    ) -> Self {
        let mut original = source(scenario, active, b_kind, length, count);
        let model = Model::new(
            original.clone(),
            selected,
            &persistent(p_kind, count, completion),
        );
        let mut p = Self {
            model,
            input: format!(
                "N {scenario} {active} {selected} {p_kind} {b_kind} {length} {count} {completion}\n"
            ),
            expected: vec![],
            moves: 0,
            disabled: 0,
            source_allocs: 3 + active + usize::from(length != 0),
        };
        p.snapshot(-1);
        original.entry.response_data[0] ^= 255;
        if let Some(first) = original.primary_entry.as_mut() {
            first.response_data[0] ^= 255;
        }
        original.trailing_values[0] ^= 255;
        if let Some(first) = original.byte_variables.first_mut() {
            *first ^= 255;
        }
        assert_ne!(
            original.entry.response_data,
            p.model.data.progress.active_entry().response_data
        );
        assert_ne!(original.trailing_values, p.model.data.trailing_values);
        p
    }
    fn snapshot(&mut self, result: i32) {
        let m = &self.model;
        let saved = m.data.save_data();
        let mut row = format!(
            "M {result} {} {} {} {} {}",
            u8::from(m.alternate),
            m.page,
            m.selected,
            m.bgm_id,
            m.statuses.len()
        );
        for v in &m.statuses {
            write!(row, " {v}").unwrap();
        }
        write!(row, " {}", m.pages.len()).unwrap();
        for page in &m.pages {
            write!(row, " {}", page.len()).unwrap();
            for id in page {
                write!(row, " {id}").unwrap();
            }
        }
        let details = m.details();
        write!(row, " {}", details.len()).unwrap();
        for label in details {
            write!(
                row,
                " {} {:016x} {}",
                label.title.len(),
                hash(label.title.as_bytes()),
                u8::from(label.grey)
            )
            .unwrap();
        }
        write!(
            row,
            " 1 {} {} 0 {} {:08x}",
            self.moves,
            self.disabled,
            usize::from(saved.primary_entry.is_some()),
            saved.extra_word
        )
        .unwrap();
        let entries = if let Some(first) = &saved.primary_entry {
            [Some(first), Some(&saved.entry)]
        } else {
            [Some(&saved.entry), None]
        };
        for entry in entries {
            if let Some(e) = entry {
                write!(
                    row,
                    " {:08x} {:08x} {:08x} {:08x} {} {:016x}",
                    e.scenario_id as u32,
                    e.random_seed,
                    e.save_position,
                    e.replay_target,
                    e.response_data.len(),
                    hash(&e.response_data)
                )
                .unwrap();
            } else {
                write!(
                    row,
                    " 00000000 00000000 00000000 00000000 0 {:016x}",
                    hash(&[])
                )
                .unwrap();
            }
        }
        let tail = m
            .data
            .trailing_values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        write!(
            row,
            " {} {:016x} {} {:016x}",
            m.data.trailing_values.len(),
            hash(&tail),
            m.data.byte_variables.len(),
            hash(&m.data.byte_variables)
        )
        .unwrap();
        self.expected.push(row);
    }
    fn navigation(&mut self, permission: bool, captured: bool, repeat: u32) {
        if !permission {
            self.disabled += 1;
        }
        if permission && !captured && self.model.navigate(repeat) {
            self.moves += 1;
        }
        self.input.push_str(&format!(
            "U {} {} {repeat:x}\n",
            u8::from(permission),
            u8::from(captured)
        ));
        self.snapshot(1);
    }
    fn point(&mut self, x: f32, y: f32) {
        self.input.push_str(&format!("V {x} {y}\n"));
        self.snapshot(self.model.cell_at(x, y).map_or(-1, |i| i as i32));
    }
    fn finish(mut self, input: &mut String, expected: &mut Vec<String>) {
        self.input.push_str("D\n");
        self.expected.push(format!("D {}", self.source_allocs));
        input.push_str(&self.input);
        expected.extend(self.expected);
    }
}

#[test]
#[ignore = "updated actual616a0/62d08/64890/64ac0/630e0 catalogs/unlocks/pages/details/navigation/contact and GameData vector retirement;headless,no window/audio/saves"]
fn higurashi_kakera_catalog_pages_details_and_grid_match_native_menu() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    let mut record = |mut p: Profile| {
        for repeat in [0x20, 0x40, 0x80, 0x10, 0xa0, 0x50, 0x30, 0xc0] {
            p.navigation(true, false, repeat);
        }
        p.navigation(false, false, 0x20);
        p.navigation(true, true, 0x40);
        for (x, y) in [
            (249.0, 284.0),
            (250.0, 284.0),
            (681.999, 527.999),
            (682.0, 528.0),
            (744.0, 284.0),
            (1240.0, 284.0),
            (250.0, 546.0),
            (744.0, 546.0),
            (1240.0, 546.0),
            (1672.0, 790.0),
        ] {
            p.point(x, y);
        }
        p.finish(&mut input, &mut expected);
        owners += 1;
    };
    for kind in 0..5 {
        for selected in (-1..=104).chain([500]) {
            record(Profile::new(
                if selected % 2 == 0 { 9999 } else { -1 },
                kind % 2,
                selected,
                kind,
                0,
                0,
                0,
                0,
            ));
        }
    }
    for count in 0..=100 {
        record(Profile::new(0, count % 2, 87, 5, 0, 0, count, 0));
    }
    for kind in 0..5 {
        for length in [0, 1, 49, 50, 51, 52, 53, 64] {
            for completion in [1, 2] {
                for selected in [-1, 0, 49, 50, 51, 52, 53, 100] {
                    record(Profile::new(
                        if completion == 1 { 10000 } else { i32::MAX },
                        length % 2,
                        selected,
                        0,
                        kind,
                        length,
                        51,
                        completion,
                    ));
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_KAKERA_MENU_MODEL").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let sent = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(sent.as_bytes()));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_KAKERA_MENU_MODEL_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in [
            ("input.txt", input.as_bytes()),
            ("output.txt", &output.stdout),
            ("stderr.txt", &output.stderr),
        ] {
            std::fs::write(std::path::Path::new(&dir).join(name), data).unwrap();
        }
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let actual = text.lines().collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(*a, e, "Kakera model native row {i}");
    }
    eprintln!(
        "Kakera menu model:{owners} native constructors,{} exact observations;159 descriptors,26 pages,both unlock banks/thresholds/hidden cells,all initial positions/detail labels,grid/contact,independent source vectors and balanced native final retirement pass",
        expected.len()
    );
}

#[test]
fn patchless_kakera_sparse_artwork_skips_missing_pages_and_cells_with_bounded_navigation() {
    let mut model = Model::new(source(400, 0, 2, 64, 0), 0, &persistent(2, 0, 0));
    let statuses = model.statuses.clone();
    let pages = model.pages.clone();
    let mut artwork = model
        .pages
        .iter()
        .map(|rows| vec![false; rows.len()])
        .collect::<Vec<_>>();
    artwork[0][1] = true;
    artwork[0][4] = true;
    artwork[2][5] = true;
    artwork[16][0] = true;
    model.artwork = Some(artwork);
    model.selected = 1;
    assert!(model.any_artwork());
    for direction in [1, -1] {
        for expected in if direction == 1 {
            [2, 16, 0]
        } else {
            [16, 2, 0]
        } {
            model.turn_page(direction);
            assert_eq!(model.page, expected);
            assert!(model.cell_available(model.page, model.selected));
            for repeat in [
                1 << 4,
                1 << 5,
                1 << 6,
                1 << 7,
                (1 << 4) | (1 << 6),
                (1 << 5) | (1 << 7),
            ] {
                for _ in 0..16 {
                    model.navigate(repeat);
                    assert!(model.selected < model.pages[model.page].len());
                    assert!(model.cell_available(model.page, model.selected));
                }
            }
            for (cell, &(x, y)) in catalog::CELLS
                .iter()
                .take(model.pages[model.page].len())
                .enumerate()
            {
                assert_eq!(
                    model.cell_at(x as f32 + 1., y as f32 + 1.),
                    model.cell_available(model.page, cell).then_some(cell)
                );
            }
        }
    }
    assert_eq!(model.statuses, statuses);
    assert_eq!(model.pages, pages);
    model
        .artwork
        .as_mut()
        .unwrap()
        .iter_mut()
        .flatten()
        .for_each(|cell| *cell = false);
    let position = (model.page, model.selected);
    for direction in [-1, 1] {
        model.turn_page(direction);
    }
    for bit in [4, 5, 6, 7] {
        assert!(!model.navigate(1 << bit));
    }
    assert_eq!((model.page, model.selected), position);
    assert!(model.details().is_empty());
}

#[test]
#[ignore = "actual base-only Kakera atlas capability, normal cards and unavailable Origin input; offscreen GPU, no audio"]
fn patchless_kakera_actual_base_atlas_keeps_normal_cards_and_allows_escape_from_missing_origin() {
    use crate::{
        adv::{
            characters::view::CharacterTextures,
            higurashi::{input::Buttons, reference_gpu_assets},
            input_area::InputAreaTarget,
        },
        asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo},
    };
    let (device, queue, _) = reference_gpu_assets();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(root.join("higurashi/data.rom")).unwrap();
    let server = AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Higurashi,
            wgpu_device: device,
            wgpu_queue: queue,
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    let atlas = server
        .load_sync::<CharacterTextures>("/kakera.txa")
        .unwrap();
    let mut normal = Model::new(source(400, 0, 2, 64, 0), 0, &persistent(2, 0, 0));
    let statuses = normal.statuses.clone();
    normal.apply_artwork(&atlas);
    assert_eq!(normal.pages.len(), 17);
    assert_eq!(normal.pages.iter().map(Vec::len).sum::<usize>(), 100);
    for (page, rows) in normal.pages.iter().enumerate() {
        for cell in 0..rows.len() {
            assert!(normal.cell_available(page, cell));
        }
    }
    assert!(normal.any_artwork());
    assert!(normal.can_exit());
    assert_eq!(normal.statuses, statuses);
    let mut alternate = Model::new(source(10008, 0, 2, 64, 0), 0, &persistent(2, 0, 0));
    let statuses = alternate.statuses.clone();
    alternate.apply_artwork(&atlas);
    assert!(!alternate.any_artwork());
    assert!(alternate.can_exit());
    for page in 0..alternate.pages.len() {
        alternate.page = page;
        for cell in 0..alternate.pages[page].len() {
            assert!(!alternate.cell_available(page, cell));
        }
        for &(x, y) in &catalog::CELLS {
            assert_eq!(alternate.cell_at(x as f32 + 1., y as f32 + 1.), None);
        }
        assert!(alternate.details().is_empty());
    }
    assert_eq!(alternate.statuses, statuses);
    let mut state = motion::Motion::new(alternate, false);
    let position = (state.model.page, state.model.selected);
    for bit in [4, 5, 6, 7] {
        state.controls(
            true,
            false,
            Buttons {
                repeat: 1 << bit,
                ..Default::default()
            },
        );
    }
    state.controls(
        true,
        false,
        Buttons {
            pressed: 1 << 24,
            ..Default::default()
        },
    );
    assert_eq!((state.model.page, state.model.selected), position);
    assert!(
        state
            .tasks()
            .all(|(_, kind, _)| !matches!(kind, motion::Kind::DecideShake(_)))
    );
    assert!(state.actions.contains(&motion::Action::Cue("error")));
    state.probe(glam::vec2(251., 285.));
    assert_eq!(state.pointer_cell, -1);
    state.release(glam::vec2(251., 285.));
    assert!(state.request.is_none());
    state.controls(
        true,
        false,
        Buttons {
            pressed: 1 << 25,
            ..Default::default()
        },
    );
    assert!(state.actions.contains(&motion::Action::Cue("cancel")));
    assert!(
        state
            .tasks()
            .any(|(_, kind, _)| kind == motion::Kind::ExitShake)
    );
}
