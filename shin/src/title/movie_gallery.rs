//! MovieMode constructor87ae0, controller88d10 and deferred playback8b220.
//! Native list/task model; gallery artwork and the playback child still need an owner.
use shin_core::{
    format::{
        save::SaveData,
        scenario::info::{MovieId, MovieInfo},
    },
    time::{Ticks, Tween, Tweener},
};

/// Original ten signed IDs at170ae4, referenced through native224e40.
pub(super) const MOVIE_GALLERY_IDS: [i32; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
#[derive(Debug, Clone, Copy)]
pub(super) struct MovieRow {
    pub movie: MovieId,
    pub unlocked: bool,
    /// Authored plate rectangle before the renderer's layout transformation.
    pub plate: [f32; 4],
}
impl MovieRow {
    ///89210 uses the fixed384x216 inset, even while focus expands the artwork.
    pub fn thumbnail_bounds(&self) -> [f32; 4] {
        let left = 136.0 + self.plate[0] + (self.plate[2] - 384.0) * 0.5;
        let top = 190.0 + self.plate[1] + (self.plate[3] - 216.0) * 0.5;
        [left, top, left + 384.0, top + 216.0]
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MovieRequest {
    OpenMovie(MovieId),
    ReturnTitle { argument: u8, music_stop_ticks: u32 },
}
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct MovieControls {
    pub cancel: bool,
    pub confirm: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
}
#[derive(Clone, Copy)]
enum Task {
    Play,
    Close,
}

pub(super) struct MovieGalleryList {
    rows: Vec<MovieRow>,
    focus: Vec<Tweener>,
    flashes: Vec<Tweener>,
    selected: usize,
    close_flash: Tweener,
    closing: bool,
    tasks: Vec<Task>,
    frame_prepared: bool,
    requests: Vec<MovieRequest>,
    sounds: Vec<&'static str>,
    pointer_target: i32,
    pointer_row: i32,
}
fn rate(clock: &mut Tweener, value: f32, speed: f32) {
    clock.enqueue_now(
        value,
        Tween::linear(Ticks::from_f32((value - clock.value()).abs() / speed)),
    );
}
fn flash(clock: &mut Tweener) {
    rate(clock, 0.7, 0.2);
    clock.enqueue(0.0, Tween::linear(Ticks::from_f32(0.7 / 0.2)));
}
impl MovieGalleryList {
    pub fn new(info: &MovieInfo, save: &SaveData) -> anyhow::Result<Self> {
        use shin_core::format::scenario::instruction_elements::FromNumber;
        let rows: Vec<_> = MOVIE_GALLERY_IDS
            .into_iter()
            .filter_map(|id| {
                info.get(id as usize)?;
                Some(MovieRow {
                    movie: MovieId::from_number(id),
                    unlocked: save.save_vectors.unlocked_movies.contains(id as usize),
                    plate: [
                        (id % 4) as f32 * 412.0,
                        (id / 4) as f32 * 242.0,
                        412.0,
                        242.0,
                    ],
                })
            })
            .collect();
        anyhow::ensure!(
            !rows.is_empty(),
            "Movie gallery has no authored movie entries"
        );
        let focus = rows
            .iter()
            .map(|row| Tweener::new(if row.movie.into_i32() == 0 { 1.0 } else { 0.0 }))
            .collect();
        let flashes = rows.iter().map(|_| Tweener::new(0.0)).collect();
        Ok(Self {
            rows,
            focus,
            flashes,
            selected: 0,
            close_flash: Tweener::new(0.0),
            closing: false,
            tasks: Vec::new(),
            frame_prepared: false,
            requests: Vec::new(),
            sounds: Vec::new(),
            pointer_target: -1,
            pointer_row: -1,
        })
    }
    pub fn rows(&self) -> &[MovieRow] {
        &self.rows
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn feedback(&self, index: usize) -> (f32, f32) {
        (self.focus[index].value(), self.flashes[index].value())
    }
    pub fn close_flash(&self) -> f32 {
        self.close_flash.value()
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
    pub fn completion_tenths(&self) -> usize {
        self.rows.iter().filter(|row| row.unlocked).count() * 1000 / self.rows.len()
    }
    pub fn completion_label(&self) -> String {
        let value = self.completion_tenths();
        if value >= 1000 {
            "達成率:100％".into()
        } else {
            format!("達成率:{}.{}％", value / 10, value % 10)
        }
    }
    ///100300 polls newest tasks before any clocks or input. Play tasks inspect
    /// the live selected row, rather than retaining the originally confirmed ID.
    pub fn prepare_frame(&mut self) {
        if self.frame_prepared {
            return;
        }
        self.frame_prepared = true;
        let mut keep = Vec::new();
        for task in std::mem::take(&mut self.tasks).into_iter().rev() {
            match task {
                Task::Play if self.flashes[self.selected].is_idle() => self
                    .requests
                    .push(MovieRequest::OpenMovie(self.rows[self.selected].movie)),
                Task::Close if self.close_flash.is_idle() => {
                    self.requests.push(MovieRequest::ReturnTitle {
                        argument: 8,
                        music_stop_ticks: 30,
                    })
                }
                _ => keep.push(task),
            }
        }
        keep.reverse();
        self.tasks = keep;
    }
    pub fn update(&mut self, delta: Ticks) {
        self.prepare_frame();
        // Native bba50 skips clocks for nonpositive time but scene tasks poll.
        if delta > Ticks::ZERO {
            for clock in self
                .focus
                .iter_mut()
                .chain(&mut self.flashes)
                .chain(std::iter::once(&mut self.close_flash))
            {
                clock.update(delta);
            }
        }
        self.frame_prepared = false;
    }
    ///Silent pointer selection also retargets focus when selecting the same row.
    pub fn select(&mut self, index: usize) {
        self.prepare_frame();
        if index >= self.rows.len() {
            return;
        }
        rate(&mut self.focus[self.selected], 0.0, 0.2);
        self.selected = index;
        rate(&mut self.focus[index], 1.0, 0.25);
    }
    ///88d10 clamps each grid coordinate first, then clamps a missing last-row
    /// column to the final actual item. Locked cells remain selectable.
    pub fn controller(&mut self, controls: MovieControls) {
        self.prepare_frame();
        if self.closing {
            return;
        }
        if controls.cancel {
            self.sounds.push("cancel");
            self.cancel();
            return;
        }
        if controls.confirm {
            self.play();
            return;
        }
        let mut column = self.selected % 4;
        let mut row = self.selected / 4;
        if controls.left {
            column = column.saturating_sub(1);
        } else if controls.right {
            column = (column + 1).min(3);
        }
        if controls.down {
            row = (row + 1).min(self.rows.len().div_ceil(4) - 1);
        } else if controls.up {
            row = row.saturating_sub(1);
        }
        let next = (row * 4 + column).min(self.rows.len() - 1);
        if next != self.selected {
            self.sounds.push("move");
            self.select(next);
        }
    }
    pub fn play(&mut self) {
        self.prepare_frame();
        flash(&mut self.flashes[self.selected]);
        if self.rows[self.selected].unlocked {
            self.sounds.push("decide");
            self.tasks.push(Task::Play);
        } else {
            self.sounds.push("error");
        }
    }
    pub fn cancel(&mut self) {
        self.prepare_frame();
        self.closing = true;
        flash(&mut self.close_flash);
        self.tasks.push(Task::Close);
    }
    pub fn take_requests(&mut self) -> Vec<MovieRequest> {
        std::mem::take(&mut self.requests)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn pointer_hit(&self, point: glam::Vec2) -> Option<usize> {
        if !(136.0..1784.0).contains(&point.x) || !(190.0..916.0).contains(&point.y) {
            return None;
        }
        self.rows.iter().position(|row| {
            let [left, top, right, bottom] = row.thumbnail_bounds();
            (left..right).contains(&point.x) && (top..bottom).contains(&point.y)
        })
    }
    ///893a0 captures close first, rejects gaps in the body band and captures
    ///background elsewhere. Release acts on the original capture, not Up coordinates.
    pub fn pointer_probe(&mut self, point: glam::Vec2) -> bool {
        self.pointer_row = -1;
        if (1764.0..1848.0).contains(&point.x) && (32.0..116.0).contains(&point.y) {
            self.pointer_target = 1;
            return true;
        }
        if (150.0..992.0).contains(&point.y) {
            self.pointer_target = -1;
            if let Some(row) = self.pointer_hit(point) {
                self.pointer_row = row as i32;
                return true;
            }
            return false;
        }
        self.pointer_target = 0;
        true
    }
    pub fn pointer_reset(&mut self) {
        self.pointer_target = -1;
        self.pointer_row = -1;
    }
    pub fn pointer_press(&mut self) {
        self.prepare_frame();
        if self.pointer_target == 1 {
            rate(&mut self.close_flash, 0.7, 0.2);
        } else if self.pointer_target < 0 && self.pointer_row >= 0 {
            let row = self.pointer_row as usize;
            rate(&mut self.flashes[row], 0.7, 0.2);
            self.select(row);
        }
    }
    pub fn pointer_release(&mut self) {
        self.prepare_frame();
        if self.pointer_target == 1 {
            self.sounds.push("decide"); //896a8 uses decide; controller uses cancel.
            self.cancel();
        } else if self.pointer_target < 0 && self.pointer_row >= 0 {
            self.select(self.pointer_row as usize);
            self.play();
        }
    }
    ///897a0/898e0 cancel the matching flash. Body capture keeps its row;
    ///close/background capture becomes inert0. Dragging has no scroll effect.
    pub fn pointer_cancel(&mut self) {
        self.prepare_frame();
        if self.pointer_target >= 0 {
            rate(&mut self.close_flash, 0.0, 0.2);
            self.pointer_target = 0;
        } else if self.pointer_row >= 0 {
            rate(&mut self.flashes[self.pointer_row as usize], 0.0, 0.2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::format::scenario::{
        info::{BgmIdOpt, MovieInfoItem, MovieTransparencyMode, MovieVolumeSource, PictureIdOpt},
        instruction_elements::FromNumber,
    };
    fn info(count: usize) -> MovieInfo {
        (0..count)
            .map(|id| MovieInfoItem {
                name: shin_core::format::text::U16String::new(format!("movie{id}")),
                linked_picture_id: PictureIdOpt::from_number(0),
                volume_source: MovieVolumeSource::Bgm,
                transparency: MovieTransparencyMode::Opaque,
                linked_bgm_id: BgmIdOpt::from_number(-1),
            })
            .collect()
    }
    fn list(count: usize) -> MovieGalleryList {
        let mut save = SaveData::default();
        for id in 0..count {
            save.save_vectors.unlocked_movies.add(id);
        }
        MovieGalleryList::new(&info(count), &save).unwrap()
    }
    #[test]
    fn authored_ten_movies_and_completion_exclude_other_cutscenes() {
        let mut save = SaveData::default();
        save.save_vectors.unlocked_movies.add(10);
        let locked = MovieGalleryList::new(&info(50), &save).unwrap();
        assert_eq!(locked.rows().len(), 10);
        assert_eq!(locked.completion_label(), "達成率:0.0％");
        save.save_vectors.unlocked_movies.add(9);
        let partial = MovieGalleryList::new(&info(50), &save).unwrap();
        assert_eq!(partial.completion_label(), "達成率:10.0％");
        assert_eq!(partial.rows()[9].plate, [412.0, 484.0, 412.0, 242.0]);
        assert_eq!(partial.feedback(0), (1.0, 0.0));
        assert_eq!(partial.feedback(9), (0.0, 0.0));
        assert_eq!(list(3).completion_label(), "達成率:100％");
        assert!(MovieGalleryList::new(&info(0), &save).is_err());
    }
    #[test]
    fn grid_edges_tail_and_opposed_direction_priorities_match_native() {
        let mut g = list(10);
        g.controller(MovieControls {
            left: true,
            right: true,
            up: true,
            down: true,
            ..Default::default()
        });
        assert_eq!(g.selected(), 4); // Left wins horizontally; Down wins vertically.
        g.select(3);
        g.controller(MovieControls {
            down: true,
            ..Default::default()
        });
        assert_eq!(g.selected(), 7);
        g.controller(MovieControls {
            down: true,
            ..Default::default()
        });
        assert_eq!(g.selected(), 9);
        g.controller(MovieControls {
            right: true,
            down: true,
            ..Default::default()
        });
        assert_eq!(g.selected(), 9);
        assert_eq!(g.take_sounds(), ["move", "move", "move"]);
        g.controller(MovieControls {
            up: true,
            ..Default::default()
        });
        assert_eq!(g.selected(), 5); // Tail clamp changes the column to1.
    }
    #[test]
    fn focus_reversal_uses_evaluated_values_and_unequal_rates() {
        let mut g = list(10);
        g.select(1);
        g.update(Ticks::from_f32(2.0));
        assert!((g.feedback(0).0 - 0.6).abs() < 0.00001);
        assert_eq!(g.feedback(1).0, 0.5);
        g.select(0);
        g.update(Ticks::from_f32(1.0));
        assert!((g.feedback(0).0 - 0.85).abs() < 0.00001);
        assert!((g.feedback(1).0 - 0.3).abs() < 0.00001);
        let before = (g.feedback(0), g.feedback(1));
        g.update(Ticks::from_f32(-100.0));
        g.update(Ticks::ZERO);
        assert_eq!((g.feedback(0), g.feedback(1)), before);
    }
    #[test]
    fn locked_play_flashes_and_errors_without_opening_or_disabling_navigation() {
        let mut g = MovieGalleryList::new(&info(10), &SaveData::default()).unwrap();
        g.controller(MovieControls {
            confirm: true,
            right: true,
            ..Default::default()
        });
        assert_eq!(g.selected(), 0);
        assert_eq!(g.take_sounds(), ["error"]);
        g.update(Ticks::from_f32(3.5));
        assert!((g.feedback(0).1 - 0.7).abs() < 0.00001);
        g.controller(MovieControls {
            right: true,
            ..Default::default()
        });
        assert_eq!(g.selected(), 1);
        g.update(Ticks::from_f32(10.0));
        assert!(g.take_requests().is_empty());
    }
    #[test]
    fn playback_waits_current_row_flash_and_dispatches_on_following_poll() {
        let mut g = list(10);
        g.play();
        g.update(Ticks::from_f32(7.0));
        assert!(g.take_requests().is_empty());
        g.update(Ticks::ZERO);
        assert_eq!(
            g.take_requests(),
            [MovieRequest::OpenMovie(g.rows()[0].movie)]
        );
        g.play();
        g.play();
        g.select(1); //8b238 reads live selection; row1's flash is already idle.
        g.update(Ticks::ZERO);
        assert!(g.take_requests().is_empty()); // New tasks wait until the next poll.
        g.update(Ticks::ZERO);
        assert_eq!(
            g.take_requests(),
            [MovieRequest::OpenMovie(g.rows()[1].movie); 2]
        );
        // Eligibility is checked when confirming, not again when the task runs.
        let mut save = SaveData::default();
        save.save_vectors.unlocked_movies.add(0);
        let mut g = MovieGalleryList::new(&info(10), &save).unwrap();
        g.play();
        g.update(Ticks::from_f32(2.0));
        g.select(1); // This frame's poll still sees row0's busy flash.
        g.update(Ticks::ZERO);
        assert!(g.take_requests().is_empty());
        g.update(Ticks::ZERO);
        assert_eq!(
            g.take_requests(),
            [MovieRequest::OpenMovie(g.rows()[1].movie)]
        );
    }
    #[test]
    fn close_priority_blocks_controller_and_waits_flash_before_title_eight() {
        let mut g = list(10);
        g.controller(MovieControls {
            cancel: true,
            confirm: true,
            right: true,
            ..Default::default()
        });
        assert!(g.closing());
        assert_eq!(g.take_sounds(), ["cancel"]);
        g.update(Ticks::from_f32(3.5));
        assert!((g.close_flash() - 0.7).abs() < 0.00001);
        g.controller(MovieControls {
            right: true,
            ..Default::default()
        });
        assert_eq!(g.selected(), 0);
        g.update(Ticks::from_f32(3.5));
        assert!(g.take_requests().is_empty());
        g.update(Ticks::ZERO);
        assert_eq!(
            g.take_requests(),
            [MovieRequest::ReturnTitle {
                argument: 8,
                music_stop_ticks: 30
            }]
        );
        g.update(Ticks::from_f32(100.0));
        assert!(g.take_requests().is_empty());
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires supplied ROM and executable (SHIN_REFERENCE_ROM)"]
fn reference_native_movie_gallery_authored_ids_grid_and_deferred_dispatch() {
    use shin_core::{
        format::{rom::RomReader, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0xbba74, 0x1e202008),
        (0xbba78, 0x54000bed),
        (0x8be3c, 0x321a0be0),
        (0x8be48, 0x97ffef26),
        (0x87bfc, 0xf9472108),
        (0x87c00, 0xb8736917),
        (0x87c14, 0x940189cb),
        (0x87c1c, 0xb4001220),
        (0x87c44, 0x710002ff),
        (0x87c4c, 0x13027d09),
        (0x87c54, 0x4b0802e8),
        (0x87e04, 0x9401271b),
        (0x87e08, 0x360000a0),
        (0x87e64, 0xf100a27f),
        (0x87e78, 0x321b03e0),
        (0x87e88, 0x1ad90d08),
        (0x88d4c, 0x37e80288),
        (0x88d50, 0x37e00368),
        (0x88d70, 0x37d002ca),
        (0x88d74, 0x36d802ea),
        (0x88d8c, 0x36c8026a),
        (0x88ddc, 0x0aac7d8c),
        (0x88e24, 0x0b0a0929),
        (0x88e48, 0x1a8bb135),
        (0x88e98, 0xbd497501),
        (0x88ec4, 0x1e2a1001),
        (0x890c4, 0x39403288),
        (0x890c8, 0x34000148),
        (0x890ec, 0x17fe7871),
        (0x89100, 0x97fe786c),
        (0x8b238, 0xb980d275),
        (0x8b244, 0x9b085aa8),
        (0x8b24c, 0x9400c1e9),
        (0x8b250, 0x36000060),
        (0x8b26c, 0xb9400902),
        (0x8b278, 0x97fffafa),
        (0x8b098, 0x9400c256),
        (0x8b0c0, 0x321f0fe1),
        (0x8b0cc, 0x321d03e0),
        (0x8b0d4, 0x94020bb3),
        (0x8b0dc, 0x97fe70a5),
    ] {
        assert_eq!(word(a), expected, "native MovieMode{a:x}");
    }
    for (a, value) in [
        (0x159910, 412.0f32),
        (0x159a28, 242.0),
        (0x159974, 0.2),
        (0x1595a4, 0.7),
    ] {
        assert_eq!(word(a), value.to_bits(), "native MovieMode constant{a:x}");
    }
    for (index, id) in MOVIE_GALLERY_IDS.into_iter().enumerate() {
        assert_eq!(word(0x170ae4 + index * 4), id as u32);
    }
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap()).unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Scenario::new(bytes.into()).unwrap();
    let info = &scenario.info_tables().movie_info;
    let mut save = SaveData::default();
    let locked = MovieGalleryList::new(info, &save).unwrap();
    assert_eq!(locked.rows().len(), 10);
    assert_eq!(locked.completion_tenths(), 0);
    for (index, row) in locked.rows().iter().enumerate() {
        assert_eq!(row.movie.into_i32(), index as i32);
        assert!(!row.unlocked);
        assert_eq!(
            row.plate,
            [
                (index % 4) as f32 * 412.0,
                (index / 4) as f32 * 242.0,
                412.0,
                242.0
            ]
        );
        assert!(rom.find_file(&info[index].path()).is_ok());
        save.save_vectors.unlocked_movies.add(index);
    }
    let mut ready = MovieGalleryList::new(info, &save).unwrap();
    assert_eq!(ready.completion_label(), "達成率:100％");
    ready.select(9);
    ready.play();
    ready.update(Ticks::from_f32(7.0));
    assert!(ready.take_requests().is_empty());
    ready.update(Ticks::ZERO);
    assert_eq!(
        ready.take_requests(),
        [MovieRequest::OpenMovie(ready.rows()[9].movie)]
    );
    println!(
        "Original MovieMode: ten authored IDs out of{} movie entries, original MP4 paths, save unlocks, completion and deferred dispatch pass",
        info.len()
    );
}
