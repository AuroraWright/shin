//! MusicMode list2fd50, selection31a60, play31970, opening327f0 and close32fd0.
//! The model emits scene/audio requests consumed by MusicGalleryScene.
use shin_core::{
    format::{
        save::SaveData,
        scenario::info::{BgmId, MusicBoxInfoItem},
    },
    time::{Ticks, Tween, Tweener},
};

#[derive(Debug, Clone, Copy)]
pub(super) struct MusicRow {
    pub bgm: BgmId,
    pub name_index: u16,
    pub once_flag: u16,
    pub unlocked: bool,
}
#[derive(Debug, Clone, Copy)]
pub(super) enum MusicRequest {
    Play { bgm: BgmId, once_flag: u16 },
    Stop { ticks: u32 },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MusicPhase {
    Loading,
    Opening,
    Active,
    Closing,
    Closed,
}
/// Effects the scene owner must apply when native tasks dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MusicSceneRequest {
    BeginOpening { ticks: u32 },
    ReturnToTitle { argument: u8, music_stop_ticks: u32 },
}
///327f0 owns the captured scene until its fade completes. Input is installed
///31480 at the next task poll, and becomes eligible on the following frame.
pub(super) struct MusicGallery {
    list: MusicGalleryList,
    phase: MusicPhase,
    resources_ready: bool,
    capture_elapsed: f32,
    capture_present: bool,
    capture_installing: bool,
    close_flash: Tweener,
    frame_prepared: bool,
    input_enabled: bool,
    pointer_target: i32,
    pointer_row: i32,
    velocity: f32,
    requests: Vec<MusicSceneRequest>,
}
impl MusicGallery {
    pub fn new(items: &[MusicBoxInfoItem], save: &SaveData) -> anyhow::Result<Self> {
        Ok(Self {
            list: MusicGalleryList::new(items, save)?,
            phase: MusicPhase::Loading,
            resources_ready: false,
            capture_elapsed: 0.0,
            capture_present: true,
            capture_installing: false,
            close_flash: Tweener::new(0.0),
            frame_prepared: false,
            input_enabled: false,
            pointer_target: -1,
            pointer_row: -1,
            velocity: 0.0,
            requests: Vec::new(),
        })
    }
    pub fn list(&self) -> &MusicGalleryList {
        &self.list
    }
    pub fn phase(&self) -> MusicPhase {
        self.phase
    }
    pub fn publish_resources(&mut self) {
        self.resources_ready = true;
    }
    pub fn captured_opacity(&self) -> f32 {
        match self.phase {
            MusicPhase::Loading => 1.0,
            _ if self.capture_present => 1.0 - self.capture_elapsed / 60.0,
            _ => 0.0,
        }
    }
    pub fn close_flash(&self) -> f32 {
        self.close_flash.value()
    }
    pub fn capture_retired(&self) -> bool {
        !self.capture_present
    }
    pub fn prepare_frame(&mut self) {
        if self.frame_prepared {
            return;
        }
        self.frame_prepared = true;
        if self.capture_installing {
            //1000b0 retires the completed opening task before the next snapshot.
            self.capture_installing = false;
            self.capture_present = false;
        }
        self.input_enabled = self.phase == MusicPhase::Active;
        match self.phase {
            MusicPhase::Loading if self.resources_ready => {
                self.phase = MusicPhase::Opening;
                self.requests
                    .push(MusicSceneRequest::BeginOpening { ticks: 60 });
            }
            MusicPhase::Opening if self.capture_elapsed >= 60.0 => {
                self.phase = MusicPhase::Active;
                self.capture_installing = true;
            }
            MusicPhase::Closing if self.close_flash.is_idle() => {
                //32fd0 replaces this scene immediately; there is no exit fade.
                self.phase = MusicPhase::Closed;
                self.requests.push(MusicSceneRequest::ReturnToTitle {
                    argument: 7,
                    music_stop_ticks: 30,
                });
            }
            _ => {}
        }
    }
    pub fn controller_ready(&mut self) -> bool {
        self.prepare_frame();
        self.input_enabled && self.list.controller_ready()
    }
    pub fn accepts_input(&self) -> bool {
        self.input_enabled
    }
    pub fn velocity(&self) -> f32 {
        self.velocity
    }
    pub fn navigate(&mut self, column: i32, row: i32) {
        if self.controller_ready() {
            if self.list.navigate(column, row) {
                self.velocity = 0.0;
            }
        }
    }
    pub fn play(&mut self) {
        if self.controller_ready() {
            self.list.play();
            if self.list.rows[self.list.selected].unlocked {
                self.velocity = 0.0;
            }
        }
    }
    pub fn stop(&mut self) {
        if self.controller_ready() {
            self.list.stop();
        }
    }
    pub fn cancel(&mut self) {
        if self.controller_ready() {
            self.list.sounds.push("cancel");
            self.begin_close();
        }
    }
    pub fn press_row(&mut self, index: usize) {
        self.prepare_frame();
        if self.input_enabled && index < self.list.rows.len() {
            rate(&mut self.list.flashes[index], 0.7, 0.2);
            self.list.select(index, true);
            self.velocity = 0.0;
        }
    }
    pub fn press_close(&mut self) {
        self.prepare_frame();
        if self.input_enabled {
            rate(&mut self.close_flash, 0.7, 0.2);
        }
    }
    ///32090 pointer release toggles the clicked row, even during reveal scroll.
    pub fn release_row(&mut self, index: usize) {
        self.prepare_frame();
        if !self.input_enabled || index >= self.list.rows.len() {
            return;
        }
        self.list.select(index, true);
        self.velocity = 0.0;
        if self.list.playing == Some(index) {
            self.list.stop();
        } else {
            self.list.play();
        }
    }
    pub fn release_close(&mut self) {
        self.prepare_frame();
        if self.input_enabled {
            self.list.sounds.push("cancel");
            self.begin_close();
        }
    }
    fn begin_close(&mut self) {
        self.phase = MusicPhase::Closing;
        self.input_enabled = false;
        self.velocity = 0.0;
        rate(&mut self.close_flash, 0.7, 0.2);
        self.close_flash
            .enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
    }
    pub fn update(&mut self, delta: Ticks, music_is_playing: bool) {
        self.prepare_frame();
        if self.phase == MusicPhase::Opening || self.capture_installing {
            //62f40 advances after327f0 checks completion, including the frame
            // on which the async resource task first publishes its result.
            self.capture_elapsed = (self.capture_elapsed + delta.as_f32()).min(60.0);
        }
        //306e0 integrates fling before scroll/feedback clocks and stops it
        // on a clamped/no-movement frame, including a zero-delta poll.
        if self.velocity.abs() >= f32::EPSILON {
            let next = self.clamp_scroll(self.list.scroll() - self.velocity * delta.as_f32());
            if (next - self.list.scroll()).abs() < f32::EPSILON {
                self.velocity = 0.0;
            } else {
                self.scroll_to(next);
                self.velocity *= 0.95_f32.powf(delta.as_f32());
                if self.velocity.abs() < 0.1 {
                    self.velocity = 0.0;
                }
            }
        }
        self.list.update(delta);
        //30804 consults33e10: finishing a one-shot clears its indicator
        // without requesting a stop.
        if !music_is_playing {
            self.list.playing = None;
        }
        self.close_flash.update(delta);
        self.frame_prepared = false;
    }
    pub fn take_requests(&mut self) -> Vec<MusicSceneRequest> {
        std::mem::take(&mut self.requests)
    }
    pub fn take_music_requests(&mut self) -> Vec<MusicRequest> {
        self.list.take_requests()
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        self.list.take_sounds()
    }
    fn clamp_scroll(&self, value: f32) -> f32 {
        value.min(self.list.content_height() - 730.0).max(0.0)
    }
    ///308b0 preserves column and moves focus only when the whole88px row
    /// leaves the730px viewport. An invalid odd-tail column is left unchanged.
    fn scroll_to(&mut self, scroll: f32) {
        let mut target = self.list.selected as i32;
        while ((target / 2 * 92) as f32) < scroll {
            target += 2;
        }
        while (target / 2 * 92 + 88) as f32 > scroll + 730.0 {
            target -= 2;
        }
        if target != self.list.selected as i32
            && target >= 0
            && target < self.list.rows.len() as i32
        {
            self.list.select(target as usize, false);
        }
        self.list.scroll.enqueue_now(scroll, Tween::IMMEDIATE);
    }
    pub fn analog_scroll(&mut self, axis: f32) {
        if self.controller_ready() && axis.is_finite() && axis != 0.0 {
            self.scroll_to(self.clamp_scroll(self.list.scroll() + axis * 32.0));
            self.velocity = 0.0;
        }
    }
    ///31bc0 returns -1 outside the viewport,0 in a gap or beyond the table,
    /// and rowIndex+1 on a plate. Native integer truncation includes remainder88.
    pub fn pointer_hit(&self, point: glam::Vec2) -> i32 {
        if !(136.0..1784.0).contains(&point.x) || !(188.0..918.0).contains(&point.y) {
            return -1;
        }
        let column = if (150.0..950.0).contains(&point.x) {
            0
        } else if (970.0..1770.0).contains(&point.x) {
            1
        } else {
            return 0;
        };
        let y = (self.list.scroll() - 188.0 + point.y) as i32;
        if y % 92 > 88 {
            return 0;
        }
        let index = y / 92 * 2 + column;
        if index >= self.list.rows.len() as i32 {
            0
        } else {
            index + 1
        }
    }
    pub fn pointer_probe(&mut self, point: glam::Vec2) -> bool {
        self.prepare_frame();
        if !self.accepts_input() {
            return false;
        }
        self.pointer_row = -1;
        self.pointer_target =
            if (1764.0..1848.0).contains(&point.x) && (32.0..116.0).contains(&point.y) {
                1
            } else if (150.0..992.0).contains(&point.y) {
                -1
            } else {
                0
            };
        if self.pointer_target < 0 {
            self.pointer_row = self.pointer_hit(point);
            if self.pointer_row < 0 {
                return false;
            }
            if self.velocity != 0.0 {
                self.pointer_row = 0;
                self.velocity = 0.0;
            }
        }
        true
    }
    pub fn pointer_reset(&mut self) {
        self.pointer_target = -1;
        self.pointer_row = -1;
    }
    pub fn pointer_press(&mut self) {
        if self.pointer_target == 1 {
            self.press_close();
        } else if self.pointer_target < 0 && self.pointer_row > 0 {
            self.press_row(self.pointer_row as usize - 1);
        }
    }
    pub fn pointer_release(&mut self) {
        if self.pointer_target == 1 {
            self.release_close();
        } else if self.pointer_target < 0 && self.pointer_row > 0 {
            self.release_row(self.pointer_row as usize - 1);
        }
    }
    pub fn pointer_cancel(&mut self) {
        if self.pointer_target >= 0 {
            rate(&mut self.close_flash, 0.0, 0.2);
            self.pointer_target = 0;
        } else if self.pointer_row > 0 {
            rate(
                &mut self.list.flashes[self.pointer_row as usize - 1],
                0.0,
                0.2,
            );
        }
    }
    pub fn pointer_drag(&mut self, dy: f32) {
        if self.pointer_target >= 0 {
            self.pointer_cancel();
        } else if self.pointer_row >= 0 && dy.is_finite() {
            if self.pointer_row > 0 {
                rate(
                    &mut self.list.flashes[self.pointer_row as usize - 1],
                    0.0,
                    0.2,
                );
                self.pointer_row = 0;
            }
            self.scroll_to(self.clamp_scroll(self.list.scroll() - dy));
        }
    }
    pub fn pointer_gesture(&mut self, velocity: glam::Vec2) {
        if self.pointer_target < 0 && self.pointer_row >= 0 && velocity.y.is_finite() {
            self.velocity = velocity.y;
        }
    }
}
pub(super) struct MusicGalleryList {
    rows: Vec<MusicRow>,
    focus: Vec<Tweener>,
    flashes: Vec<Tweener>,
    selected: usize,
    playing: Option<usize>,
    scroll: Tweener,
    requests: Vec<MusicRequest>,
    sounds: Vec<&'static str>,
}
fn rate(clock: &mut Tweener, value: f32, speed: f32) {
    clock.enqueue_now(
        value,
        Tween::linear(Ticks::from_f32((value - clock.value()).abs() / speed)),
    );
}
impl MusicGalleryList {
    pub fn new(items: &[MusicBoxInfoItem], save: &SaveData) -> anyhow::Result<Self> {
        anyhow::ensure!(!items.is_empty(), "Music gallery has no authored entries");
        let rows: Vec<_> = items
            .iter()
            .map(|item| {
                let unlocked = save
                    .save_vectors
                    .unlocked_bgms
                    .contains(item.bgm_id.into_i32() as usize);
                MusicRow {
                    bgm: item.bgm_id,
                    name_index: if unlocked { item.name_index } else { 0 },
                    once_flag: item.once_flag,
                    unlocked,
                }
            })
            .collect();
        let count = rows.len();
        Ok(Self {
            rows,
            focus: (0..count)
                .map(|i| Tweener::new(if i == 0 { 1.0 } else { 0.0 }))
                .collect(),
            flashes: vec![Tweener::new(0.0); count],
            selected: 0,
            playing: None,
            scroll: Tweener::new(0.0),
            requests: Vec::new(),
            sounds: Vec::new(),
        })
    }
    pub fn rows(&self) -> &[MusicRow] {
        &self.rows
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn playing(&self) -> Option<usize> {
        self.playing
    }
    pub fn content_height(&self) -> f32 {
        (self.rows.len().div_ceil(2) * 92 - 4) as f32
    }
    pub fn completion_tenths(&self) -> usize {
        self.rows.iter().filter(|row| row.unlocked).count() * 1000 / self.rows.len()
    }
    pub fn completion_label(&self) -> String {
        let percent = self.completion_tenths();
        if percent >= 1000 {
            "達成率:100％".to_owned()
        } else {
            format!("達成率:{}.{}％", percent / 10, percent % 10)
        }
    }
    pub fn all_unlocked(&self) -> bool {
        self.rows.iter().all(|row| row.unlocked)
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn controller_ready(&self) -> bool {
        self.scroll.is_idle()
    }
    pub fn feedback(&self, index: usize) -> (f32, f32) {
        (self.focus[index].value(), self.flashes[index].value())
    }
    ///31a60 also handles silent pointer selection; controller supplies its cue.
    pub fn select(&mut self, index: usize, reveal: bool) {
        if index >= self.rows.len() {
            return;
        }
        rate(&mut self.focus[self.selected], 0.0, 0.2);
        self.selected = index;
        rate(&mut self.focus[index], 1.0, 0.25);
        if reveal {
            let top = (index / 2 * 92) as f32;
            let mut target = self.scroll.value();
            if top < target {
                target = top;
            } else if top + 88.0 - target > 730.0 {
                target = (top + 88.0).min(self.content_height()) - 730.0;
            }
            self.scroll
                .enqueue_now(target, Tween::linear(Ticks::from_f32(4.0)));
        }
    }
    ///315b0 clamps two-column navigation, retaining locked entries. A page
    /// changes eight rows. Scrolling blocks all controller actions until idle.
    pub fn navigate(&mut self, column: i32, row: i32) -> bool {
        if !self.controller_ready() {
            return false;
        }
        let target_column = (self.selected as i32 % 2 + column).clamp(0, 1);
        let target_row =
            (self.selected as i32 / 2 + row).clamp(0, self.rows.len().div_ceil(2) as i32 - 1);
        let target = (target_row as usize * 2 + target_column as usize).min(self.rows.len() - 1);
        if target == self.selected {
            return false;
        }
        self.sounds.push("move");
        self.select(target, true);
        true
    }
    pub fn play(&mut self) {
        //315ec gates controller input, while pointer release can call31970
        // after selection has already started a scroll. Gate in the input owner.
        let flash = &mut self.flashes[self.selected];
        rate(flash, 0.7, 0.2);
        flash.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
        let row = self.rows[self.selected];
        if row.unlocked {
            self.requests.push(MusicRequest::Play {
                bgm: row.bgm,
                once_flag: row.once_flag,
            });
            self.playing = Some(self.selected);
        } else {
            self.sounds.push("error");
        }
    }
    pub fn stop(&mut self) {
        if self.playing.is_none() {
            return;
        }
        self.requests.push(MusicRequest::Stop { ticks: 30 });
        self.playing = None;
    }
    pub fn update(&mut self, delta: Ticks) {
        self.scroll.update(delta);
        for clock in self.focus.iter_mut().chain(self.flashes.iter_mut()) {
            clock.update(delta);
        }
    }
    pub fn take_requests(&mut self) -> Vec<MusicRequest> {
        std::mem::take(&mut self.requests)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::format::scenario::instruction_elements::FromNumber;
    fn items(n: usize) -> Vec<MusicBoxInfoItem> {
        (0..n)
            .map(|i| MusicBoxInfoItem {
                bgm_id: BgmId::from_number((i + 100) as i32),
                name_index: i as u16 + 1,
                once_flag: if i == 1 { 1 } else { 0 },
            })
            .collect()
    }
    fn open_gallery() -> MusicGallery {
        let mut save = SaveData::default();
        save.save_vectors.unlocked_bgms.add(100);
        save.save_vectors.unlocked_bgms.add(101);
        let mut gallery = MusicGallery::new(&items(25), &save).unwrap();
        gallery.publish_resources();
        gallery.update(Ticks::from_f32(60.0), false);
        gallery.update(Ticks::ZERO, false);
        assert_eq!(gallery.phase(), MusicPhase::Active);
        gallery.take_requests();
        gallery
    }
    #[test]
    fn pointer_hit_uses_plate_insets_integer_body_edge_and_odd_tail() {
        use glam::vec2;
        let mut g = open_gallery();
        for (x, expected) in [
            (135.9, -1),
            (136.0, 0),
            (149.9, 0),
            (150.0, 1),
            (949.9, 1),
            (950.0, 0),
            (969.9, 0),
            (970.0, 2),
            (1769.9, 2),
            (1770.0, 0),
            (1784.0, -1),
        ] {
            assert_eq!(g.pointer_hit(vec2(x, 200.0)), expected, "native X{x}");
        }
        assert_eq!(g.pointer_hit(vec2(200.0, 187.9)), -1);
        assert_eq!(g.pointer_hit(vec2(200.0, 276.9)), 1);
        assert_eq!(g.pointer_hit(vec2(200.0, 277.0)), 0);
        assert_eq!(g.pointer_hit(vec2(200.0, 280.0)), 3);
        assert_eq!(g.pointer_hit(vec2(200.0, 918.0)), -1);
        assert!(!g.pointer_probe(vec2(200.0, 160.0)));
        assert!(g.pointer_probe(vec2(10.0, 10.0))); // background consumes input
        g.pointer_release();
        assert_eq!(g.phase(), MusicPhase::Active);
        g.analog_scroll(100.0);
        assert_eq!(g.list().scroll(), 462.0);
        assert_eq!(g.pointer_hit(vec2(200.0, 840.0)), 25);
        assert_eq!(g.pointer_hit(vec2(1000.0, 840.0)), 0);
    }
    #[test]
    fn drag_cancels_row_flash_and_fling_touch_suppresses_playback() {
        use glam::vec2;
        let mut g = open_gallery();
        assert!(g.pointer_probe(vec2(200.0, 300.0)));
        g.pointer_press();
        g.update(Ticks::from_f32(1.0), false);
        assert!(g.list().feedback(2).1 > 0.0);
        g.pointer_drag(-40.0);
        assert_eq!(g.list().scroll(), 40.0);
        g.pointer_release();
        assert!(g.take_music_requests().is_empty());
        g.pointer_gesture(vec2(0.0, -10.0));
        g.pointer_reset();
        assert_eq!(g.velocity(), -10.0);
        g.update(Ticks::from_f32(2.0), false);
        assert_eq!(g.list().scroll(), 60.0);
        assert!((g.velocity() + 10.0 * 0.95_f32.powf(2.0)).abs() < 0.00001);
        assert!(g.pointer_probe(vec2(1000.0, 300.0)));
        assert_eq!(g.velocity(), 0.0);
        g.pointer_release();
        assert!(g.take_music_requests().is_empty());
        g.update(Ticks::ZERO, false);
        assert!(g.pointer_probe(vec2(1000.0, 300.0)));
        g.pointer_release();
        assert_eq!(g.take_sounds(), ["error"]); // next separate touch can play
    }
    #[test]
    fn free_scroll_keeps_column_and_full_row_visible_without_navigation_cue() {
        let mut g = open_gallery();
        g.navigate(1, 0);
        g.update(Ticks::from_f32(4.0), false);
        g.take_sounds();
        g.analog_scroll(1.0);
        assert_eq!(g.list().scroll(), 32.0);
        assert_eq!(g.list().selected(), 3);
        g.analog_scroll(3.0);
        assert_eq!(g.list().scroll(), 128.0);
        assert_eq!(g.list().selected(), 5);
        assert!(g.list().controller_ready());
        assert!(g.take_sounds().is_empty());
        g.analog_scroll(-100.0);
        assert_eq!(g.list().scroll(), 0.0);
        // Existing selected row remains fully visible, so focus does not jump.
        assert_eq!(g.list().selected(), 5);
    }
    #[test]
    fn clamped_or_zero_delta_inertia_stops_and_close_drag_cannot_dismiss() {
        use glam::vec2;
        let mut g = open_gallery();
        assert!(g.pointer_probe(vec2(960.0, 300.0))); // gap can drag
        g.pointer_gesture(vec2(0.0, 10.0));
        g.update(Ticks::from_f32(1.0), false);
        assert_eq!(g.velocity(), 0.0);
        g.analog_scroll(4.0);
        assert!(g.pointer_probe(vec2(960.0, 300.0)));
        g.pointer_gesture(vec2(0.0, -10.0));
        g.update(Ticks::ZERO, false);
        assert_eq!(g.velocity(), 0.0);
        assert!(g.pointer_probe(vec2(1800.0, 80.0)));
        g.pointer_press();
        g.update(Ticks::from_f32(1.0), false);
        assert!(g.close_flash() > 0.0);
        g.pointer_drag(100.0);
        g.pointer_release();
        g.update(Ticks::from_f32(1.0), false);
        assert_eq!(g.close_flash(), 0.0);
        assert_eq!(g.phase(), MusicPhase::Active);
    }
    #[test]
    fn signed_capture_clock_survives_input_installation_until_next_poll_retirement() {
        let mut gallery = MusicGallery::new(&items(3), &SaveData::default()).unwrap();
        gallery.update(Ticks::from_f32(-90.0), false);
        assert_eq!(gallery.captured_opacity(), 1.0);
        gallery.publish_resources();
        gallery.update(Ticks::from_f32(-3.0), false);
        assert!((gallery.captured_opacity() - 1.05).abs() < 0.00001);
        assert!(!gallery.capture_retired());
        gallery.update(Ticks::from_f32(63.0), false);
        assert_eq!(gallery.captured_opacity(), 0.0);
        //327f0 installs input before62f40 rewinds this still-owned capture.
        gallery.update(Ticks::from_f32(-2.0), false);
        assert_eq!(gallery.phase(), MusicPhase::Active);
        assert!(!gallery.accepts_input());
        assert!((gallery.captured_opacity() - 1.0 / 30.0).abs() < 0.00001);
        assert!(!gallery.capture_retired());
        gallery.prepare_frame();
        assert!(gallery.accepts_input());
        assert!(gallery.capture_retired());
        assert_eq!(gallery.captured_opacity(), 0.0);
        gallery.update(Ticks::from_f32(-100.0), false);
        assert_eq!(gallery.captured_opacity(), 0.0);
    }
    #[test]
    fn resource_publication_starts_capture_fade_and_defers_new_input_task() {
        let mut gallery = MusicGallery::new(&items(3), &SaveData::default()).unwrap();
        gallery.update(Ticks::from_f32(100.0), false);
        assert_eq!(gallery.phase(), MusicPhase::Loading);
        assert_eq!(gallery.captured_opacity(), 1.0);
        gallery.cancel();
        assert!(gallery.take_sounds().is_empty());
        gallery.publish_resources();
        // This frame was already polled before publication.
        gallery.update(Ticks::ZERO, false);
        assert!(gallery.take_requests().is_empty());
        gallery.update(Ticks::from_f32(30.0), false);
        assert_eq!(gallery.phase(), MusicPhase::Opening);
        assert_eq!(gallery.captured_opacity(), 0.5);
        assert_eq!(
            gallery.take_requests(),
            [MusicSceneRequest::BeginOpening { ticks: 60 }]
        );
        gallery.cancel();
        gallery.update(Ticks::from_f32(30.0), false);
        assert_eq!(gallery.phase(), MusicPhase::Opening);
        assert_eq!(gallery.captured_opacity(), 0.0);
        gallery.navigate(1, 0); // Opening task installs input, but cannot run it yet.
        assert_eq!(gallery.phase(), MusicPhase::Active);
        assert_eq!(gallery.list().selected(), 0);
        gallery.update(Ticks::ZERO, false);
        gallery.navigate(1, 0);
        assert_eq!(gallery.list().selected(), 1);
    }
    #[test]
    fn controller_scroll_gate_and_pointer_toggle_follow_distinct_native_paths() {
        let mut gallery = open_gallery();
        gallery.navigate(1, 0);
        gallery.play();
        gallery.cancel();
        assert!(gallery.take_music_requests().is_empty());
        assert_eq!(gallery.take_sounds(), ["move"]);
        gallery.press_row(1);
        gallery.release_row(1);
        assert_eq!(gallery.list().playing(), Some(1));
        assert!(matches!(
            gallery.take_music_requests().as_slice(),
            [MusicRequest::Play { .. }]
        ));
        gallery.release_row(1);
        assert_eq!(gallery.list().playing(), None);
        assert!(matches!(
            gallery.take_music_requests().as_slice(),
            [MusicRequest::Stop { ticks: 30 }]
        ));
        gallery.release_close();
        assert_eq!(gallery.phase(), MusicPhase::Closing);
        assert_eq!(gallery.take_sounds(), ["cancel"]);
    }
    #[test]
    fn close_waits_for_prior_frame_flash_and_returns_to_title_bgm_without_exit_fade() {
        let mut gallery = open_gallery();
        gallery.press_close();
        gallery.update(Ticks::from_f32(3.5), false);
        assert!((gallery.close_flash() - 0.7).abs() < 0.0001);
        gallery.release_close();
        gallery.update(Ticks::from_f32(3.5), false);
        assert_eq!(gallery.phase(), MusicPhase::Closing);
        assert!(gallery.take_requests().is_empty());
        gallery.update(Ticks::ZERO, false);
        assert_eq!(gallery.phase(), MusicPhase::Closed);
        assert_eq!(
            gallery.take_requests(),
            [MusicSceneRequest::ReturnToTitle {
                argument: 7,
                music_stop_ticks: 30
            }]
        );
        gallery.update(Ticks::from_f32(100.0), false);
        assert!(gallery.take_requests().is_empty());
        assert_eq!(gallery.take_sounds(), ["cancel"]);
    }
    #[test]
    fn finished_music_clears_indicator_without_emitting_stop() {
        let mut gallery = open_gallery();
        gallery.play();
        assert_eq!(gallery.list().playing(), Some(0));
        gallery.take_music_requests();
        gallery.update(Ticks::ZERO, true);
        assert_eq!(gallery.list().playing(), Some(0));
        gallery.update(Ticks::ZERO, false);
        assert_eq!(gallery.list().playing(), None);
        gallery.stop();
        assert!(gallery.take_music_requests().is_empty());
    }
    #[test]
    fn unlocks_use_bgm_ids_and_preserve_locked_positions_with_hidden_labels() {
        let mut save = SaveData::default();
        save.save_vectors.unlocked_bgms.add(1);
        save.save_vectors.unlocked_bgms.add(101);
        let list = MusicGalleryList::new(&items(3), &save).unwrap();
        assert_eq!(list.rows().len(), 3);
        assert!(!list.rows()[0].unlocked);
        assert_eq!(list.rows()[0].name_index, 0);
        assert!(list.rows()[1].unlocked);
        assert_eq!(list.rows()[1].name_index, 2);
        assert_eq!(list.rows()[2].name_index, 0);
        assert_eq!(list.completion_tenths(), 333);
        assert!(!list.all_unlocked());
        assert_eq!(list.content_height(), 180.0);
        assert!(MusicGalleryList::new(&[], &save).is_err());
    }
    #[test]
    fn play_is_immediate_even_while_flash_runs_and_locked_play_only_emits_error() {
        let mut save = SaveData::default();
        save.save_vectors.unlocked_bgms.add(101);
        let mut list = MusicGalleryList::new(&items(3), &save).unwrap();
        list.play();
        assert!(list.take_requests().is_empty());
        assert_eq!(list.take_sounds(), ["error"]);
        assert_eq!(list.playing(), None);
        list.select(1, false);
        list.play();
        match list.take_requests().as_slice() {
            [MusicRequest::Play { bgm, once_flag }] => {
                assert_eq!(bgm.into_i32(), 101);
                assert_eq!(*once_flag, 1)
            }
            _ => panic!("Play request missing"),
        }
        assert_eq!(list.playing(), Some(1));
        assert_eq!(list.feedback(1).1, 0.0);
        list.update(Ticks::from_f32(3.5));
        assert!((list.feedback(1).1 - 0.7).abs() < 0.0001);
        list.stop();
        assert!(matches!(
            list.take_requests().as_slice(),
            [MusicRequest::Stop { ticks: 30 }]
        ));
        list.stop();
        assert!(list.take_requests().is_empty());
        assert!(list.take_sounds().is_empty());
    }
    #[test]
    fn two_column_navigation_clamps_odd_tail_and_waits_four_tick_scroll() {
        let mut list = MusicGalleryList::new(&items(25), &SaveData::default()).unwrap();
        assert!(!list.navigate(-1, -1));
        assert!(list.navigate(1, 0));
        assert_eq!(list.selected(), 1);
        list.update(Ticks::from_f32(4.0));
        assert!(list.navigate(0, 8));
        assert_eq!(list.selected(), 17);
        assert_eq!(list.scroll(), 0.0);
        assert!(!list.navigate(0, 8));
        assert!(list.take_sounds().iter().all(|name| *name == "move"));
        list.update(Ticks::from_f32(2.0));
        assert_eq!(list.scroll(), 47.0);
        list.update(Ticks::from_f32(2.0));
        assert_eq!(list.scroll(), 94.0);
        assert!(list.navigate(0, 8));
        assert_eq!(list.selected(), 24);
        list.update(Ticks::from_f32(4.0));
        assert_eq!(list.scroll(), 462.0);
        assert!(!list.navigate(1, 1));
        assert!(list.navigate(0, -8));
        assert_eq!(list.selected(), 8);
        list.update(Ticks::from_f32(4.0));
        assert_eq!(list.scroll(), 368.0);
    }
    #[test]
    #[ignore = "requires supplied ROM/executable (SHIN_REFERENCE_ROM)"]
    fn reference_native_music_gallery_metadata_and_play_arguments() {
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
        for (address, expected) in [
            (0x2feb4, 0x940286efu32),
            (0x2fedc, 0xb901b3ff),
            (0x2fec0, 0xb901b3e8),
            (0x300cc, 0x1ada0d08),
            (0x30254, 0x51001108),
            (0x319e8, 0x34000269),
            (0x31a08, 0x79400101),
            (0x31a0c, 0x79400902),
            (0x31a14, 0x9400077b),
            (0x316a8, 0x321f0fe1),
            (0x316ac, 0x94000961),
            (0x315ec, 0x94022901),
            (0x31b8c, 0x1e221001),
            (0x32830, 0x321e0fe1), // capture60
            (0x32844, 0x321e0fe1), // stop prior music60
            (0x32848, 0x940004fa),
            (0x32868, 0x97fffb06), // install input after completion poll
            (0x3287c, 0x9400c1b1), // then advance capture clock
            (0x62f20, 0x1a9f57e0), // busy while elapsed < duration
            (0x6320c, 0x1e211800), // captured opacity = 1 - elapsed/duration
            (0x63210, 0x1e203900),
            (0x32fe8, 0x94022282), // wait for close flash
            (0x32fec, 0x360000c0),
            (0x33010, 0x321f0fe1), // stop music30
            (0x33014, 0x94000307),
            (0x3301c, 0x32000be0), // returning Title argument7
            (0x33024, 0x94036bdf),
            (0x3302c, 0x97ffd0d1), // replace scene
            (0x320e8, 0x97fffe5e), // pointer release first selects/reveals
            (0x320f4, 0x54000161), // same playing row stops, different plays
            (0x32108, 0x940006ca),
            (0x30818, 0x94000d7e), // query music status
            (0x3081c, 0x37080080),
            (0x30824, 0xb900de68), // clear finished indicator
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [(0x159440, -188.0f32), (0x159618, 730.0), (0x159a70, -730.0)] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap())
                .unwrap();
        let file = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let items = &scenario.info_tables().music_box_info;
        assert!(!items.is_empty());
        let mut save = SaveData::default();
        let locked = MusicGalleryList::new(items, &save).unwrap();
        assert_eq!(locked.completion_tenths(), 0);
        assert_eq!(locked.selected(), 0);
        for (index, item) in items.iter().enumerate() {
            assert_eq!(locked.rows()[index].bgm.into_i32(), item.bgm_id.into_i32());
            assert_eq!(locked.rows()[index].name_index, 0);
            assert!(
                scenario
                    .info_tables()
                    .bgm_info
                    .get(item.bgm_id.into_i32() as usize)
                    .is_some()
            );
            save.save_vectors
                .unlocked_bgms
                .add(item.bgm_id.into_i32() as usize);
        }
        let mut unlocked = MusicGalleryList::new(items, &save).unwrap();
        assert!(unlocked.all_unlocked());
        assert_eq!(unlocked.completion_tenths(), 1000);
        for (index, item) in items.iter().enumerate() {
            unlocked.select(index, true);
            // Pointer dispatch may play while the reveal scroll is in flight.
            unlocked.play();
            assert_eq!(unlocked.playing(), Some(index));
            match unlocked.take_requests().as_slice() {
                [MusicRequest::Play { bgm, once_flag }] => {
                    assert_eq!(bgm.into_i32(), item.bgm_id.into_i32());
                    assert_eq!(*once_flag, item.once_flag);
                }
                _ => panic!("Original music entry{index} did not dispatch"),
            }
            assert_eq!(unlocked.rows()[index].name_index, item.name_index);
        }
        println!(
            "Original music gallery: {} authored entries verified",
            items.len()
        );
    }
}
