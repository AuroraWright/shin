//! Native scene-jump confirmation and saved-route handoff.
//! The dialog renderer is in dialog_view; BacklogModal owns live installation.
use super::{backlog::Backlog, reconstruction::ReconstructedScene};
use anyhow::{Result, ensure};
use shin_core::{
    format::{
        save::{GameData, GameDataEntry, SaveData},
        scenario::Scenario,
    },
    time::{Easing, Ticks, Tween, Tweener},
};

pub(super) const PROMPT: &str = "シーンジャンプを実行します。@rよろしいですか？";
pub(super) const ANSWERS: [&str; 2] = ["はい", "いいえ"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Yes,
    No,
    Cancel,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    Opening,
    Open,
    Flash,
    Closing,
    Done,
}

/// Dialog239c0 emits its final callback only after button flash, twelve-tick
/// exit animation and five further ticks. ad290 accepts only (final=1, Yes=0).
pub(super) struct Confirmation {
    button_count: usize,
    cancellable: bool,
    phase: Phase,
    selected: Option<usize>,
    pub opacity: Tweener,
    pub scale: Tweener,
    pub flashes: [Tweener; 2],
    pub focus: [Tweener; 2],
    cooldown: f32,
    result: Option<Outcome>,
    intermediate: Option<Outcome>,
}
impl Default for Confirmation {
    fn default() -> Self {
        Self {
            button_count: 2,
            cancellable: true,
            phase: Phase::Start,
            selected: Some(1), // ad628 passes222d0 default index1 (No).
            opacity: Tweener::new(0.0),
            scale: Tweener::new(0.5),
            flashes: std::array::from_fn(|_| Tweener::new(0.0)),
            focus: [Tweener::new(0.0), Tweener::new(1.0)],
            cooldown: 5.0,
            result: None,
            intermediate: None,
        }
    }
}
impl Confirmation {
    /// Generic222d0 confirmation and223a0 single-button information task.
    pub fn new(button_count: usize, selected: usize, cancellable: bool) -> Self {
        assert!((1..=2).contains(&button_count) && selected < button_count);
        Self {
            button_count,
            cancellable,
            selected: Some(selected),
            focus: std::array::from_fn(|index| {
                Tweener::new(if index == selected { 1.0 } else { 0.0 })
            }),
            ..Default::default()
        }
    }
    pub fn button_count(&self) -> usize {
        self.button_count
    }
    /// Native SaveGame uses a generic dialog with no answers. Only its worker
    /// predicate can dismiss it; keyboard/pointer confirmation is unavailable.
    pub fn notification() -> Self {
        Self {
            button_count: 0,
            cancellable: false,
            selected: None,
            focus: std::array::from_fn(|_| Tweener::new(0.0)),
            ..Default::default()
        }
    }
    pub fn close_notification(&mut self) {
        assert_eq!(self.button_count, 0);
        if self.is_open() {
            self.result = Some(Outcome::Cancel);
            self.phase = Phase::Flash;
        }
    }
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn is_open(&self) -> bool {
        self.phase == Phase::Open
    }
    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }
    pub fn select(&mut self, index: usize) -> bool {
        if !self.is_open() || index >= self.button_count || self.selected == Some(index) {
            return false;
        }
        self.fade_focus();
        let focus = &mut self.focus[index];
        let current = focus.value();
        focus.fast_forward_to(current);
        focus.enqueue(1.0, Tween::linear(Ticks::from_f32((1.0 - current).abs())));
        self.selected = Some(index);
        true
    }
    pub fn confirm(&mut self) -> bool {
        if !self.is_open() {
            return false;
        }
        let Some(index) = self.selected else {
            return false;
        };
        let flash = &mut self.flashes[index];
        let current = flash.value();
        flash.fast_forward_to(current);
        flash.enqueue(
            0.7,
            Tween::linear(Ticks::from_f32((0.7 - current).abs() / 0.2)),
        );
        flash.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
        self.result = Some(if index == 0 {
            Outcome::Yes
        } else {
            Outcome::No
        });
        self.phase = Phase::Flash;
        true
    }
    fn fade_focus(&mut self) {
        if let Some(index) = self.selected {
            let focus = &mut self.focus[index];
            let current = focus.value();
            focus.fast_forward_to(current);
            focus.enqueue(0.0, Tween::linear(Ticks::from_f32(current.abs() / 0.1)));
        }
    }
    pub fn hold(&mut self, index: usize) -> bool {
        if !self.is_open() || index >= self.button_count {
            return false;
        }
        self.select(index);
        let flash = &mut self.flashes[index];
        let current = flash.value();
        flash.fast_forward_to(current);
        flash.enqueue(
            0.7,
            Tween::linear(Ticks::from_f32((0.7 - current).abs() / 0.2)),
        );
        true
    }
    pub fn cancel_hold(&mut self, index: usize) {
        if !self.is_open() || index >= self.button_count {
            return;
        }
        let flash = &mut self.flashes[index];
        let current = flash.value();
        flash.fast_forward_to(current);
        flash.enqueue(0.0, Tween::linear(Ticks::from_f32(current.abs() / 0.2)));
    }
    pub fn cancel(&mut self) -> bool {
        if !self.is_open() || !self.cancellable {
            return false;
        }
        self.fade_focus();
        self.selected = None;
        self.result = Some(Outcome::Cancel);
        self.phase = Phase::Flash;
        true
    }
    pub fn update(&mut self, delta: Ticks) {
        if self.phase == Phase::Start {
            self.scale.enqueue(
                1.0,
                Tween {
                    duration: Ticks::from_f32(12.0),
                    easing: Easing::SineOut,
                },
            );
            self.opacity
                .enqueue(1.0, Tween::linear(Ticks::from_f32(12.0)));
            self.phase = Phase::Opening;
        }
        match self.phase {
            Phase::Opening if self.opacity.is_idle() && self.scale.is_idle() => {
                self.phase = Phase::Open
            }
            Phase::Flash
                if self
                    .selected
                    .is_none_or(|index| self.flashes[index].is_idle()) =>
            {
                //23bbc passes final0 before starting the exit animation.
                self.intermediate = self.result;
                self.scale.enqueue(
                    0.5,
                    Tween {
                        duration: Ticks::from_f32(12.0),
                        easing: Easing::SineIn,
                    },
                );
                self.opacity
                    .enqueue(0.0, Tween::linear(Ticks::from_f32(12.0)));
                self.phase = Phase::Closing;
            }
            Phase::Closing if self.opacity.is_idle() && self.scale.is_idle() => {
                self.cooldown -= delta.as_f32();
                if self.cooldown <= 0.0 {
                    self.phase = Phase::Done;
                }
            }
            _ => {}
        }
        self.opacity.update(delta);
        self.scale.update(delta);
        for focus in &mut self.focus {
            focus.update(delta);
        }
        for flash in &mut self.flashes {
            flash.update(delta);
        }
    }
    pub fn take_outcome(&mut self) -> Option<Outcome> {
        self.is_done().then(|| self.result.take()).flatten()
    }
    pub fn take_intermediate_outcome(&mut self) -> Option<Outcome> {
        self.intermediate.take()
    }
}

/// Preparing a jump keeps the original snapshot/route untouched. The renderer
/// must retain the browser and its pause scopes until this dialog finishes.
pub(super) struct BacklogRollback {
    pub confirmation: Confirmation,
    pub previous_position: u32,
    target: GameData,
}
impl BacklogRollback {
    pub fn new(history: &Backlog, index: usize, source: &GameDataEntry) -> Result<Self> {
        Self::from_game_data(history, index, &GameData::from_entry(source.clone()))
    }
    pub fn from_game_data(history: &Backlog, index: usize, game: &GameData) -> Result<Self> {
        let source = &game.entry;
        let row = history
            .entries
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Backlog row is no longer available"))?;
        ensure!(
            row.save_position > 0 && row.save_position <= source.save_position,
            "Backlog boundary is outside the recorded source"
        );
        let mut target = game.clone();
        target.entry.save_position = row.save_position;
        Ok(Self {
            confirmation: Confirmation::default(),
            previous_position: source.save_position,
            target,
        })
    }
    // Keep VM-sized reconstruction temporaries out of the per-frame UI stack.
    #[inline(never)]
    pub fn reconstruct_confirmed_boxed(
        &mut self,
        scenario: &Scenario,
        globals: &SaveData,
        limit: usize,
    ) -> Option<Result<Box<ReconstructedScene>>> {
        self.reconstruct_confirmed(scenario, globals, limit)
            .map(|scene| scene.map(Box::new))
    }
    pub fn reconstruct_confirmed(
        &mut self,
        scenario: &Scenario,
        globals: &SaveData,
        command_limit: usize,
    ) -> Option<Result<ReconstructedScene>> {
        match self.confirmation.take_outcome()? {
            Outcome::Yes => Some(self.reconstruct_target(scenario, globals, command_limit)),
            Outcome::No | Outcome::Cancel => None,
        }
    }
    /// ad2c8/af238 load the current browser cursor at the accepted callback,
    /// after the dialog's flash, exit animation and cooldown. The opening row
    /// is only the feedback target; it does not lock the eventual save boundary.
    #[inline(never)]
    pub fn reconstruct_confirmed_at_current_boxed(
        &mut self,
        history: &Backlog,
        index: Option<usize>,
        source: &GameDataEntry,
        scenario: &Scenario,
        globals: &SaveData,
        limit: usize,
    ) -> Option<Result<Box<ReconstructedScene>>> {
        match self.confirmation.take_outcome()? {
            Outcome::Yes => Some((|| {
                self.retarget(history, index, source)?;
                self.reconstruct_target(scenario, globals, limit)
                    .map(Box::new)
            })()),
            Outcome::No | Outcome::Cancel => None,
        }
    }
    fn retarget(
        &mut self,
        history: &Backlog,
        index: Option<usize>,
        source: &GameDataEntry,
    ) -> Result<()> {
        let row = index
            .and_then(|i| history.entries.get(i))
            .ok_or_else(|| anyhow::anyhow!("Accepted backlog cursor is unavailable"))?;
        ensure!(
            row.save_position > 0 && row.save_position <= source.save_position,
            "Accepted backlog boundary is outside the collected route"
        );
        self.target.entry = source.clone();
        self.target.entry.save_position = row.save_position;
        Ok(())
    }
    fn reconstruct_target(
        &self,
        scenario: &Scenario,
        globals: &SaveData,
        limit: usize,
    ) -> Result<ReconstructedScene> {
        ReconstructedScene::from_game_data(scenario, &self.target, globals, limit).and_then(
            |scene| {
                ensure!(
                    scene.state.save_position == self.target.entry.save_position,
                    "Rollback boundary was not reached exactly"
                );
                Ok(scene)
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open(dialog: &mut Confirmation) {
        dialog.update(Ticks::from_f32(12.0));
        assert!(!dialog.is_open());
        dialog.update(Ticks::ZERO);
        assert!(dialog.is_open());
    }
    fn finish(dialog: &mut Confirmation) {
        if dialog.flashes.iter().any(|flash| !flash.is_idle()) {
            dialog.update(Ticks::from_f32(7.0));
        }
        dialog.update(Ticks::ZERO);
        dialog.update(Ticks::from_f32(12.0));
        assert!(!dialog.is_done());
        dialog.update(Ticks::from_f32(4.0));
        assert!(!dialog.is_done());
        dialog.update(Ticks::from_f32(1.0));
        assert!(dialog.is_done());
    }
    fn history_for_retarget() -> Backlog {
        use super::super::backlog::{BacklogEntry, BacklogItem};
        Backlog {
            entries: [1, 2, 9]
                .into_iter()
                .map(|save_position| BacklogEntry {
                    save_position,
                    item: BacklogItem::Checkpoint {
                        title: String::new(),
                        text: String::new(),
                    },
                })
                .collect(),
        }
    }
    #[test]
    fn late_target_preserves_original_previous_boundary_and_route_and_rejects_invalid_cursor() {
        let history = history_for_retarget();
        let mut route = shin_core::format::save::RouteRecorder::default();
        route.record_choice(7);
        let source = GameDataEntry {
            scenario_id: 19,
            random_seed: 42,
            save_position: 2,
            route_data: route.snapshot(),
        };
        let mut game = GameData::from_entry(source.clone());
        game.primary_entry = Some(GameDataEntry {
            scenario_id: 7,
            random_seed: 99,
            save_position: 19,
            route_data: route.snapshot(),
        });
        let retained = format!("{:?}", game.primary_entry);
        let mut jump = BacklogRollback::from_game_data(&history, 0, &game).unwrap();
        jump.previous_position = 1; // actual story boundary, before the collected future
        jump.retarget(&history, Some(1), &source).unwrap();
        assert_eq!(jump.target.entry.save_position, 2);
        assert_eq!(jump.previous_position, 1);
        assert_eq!(format!("{:?}", jump.target.primary_entry), retained);
        assert_eq!(jump.target.entry.scenario_id, 19);
        assert_eq!(jump.target.entry.random_seed, 42);
        assert_eq!(
            format!("{:?}", jump.target.entry.route_data),
            format!("{:?}", source.route_data)
        );
        for index in [None, Some(2), Some(3)] {
            assert!(jump.retarget(&history, index, &source).is_err());
            assert_eq!(jump.target.entry.save_position, 2);
        }
    }
    #[test]
    fn no_or_cancel_does_not_read_or_validate_the_late_cursor() {
        let history = history_for_retarget();
        let source = GameDataEntry {
            scenario_id: 0,
            random_seed: 42,
            save_position: 2,
            route_data: shin_core::format::save::RouteRecorder::default().snapshot(),
        };
        let scenario = super::super::backlog::tests::fixture(false);
        for outcome in [Outcome::No, Outcome::Cancel] {
            let mut jump = BacklogRollback::new(&history, 0, &source).unwrap();
            open(&mut jump.confirmation);
            if outcome == Outcome::No {
                jump.confirmation.confirm();
            } else {
                jump.confirmation.cancel();
            }
            finish(&mut jump.confirmation);
            assert!(
                jump.reconstruct_confirmed_at_current_boxed(
                    &history,
                    None,
                    &source,
                    &scenario,
                    &SaveData::default(),
                    0
                )
                .is_none()
            );
            assert_eq!(jump.target.entry.save_position, 1);
        }
    }
    #[test]
    fn focus_rises_in_one_tick_and_fades_in_ten_ticks() {
        let mut dialog = Confirmation::default();
        open(&mut dialog);
        assert!(dialog.select(0));
        dialog.update(Ticks::from_f32(0.5));
        assert_eq!(dialog.focus[0].value(), 0.5);
        assert_eq!(dialog.focus[1].value(), 0.95);
        dialog.update(Ticks::from_f32(0.5));
        assert_eq!(dialog.focus[0].value(), 1.0);
        assert_eq!(dialog.focus[1].value(), 0.9);
        assert!(dialog.select(1));
        dialog.update(Ticks::from_f32(1.0));
        assert_eq!(dialog.focus[0].value(), 0.9);
        assert_eq!(dialog.focus[1].value(), 1.0);
        assert!(dialog.cancel());
        dialog.update(Ticks::from_f32(5.0));
        assert_eq!(dialog.focus[1].value(), 0.5);
        assert_eq!(dialog.selected(), None);
    }
    #[test]
    fn defaults_to_no_and_waits_for_flash_exit_and_final_callback() {
        let mut dialog = Confirmation::default();
        assert_eq!(dialog.selected(), Some(1));
        assert!(!dialog.confirm());
        open(&mut dialog);
        assert!(dialog.confirm());
        assert_eq!(dialog.take_outcome(), None);
        assert!(!dialog.select(0));
        assert!(!dialog.cancel());
        dialog.update(Ticks::from_f32(3.5));
        assert_eq!(dialog.flashes[1].value(), 0.7);
        dialog.update(Ticks::from_f32(3.5));
        assert_eq!(dialog.flashes[1].value(), 0.0);
        finish(&mut dialog);
        assert_eq!(dialog.take_outcome(), Some(Outcome::No));
        assert_eq!(dialog.take_outcome(), None);
    }
    #[test]
    fn long_frames_do_not_skip_native_task_boundaries_or_final_cooldown() {
        let mut dialog = Confirmation::default();
        dialog.update(Ticks::from_f32(100.0));
        assert!(!dialog.confirm());
        dialog.update(Ticks::ZERO);
        assert!(dialog.select(0));
        assert!(dialog.confirm());
        dialog.update(Ticks::from_f32(100.0));
        assert_eq!(dialog.take_outcome(), None);
        dialog.update(Ticks::from_f32(100.0));
        assert_eq!(dialog.opacity.value(), 0.0);
        assert_eq!(dialog.take_outcome(), None);
        dialog.update(Ticks::from_f32(4.0));
        assert_eq!(dialog.take_outcome(), None);
        dialog.update(Ticks::from_f32(1.0));
        assert_eq!(dialog.take_outcome(), Some(Outcome::Yes));
    }

    #[test]
    fn yes_and_cancel_are_distinct_and_finish_once() {
        for outcome in [Outcome::Yes, Outcome::Cancel] {
            let mut dialog = Confirmation::default();
            open(&mut dialog);
            match outcome {
                Outcome::Yes => {
                    assert!(dialog.select(0));
                    assert!(dialog.confirm());
                }
                Outcome::Cancel => {
                    assert!(dialog.cancel());
                }
                _ => unreachable!(),
            }
            finish(&mut dialog);
            assert_eq!(dialog.take_outcome(), Some(outcome));
            assert_eq!(dialog.take_outcome(), None);
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_backlog_rollback_confirmation_and_boundary_handoff() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (address, expected) in [
            (0xad618, 0x91242c21),
            (0xad624, 0x320003e1),
            (0xad628, 0x97fdd32a),
            (0x22378, 0xb900a293),
            (0x22fe4, 0xb940a368),
            (0x22fec, 0xb900cae8),
            (0x23a14, 0x1e25100a),
            (0x23a24, 0x321f03e1),
            (0x23a28, 0x2a1f03e2),
            (0x23a44, 0x94025fef),
            (0x23b7c, 0x94025f9d),
            (0x23b80, 0x37000700),
            (0x23b84, 0x1e251009),
            (0x23b88, 0x1e2c1000),
            (0x23b8c, 0x320003e1),
            (0x23b9c, 0x94025f99),
            (0x23ba0, 0x1e2703e0),
            (0x23bb4, 0x94025f93),
            (0x23bdc, 0xd63f0100),
            (0x23be0, 0x52a81408),
            (0x23be4, 0xb900ce68),
            (0x23c10, 0x1e283800),
            (0x23c1c, 0x5400022c),
            (0x23c24, 0x320003e9),
            (0x23c48, 0xd63f0100),
            (0xad298, 0x7100051f),
            (0xad29c, 0x7a400920),
            (0xad2a0, 0x54000040),
            (0xad2b0, 0x3906440a),
            (0xad2d0, 0xb9000d4d),
            (0xad2c8, 0xb981a008),
            (0xaf238, 0xb981a008),
            (0xaf23c, 0xf9411c09),
            (0xaf254, 0xb9400508),
            (0xaf258, 0xb9000928),
            (0xad2e4, 0xb9400508),
            (0xad2e8, 0xb9000928),
            (0xad2ec, 0x17ffe565),
        ] {
            assert_eq!(
                word(address),
                expected,
                "native rollback confirmation {address:x}"
            );
        }
        for (address, expected) in [
            (0x15b90b, PROMPT),
            (0x160990, ANSWERS[0]),
            (0x160c97, ANSWERS[1]),
        ] {
            let start = address + 256;
            let end = exe[start..].iter().position(|&byte| byte == 0).unwrap() + start;
            assert_eq!(std::str::from_utf8(&exe[start..end]).unwrap(), expected);
        }
    }
}
