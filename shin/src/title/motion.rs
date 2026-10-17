//! Title task clocks10d5b0/111970 and row entrance10cdb0.
use super::{
    TitleItem, TitleSetup,
    view::{RowFeedback, TitleFrame},
};
use shin_core::time::{Easing, Ticks, Tween, Tweener};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    AwaitingAssets,
    FirstWait,
    RowGate,
    RowsOpening,
    Active,
    Retreating,
    Continuing,
    Destination,
    Child,
}
#[derive(Debug, Clone, Copy)]
enum Confirmation {
    Flash(usize),
    Rows,
}
struct Row {
    x: Tweener,
    y: Tweener,
    opacity: Tweener,
    focus: Tweener,
    flash: Tweener,
}
pub(crate) struct TitleMotion {
    setup: TitleSetup,
    selected: usize,
    phase: Phase,
    first_wait: Tweener,
    background_reveal: Tweener,
    row_gate: Tweener,
    logo_offset: Tweener,
    logo_alpha: Tweener,
    caption_alpha: Tweener,
    black_alpha: Tweener,
    continue_waits: usize,
    child_requests: Vec<TitleItem>,
    music_stops: Vec<Ticks>,
    caption_visible: bool,
    rows: Vec<Row>,
    confirmations: Vec<Confirmation>,
    destinations: std::collections::VecDeque<TitleItem>,
    pointer_row: Option<usize>,
    sounds: Vec<&'static str>,
}
fn enqueue(clock: &mut Tweener, value: f32, ticks: f32, easing: Easing) {
    clock.enqueue(
        value,
        Tween {
            duration: Ticks::from_f32(ticks),
            easing,
        },
    );
}
fn wait(clock: &mut Tweener, ticks: f32) {
    // Native flag80 is relative zero, holding the current queued target.
    let target = clock.target_value();
    enqueue(clock, target, ticks, Easing::Linear);
}
pub(super) fn rate(clock: &mut Tweener, value: f32, speed: f32) {
    // Native bb420 freezes the evaluated value and drops the prior queue;
    // flag40 then converts speed to abs(target-current)/speed.
    clock.enqueue_now(
        value,
        Tween::linear(Ticks::from_f32((value - clock.value()).abs() / speed)),
    );
}
impl TitleMotion {
    pub fn new(setup: TitleSetup) -> Self {
        let returning = setup.returning_from_child;
        Self {
            selected: setup.selected,
            rows: (0..setup.items.len())
                .map(|index| {
                    let origin = setup.row_origin(index).unwrap();
                    Row {
                        x: Tweener::new(origin.x),
                        y: Tweener::new(origin.y),
                        opacity: Tweener::new(0.0),
                        focus: Tweener::new(0.0),
                        flash: Tweener::new(0.0),
                    }
                })
                .collect(),
            setup,
            phase: Phase::AwaitingAssets,
            first_wait: Tweener::new(0.0),
            background_reveal: Tweener::new(0.0),
            row_gate: Tweener::new(0.0),
            // The startup entrance snaps the logo to these once assets load;
            // start there so it is not shown at full opacity while loading.
            logo_offset: Tweener::new(if returning { 0.0 } else { -100.0 }),
            logo_alpha: Tweener::new(if returning { 1.0 } else { 0.0 }),
            caption_alpha: Tweener::new(0.0),
            black_alpha: Tweener::new(0.0),
            continue_waits: 0,
            child_requests: Vec::new(),
            music_stops: Vec::new(),
            caption_visible: false,
            confirmations: Vec::new(),
            destinations: std::collections::VecDeque::new(),
            pointer_row: None,
            sounds: Vec::new(),
        }
    }
    /// Called once after the loading task installs the background picture.
    pub fn assets_ready(&mut self) {
        if self.phase != Phase::AwaitingAssets {
            return;
        }
        wait(
            &mut self.first_wait,
            if self.setup.returning_from_child {
                60.0
            } else {
                90.0
            },
        );
        if self.setup.returning_from_child {
            //10ca20 snaps rows, while constructor logo alpha stays1.
            for (index, row) in self.rows.iter_mut().enumerate() {
                row.opacity.fast_forward_to(1.0);
                row.focus
                    .fast_forward_to(if index == self.selected { 1.0 } else { 0.0 });
            }
            self.caption_visible = true;
            self.caption_alpha.fast_forward_to(1.0);
        } else {
            // Background group property0x42 has a separate native2000->0 clock.
            self.background_reveal.fast_forward_to(2000.0);
            enqueue(&mut self.background_reveal, 0.0, 90.0, Easing::Linear);
            self.row_gate.fast_forward_to(-200.0);
            enqueue(&mut self.row_gate, 0.0, 90.0, Easing::SineOut);
            wait(&mut self.row_gate, 30.0);
            self.logo_offset.fast_forward_to(-100.0);
            wait(&mut self.logo_offset, 90.0);
            enqueue(&mut self.logo_offset, 0.0, 60.0, Easing::SineOut);
            self.logo_alpha.fast_forward_to(0.0);
            wait(&mut self.logo_alpha, 90.0);
            enqueue(&mut self.logo_alpha, 1.0, 60.0, Easing::Linear);
        }
        self.phase = Phase::FirstWait;
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    /// The background owner applies this as native background property0x42, before UI.
    pub fn background_reveal(&self) -> f32 {
        self.background_reveal.value()
    }
    pub fn frame(&self) -> TitleFrame {
        TitleFrame {
            rows: self
                .rows
                .iter()
                .map(|row| RowFeedback {
                    x: row.x.value(),
                    y: row.y.value(),
                    opacity: row.opacity.value(),
                    focus: row.focus.value(),
                    flash: row.flash.value(),
                })
                .collect(),
            logo_offset: self.logo_offset.value(),
            logo_alpha: self.logo_alpha.value(),
            caption: self
                .caption_visible
                .then_some(self.setup.items[self.selected]),
            caption_alpha: self.caption_alpha.value(),
            black_alpha: self.black_alpha.value(),
        }
    }
    /// Native task-group callbacks inspect clock state before10c11c advances it.
    pub fn prepare_frame(&mut self) {
        // Continue111db0 waits for the entire black-clock queue, then reads
        // global autosave and replaces the scene. New callbacks inserted below
        // are not eligible until the next task-group update.
        if self.continue_waits != 0 && self.black_alpha.is_idle() {
            for _ in 0..std::mem::take(&mut self.continue_waits) {
                self.destinations.push_back(TitleItem::Continue);
            }
            self.phase = Phase::Destination;
        }
        // Task-group100300 invokes newest tasks first. Flash tasks retain their
        // original row as a wait gate;10da50 reads the current selected item.
        let pending = std::mem::take(&mut self.confirmations);
        for confirmation in pending.into_iter().rev() {
            let idle = match confirmation {
                Confirmation::Flash(row) => self.rows[row].flash.is_idle(),
                Confirmation::Rows => self
                    .rows
                    .iter()
                    .all(|row| row.opacity.is_idle() && row.y.is_idle()),
            };
            if idle {
                let item = self.setup.items[self.selected];
                if item == TitleItem::Continue {
                    //10de50 does not reset c8 before enqueue: repeated retained
                    // confirmations append additional90tick segments/tasks.
                    enqueue(&mut self.black_alpha, 1.0, 90.0, Easing::Linear);
                    self.continue_waits += 1;
                    self.music_stops.push(Ticks::from_f32(90.0));
                    self.phase = Phase::Continuing;
                } else if matches!(item, TitleItem::Characters | TitleItem::Tips) {
                    //10db14/10db3c retain Title without dimming.10f060
                    // restores header opacity and starts chooser caption fade.
                    enqueue(
                        &mut self.rows[self.selected].opacity,
                        1.0,
                        10.0,
                        Easing::Linear,
                    );
                    let mask = if item == TitleItem::Tips {
                        0x7f00
                    } else {
                        0xff00
                    };
                    let count = (self.setup.completed_episodes & mask).count_ones();
                    wait(&mut self.caption_alpha, count as f32 * 4.0);
                    enqueue(&mut self.caption_alpha, 1.0, 30.0, Easing::Linear);
                    self.child_requests.push(item);
                    self.phase = Phase::Child;
                } else if matches!(
                    item,
                    TitleItem::Manual | TitleItem::Config | TitleItem::Load
                ) {
                    //10db90/10dc08 reset c8 and move to half-black at speed.1;
                    //87100/5e1c0 create task children without replacing Title.
                    rate(&mut self.black_alpha, 0.5, 0.1);
                    self.child_requests.push(item);
                    self.phase = Phase::Child;
                } else {
                    self.destinations.push_back(item);
                    self.phase = Phase::Destination;
                }
            } else {
                self.confirmations.push(confirmation);
            }
        }
        self.confirmations.reverse();
        if self.phase == Phase::FirstWait && self.first_wait.is_idle() {
            self.phase = Phase::RowGate;
            return; // The newly inserted111970 task runs on the next frame.
        }
        if self.phase == Phase::RowGate && self.row_gate.is_idle() {
            if self.setup.returning_from_child {
                self.phase = Phase::RowsOpening;
            } else {
                self.open_rows(false);
            }
            return;
        }
        if self.phase == Phase::RowsOpening
            && self
                .rows
                .iter()
                .all(|row| row.opacity.is_idle() && row.y.is_idle())
        {
            self.phase = Phase::Active;
        }
    }
    pub fn advance_clocks(&mut self, delta: Ticks) {
        for clock in [
            &mut self.first_wait,
            &mut self.background_reveal,
            &mut self.row_gate,
            &mut self.logo_offset,
            &mut self.logo_alpha,
            &mut self.caption_alpha,
            &mut self.black_alpha,
        ] {
            clock.update(delta);
        }
        for row in &mut self.rows {
            for clock in [
                &mut row.x,
                &mut row.y,
                &mut row.opacity,
                &mut row.focus,
                &mut row.flash,
            ] {
                clock.update(delta);
            }
        }
    }
    /// Native10cdb0 reopens all rows; selected row can retain its child pose.
    pub fn open_rows(&mut self, retain_selected: bool) {
        let mut delay = if retain_selected { 30.0 } else { 0.0 };
        for (index, row) in self.rows.iter_mut().enumerate() {
            let origin = self.setup.row_origin(index).unwrap();
            if retain_selected && index == self.selected {
                enqueue(&mut row.y, origin.y, 30.0, Easing::SineInOut);
                enqueue(&mut row.focus, 1.0, 30.0, Easing::Linear);
            } else {
                row.focus
                    .fast_forward_to(if index == self.selected { 1.0 } else { 0.0 });
                row.opacity.fast_forward_to(0.0);
                wait(&mut row.opacity, delay);
                enqueue(&mut row.opacity, 1.0, 10.0, Easing::Linear);
                row.x.fast_forward_to(origin.x + 100.0);
                wait(&mut row.x, delay);
                enqueue(&mut row.x, origin.x, 10.0, Easing::SineOut);
            }
            row.flash.fast_forward_to(0.0);
            delay += 4.0;
        }
        self.caption_visible = true;
        wait(&mut self.caption_alpha, delay);
        enqueue(&mut self.caption_alpha, 1.0, 30.0, Easing::Linear);
        self.phase = Phase::RowsOpening;
    }
    /// Native10d150 queues confirmation rather than immediately constructing
    /// a scene/child. The host drains destinations only after task polling.
    pub fn confirm(&mut self) -> bool {
        if !self.accepts_input() {
            return false;
        }
        let item = self.setup.items[self.selected];
        self.sounds.push(if item == TitleItem::Start {
            "bell"
        } else {
            "decide"
        });
        let flash = &mut self.rows[self.selected].flash;
        rate(flash, 0.7, 0.2);
        enqueue(flash, 0.0, 3.5, Easing::Linear);
        if matches!(item, TitleItem::Characters | TitleItem::Tips) {
            let mut delay = 0.0;
            for (index, row) in self.rows.iter_mut().enumerate() {
                if index == self.selected {
                    continue;
                }
                wait(&mut row.opacity, delay);
                enqueue(&mut row.opacity, 0.0, 10.0, Easing::Linear);
                wait(&mut row.x, delay);
                let origin = self.setup.row_origin(index).unwrap();
                enqueue(&mut row.x, origin.x + 100.0, 10.0, Easing::SineIn);
                delay += 4.0;
            }
            delay += 30.0;
            let selected = &mut self.rows[self.selected];
            wait(&mut selected.y, delay);
            enqueue(&mut selected.y, 228.0, 30.0, Easing::SineInOut);
            wait(&mut selected.focus, delay);
            enqueue(&mut selected.focus, 0.0, 30.0, Easing::SineOut);
            self.confirmations.push(Confirmation::Rows);
            self.phase = Phase::Retreating;
        } else {
            if item == TitleItem::Start {
                wait(flash, 30.0);
            }
            self.confirmations.push(Confirmation::Flash(self.selected));
        }
        true
    }
    pub fn take_child_requests(&mut self) -> Vec<TitleItem> {
        std::mem::take(&mut self.child_requests)
    }
    ///111cb0/111bb0 ignore response0; final1 resets c8 to zero at speed.1.
    pub fn child_finished(&mut self, all_finished: bool) {
        rate(&mut self.black_alpha, 0.0, 0.1);
        if all_finished {
            self.phase = Phase::Active;
        }
    }
    pub fn viewer_dim(&mut self, value: f32) {
        rate(&mut self.black_alpha, value, 0.1);
    }
    pub fn take_destination(&mut self) -> Option<TitleItem> {
        self.destinations.pop_front()
    }
    pub fn take_music_stops(&mut self) -> Vec<Ticks> {
        std::mem::take(&mut self.music_stops)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn accepts_input(&self) -> bool {
        // Native FunctionTask17ae0 returns input & !task[0x40], independently
        // of whether its flash callback remains pending. Confirm10d150 sets
        // that blocking byte to1; TaskGroup100300 visits it before title input.
        self.phase == Phase::Active && self.confirmations.is_empty()
    }
    fn select_row(&mut self, target: usize) {
        rate(&mut self.rows[self.selected].focus, 0.0, 0.1);
        self.selected = target;
        self.caption_visible = true;
        rate(&mut self.rows[target].focus, 1.0, 0.25);
    }
    /// Native10e250 probes original row positions,704x88, in packed order.
    /// The six-pixel overlap belongs to the earlier row; animation is ignored.
    pub fn row_at(&self, point: glam::Vec2) -> Option<usize> {
        (0..self.rows.len()).find(|&index| {
            let origin = self.setup.row_origin(index).unwrap();
            point.x >= origin.x
                && point.x < origin.x + 704.0
                && point.y >= origin.y
                && point.y < origin.y + 88.0
        })
    }
    pub fn pointer_probe(&mut self, point: glam::Vec2) -> bool {
        if !self.accepts_input() {
            return false;
        }
        self.pointer_row = self.row_at(point);
        true // Native captures blank title background too.
    }
    pub fn pointer_reset(&mut self) {
        self.pointer_row = None;
    }
    pub fn pointer_press(&mut self) {
        if !self.accepts_input() {
            return;
        }
        if let Some(row) = self.pointer_row {
            rate(&mut self.rows[row].flash, 0.7, 0.2);
            self.select_row(row);
        }
    }
    pub fn pointer_release(&mut self) {
        if !self.accepts_input() {
            return;
        }
        if let Some(row) = self.pointer_row {
            self.select_row(row);
            self.confirm();
        }
    }
    /// Native drag/cancel fades the captured press flash and clears its row.
    pub fn pointer_leave(&mut self) {
        if let Some(row) = self.pointer_row.take() {
            rate(&mut self.rows[row].flash, 0.0, 0.2);
        }
    }
    /// Native10d944 prioritizes Down over Up and wraps packed rows.
    pub fn navigate(&mut self, down: bool, up: bool) -> bool {
        if !self.accepts_input() {
            return false;
        }
        let count = self.rows.len();
        let target = if down {
            (self.selected + 1) % count
        } else if up {
            (self.selected + count - 1) % count
        } else {
            self.selected
        };
        if target == self.selected {
            return false;
        }
        self.sounds.push("move");
        self.select_row(target);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::format::save::SaveData;
    fn ticks(value: f32) -> Ticks {
        Ticks::from_f32(value)
    }
    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 0.0001, "{a} vs {b}");
    }
    fn startup() -> TitleMotion {
        let mut model = TitleMotion::new(TitleSetup::from_save(&SaveData::default(), -1));
        model.assets_ready();
        model
    }
    fn ready() -> TitleMotion {
        let mut model = startup();
        model.advance_clocks(ticks(120.0));
        model.prepare_frame();
        model.prepare_frame();
        model.advance_clocks(ticks(60.0));
        model.prepare_frame();
        assert_eq!(model.phase(), Phase::Active);
        model
    }
    #[test]
    fn load_completion_starts_distinct_logo_and_background_clocks_once() {
        let mut model = TitleMotion::new(TitleSetup::from_save(&SaveData::default(), -1));
        model.advance_clocks(ticks(300.0));
        model.prepare_frame();
        assert_eq!(model.phase(), Phase::AwaitingAssets);
        assert!(model.frame().caption.is_none());
        close(model.frame().logo_alpha, 0.0);
        model.assets_ready();
        close(model.background_reveal(), 2000.0);
        close(model.frame().logo_alpha, 0.0);
        model.advance_clocks(ticks(45.0));
        model.assets_ready();
        close(model.background_reveal(), 1000.0);
        close(model.frame().logo_offset, -100.0);
        model.advance_clocks(ticks(60.0));
        close(model.background_reveal(), 0.0);
        close(model.frame().logo_alpha, 0.25);
        close(
            model.frame().logo_offset,
            -100.0 + 100.0 * (std::f32::consts::PI / 8.0).sin(),
        );
        assert_eq!(model.phase(), Phase::FirstWait);
    }
    #[test]
    fn row_gate_waits_for_global_motion_hold_then_staggers_packed_rows() {
        let mut model = startup();
        model.advance_clocks(ticks(90.0));
        model.prepare_frame();
        assert_eq!(model.phase(), Phase::RowGate);
        model.prepare_frame();
        assert_eq!(model.phase(), Phase::RowGate);
        model.advance_clocks(ticks(30.0));
        model.prepare_frame();
        assert_eq!(model.phase(), Phase::RowsOpening);
        assert!(!model.navigate(true, false));
        let frame = model.frame();
        assert_eq!(frame.caption, Some(super::super::TitleItem::Start));
        close(frame.caption_alpha, 0.0);
        assert!(
            frame
                .rows
                .iter()
                .all(|row| row.x == 1284.0 && row.opacity == 0.0)
        );
        model.advance_clocks(ticks(5.0));
        let frame = model.frame();
        close(frame.rows[0].opacity, 0.5);
        close(frame.rows[1].opacity, 0.1);
        close(frame.rows[2].opacity, 0.0);
        close(
            frame.rows[0].x,
            1284.0 - 100.0 * std::f32::consts::FRAC_1_SQRT_2,
        );
        model.advance_clocks(ticks(17.0));
        model.prepare_frame();
        assert_eq!(model.phase(), Phase::Active);
        //111350 gates opacity/Y, not caption or focus. Caption is still fading.
        close(model.frame().caption_alpha, 0.2);
        assert!(
            model
                .frame()
                .rows
                .iter()
                .all(|row| row.x == 1184.0 && row.opacity == 1.0)
        );
    }
    #[test]
    fn returning_child_snaps_rows_and_keeps_logo_visible_without_startup_motion() {
        let mut model = TitleMotion::new(TitleSetup::from_save(&SaveData::default(), 5));
        model.assets_ready();
        assert_eq!(model.selected(), 2);
        let frame = model.frame();
        close(frame.logo_alpha, 1.0);
        close(frame.logo_offset, 0.0);
        close(frame.caption_alpha, 1.0);
        assert_eq!(frame.caption, Some(super::super::TitleItem::Config));
        close(frame.rows[2].focus, 1.0);
        assert!(frame.rows.iter().all(|row| row.opacity == 1.0));
        model.advance_clocks(ticks(59.0));
        model.prepare_frame();
        assert_eq!(model.phase(), Phase::FirstWait);
        model.advance_clocks(ticks(1.0));
        for expected in [Phase::RowGate, Phase::RowsOpening, Phase::Active] {
            model.prepare_frame();
            assert_eq!(model.phase(), expected);
        }
    }
    #[test]
    fn navigation_wraps_with_down_priority_and_retargets_current_focus_at_native_rates() {
        let mut model = ready();
        assert!(model.navigate(false, true));
        assert_eq!(model.selected(), 3);
        model.advance_clocks(ticks(2.0));
        close(model.frame().rows[0].focus, 0.8);
        close(model.frame().rows[3].focus, 0.5);
        assert!(model.navigate(true, true));
        assert_eq!(model.selected(), 0);
        model.advance_clocks(ticks(1.0));
        close(model.frame().rows[0].focus, 1.0);
        close(model.frame().rows[3].focus, 0.4);
        assert_eq!(model.frame().caption, Some(super::super::TitleItem::Start));
        assert!(!model.navigate(false, false));
    }
    #[test]
    fn large_clock_steps_preserve_queued_waits_and_all_row_samples() {
        let mut one = startup();
        let mut many = startup();
        one.advance_clocks(ticks(120.0));
        for _ in 0..120 {
            many.advance_clocks(ticks(1.0));
        }
        for model in [&mut one, &mut many] {
            model.prepare_frame();
            model.prepare_frame();
        }
        one.advance_clocks(ticks(19.0));
        for _ in 0..19 {
            many.advance_clocks(ticks(1.0));
        }
        let a = one.frame();
        let b = many.frame();
        close(a.logo_offset, b.logo_offset);
        close(a.logo_alpha, b.logo_alpha);
        close(a.caption_alpha, b.caption_alpha);
        for (a, b) in a.rows.iter().zip(b.rows) {
            close(a.x, b.x);
            close(a.opacity, b.opacity);
            close(a.focus, b.focus);
        }
    }
    #[test]
    fn reopen_retains_selected_row_and_counts_its_stagger_slot() {
        let mut model = ready();
        model.open_rows(true);
        close(model.frame().rows[0].opacity, 1.0);
        model.advance_clocks(ticks(35.0));
        close(model.frame().rows[0].focus, 1.0);
        close(model.frame().rows[1].opacity, 0.1);
        close(model.frame().rows[2].opacity, 0.0);
        model.advance_clocks(ticks(17.0));
        model.prepare_frame();
        assert_eq!(model.phase(), Phase::Active);
    }

    fn ready_item(item: TitleItem) -> TitleMotion {
        let mut save = SaveData::default();
        save.persist_data.set(0, 31);
        save.persist_data.set(17, 3);
        let mut model = TitleMotion::new(TitleSetup::from_save(&save, item as i32));
        model.assets_ready();
        model.advance_clocks(ticks(60.0));
        for _ in 0..3 {
            model.prepare_frame();
        }
        assert_eq!(model.phase(), Phase::Active);
        model
    }
    #[test]
    fn manual_is_a_retained_child_with_speed_based_title_dimming() {
        let mut model = ready_item(TitleItem::Manual);
        let selected = model.selected();
        assert!(model.confirm());
        model.advance_clocks(ticks(7.0));
        model.prepare_frame();
        assert_eq!(model.phase(), Phase::Child);
        assert_eq!(model.take_child_requests(), vec![TitleItem::Manual]);
        assert!(model.take_destination().is_none());
        assert!(!model.confirm());
        model.advance_clocks(ticks(2.5));
        close(model.frame().black_alpha, 0.25);
        model.advance_clocks(ticks(2.5));
        close(model.frame().black_alpha, 0.5);
        model.child_finished(true);
        assert_eq!(model.phase(), Phase::Active);
        assert_eq!(model.selected(), selected);
        model.advance_clocks(ticks(2.5));
        close(model.frame().black_alpha, 0.25);
        model.advance_clocks(ticks(2.5));
        close(model.frame().black_alpha, 0.0);
    }
    fn ready_continue() -> TitleMotion {
        use shin_core::format::save::{GameData, GameDataEntry};
        let mut save = SaveData::default();
        save.auto_save_slot = Some(GameData::from_entry(GameDataEntry {
            scenario_id: 0,
            random_seed: 42,
            save_position: 1,
            route_data: Default::default(),
        }));
        let mut model = TitleMotion::new(TitleSetup::from_save(&save, TitleItem::Continue as i32));
        model.assets_ready();
        model.advance_clocks(ticks(60.0));
        for _ in 0..3 {
            model.prepare_frame();
        }
        assert_eq!(model.phase(), Phase::Active);
        model
    }
    #[test]
    fn continue_fades_after_flash_and_dispatches_only_on_post_clock_task_poll() {
        let mut model = ready_continue();
        assert!(model.confirm());
        assert_eq!(model.take_sounds(), ["decide"]);
        model.advance_clocks(ticks(7.0));
        assert!(model.take_destination().is_none());
        model.prepare_frame();
        assert_eq!(model.phase(), Phase::Continuing);
        assert_eq!(model.take_music_stops(), [ticks(90.0)]);
        assert!(model.take_music_stops().is_empty());
        close(model.frame().black_alpha, 0.0);
        assert!(!model.accepts_input());
        assert!(!model.confirm());
        model.advance_clocks(ticks(45.0));
        close(model.frame().black_alpha, 0.5);
        model.prepare_frame();
        assert!(model.take_destination().is_none());
        model.advance_clocks(ticks(45.0));
        close(model.frame().black_alpha, 1.0);
        assert!(model.take_destination().is_none());
        model.prepare_frame();
        assert_eq!(model.take_destination(), Some(TitleItem::Continue));
        model.prepare_frame();
        assert!(model.take_destination().is_none());
    }
    #[test]
    fn repeated_continue_confirmation_cannot_queue_another_fade_or_destination() {
        let mut model = ready_continue();
        assert!(model.confirm());
        assert!(!model.confirm());
        assert_eq!(model.take_sounds(), ["decide"]);
        model.advance_clocks(ticks(7.0));
        model.prepare_frame();
        assert_eq!(model.take_music_stops(), [ticks(90.0)]);
        model.advance_clocks(ticks(90.0));
        close(model.frame().black_alpha, 1.0);
        model.prepare_frame();
        assert_eq!(model.take_destination(), Some(TitleItem::Continue));
        assert!(model.take_destination().is_none());
    }
    #[test]
    fn confirmation_task_blocks_navigation_until_captured_flash_finishes() {
        let mut model = ready_item(TitleItem::Load);
        let captured = model.selected();
        assert!(model.confirm());
        assert_eq!(model.phase(), Phase::Active);
        assert!(!model.accepts_input());
        assert!(!model.navigate(true, false));
        assert_eq!(model.selected(), captured);
        model.advance_clocks(ticks(3.5));
        model.prepare_frame();
        close(model.frame().rows[captured].flash, 0.7);
        assert!(model.take_destination().is_none());
        model.advance_clocks(ticks(3.5));
        assert!(model.take_destination().is_none());
        model.prepare_frame();
        assert_eq!(model.take_child_requests(), vec![TitleItem::Load]);
        assert!(model.take_destination().is_none());
        assert!(!model.navigate(true, false));
        assert!(!model.confirm());
    }
    #[test]
    fn start_adds_thirty_tick_hold_after_flash_and_repeated_poll_does_not_dispatch_twice() {
        let mut model = ready_item(TitleItem::Start);
        assert!(model.confirm());
        model.advance_clocks(ticks(7.0));
        model.prepare_frame();
        close(model.frame().rows[0].flash, 0.0);
        assert!(model.take_destination().is_none());
        model.advance_clocks(ticks(29.0));
        model.prepare_frame();
        assert!(model.take_destination().is_none());
        model.advance_clocks(ticks(1.0));
        model.prepare_frame();
        assert_eq!(model.take_destination(), Some(TitleItem::Start));
        model.prepare_frame();
        assert!(model.take_destination().is_none());
    }
    #[test]
    fn episode_picker_retreat_excludes_selected_from_stagger_and_moves_it_to_228() {
        for item in [TitleItem::Characters, TitleItem::Tips] {
            let mut model = ready_item(item);
            let selected = model.selected();
            let original = model.frame().rows[selected].y;
            assert!(model.confirm());
            assert_eq!(model.phase(), Phase::Retreating);
            assert!(!model.navigate(true, false));
            assert!(!model.confirm());
            model.advance_clocks(ticks(5.0));
            let frame = model.frame();
            close(frame.rows[0].opacity, 0.5);
            close(frame.rows[1].opacity, 0.9);
            close(frame.rows[selected].opacity, 1.0);
            close(frame.rows[selected].y, original);
            // Ten rows total: nine retreating rows give36 delay ticks, then30.
            model.advance_clocks(ticks(76.0));
            let frame = model.frame();
            close(frame.rows[selected].y, (original + 228.0) / 2.0);
            close(
                frame.rows[selected].focus,
                1.0 - std::f32::consts::FRAC_1_SQRT_2,
            );
            assert!(
                frame
                    .rows
                    .iter()
                    .enumerate()
                    .all(|(i, row)| i == selected || row.opacity == 0.0)
            );
            assert_eq!(frame.caption, Some(item));
            close(frame.caption_alpha, 1.0);
            model.prepare_frame();
            assert!(model.take_destination().is_none());
            model.advance_clocks(ticks(15.0));
            model.prepare_frame();
            close(model.frame().rows[selected].y, 228.0);
            assert!(model.take_destination().is_none());
            assert_eq!(model.take_child_requests(), [item]);
            assert_eq!(model.phase(), Phase::Child);
            close(model.frame().black_alpha, 0.0);
            model.open_rows(true);
            model.advance_clocks(ticks(30.0));
            close(model.frame().rows[selected].y, original);
        }
    }
    #[test]
    fn repeated_confirmation_does_not_restart_flash_or_queue_another_child() {
        let mut model = ready_item(TitleItem::Config);
        let row = model.selected();
        assert!(model.confirm());
        model.advance_clocks(ticks(2.0));
        close(model.frame().rows[row].flash, 0.4);
        assert!(!model.confirm());
        assert_eq!(model.take_sounds(), ["decide"]);
        model.advance_clocks(ticks(1.5));
        close(model.frame().rows[row].flash, 0.7);
        model.prepare_frame();
        assert!(model.take_destination().is_none());
        model.advance_clocks(ticks(3.5));
        model.prepare_frame();
        assert_eq!(model.take_child_requests(), vec![TitleItem::Config]);
        assert!(model.take_destination().is_none());
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_title_continue_fade_audio_stop_and_autosave_factory() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x10de64, 0x3908d008),
            (0x10de78, 0x52800b41),
            (0x10de7c, 0x97fc976d),
            (0x10de84, 0xbd49e101),
            (0x10de88, 0x1e2e1000),
            (0x10de8c, 0x91032260),
            (0x10de98, 0x97feb6da),
            (0x10def0, 0x97ffc860),
            (0x111dc4, 0x91032100),
            (0x111dc8, 0x97fea70a),
            (0x111dcc, 0x360000c0),
            (0x111dec, 0x91032100),
            (0x111df4, 0x2a1f03e1),
            (0x111dfc, 0x97fc1185),
            (0x111e04, 0x97fc555b),
            (0x10c120, 0x91032260),
            (0x10c124, 0x97febe4b),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        assert_eq!(
            f32::from_le_bytes(exe[0x1599e0 + 256..0x1599e0 + 260].try_into().unwrap()),
            90.0
        );
        let callback = (0x11e040usize..0x152eb8)
            .step_by(24)
            .find(|&a| u64::from_le_bytes(exe[a + 256..a + 264].try_into().unwrap()) == 0x222480)
            .unwrap();
        assert_eq!(
            u64::from_le_bytes(exe[callback + 272..callback + 280].try_into().unwrap()),
            0x111db0
        );
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_title_opening_tasks_stagger_and_focus_rates() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x10d5cc, 0x37f80268),
            (0x10d690, 0x321f03e1),
            (0x10d6a8, 0x321903e1),
            (0x10d6cc, 0x321903e1),
            (0x10d71c, 0x2a1f03e1),
            (0x10d764, 0x54000061),
            (0x110b08, 0x321e0fe1),
            (0x110b14, 0x52800b41),
            (0x111984, 0x97fea81b),
            (0x1119a0, 0x340000e8),
            (0x1119c4, 0x97ffecfb),
            (0x10ce70, 0x1e2e100a),
            (0x10cf90, 0x97feba9c),
            (0x10cfc0, 0x321f03e1),
            (0x10d074, 0x1e2c2908),
            (0x10d0d8, 0x1e27d001),
            (0x111398, 0x97fea996),
            (0x1113a8, 0x97fea992),
            (0x10d944, 0x37c801aa),
            (0x10d948, 0x36c0072a),
            (0x10d9c8, 0x321a03e1),
            (0x10da1c, 0x1e2a1001),
            (0xbb97c, 0x1e221800),
            (0xbb980, 0x1e20c000),
            (0x100204, 0xf940067a),
            (0x100300, 0xf85f8ee0),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [
            (0x159500, 2000.0f32),
            (0x159650, -200.0),
            (0x1598f4, -100.0),
            (0x1599e0, 90.0),
            (0x159798, 60.0),
            (0x159a60, 100.0),
            (0x159570, 0.1),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native float{address:x}"
            );
        }
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_title_confirmation_flash_retreat_and_live_dispatch() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x10d180, 0xb9400508),
            (0x10d1d8, 0x321a03e1),
            (0x10d1f0, 0x321a03e1),
            (0x10d210, 0x540009c2),
            (0x10d240, 0x54000320),
            (0x10d2a0, 0x1e2c2908),
            (0x10d308, 0xbd460d00),
            (0x10d30c, 0x320007e1),
            (0x10d340, 0x321f03e1),
            (0x10d358, 0x1e27d001),
            (0x10d47c, 0x79046a68),
            (0x1116d0, 0xb9801008),
            (0x1116e0, 0x9105e100),
            (0x111700, 0x97fff0d4),
            (0x111824, 0x97fea873),
            (0x111834, 0x97fea86f),
            (0x111864, 0x97fff07b),
            (0x10da60, 0xb9822c08),
            (0x10da70, 0xb9400508),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [(0x1595a4, 0.7f32), (0x159974, 0.2), (0x15960c, 228.0)] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native float{address:x}"
            );
        }
        assert_eq!(
            &exe[0x18662d + 256..0x186638 + 256],
            &[0, 6, 11, 31, 41, 92, 124, 56, 130, 62, 136]
        );
    }
}
