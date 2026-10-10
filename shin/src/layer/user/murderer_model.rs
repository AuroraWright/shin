//! Script-facing behavior of interactive mode 2, recovered from MurdererLayer.
//! Shared by the menus, purple-statement browsers, and accusation view.
//! Replay/hint response phases and accusation completion timing follow native
//! polling. Top opening and submenu return capture/staging follow the native
//! parent lifecycle. Replay and Accusation use native directional selection;
//! Purple uses native person/chapter selection and held pointer feedback.
//! Purple pointer drag/inertia follows native callbacks; platform event arbitration remains pending.

use super::purple_text::{CHAPTERS, PERSON_TEXT};
use glam::{Vec2, Vec4, vec4};
use shin_core::{
    primitives::update::FrameId,
    time::{Easing, Ticks, Tween, Tweener},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MurdererMenu {
    Top,
    Replay,
    Rules,
    PurpleByPerson,
    PurpleByChapter,
    Hints,
    Accusation,
}

// Fourth bytes of native descriptors 28..44 at 0x17118b. Bit 6 marks a
// required suspect; the executable additionally checks IDs 34..36 explicitly.
const SUSPECT_FLAGS: [u8; 17] = [
    0, 1, 2, 131, 132, 5, 198, 199, 72, 137, 10, 11, 140, 13, 14, 15, 16,
];
pub(super) fn purple_person_is_red(index: usize) -> bool {
    // Native95cc8..95cd0: descriptor-mask bits28..44 clear => AARRGGBBffff0f0f.
    SUSPECT_FLAGS[index] & 0x80 != 0
}
pub const FIRST_SUSPECT_ENTRY: usize = 28;
pub const SUBMIT_ENTRY: usize = 45;
pub const BACK_RECT: Vec4 = vec4(1640.0, 32.0, 160.0, 84.0);
pub fn purple_person_rect(index: usize) -> Option<Vec4> {
    let &(column, row) = SUSPECT_GRID.get(index)?;
    Some(vec4(
        134.0 + 32.0 * f32::from(column),
        162.0 + 36.0 * f32::from(row),
        96.0,
        108.0,
    ))
}

pub fn top_menu_rect(index: usize) -> Option<Vec4> {
    (index < 6).then(|| vec4(1000.0, 441.0 + 96.0 * index as f32, 704.0, 94.0))
}
pub fn replay_rect(index: usize) -> Option<Vec4> {
    (index < 6).then(|| {
        vec4(
            124.0 + 564.0 * (index % 3) as f32,
            193.0 + 326.0 * (index / 3) as f32,
            544.0,
            306.0,
        )
    })
}
fn contains(rect: Vec4, point: Vec2) -> bool {
    point.is_finite()
        && point.x >= rect.x
        && point.x < rect.x + rect.z
        && point.y >= rect.y
        && point.y < rect.y + rect.w
}
// Grid coordinates of native descriptors 28..44 at 0x17118b.
const SUSPECT_GRID: [(u8, u8); 17] = [
    (5, 1),
    (9, 1),
    (13, 1),
    (5, 5),
    (9, 5),
    (13, 5),
    (5, 9),
    (9, 9),
    (13, 9),
    (5, 13),
    (13, 13),
    (1, 5),
    (1, 13),
    (1, 17),
    (5, 17),
    (9, 17),
    (13, 17),
];

// Native Accusation0x99ddc..0x99f48 and Purple0x9687c..0x969d8
// rank descriptors by distance / cosine^4 from
// an aim point one grid unit ahead. Submit's horizontal aim offset is seven.
fn murderer_grid_directional_target(
    current: usize,
    dx: i32,
    dy: i32,
    accusation: bool,
) -> Option<usize> {
    let grid = |entry: usize| {
        if entry == SUBMIT_ENTRY {
            (7, 21)
        } else {
            SUSPECT_GRID[entry - FIRST_SUSPECT_ENTRY]
        }
    };
    if !(FIRST_SUSPECT_ENTRY..=SUBMIT_ENTRY).contains(&current) || (dx == 0 && dy == 0) {
        return None;
    }
    let (x, y) = grid(current);
    let aim_x = i32::from(x) + dx * if current == SUBMIT_ENTRY { 7 } else { 1 };
    let aim_y = i32::from(y) + dy;
    let inverse_length = 1.0 / ((dx * dx + dy * dy) as f32).sqrt();
    let direction_x = inverse_length * dx as f32;
    let direction_y = inverse_length * dy as f32;
    let mut best_score = f32::MAX;
    let mut best = None;
    let last = if accusation {
        SUBMIT_ENTRY
    } else {
        SUBMIT_ENTRY - 1
    };
    for entry in FIRST_SUSPECT_ENTRY..=last {
        // Purple's native mask excludes portraits with empty transcripts.
        if entry == current
            || (!accusation && SUSPECT_FLAGS[entry - FIRST_SUSPECT_ENTRY] & 0x80 != 0)
        {
            continue;
        }
        let (x, y) = grid(entry);
        let vx = (i32::from(x) - aim_x) as f32;
        let vy = (i32::from(y) - aim_y) as f32;
        let distance = (vx * vx + vy * vy).sqrt();
        let inverse_distance = 1.0 / distance;
        let cosine = direction_x * (inverse_distance * vx) + direction_y * (inverse_distance * vy);
        if cosine > f32::EPSILON {
            let score = distance * (1.0 / cosine.powf(4.0));
            if score < best_score {
                best_score = score;
                best = Some(entry);
            }
        }
    }
    best
}

#[derive(Debug, Clone)]
enum AccusationCompletion {
    SubmitFlash {
        correct: bool,
    },
    WhiteFlashWait {
        remaining: f32,
        elapsed: f32,
        correct: bool,
    },
    FailureWait {
        remaining: f32,
    },
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponsePhase {
    Flash,
    Exit,
    Ready,
}

#[derive(Debug, Clone)]
struct ResponseCompletion {
    phase: ResponsePhase,
    flash: Tweener,
    timer: Tweener,
}

impl ResponseCompletion {
    fn from_flash(menu: MurdererMenu, mut flash: Tweener) -> Self {
        flash.enqueue_now(
            0.7,
            Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
        );
        flash.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
        let mut timer = Tweener::new(0.0);
        if menu == MurdererMenu::Hints {
            // Native0x929f0..0x92a08 starts this at the confirm click.
            timer.enqueue(0.0, Tween::linear(Ticks::from_f32(60.0)));
        }
        Self {
            phase: ResponsePhase::Flash,
            flash,
            timer,
        }
    }

    fn update(&mut self, menu: MurdererMenu, delta: Ticks) {
        if self.phase == ResponsePhase::Flash && self.flash.is_idle() {
            self.phase = ResponsePhase::Exit;
            if menu == MurdererMenu::Replay {
                // Native0x8e580..0x8e5a8: hold30 then fade60/SineOut.
                self.timer
                    .enqueue(0.0, Tween::linear(Ticks::from_f32(30.0)));
                self.timer.enqueue(
                    1.0,
                    Tween {
                        duration: Ticks::from_f32(60.0),
                        easing: Easing::SineOut,
                    },
                );
            }
        }
        if self.phase == ResponsePhase::Exit && self.timer.is_idle() {
            self.phase = ResponsePhase::Ready;
        }
        // Native phase conditions precede tween updates; frames do not carry
        // unused flash time into the replay exit sequence.
        self.flash.update(delta);
        self.timer.update(delta);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MurdererWipeKind {
    Default,
    Glass,
}

#[derive(Debug, Clone)]
pub(crate) struct MurdererWipe {
    pub serial: u64,
    pub kind: MurdererWipeKind,
    pub elapsed: f32,
    pub duration: f32,
    pub glass: Option<crate::wiper::glass::GlassWiper>,
    last_frame: Option<FrameId>,
    // A value snapshot is only a deferred rendering command. It never updates;
    // the renderer materializes it into a frozen texture once per serial.
    pub source: Box<MurdererModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TopPhase {
    Waiting,
    Flash,
    Prepare,
    Staged,
}

#[derive(Debug, Clone)]
struct BackCompletion {
    staged: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerTarget {
    Heading(usize),
    Row(usize),
    Back,
}

/// Native Select constructor 0x97cb0, plus render offset (320,200).
pub fn accusation_rect(entry: usize) -> Option<Vec4> {
    if entry == SUBMIT_ENTRY {
        return Some(vec4(340.0, 819.0, 420.0, 88.0));
    }
    let &(column, row) = SUSPECT_GRID.get(entry.checked_sub(FIRST_SUSPECT_ENTRY)?)?;
    Some(vec4(
        310.0 + 28.0 * f32::from(column),
        183.5 + 30.0 * f32::from(row),
        88.0,
        99.0,
    ))
}

/// Select hit test 0x9a160 first clips to (320,200)..(780,900).
/// The submit sprite extends seven pixels below that interactive area.
pub fn accusation_hit_test(position: Vec2) -> Option<usize> {
    if !position.is_finite()
        || position.x < 320.0
        || position.x >= 780.0
        || position.y < 200.0
        || position.y >= 900.0
    {
        return None;
    }
    (FIRST_SUSPECT_ENTRY..=SUBMIT_ENTRY).find(|&entry| {
        let rect = accusation_rect(entry).unwrap();
        position.x >= rect.x
            && position.x < rect.x + rect.z
            && position.y >= rect.y
            && position.y < rect.y + rect.w
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollTarget {
    List,
    Text,
    Hint,
}
#[derive(Debug, Clone, Default)]
struct ScrollPointer {
    held: bool,
    origin: Vec2,
    previous: Vec2,
    displacement: Vec2,
    dragging: bool,
    target: Option<ScrollTarget>,
}

#[derive(Debug, Clone)]
pub struct MurdererModel {
    pub menu: MurdererMenu,
    pub result: Option<i32>,
    hint_progress: i32,
    suspects: [bool; 17],
    pub accusation_focus: Option<usize>,
    accusation_row_focus: [Tweener; 18],
    accusation_row_flash: [Tweener; 18],
    last_pointer: Option<Vec2>,
    pub menu_focus: usize,
    top_focus: usize,
    pub hint_scroll: f32,
    hint_scroll_motion: Tweener,
    hint_scroll_velocity: f32,
    pub purple_list_scroll: f32,
    purple_list_motion: Tweener,
    pub purple_text_scroll: f32,
    purple_text_height: f32,
    purple_list_velocity: f32,
    purple_text_velocity: f32,
    scroll_pointer: ScrollPointer,
    sounds: Vec<&'static str>,
    completion: Option<AccusationCompletion>,
    response_completion: Option<ResponseCompletion>,
    restored_replay: Option<usize>,
    restored_battler_state: Option<usize>,
    replay_focus: [Tweener; 6],
    replay_row_flash: [Tweener; 6],
    purple_chapter_focus: [Tweener; 23],
    purple_chapter_flash: [Tweener; 23],
    purple_heading_flash: [Tweener; 6],
    pub(crate) purple_silhouette: usize,
    purple_person_focus: [Tweener; 17],
    purple_person_flash: [Tweener; 17],
    hint_focus: [Tweener; 21],
    hint_row_flash: [Tweener; 21],
    back_feedback: Tweener,
    top_row_focus: [Tweener; 6],
    top_phase: TopPhase,
    top_row_flash: [Tweener; 6],
    pointer_held: bool,
    pointer_row: Option<PointerTarget>,
    top_wait: Tweener,
    pub(crate) top_accusation_art: bool,
    pending_menu: Option<usize>,
    pending_top: bool,
    back_completion: Option<BackCompletion>,
    pub(crate) wipe: Option<MurdererWipe>,
    wipe_serial: u64,
}

impl MurdererModel {
    #[cfg(test)]
    pub(crate) fn reference_set_top_drawing(
        &mut self,
        art: bool,
        focus: [f32; 6],
        flash: [f32; 6],
    ) {
        self.top_accusation_art = art;
        for row in 0..6 {
            self.top_row_focus[row].fast_forward_to(focus[row]);
            self.top_row_flash[row].fast_forward_to(flash[row]);
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_set_replay_drawing(
        &mut self,
        phase: u32,
        selected: usize,
        amount: f32,
        focus: [f32; 6],
        flash: [f32; 6],
        back: f32,
    ) {
        self.menu = MurdererMenu::Replay;
        self.restored_replay = None;
        self.result = if phase == 0 {
            None
        } else {
            Some(selected as i32)
        };
        self.response_completion = if phase == 0 {
            None
        } else {
            Some(ResponseCompletion {
                phase: match phase {
                    1 => ResponsePhase::Flash,
                    2 => ResponsePhase::Exit,
                    3 => ResponsePhase::Ready,
                    _ => unreachable!(),
                },
                flash: Tweener::new(flash[selected]),
                timer: Tweener::new(amount),
            })
        };
        self.back_feedback.fast_forward_to(back);
        for row in 0..6 {
            self.replay_focus[row].fast_forward_to(focus[row]);
            self.replay_row_flash[row].fast_forward_to(flash[row]);
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_set_hints_drawing(
        &mut self,
        progress: i32,
        scroll: f32,
        focus: [f32; 21],
        flash: [f32; 21],
        back: f32,
    ) {
        self.menu = MurdererMenu::Hints;
        self.hint_progress = progress;
        self.hint_scroll = scroll;
        self.hint_scroll_motion.fast_forward_to(scroll);
        self.back_feedback.fast_forward_to(back);
        for row in 0..21 {
            self.hint_focus[row].fast_forward_to(focus[row]);
            self.hint_row_flash[row].fast_forward_to(flash[row]);
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_set_purple_drawing(
        &mut self,
        chapter: bool,
        index: usize,
        list: f32,
        text: f32,
        focus: [f32; 23],
        flash: [f32; 23],
        heading: [f32; 6],
        back: f32,
    ) {
        self.wipe = None;
        self.menu = if chapter {
            MurdererMenu::PurpleByChapter
        } else {
            MurdererMenu::PurpleByPerson
        };
        self.menu_focus = if chapter {
            CHAPTERS
                .iter()
                .filter(|entry| entry.heading().is_none())
                .position(|entry| entry.original_index == index)
                .unwrap()
        } else {
            index
        };
        self.purple_silhouette = if matches!(index, 0 | 3 | 4 | 6 | 7 | 9 | 12) {
            0
        } else {
            index
        };
        self.purple_list_scroll = list;
        self.purple_list_motion.fast_forward_to(list);
        self.purple_text_scroll = text;
        self.back_feedback.fast_forward_to(back);
        if chapter {
            for i in 0..23 {
                self.purple_chapter_focus[i].fast_forward_to(focus[i]);
                self.purple_chapter_flash[i].fast_forward_to(flash[i]);
            }
            for i in 0..6 {
                self.purple_heading_flash[i].fast_forward_to(heading[i]);
            }
        } else {
            for i in 0..17 {
                self.purple_person_focus[i].fast_forward_to(focus[i]);
                self.purple_person_flash[i].fast_forward_to(flash[i]);
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_set_accusation_drawing(
        &mut self,
        state: usize,
        mask: u32,
        elapsed: Option<f32>,
        focus: [f32; 18],
        flash: [f32; 18],
        back: f32,
    ) {
        self.wipe = None;
        self.menu = MurdererMenu::Accusation;
        self.restored_battler_state = Some(state);
        self.completion = elapsed.map(|elapsed| AccusationCompletion::WhiteFlashWait {
            remaining: 0.,
            elapsed,
            correct: true,
        });
        for i in 0..17 {
            self.suspects[i] = (mask >> i) & 1 != 0;
        }
        for i in 0..18 {
            self.accusation_row_focus[i].fast_forward_to(focus[i]);
            self.accusation_row_flash[i].fast_forward_to(flash[i]);
        }
        self.back_feedback.fast_forward_to(back);
    }
    #[cfg(test)]
    pub(crate) fn reference_set_back_flash(&mut self, value: f32) {
        self.back_feedback.fast_forward_to(value);
    }
    pub fn new(hint_progress: i32) -> Self {
        Self {
            menu: MurdererMenu::Top,
            result: None,
            hint_progress,
            suspects: [false; 17],
            accusation_focus: Some(FIRST_SUSPECT_ENTRY),
            accusation_row_focus: std::array::from_fn(|i| {
                Tweener::new(if i == 0 { 1.0 } else { 0.0 })
            }),
            accusation_row_flash: std::array::from_fn(|_| Tweener::new(0.0)),
            last_pointer: None,
            menu_focus: 0,
            top_focus: 0,
            hint_scroll: 0.0,
            hint_scroll_motion: Tweener::new(0.0),
            hint_scroll_velocity: 0.0,
            purple_list_scroll: 0.0,
            purple_list_motion: Tweener::new(0.0),
            purple_text_scroll: 0.0,
            purple_text_height: 0.0,
            purple_list_velocity: 0.0,
            purple_text_velocity: 0.0,
            scroll_pointer: ScrollPointer::default(),
            sounds: Vec::new(),
            completion: None,
            response_completion: None,
            restored_replay: None,
            restored_battler_state: None,
            replay_focus: std::array::from_fn(|_| Tweener::new(0.0)),
            replay_row_flash: std::array::from_fn(|_| Tweener::new(0.0)),
            purple_chapter_focus: std::array::from_fn(|_| Tweener::new(0.0)),
            purple_chapter_flash: std::array::from_fn(|_| Tweener::new(0.0)),
            purple_heading_flash: std::array::from_fn(|_| Tweener::new(0.0)),
            purple_silhouette: 0,
            purple_person_focus: std::array::from_fn(|_| Tweener::new(0.0)),
            purple_person_flash: std::array::from_fn(|_| Tweener::new(0.0)),
            hint_focus: std::array::from_fn(|_| Tweener::new(0.0)),
            hint_row_flash: std::array::from_fn(|_| Tweener::new(0.0)),
            back_feedback: Tweener::new(0.0),
            top_row_focus: std::array::from_fn(|i| Tweener::new(if i == 0 { 1.0 } else { 0.0 })),
            top_phase: TopPhase::Waiting,
            top_row_flash: std::array::from_fn(|_| Tweener::new(0.0)),
            pointer_held: false,
            pointer_row: None,
            top_wait: Tweener::new(0.0),
            top_accusation_art: false,
            pending_menu: None,
            pending_top: false,
            back_completion: None,
            wipe: None,
            wipe_serial: 0,
        }
    }

    pub fn restore_completed_answer(&mut self, answer: i32) {
        // Native9b7a0 replaces only the active child. The parent's response58
        // stays unchanged; these callbacks restore artwork, not another answer.
        let menu = match answer {
            0..=5 => MurdererMenu::Replay,
            6..=26 => MurdererMenu::Hints,
            27..=48 => MurdererMenu::Accusation,
            _ => return,
        };
        self.menu = menu;
        self.restored_replay = None;
        self.restored_battler_state = None;
        self.completion = None;
        self.response_completion = None;
        self.back_completion = None;
        self.back_feedback.fast_forward_to(0.0);
        self.last_pointer = None;
        self.pointer_row = None;
        self.pointer_held = false;
        self.scroll_pointer = ScrollPointer::default();
        match menu {
            MurdererMenu::Replay => {
                // Native8ec20: phase3, selected row1, other rows0, expansion1.
                self.menu_focus = answer as usize;
                self.restored_replay = Some(self.menu_focus);
                self.replay_focus = std::array::from_fn(|row| {
                    Tweener::new(if row == self.menu_focus { 1.0 } else { 0.0 })
                });
                self.replay_row_flash = std::array::from_fn(|_| Tweener::new(0.0));
            }
            MurdererMenu::Hints => {
                // Native923b0 retains the constructor's input phase0. Scroll
                // centers the selected90px row and snaps downward to a row.
                self.menu_focus = (answer - 6) as usize;
                self.hint_focus = std::array::from_fn(|row| {
                    Tweener::new(if row == self.menu_focus && row < self.available_hints() {
                        1.0
                    } else {
                        0.0
                    })
                });
                self.hint_row_flash = std::array::from_fn(|_| Tweener::new(0.0));
                let maximum = (self.available_hints() as i32 * 90 - 720).max(0);
                let scroll =
                    ((self.menu_focus as i32 * 90 - 315).clamp(0, maximum) / 90 * 90) as f32;
                self.hint_scroll_motion.fast_forward_to(scroll);
                self.hint_scroll = scroll;
                self.hint_scroll_velocity = 0.0;
            }
            MurdererMenu::Accusation => {
                // Native99830 selects Submit and retains input phase0. Answer
                // 27 marks bit6 suspects and expression1; failure clears them
                // and uses expression2. Neither plays a new submission flash.
                self.accusation_focus = Some(SUBMIT_ENTRY);
                self.menu_focus = 0;
                self.accusation_row_focus = std::array::from_fn(|row| {
                    Tweener::new(if row == SUBMIT_ENTRY - FIRST_SUSPECT_ENTRY {
                        1.0
                    } else {
                        0.0
                    })
                });
                self.accusation_row_flash = std::array::from_fn(|_| Tweener::new(0.0));
                self.suspects =
                    std::array::from_fn(|row| answer == 27 && SUSPECT_FLAGS[row] & 0x40 != 0);
                self.restored_battler_state = Some(if answer == 27 { 1 } else { 2 });
            }
            _ => unreachable!(),
        }
    }

    /// Validation stores the answer immediately; native poll publishes it
    /// after the completion phases (98700), rather than at the submit click.
    pub fn poll_result(&self) -> Option<i32> {
        if self
            .response_completion
            .as_ref()
            .is_some_and(|completion| completion.phase != ResponsePhase::Ready)
        {
            return None;
        }
        match self.completion {
            None | Some(AccusationCompletion::Ready) => self.result,
            _ => None,
        }
    }
    pub fn allows_playback_mode_changes(&self) -> bool {
        // MurdererOverlay +0x58 inverts its +0x70, which delegates to the
        // current child's +0x30. Top/Rule/Check always return true there.
        match self.menu {
            MurdererMenu::Replay | MurdererMenu::Hints => {
                self.restored_replay.is_some()
                    || self
                        .response_completion
                        .as_ref()
                        .is_some_and(|completion| completion.phase != ResponsePhase::Flash)
            }
            MurdererMenu::Accusation => matches!(
                self.completion,
                Some(
                    AccusationCompletion::WhiteFlashWait { .. }
                        | AccusationCompletion::FailureWait { .. }
                        | AccusationCompletion::Ready
                )
            ),
            _ => false,
        }
    }
    pub(crate) fn battler_sprite_state(&self) -> usize {
        match self.completion {
            Some(AccusationCompletion::WhiteFlashWait { .. }) => 1,
            Some(AccusationCompletion::FailureWait { .. }) => 2,
            Some(AccusationCompletion::Ready) => {
                if self.suspects.iter().enumerate().all(|(index, selected)| {
                    if SUSPECT_FLAGS[index] & 0x40 != 0 && *selected {
                        true
                    } else {
                        !(34..37).contains(&(FIRST_SUSPECT_ENTRY + index)) && !selected
                    }
                }) {
                    1
                } else {
                    2
                }
            }
            _ => self.restored_battler_state.unwrap_or(0),
        }
    }
    pub fn white_flash_opacity(&self) -> f32 {
        match self.completion {
            Some(AccusationCompletion::WhiteFlashWait { elapsed, .. }) => {
                (1.0 - elapsed / 30.0).max(0.0)
            }
            _ => 0.0,
        }
    }
    pub fn replay_expansion(&self) -> Option<(usize, Vec4, f32)> {
        if self.menu != MurdererMenu::Replay {
            return None;
        }
        if let Some(index) = self.restored_replay {
            return Some((index, vec4(0.0, 0.0, 1920.0, 1080.0), 1.0));
        }
        let completion = self.response_completion.as_ref()?;
        if completion.phase == ResponsePhase::Flash {
            return None;
        }
        let index = usize::try_from(self.result?).ok()?;
        let initial = replay_rect(index)?;
        let amount = completion.timer.value();
        // Native8ea9c..8eb28 computes start + amount * delta, rather than
        // glam lerp's start*(1-amount) + end*amount. Preserve f32 rounding.
        let initial_center = Vec2::new(initial.x + initial.z * 0.5, initial.y + initial.w * 0.5);
        let center = initial_center + (Vec2::new(960.0, 540.0) - initial_center) * amount;
        let size = Vec2::new(1376.0, 774.0) * amount + Vec2::new(544.0, 306.0);
        Some((
            index,
            vec4(
                center.x - size.x * 0.5,
                center.y - size.y * 0.5,
                size.x,
                size.y,
            ),
            amount,
        ))
    }
    pub fn replay_feedback(&self, index: usize) -> (f32, f32) {
        let focus = self.replay_focus.get(index).map_or(0.0, Tweener::value);
        let flash = self
            .response_completion
            .as_ref()
            .filter(|_| self.result == Some(index as i32))
            .map_or_else(
                || self.replay_row_flash.get(index).map_or(0.0, Tweener::value),
                |completion| completion.flash.value(),
            );
        (focus, flash)
    }
    pub fn purple_chapter_feedback(&self, original: usize) -> (f32, f32) {
        let Some(row) = CHAPTERS.get(original).and_then(|entry| entry.label_row()) else {
            return (0.0, 0.0);
        };
        (
            self.purple_chapter_focus[row].value(),
            self.purple_chapter_flash[row].value(),
        )
    }
    pub fn purple_heading_flash(&self, heading: usize) -> f32 {
        self.purple_heading_flash
            .get(heading)
            .map_or(0.0, Tweener::value)
    }
    fn purple_chapter_pointer_target(&self, point: Vec2) -> Option<PointerTarget> {
        // Native970c0 checks heading hits before statement hits. The native
        // heading callback itself has no viewport clipping; artwork does.
        if let Some(heading) = (0..6).find(|&heading| {
            let mut rect = self.purple_heading_rect(heading).unwrap();
            rect.w = 90.0;
            contains(rect, point)
        }) {
            return Some(PointerTarget::Heading(heading));
        }
        if !contains(vec4(166.0, 180.0, 480.0, 720.0), point) {
            return None;
        }
        CHAPTERS
            .iter()
            .filter(|entry| entry.heading().is_none())
            .enumerate()
            .find_map(|(row, entry)| {
                let rect = vec4(
                    166.0,
                    180.0 + entry.original_index as f32 * 90.0 - self.purple_list_scroll,
                    480.0,
                    90.0,
                );
                ((row != self.menu_focus || self.pointer_row == Some(PointerTarget::Row(row)))
                    && contains(rect, point))
                .then_some(PointerTarget::Row(row))
            })
    }
    fn release_purple_heading(&mut self, heading: usize) {
        let flash = &mut self.purple_heading_flash[heading];
        flash.enqueue_now(
            0.7,
            Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
        );
        flash.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
        self.sounds.push("decide");
        let start = CHAPTERS
            .iter()
            .filter(|entry| entry.heading().is_some())
            .nth(heading)
            .unwrap()
            .original_index as f32
            * 90.0;
        self.purple_list_motion.enqueue_now(
            start.clamp(0.0, 1890.0),
            Tween {
                duration: Ticks::from_f32(4.0),
                easing: Easing::SineOut,
            },
        );
    }
    pub fn purple_person_feedback(&self, index: usize) -> (f32, f32) {
        (
            self.purple_person_focus
                .get(index)
                .map_or(0.0, Tweener::value),
            self.purple_person_flash
                .get(index)
                .map_or(0.0, Tweener::value),
        )
    }
    pub fn accusation_feedback(&self, entry: usize) -> (f32, f32) {
        let Some(index) = entry.checked_sub(FIRST_SUSPECT_ENTRY).filter(|i| *i < 18) else {
            return (0.0, 0.0);
        };
        (
            self.accusation_row_focus[index].value(),
            self.accusation_row_flash[index].value(),
        )
    }
    fn select_accusation(&mut self, entry: usize) {
        if self.accusation_focus == Some(entry) {
            return;
        }
        if let Some(old) = self.accusation_focus {
            let focus = &mut self.accusation_row_focus[old - FIRST_SUSPECT_ENTRY];
            focus.enqueue_now(0.0, Tween::linear(Ticks::from_f32(focus.value() / 0.1)));
        }
        let focus = &mut self.accusation_row_focus[entry - FIRST_SUSPECT_ENTRY];
        focus.enqueue_now(
            1.0,
            Tween::linear(Ticks::from_f32((1.0 - focus.value()) / 0.25)),
        );
        self.accusation_focus = Some(entry);
    }
    fn flash_accusation(&mut self, entry: usize) {
        let flash = &mut self.accusation_row_flash[entry - FIRST_SUSPECT_ENTRY];
        flash.enqueue_now(
            0.7,
            Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
        );
        flash.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
    }
    fn change_menu_focus(&mut self, index: usize) {
        if self.menu_focus == index {
            return;
        }
        if self.menu == MurdererMenu::PurpleByPerson && !matches!(index, 3 | 4 | 6 | 7 | 9 | 12) {
            // Native94768 leaves the last loaded image intact for empty names.
            self.purple_silhouette = index;
        }
        // Top8d5d4..8d61c clears queues and retargets from current
        // values at speed0.1 out/0.25 in, matching Replay focus rates.
        let focus: &mut [Tweener] = match self.menu {
            MurdererMenu::Top => &mut self.top_row_focus,
            MurdererMenu::Replay => &mut self.replay_focus,
            MurdererMenu::Hints => &mut self.hint_focus,
            MurdererMenu::PurpleByPerson => &mut self.purple_person_focus,
            MurdererMenu::PurpleByChapter => &mut self.purple_chapter_focus,
            _ => return,
        };
        let old = &mut focus[self.menu_focus];
        old.enqueue_now(0.0, Tween::linear(Ticks::from_f32(old.value() / 0.1)));
        let new = &mut focus[index];
        new.enqueue_now(
            1.0,
            Tween::linear(Ticks::from_f32((1.0 - new.value()) / 0.25)),
        );
    }
    pub fn hint_feedback(&self, index: usize) -> (f32, f32) {
        let focus = self.hint_focus.get(index).map_or(0.0, Tweener::value);
        let flash = self.hint_row_flash.get(index).map_or(0.0, Tweener::value);
        (focus, flash)
    }
    fn select_hint(&mut self, index: usize, ensure_visible: bool) {
        if index == self.menu_focus {
            return;
        }
        self.change_menu_focus(index);
        self.menu_focus = index;
        if ensure_visible {
            // Native92b84..92c34: clear scroll queue and move minimally
            // to reveal the selected90px row over4ticks, even if in view.
            let row_top = index as f32 * 90.0;
            let maximum = (self.available_hints() as f32 * 90.0 - 720.0).max(0.0);
            let target = if row_top < self.hint_scroll {
                row_top
            } else if row_top + 90.0 > self.hint_scroll + 720.0 {
                (row_top + 90.0 - 720.0).min(maximum)
            } else {
                self.hint_scroll
            };
            self.hint_scroll_motion
                .enqueue_now(target, Tween::linear(Ticks::from_f32(4.0)));
        }
    }
    pub fn top_focus_amount(&self, index: usize) -> f32 {
        self.top_row_focus.get(index).map_or(0.0, Tweener::value)
    }
    pub fn top_flash(&self, index: usize) -> f32 {
        if self.menu == MurdererMenu::Top {
            self.top_row_flash.get(index).map_or(0.0, Tweener::value)
        } else {
            0.0
        }
    }
    pub fn menu_input_busy(&self) -> bool {
        self.restored_replay.is_some()
            || self.wipe.is_some()
            || self.back_completion.is_some()
            || (self.menu == MurdererMenu::Top && self.top_phase != TopPhase::Waiting)
    }
    #[cfg(test)]
    pub(crate) fn reference_capture_source(&mut self, source: Self, kind: MurdererWipeKind) {
        self.capture_wipe(kind, 30.0);
        self.wipe.as_mut().unwrap().source = Box::new(source);
    }

    fn capture_wipe(&mut self, kind: MurdererWipeKind, duration: f32) {
        self.wipe_serial += 1;
        let mut source = self.clone();
        source.wipe = None;
        source.pending_menu = None;
        source.pending_top = false;
        source.sounds.clear();
        let seed = if kind == MurdererWipeKind::Glass {
            crate::engine_random::next_seed()
        } else {
            0
        };
        let glass = (kind == MurdererWipeKind::Glass)
            .then(|| crate::wiper::glass::GlassWiper::with_seed(Ticks::from_f32(duration), seed));
        self.wipe = Some(MurdererWipe {
            serial: self.wipe_serial,
            kind,
            elapsed: 0.0,
            duration,
            glass,
            last_frame: None,
            source: Box::new(source),
        });
    }
    fn update_top_phase(&mut self) {
        if self.menu != MurdererMenu::Top {
            return;
        }
        if self.top_phase == TopPhase::Flash && self.top_row_flash[self.menu_focus].is_idle() {
            if self.menu_focus == 5 {
                self.capture_wipe(MurdererWipeKind::Default, 10.0);
                self.top_accusation_art = true;
                // Top+20 holds zero for30ticks; render does not read this
                // value. Native+90 also switches decorative character art.
                self.top_wait
                    .enqueue_now(0.0, Tween::linear(Ticks::from_f32(30.0)));
            }
            self.top_phase = TopPhase::Prepare;
        }
        if self.top_phase == TopPhase::Prepare && self.wipe.is_none() && self.top_wait.is_idle() {
            let kind = if self.menu_focus == 5 {
                MurdererWipeKind::Glass
            } else {
                MurdererWipeKind::Default
            };
            self.capture_wipe(
                kind,
                if kind == MurdererWipeKind::Glass {
                    60.0
                } else {
                    30.0
                },
            );
            if self.menu_focus == 5 {
                self.sounds.push("execute");
            }
            self.pending_menu = Some(self.menu_focus);
            self.top_phase = TopPhase::Staged;
        }
    }
    pub fn back_flash(&self) -> f32 {
        self.back_feedback.value()
    }
    fn install_top(&mut self) {
        self.pending_top = false;
        self.back_completion = None;
        self.back_feedback = Tweener::new(0.0);
        self.menu = MurdererMenu::Top;
        self.top_phase = TopPhase::Waiting;
        self.top_row_flash = std::array::from_fn(|_| Tweener::new(0.0));
        self.pointer_row = None;
        self.top_wait = Tweener::new(0.0);
        self.top_accusation_art = false;
        self.menu_focus = self.top_focus;
        // Fresh Top constructor8c08c..8c09c restores one selected row.
        self.top_row_focus =
            std::array::from_fn(|i| Tweener::new(if i == self.top_focus { 1.0 } else { 0.0 }));
        self.last_pointer = None;
    }
    fn update_back_phase(&mut self) {
        if self
            .back_completion
            .as_ref()
            .is_some_and(|completion| !completion.staged && self.back_feedback.is_idle())
        {
            self.capture_wipe(MurdererWipeKind::Default, 30.0);
            self.pending_top = true;
            self.back_completion.as_mut().unwrap().staged = true;
        }
    }
    pub fn update(&mut self, delta: f32) {
        self.update_with_wipe_permission(delta, true);
    }
    pub(crate) fn update_with_wipe_permission(&mut self, delta: f32, allow_wipe: bool) {
        self.update_menu(delta, allow_wipe, None);
    }
    pub(crate) fn update_for_frame(&mut self, delta: f32, allow_wipe: bool, frame_id: FrameId) {
        self.update_menu(delta, allow_wipe, Some(frame_id));
    }
    pub(crate) fn fast_forward(&mut self) {
        // Native9b0b0 installs the staged child before forwarding to it. Child
        // callbacks finish eligible tweens, but leave their response phases
        // alone. Parent update subsequently publishes answers and retires wipes.
        if self.pending_top {
            self.install_top();
        }
        if let Some(index) = self.pending_menu.take() {
            self.open_menu(index);
        }
        let finish = |tweens: &mut [Tweener]| {
            for tween in tweens {
                tween.fast_forward_unless_prohibited();
            }
        };
        match self.menu {
            MurdererMenu::Top => {
                self.top_wait.fast_forward_unless_prohibited();
                finish(&mut self.top_row_focus);
                finish(&mut self.top_row_flash);
            }
            MurdererMenu::Replay => {
                finish(&mut self.replay_focus);
                finish(&mut self.replay_row_flash);
            }
            MurdererMenu::Rules => {}
            MurdererMenu::Hints => {
                let count = self.available_hints();
                finish(&mut self.hint_focus[..count]);
                finish(&mut self.hint_row_flash[..count]);
                self.hint_scroll_motion.fast_forward_unless_prohibited();
                self.hint_scroll = self.hint_scroll_motion.value();
            }
            MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter => {
                self.purple_list_motion.fast_forward_unless_prohibited();
                self.purple_list_scroll = self.purple_list_motion.value();
                if self.menu == MurdererMenu::PurpleByPerson {
                    finish(&mut self.purple_person_focus);
                    finish(&mut self.purple_person_flash);
                } else {
                    finish(&mut self.purple_chapter_focus);
                    finish(&mut self.purple_chapter_flash);
                    finish(&mut self.purple_heading_flash);
                }
            }
            MurdererMenu::Accusation => {
                finish(&mut self.accusation_row_focus);
                finish(&mut self.accusation_row_flash);
                match &mut self.completion {
                    Some(AccusationCompletion::WhiteFlashWait {
                        remaining, elapsed, ..
                    }) => {
                        *remaining = 0.0;
                        *elapsed = 60.0;
                    }
                    Some(AccusationCompletion::FailureWait { remaining }) => *remaining = 0.0,
                    _ => {}
                }
            }
        }
        if self.menu != MurdererMenu::Top {
            self.back_feedback.fast_forward_unless_prohibited();
        }
        if matches!(self.menu, MurdererMenu::Replay | MurdererMenu::Hints) {
            if let Some(completion) = &mut self.response_completion {
                completion.flash.fast_forward_unless_prohibited();
                completion.timer.fast_forward_unless_prohibited();
            }
        }
        if let Some(wipe) = &mut self.wipe {
            wipe.elapsed = wipe.duration;
            if let Some(glass) = &mut wipe.glass {
                crate::wiper::Wiper::fast_forward(glass);
            }
            // Native9b0b0 leaves wiper+40 and frozen capture+30 retained.
            // Only9b130's next update checks completion and releases them.
        }
    }
    fn update_menu(&mut self, delta: f32, allow_wipe: bool, frame_id: Option<FrameId>) {
        if !delta.is_finite() || delta < 0.0 {
            return;
        }
        // Parent9b130 installs staged child before updating active child.
        if self.pending_top {
            self.install_top();
        }
        if let Some(index) = self.pending_menu.take() {
            self.open_menu(index);
        }
        self.update_top_phase();
        self.update_back_phase();
        self.update_purple_inertia(delta);
        self.update_hint_response_phase();
        if self.menu != MurdererMenu::Top {
            self.back_feedback.update(Ticks::from_f32(delta));
        }
        if self.menu == MurdererMenu::Replay {
            for flash in &mut self.replay_row_flash {
                flash.update(Ticks::from_f32(delta));
            }
        }
        if self.menu == MurdererMenu::PurpleByChapter {
            for focus in &mut self.purple_chapter_focus {
                focus.update(Ticks::from_f32(delta));
            }
            for flash in &mut self.purple_chapter_flash {
                flash.update(Ticks::from_f32(delta));
            }
            for flash in &mut self.purple_heading_flash {
                flash.update(Ticks::from_f32(delta));
            }
        }
        if self.menu == MurdererMenu::PurpleByPerson {
            for focus in &mut self.purple_person_focus {
                focus.update(Ticks::from_f32(delta));
            }
            for flash in &mut self.purple_person_flash {
                flash.update(Ticks::from_f32(delta));
            }
        }
        if self.menu == MurdererMenu::Hints {
            for focus in &mut self.hint_focus {
                focus.update(Ticks::from_f32(delta));
            }
            for flash in &mut self.hint_row_flash {
                flash.update(Ticks::from_f32(delta));
            }
        }
        if self.menu == MurdererMenu::Top {
            for flash in &mut self.top_row_flash {
                flash.update(Ticks::from_f32(delta));
            }
        }
        self.top_wait.update(Ticks::from_f32(delta));
        self.hint_scroll_motion.update(Ticks::from_f32(delta));
        self.hint_scroll = self.hint_scroll_motion.value();
        self.update_hint_inertia(delta);
        self.purple_list_motion.update(Ticks::from_f32(delta));
        self.purple_list_scroll = self.purple_list_motion.value();
        if let Some(completion) = &mut self.response_completion {
            let previous = completion.phase;
            if self.menu == MurdererMenu::Hints {
                completion.flash.update(Ticks::from_f32(delta));
                completion.timer.update(Ticks::from_f32(delta));
            } else {
                completion.update(self.menu, Ticks::from_f32(delta));
            }
            if self.menu == MurdererMenu::Replay
                && previous == ResponsePhase::Flash
                && completion.phase == ResponsePhase::Exit
            {
                let selected = self.result.unwrap() as usize;
                self.replay_focus[selected].enqueue_now(0.0, Tween::linear(Ticks::from_f32(30.0)));
            }
        }
        if self.menu == MurdererMenu::Top {
            for focus in &mut self.top_row_focus {
                focus.update(Ticks::from_f32(delta));
            }
        }
        for focus in &mut self.replay_focus {
            focus.update(Ticks::from_f32(delta));
        }
        // Native checks phase conditions before updating tween values. A
        // completed timer transitions on the next update, preserving frames.
        self.completion = match self.completion.take() {
            Some(AccusationCompletion::SubmitFlash { correct })
                if self.accusation_row_flash[SUBMIT_ENTRY - FIRST_SUSPECT_ENTRY].is_idle() =>
            {
                Some(AccusationCompletion::WhiteFlashWait {
                    remaining: 60.0 - delta,
                    elapsed: delta,
                    correct,
                })
            }
            Some(AccusationCompletion::SubmitFlash { correct }) => {
                Some(AccusationCompletion::SubmitFlash { correct })
            }
            Some(AccusationCompletion::WhiteFlashWait {
                remaining, correct, ..
            }) if remaining <= 0.0 => {
                if correct {
                    Some(AccusationCompletion::Ready)
                } else {
                    self.sounds.push("ahaha");
                    Some(AccusationCompletion::FailureWait {
                        remaining: 60.0 - delta,
                    })
                }
            }
            Some(AccusationCompletion::WhiteFlashWait {
                remaining,
                elapsed,
                correct,
            }) => Some(AccusationCompletion::WhiteFlashWait {
                remaining: remaining - delta,
                elapsed: elapsed + delta,
                correct,
            }),
            Some(AccusationCompletion::FailureWait { remaining }) if remaining <= 0.0 => {
                Some(AccusationCompletion::Ready)
            }
            Some(AccusationCompletion::FailureWait { remaining }) => {
                Some(AccusationCompletion::FailureWait {
                    remaining: remaining - delta,
                })
            }
            other => other,
        };
        // Native98748 checks selected flash before986c8 row clocks advance.
        if self.menu == MurdererMenu::Accusation {
            for focus in &mut self.accusation_row_focus {
                focus.update(Ticks::from_f32(delta));
            }
            for flash in &mut self.accusation_row_flash {
                flash.update(Ticks::from_f32(delta));
            }
        }
        if let Some(wipe) = &mut self.wipe {
            // Native6aa40 consumes a frame ID even when focus forbids running.
            // Only the parent wiper deduplicates; child tweens retain their own
            // update order and are not suppressed here.
            if frame_id.is_none() || wipe.last_frame != frame_id {
                wipe.last_frame = frame_id;
                if let Some(glass) = &mut wipe.glass {
                    glass.advance_ticks_with_permission(delta, allow_wipe);
                }
                if allow_wipe {
                    wipe.elapsed = (wipe.elapsed + delta).min(wipe.duration);
                }
            }
            if wipe.elapsed >= wipe.duration {
                self.wipe = None;
            }
        }
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }

    /// Initialize a stable child in native top-menu order (table0x171170).
    /// Interactive confirmation uses the staged capture/transition lifecycle.
    pub fn open_menu(&mut self, index: usize) -> bool {
        if self.result.is_some() || self.menu != MurdererMenu::Top {
            return false;
        }
        let menu = match index {
            0 => MurdererMenu::Replay,
            1 => MurdererMenu::Rules,
            2 => MurdererMenu::PurpleByPerson,
            3 => MurdererMenu::PurpleByChapter,
            4 => MurdererMenu::Hints,
            5 => MurdererMenu::Accusation,
            _ => return false,
        };
        self.menu = menu;
        self.restored_replay = None;
        self.restored_battler_state = None;
        self.pointer_row = None;
        self.back_feedback = Tweener::new(0.0);
        if menu == MurdererMenu::Replay {
            self.replay_row_flash = std::array::from_fn(|_| Tweener::new(0.0));
            self.replay_focus =
                std::array::from_fn(|index| Tweener::new(if index == 0 { 1.0 } else { 0.0 }));
        }
        self.top_focus = index;
        self.menu_focus = if menu == MurdererMenu::Hints {
            self.available_hints().saturating_sub(1)
        } else {
            0
        };
        if menu == MurdererMenu::PurpleByChapter {
            self.purple_chapter_focus =
                std::array::from_fn(|i| Tweener::new(if i == 0 { 1.0 } else { 0.0 }));
            self.purple_chapter_flash = std::array::from_fn(|_| Tweener::new(0.0));
            self.purple_heading_flash = std::array::from_fn(|_| Tweener::new(0.0));
        }
        if menu == MurdererMenu::PurpleByPerson {
            self.purple_silhouette = 0;
            self.purple_person_focus =
                std::array::from_fn(|i| Tweener::new(if i == 0 { 1.0 } else { 0.0 }));
            self.purple_person_flash = std::array::from_fn(|_| Tweener::new(0.0));
        }
        if menu == MurdererMenu::Hints {
            self.hint_focus = std::array::from_fn(|i| {
                Tweener::new(if i == self.menu_focus && i < self.available_hints() {
                    1.0
                } else {
                    0.0
                })
            });
            self.hint_row_flash = std::array::from_fn(|_| Tweener::new(0.0));
        }
        self.last_pointer = None;
        self.scroll_to_hint();
        self.hint_scroll_velocity = 0.0;
        self.purple_list_scroll = 0.0;
        self.purple_list_motion = Tweener::new(0.0);
        self.purple_text_scroll = 0.0;
        self.purple_text_velocity = 0.0;
        self.purple_text_height = 0.0;
        self.purple_list_velocity = 0.0;
        self.scroll_pointer = ScrollPointer::default();
        if menu == MurdererMenu::Accusation {
            if self.top_phase != TopPhase::Staged {
                self.sounds.push("execute");
            }
            self.suspects.fill(false);
            self.accusation_focus = Some(FIRST_SUSPECT_ENTRY);
            self.accusation_row_focus =
                std::array::from_fn(|i| Tweener::new(if i == 0 { 1.0 } else { 0.0 }));
            self.accusation_row_flash = std::array::from_fn(|_| Tweener::new(0.0));
            self.last_pointer = None;
        }
        true
    }

    pub fn back(&mut self) {
        self.back_with_sound("cancel");
    }
    fn back_with_sound(&mut self, sound: &'static str) {
        if self.result.is_none() && !self.menu_input_busy() && self.menu != MurdererMenu::Top {
            self.sounds.push(sound);
            let flash = &mut self.back_feedback;
            flash.enqueue_now(
                0.7,
                Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
            );
            flash.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
            self.back_completion = Some(BackCompletion { staged: false });
        }
    }

    /// ReplayMenu completion 0x8e5cc returns the original chapter index.
    pub fn replay_chapter(&mut self, index: usize) -> bool {
        if self.result.is_some()
            || self.menu_input_busy()
            || self.menu != MurdererMenu::Replay
            || index >= 6
        {
            return false;
        }
        self.change_menu_focus(index);
        self.menu_focus = index;
        self.sounds.push("decide");
        self.result = Some(index as i32);
        // Confirmation takes ownership of the selected rowflash. Its
        // current brightness survives clearing the native queued press.
        let flash = std::mem::replace(&mut self.replay_row_flash[index], Tweener::new(0.0));
        self.response_completion = Some(ResponseCompletion::from_flash(self.menu, flash));
        true
    }

    /// Constructor 0x90a08 exposes only hints 0..=progress (at most 21).
    pub fn available_hints(&self) -> usize {
        (i64::from(self.hint_progress) + 1).clamp(0, 21) as usize
    }

    pub fn hint_rect(&self, index: usize) -> Option<Vec4> {
        (index < self.available_hints()).then(|| {
            vec4(
                320.0,
                180.0 + 90.0 * index as f32 - self.hint_scroll,
                1280.0,
                88.0,
            )
        })
    }
    fn hint_hit_rect(&self, index: usize) -> Option<Vec4> {
        self.hint_rect(index).map(|mut rect| {
            rect.w = 90.0;
            rect
        })
    }
    pub fn hint_texture_row(&self, index: usize) -> Option<usize> {
        (index < self.available_hints()).then(|| {
            if index as i32 == self.hint_progress {
                0
            } else {
                index + 1
            }
        })
    }

    /// Native chapter browser includes 29 rows but headings are not selectable.
    /// Return the native chapter/person index, rather than the packed menu row.
    pub fn purple_original_index(&self) -> Option<usize> {
        match self.menu {
            MurdererMenu::PurpleByChapter => CHAPTERS
                .iter()
                .filter(|entry| entry.heading().is_none())
                .nth(self.menu_focus)
                .map(|entry| entry.original_index),
            MurdererMenu::PurpleByPerson => {
                (self.menu_focus < PERSON_TEXT.len()).then_some(self.menu_focus)
            }
            _ => None,
        }
    }
    pub fn purple_text(&self) -> Option<&'static str> {
        let index = self.purple_original_index()?;
        match self.menu {
            MurdererMenu::PurpleByChapter => Some(CHAPTERS[index].text),
            MurdererMenu::PurpleByPerson => Some(PERSON_TEXT[index]),
            _ => None,
        }
    }
    pub fn select_purple_original_index(&mut self, index: usize) -> bool {
        if self.result.is_some() {
            return false;
        }
        let selected = match self.menu {
            MurdererMenu::PurpleByChapter => CHAPTERS
                .iter()
                .filter(|entry| entry.heading().is_none())
                .position(|entry| entry.original_index == index),
            MurdererMenu::PurpleByPerson => (index < PERSON_TEXT.len()).then_some(index),
            _ => None,
        };
        if let Some(selected) = selected {
            self.change_menu_focus(selected);
            self.menu_focus = selected;
            self.purple_text_scroll = 0.0;
            self.purple_text_velocity = 0.0;
            self.scroll_to_purple_entry();
            true
        } else {
            false
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_set_purple_list_scroll(&mut self, scroll: f32) {
        assert!((0.0..=1890.0).contains(&scroll));
        self.purple_list_scroll = scroll;
        self.purple_list_motion = Tweener::new(scroll);
    }
    pub(super) fn purple_heading_offset(&self, heading: usize) -> Option<f32> {
        if self.menu != MurdererMenu::PurpleByChapter {
            return None;
        }
        let mut headings = CHAPTERS.iter().filter(|entry| entry.heading().is_some());
        let entry = headings.nth(heading)?;
        let start = entry.original_index as f32 * 90.0;
        let end = headings
            .next()
            .map_or(CHAPTERS.len() as f32 * 90.0, |entry| {
                entry.original_index as f32 * 90.0
            });
        // Native94eec..94f08: pin a scrolled section at the top until its
        // next heading pushes it up. Last heading cannot pin within0..1890.
        let offset = start - self.purple_list_scroll;
        let pinned = if offset < 0.0 {
            (end - self.purple_list_scroll - 90.0).min(0.0)
        } else {
            offset
        };
        Some(pinned)
    }
    pub fn purple_heading_rect(&self, heading: usize) -> Option<Vec4> {
        self.purple_heading_offset(heading)
            .map(|pinned| vec4(166., 180. + pinned, 480., 88.))
    }
    pub fn purple_list_scrolling(&self) -> bool {
        !self.purple_list_motion.is_idle()
    }
    fn select_purple_controller(&mut self, selected: usize) {
        self.select_purple(selected, true);
    }
    fn select_purple(&mut self, selected: usize, ensure_visible: bool) {
        if self.menu_focus == selected {
            return;
        }
        self.change_menu_focus(selected);
        self.menu_focus = selected;
        self.purple_text_scroll = 0.0;
        self.purple_text_velocity = 0.0;
        if ensure_visible && self.menu == MurdererMenu::PurpleByChapter {
            // Native96c18..96cd8 includes a90px top margin (the heading),
            // a720px viewport, and queues4ticks even for an unchanged offset.
            let top = self.purple_original_index().unwrap() as f32 * 90.0;
            let target = if top - self.purple_list_scroll < 90.0 {
                (top - 90.0).max(0.0)
            } else if top + 90.0 > self.purple_list_scroll + 720.0 {
                (top + 90.0 - 720.0).min(1890.0)
            } else {
                self.purple_list_scroll
            };
            self.purple_list_motion
                .enqueue_now(target, Tween::linear(Ticks::from_f32(4.0)));
        }
    }
    fn scroll_to_purple_entry(&mut self) {
        if self.menu == MurdererMenu::PurpleByChapter {
            let index = self.purple_original_index().unwrap_or(1);
            self.purple_list_scroll = (index as f32 * 90.0 - 315.0).clamp(0.0, 1890.0);
            self.purple_list_motion = Tweener::new(self.purple_list_scroll);
        }
    }
    pub fn scroll_purple_text(&mut self, delta: f32, height: f32) {
        if self.result.is_none()
            && matches!(
                self.menu,
                MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter
            )
            && delta.is_finite()
            && height.is_finite()
        {
            self.purple_text_velocity = 0.0;
            self.purple_text_height = height;
            self.purple_text_scroll =
                (self.purple_text_scroll + delta).clamp(0.0, (height - 720.0).max(0.0));
        }
    }
    pub fn set_purple_text_height(&mut self, height: f32) {
        if height.is_finite() {
            self.purple_text_height = height.max(0.0);
        }
    }
    fn purple_list_maximum(&self) -> f32 {
        if self.menu == MurdererMenu::PurpleByChapter {
            1890.0
        } else {
            0.0
        }
    }
    fn reconcile_purple_selection(&mut self, scroll: f32) {
        if self.menu != MurdererMenu::PurpleByChapter {
            return;
        }
        let mut row = self.menu_focus;
        loop {
            let original = CHAPTERS
                .iter()
                .filter(|entry| entry.heading().is_none())
                .nth(row)
                .unwrap()
                .original_index;
            let top = original as f32 * 90.0;
            if scroll + 90.0 > top && row < 22 {
                row += 1;
            } else if scroll + 720.0 < top + 90.0 && row > 0 {
                row -= 1;
            } else {
                break;
            }
        }
        self.select_purple(row, false);
    }
    fn update_hint_response_phase(&mut self) {
        if self.menu != MurdererMenu::Hints {
            return;
        }
        let Some(completion) = &mut self.response_completion else {
            return;
        };
        // Native917e8 checks the current row's flash;9181c publishes the
        // current selection before this frame's inertia can move it again.
        if completion.phase == ResponsePhase::Flash
            && self.hint_row_flash[self.menu_focus].is_idle()
        {
            completion.phase = ResponsePhase::Exit;
        }
        if completion.phase == ResponsePhase::Exit && completion.timer.is_idle() {
            completion.phase = ResponsePhase::Ready;
            self.result = Some(self.menu_focus as i32 + 6);
        }
    }
    pub fn scroll_hint_list(&mut self, delta: f32) {
        if self.menu != MurdererMenu::Hints
            || self.menu_input_busy()
            || self.result.is_some()
            || !self.hint_scroll_motion.is_idle()
            || !delta.is_finite()
        {
            return;
        }
        self.drag_scroll(ScrollTarget::Hint, -delta);
        self.hint_scroll_velocity = 0.0;
    }

    fn hint_scroll_maximum(&self) -> f32 {
        (self.available_hints() as f32 * 90.0 - 720.0).max(0.0)
    }
    fn reconcile_hint_selection(&mut self, scroll: f32) {
        if self.available_hints() == 0 {
            return;
        }
        let mut row = self.menu_focus;
        while scroll > row as f32 * 90.0 && row + 1 < self.available_hints() {
            row += 1;
        }
        while scroll + 720.0 < row as f32 * 90.0 + 90.0 && row > 0 {
            row -= 1;
        }
        self.select_hint(row, false);
    }
    fn update_hint_inertia(&mut self, ticks: f32) {
        if self.menu != MurdererMenu::Hints || self.hint_scroll_velocity == 0.0 {
            return;
        }
        let next = (self.hint_scroll - self.hint_scroll_velocity * ticks)
            .clamp(0.0, self.hint_scroll_maximum());
        if (self.hint_scroll - next).abs() < f32::EPSILON {
            self.hint_scroll_velocity = 0.0;
            return;
        }
        self.drag_scroll(ScrollTarget::Hint, self.hint_scroll_velocity * ticks);
        self.hint_scroll_velocity *= 0.95f32.powf(ticks);
        if self.hint_scroll_velocity.abs() < 0.1 {
            self.hint_scroll_velocity = 0.0;
        }
    }
    fn drag_scroll(&mut self, target: ScrollTarget, delta_y: f32) {
        if !delta_y.is_finite() {
            return;
        }
        match target {
            ScrollTarget::Hint => {
                let next = (self.hint_scroll - delta_y).clamp(0.0, self.hint_scroll_maximum());
                self.reconcile_hint_selection(next);
                self.hint_scroll_motion = Tweener::new(next);
                self.hint_scroll = next;
            }

            ScrollTarget::List => {
                let next =
                    (self.purple_list_scroll - delta_y).clamp(0.0, self.purple_list_maximum());
                self.reconcile_purple_selection(next);
                self.purple_list_motion = Tweener::new(next);
                self.purple_list_scroll = next;
            }
            ScrollTarget::Text => {
                self.purple_text_scroll = (self.purple_text_scroll - delta_y)
                    .clamp(0.0, (self.purple_text_height - 720.0).max(0.0));
            }
        }
    }
    fn update_purple_inertia(&mut self, ticks: f32) {
        if !matches!(
            self.menu,
            MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter
        ) {
            return;
        }
        for target in [ScrollTarget::List, ScrollTarget::Text] {
            let (current, velocity, maximum) = match target {
                ScrollTarget::Hint => unreachable!(),
                ScrollTarget::List => (
                    self.purple_list_scroll,
                    self.purple_list_velocity,
                    self.purple_list_maximum(),
                ),
                ScrollTarget::Text => (
                    self.purple_text_scroll,
                    self.purple_text_velocity,
                    (self.purple_text_height - 720.0).max(0.0),
                ),
            };
            if velocity == 0.0 {
                continue;
            }
            let next = (current - velocity * ticks).clamp(0.0, maximum);
            let mut remaining = 0.0;
            if (current - next).abs() >= f32::EPSILON {
                self.drag_scroll(target, velocity * ticks);
                remaining = velocity * 0.95f32.powf(ticks);
                if remaining.abs() < 0.1 {
                    remaining = 0.0;
                }
            }
            match target {
                ScrollTarget::Hint => unreachable!(),
                ScrollTarget::List => self.purple_list_velocity = remaining,
                ScrollTarget::Text => self.purple_text_velocity = remaining,
            }
        }
    }
    pub fn pointer_state_with_time(
        &mut self,
        position: Vec2,
        held: bool,
        seconds: f64,
        text_height: f32,
    ) {
        self.set_purple_text_height(text_height);
        if !matches!(
            self.menu,
            MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter | MurdererMenu::Hints
        ) || self.result.is_some()
            || self.menu_input_busy()
            || !position.is_finite()
        {
            self.scroll_pointer.target = None;
            self.scroll_pointer.held = held;
            self.pointer_state(position, held);
            return;
        }
        let was_held = self.scroll_pointer.held;
        self.scroll_pointer.held = held;
        if held && !was_held {
            let heading_hit = self.menu == MurdererMenu::PurpleByChapter
                && matches!(
                    self.purple_chapter_pointer_target(position),
                    Some(PointerTarget::Heading(_))
                );
            let target = if self.menu == MurdererMenu::Hints {
                contains(vec4(280.0, 180.0, 1360.0, 720.0), position).then_some(ScrollTarget::Hint)
            } else if heading_hit || contains(vec4(166.0, 180.0, 480.0, 720.0), position) {
                Some(ScrollTarget::List)
            } else if contains(vec4(706.0, 180.0, 1068.0, 720.0), position) {
                Some(ScrollTarget::Text)
            } else {
                None
            };
            self.scroll_pointer = ScrollPointer {
                held,
                origin: position,
                previous: position,
                displacement: Vec2::ZERO,
                dragging: false,
                target,
            };
            if target == Some(ScrollTarget::List) && self.purple_list_velocity.abs() >= f32::EPSILON
            {
                // Native971c8/97208: the first press stops a moving list;
                // it does not select or brighten the row/heading under it.
                self.purple_list_velocity = 0.0;
                self.pointer_held = held;
                self.pointer_row = None;
                self.last_pointer = Some(position);
                return;
            }
            if target == Some(ScrollTarget::Hint) && self.hint_scroll_velocity.abs() >= f32::EPSILON
            {
                // Native92ec8: first press stops inertia and captures blank list.
                self.hint_scroll_velocity = 0.0;
                self.pointer_held = held;
                self.pointer_row = None;
                self.last_pointer = Some(position);
                return;
            }
            if target == Some(ScrollTarget::Text) {
                self.purple_text_velocity = 0.0;
            }
        } else if held && self.scroll_pointer.target.is_some() {
            let first = !self.scroll_pointer.dragging
                && (position - self.scroll_pointer.origin).length() > 16.0;
            if first {
                self.scroll_pointer.dragging = true;
                if let Some(target) = self.pointer_row.take() {
                    self.cancel_pointer_flash(target);
                }
            }
            let delta = position - self.scroll_pointer.previous;
            if self.scroll_pointer.dragging {
                if first || delta.length() > 1.5 {
                    self.drag_scroll(self.scroll_pointer.target.unwrap(), delta.y);
                    self.scroll_pointer.previous = position;
                }
                self.scroll_pointer.displacement = delta;
                self.pointer_held = held;
                self.last_pointer = Some(position);
                return;
            }
        } else if !held && was_held && self.scroll_pointer.dragging {
            if seconds.is_finite() && seconds > 0.0 {
                let velocity =
                    self.scroll_pointer.displacement * (0.016683333333333335 / seconds) as f32;
                if velocity.is_finite() && velocity.length() > 20.0 {
                    match self.scroll_pointer.target {
                        Some(ScrollTarget::Hint) => self.hint_scroll_velocity = velocity.y,
                        Some(ScrollTarget::List) => self.purple_list_velocity = velocity.y,
                        Some(ScrollTarget::Text) => self.purple_text_velocity = velocity.y,
                        None => {}
                    }
                }
            }
            self.scroll_pointer.target = None;
            self.pointer_held = held;
            self.last_pointer = Some(position);
            return;
        }
        self.pointer_state(position, held);
        if !held {
            self.scroll_pointer.target = None;
        }
    }

    fn scroll_to_hint(&mut self) {
        // Native restore0x92580 centers the selected hint and rounds down to
        // a 90px row, clamped to content height minus the 720px viewport.
        let max = (self.available_hints() as f32 * 90.0 - 720.0).max(0.0);
        self.hint_scroll =
            ((self.menu_focus as f32 * 90.0 - 315.0).clamp(0.0, max) / 90.0).floor() * 90.0;
        self.hint_scroll_motion = Tweener::new(self.hint_scroll);
    }
    pub fn navigate(&mut self, delta: i32) {
        if self.result.is_some() || self.menu_input_busy() {
            return;
        }
        if self.menu == MurdererMenu::Accusation {
            // Programmatic sequence helper; production directional input uses
            // handle_controller_input and the native geometric ranking.
            let current = self.accusation_focus.unwrap_or(FIRST_SUSPECT_ENTRY);
            let old_focus = self.accusation_focus;
            self.select_accusation(
                FIRST_SUSPECT_ENTRY
                    + (current as i64 - FIRST_SUSPECT_ENTRY as i64 + i64::from(delta))
                        .rem_euclid(18) as usize,
            );
            if self.accusation_focus != old_focus {
                self.sounds.push("move");
            }
            return;
        }
        let count = match self.menu {
            MurdererMenu::Top | MurdererMenu::Replay => 6,
            MurdererMenu::Hints => self.available_hints(),
            MurdererMenu::PurpleByChapter => CHAPTERS
                .iter()
                .filter(|entry| entry.heading().is_none())
                .count(),
            MurdererMenu::PurpleByPerson => PERSON_TEXT.len(),
            _ => 0,
        };
        if count > 0 {
            let old_focus = self.menu_focus;
            let selected =
                (self.menu_focus as i64 + i64::from(delta)).rem_euclid(count as i64) as usize;
            self.change_menu_focus(selected);
            self.menu_focus = selected;
            if old_focus != self.menu_focus {
                self.sounds.push("move");
            }
            if self.menu == MurdererMenu::Hints {
                self.scroll_to_hint();
            }
            if matches!(
                self.menu,
                MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter
            ) {
                self.purple_text_scroll = 0.0;
                self.purple_text_velocity = 0.0;
                self.scroll_to_purple_entry();
            }
        }
    }
    pub fn handle_replay_controller_input(
        &mut self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
    ) -> bool {
        use crate::app::AppAction;
        if self.menu != MurdererMenu::Replay {
            return false;
        }
        if self.result.is_some() || self.menu_input_busy() {
            return true;
        }
        // Native0x8f074..0x8f0e8 calculates both coordinates from the
        // original focus, with Up over Down and Left over Right.
        let row = self.menu_focus / 3;
        let column = self.menu_focus % 3;
        let row = if state[AppAction::Up].is_clicked_or_repeated
            || state[AppAction::AnyDown].is_clicked_or_repeated
        {
            1 - row
        } else {
            row
        };
        let column = if state[AppAction::Left].is_clicked_or_repeated {
            (column + 2) % 3
        } else if state[AppAction::Right].is_clicked_or_repeated {
            (column + 1) % 3
        } else {
            column
        };
        let selected = row * 3 + column;
        if selected != self.menu_focus {
            self.change_menu_focus(selected);
            self.menu_focus = selected;
            self.sounds.push("move");
        } else if state[AppAction::Enter].is_clicked {
            self.replay_chapter(selected);
        } else if state[AppAction::Cancel].is_clicked {
            self.back();
        }
        true
    }
    pub fn handle_controller_input(
        &mut self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
    ) -> bool {
        self.handle_controller_input_with_scroll(state, 0.0, 0.0)
    }
    pub fn handle_controller_input_with_scroll(
        &mut self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
        scroll_axis: f32,
        text_height: f32,
    ) -> bool {
        use crate::app::AppAction;
        if self.menu_input_busy() {
            return true;
        }
        if self.menu == MurdererMenu::Accusation {
            if self.result.is_some() {
                return true;
            }
            let dx = if state[AppAction::Left].is_clicked_or_repeated {
                -1
            } else if state[AppAction::Right].is_clicked_or_repeated {
                1
            } else {
                0
            };
            let dy = if state[AppAction::Up].is_clicked_or_repeated {
                -1
            } else if state[AppAction::AnyDown].is_clicked_or_repeated {
                1
            } else {
                0
            };
            if dx != 0 || dy != 0 {
                let current = self.accusation_focus.unwrap_or(FIRST_SUSPECT_ENTRY);
                if let Some(selected) = murderer_grid_directional_target(current, dx, dy, true) {
                    self.select_accusation(selected);
                    self.sounds.push("move");
                }
                // Native consumes direction even when no candidate exists.
            } else if state[AppAction::Enter].is_clicked {
                self.confirm();
            } else if state[AppAction::Cancel].is_clicked {
                self.back();
            }
            return true;
        }
        if self.menu == MurdererMenu::Top {
            let selected = if state[AppAction::Up].is_clicked_or_repeated {
                (self.menu_focus + 5) % 6
            } else if state[AppAction::AnyDown].is_clicked_or_repeated {
                (self.menu_focus + 1) % 6
            } else {
                self.menu_focus
            };
            if selected != self.menu_focus {
                self.change_menu_focus(selected);
                self.menu_focus = selected;
                self.sounds.push("move");
            } else if state[AppAction::Enter].is_clicked {
                self.confirm();
            }
            // Native0x8d638 checks confirm only; Top has no local cancel.
            return true;
        }
        if matches!(
            self.menu,
            MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter
        ) {
            if self.result.is_some() || self.purple_list_scrolling() {
                return true;
            }
            let selected = if self.menu == MurdererMenu::PurpleByChapter {
                if state[AppAction::Up].is_clicked_or_repeated {
                    self.menu_focus.saturating_sub(1)
                } else if state[AppAction::AnyDown].is_clicked_or_repeated {
                    (self.menu_focus + 1).min(22)
                } else {
                    self.menu_focus
                }
            } else {
                let dx = if state[AppAction::Left].is_clicked_or_repeated {
                    -1
                } else if state[AppAction::Right].is_clicked_or_repeated {
                    1
                } else {
                    0
                };
                let dy = if state[AppAction::Up].is_clicked_or_repeated {
                    -1
                } else if state[AppAction::AnyDown].is_clicked_or_repeated {
                    1
                } else {
                    0
                };
                if dx != 0 || dy != 0 {
                    if let Some(entry) = murderer_grid_directional_target(
                        self.menu_focus + FIRST_SUSPECT_ENTRY,
                        dx,
                        dy,
                        false,
                    ) {
                        self.select_purple_controller(entry - FIRST_SUSPECT_ENTRY);
                        self.sounds.push("move");
                    }
                    // Person directions consume even a stationary edge.
                    return true;
                }
                self.menu_focus
            };
            if selected != self.menu_focus {
                self.select_purple_controller(selected);
                self.sounds.push("move");
            } else if state[AppAction::Cancel].is_clicked {
                // Purple has no controller confirm action. Chapter edges
                // fall through to Cancel, unlike person directions.
                self.back();
            } else if scroll_axis.is_finite() && scroll_axis != 0.0 {
                // Native969f4..96a60 adds32*rightY after direction/Back.
                self.scroll_purple_text(scroll_axis.clamp(-1.0, 1.0) * 32.0, text_height);
            } else if (state[AppAction::PageUp].is_clicked_or_repeated
                || state[AppAction::PageDown].is_clicked_or_repeated)
                && ![
                    AppAction::Left,
                    AppAction::Right,
                    AppAction::Up,
                    AppAction::AnyDown,
                ]
                .iter()
                .any(|action| state[*action].is_clicked_or_repeated)
            {
                // Preserve desktop transcript scrolling via ADV's existing
                // scroll_text path when no native analog axis is present.
                return false;
            }
            return true;
        }
        if self.menu != MurdererMenu::Hints {
            return self.handle_replay_controller_input(state);
        }
        // Native0x92828..0x92840 consumes all input during response phases
        // or the four-tick scroll initiated by a focus change.
        if self.result.is_some() || !self.hint_scroll_motion.is_idle() {
            return true;
        }
        let selected = if state[AppAction::Up].is_clicked_or_repeated {
            self.menu_focus.saturating_sub(1)
        } else if state[AppAction::AnyDown].is_clicked_or_repeated {
            (self.menu_focus + 1).min(self.available_hints().saturating_sub(1))
        } else {
            self.menu_focus
        };
        if selected != self.menu_focus {
            self.select_hint(selected, true);
            self.sounds.push("move");
        } else if state[AppAction::Enter].is_clicked {
            self.request_hint(selected);
        } else if state[AppAction::Cancel].is_clicked {
            self.back();
        } else if state[AppAction::KeyboardScrollUp].is_clicked_or_repeated {
            self.scroll_hint_list(-180.0);
        } else if state[AppAction::KeyboardScrollDown].is_clicked_or_repeated {
            self.scroll_hint_list(180.0);
        } else if scroll_axis.is_finite() && scroll_axis != 0.0 {
            let next = (self.hint_scroll + scroll_axis.clamp(-1.0, 1.0) * 32.0)
                .clamp(0.0, self.hint_scroll_maximum());
            self.reconcile_hint_selection(next);
            self.hint_scroll_motion = Tweener::new(next);
            self.hint_scroll = next;
            self.hint_scroll_velocity = 0.0;
        }
        true
    }
    pub fn confirm(&mut self) {
        if self.result.is_some() || self.menu_input_busy() {
            return;
        }
        match self.menu {
            MurdererMenu::Top => {
                self.sounds.push("decide");
                self.top_phase = TopPhase::Flash;
                // Native8d668..8d69c queues at speed0.2 from current
                // brightness; a held press may already have reached0.7.
                let flash = &mut self.top_row_flash[self.menu_focus];
                flash.enqueue_now(
                    0.7,
                    Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
                );
                flash.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
            }
            MurdererMenu::Replay => {
                self.replay_chapter(self.menu_focus);
            }
            MurdererMenu::Hints => {
                self.request_hint(self.menu_focus);
            }
            MurdererMenu::Accusation => {
                if let Some(entry) = self.accusation_focus {
                    self.activate_accusation_entry(entry);
                }
            }
            _ => {}
        }
    }
    pub fn pointer(&mut self, position: Vec2, clicked: bool) {
        // Complete-gesture helper; window input retains press/release edges.
        if matches!(
            self.menu,
            MurdererMenu::Top
                | MurdererMenu::Replay
                | MurdererMenu::Hints
                | MurdererMenu::Accusation
                | MurdererMenu::PurpleByPerson
                | MurdererMenu::PurpleByChapter
        ) || (self.menu != MurdererMenu::Top && contains(BACK_RECT, position))
        {
            if clicked {
                self.pointer_state_with_time(position, true, 0.0, self.purple_text_height);
            }
            self.pointer_state_with_time(position, false, 0.0, self.purple_text_height);
            return;
        }
        if self.result.is_some() || self.menu_input_busy() {
            return;
        }
        if self.menu != MurdererMenu::Top && contains(BACK_RECT, position) {
            if clicked {
                self.back_with_sound("decide");
            }
            return;
        }
        if self.menu == MurdererMenu::Accusation {
            self.accusation_pointer(position, clicked);
            return;
        }
        if self.last_pointer == Some(position) && !clicked {
            return;
        }
        let hit = match self.menu {
            MurdererMenu::PurpleByPerson => {
                (0..17).find(|&i| contains(purple_person_rect(i).unwrap(), position))
            }
            MurdererMenu::PurpleByChapter
                if contains(vec4(166.0, 180.0, 480.0, 720.0), position) =>
            {
                let index = ((position.y - 180.0 + self.purple_list_scroll) / 90.0) as usize;
                CHAPTERS
                    .iter()
                    .filter(|entry| entry.heading().is_none())
                    .position(|entry| entry.original_index == index)
            }
            MurdererMenu::Top => (0..6).find(|&i| contains(top_menu_rect(i).unwrap(), position)),
            MurdererMenu::Replay => (0..6).find(|&i| contains(replay_rect(i).unwrap(), position)),
            MurdererMenu::Hints if contains(vec4(280.0, 180.0, 1360.0, 720.0), position) => (0
                ..self.available_hints())
                .find(|&i| contains(self.hint_hit_rect(i).unwrap(), position)),
            _ => None,
        };
        if let Some(index) = hit {
            self.change_menu_focus(index);
            if self.menu_focus != index {
                self.purple_text_scroll = 0.0;
                self.purple_text_velocity = 0.0;
            }
            self.menu_focus = index;
            if clicked {
                if matches!(
                    self.menu,
                    MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter
                ) {
                    self.sounds.push("decide");
                }
                self.confirm();
            }
        }
        self.last_pointer = Some(position);
    }

    pub fn pointer_state(&mut self, position: Vec2, held: bool) {
        let was_held = self.pointer_held;
        self.pointer_held = held;
        if self.result.is_some() || self.menu_input_busy() {
            self.pointer_row = None;
            return;
        }
        let hit = if self.menu != MurdererMenu::Top && contains(BACK_RECT, position) {
            Some(PointerTarget::Back)
        } else if self.menu == MurdererMenu::PurpleByChapter {
            self.purple_chapter_pointer_target(position)
        } else {
            match self.menu {
                MurdererMenu::Top => {
                    (0..6).find(|&row| contains(top_menu_rect(row).unwrap(), position))
                }
                MurdererMenu::Replay => {
                    (0..6).find(|&row| contains(replay_rect(row).unwrap(), position))
                }
                MurdererMenu::Hints if contains(vec4(280.0, 180.0, 1360.0, 720.0), position) => (0
                    ..self.available_hints())
                    .find(|&row| contains(self.hint_hit_rect(row).unwrap(), position)),
                MurdererMenu::Accusation => accusation_hit_test(position),
                MurdererMenu::PurpleByPerson
                    if contains(vec4(166.0, 180.0, 480.0, 720.0), position) =>
                {
                    (0..17).find(|&row| {
                        !PERSON_TEXT[row].is_empty()
                            && (row != self.menu_focus
                                || self.pointer_row == Some(PointerTarget::Row(row)))
                            && contains(purple_person_rect(row).unwrap(), position)
                    })
                }
                _ => None,
            }
            .map(PointerTarget::Row)
        };
        // Other noninteractive submenu bodies retain their existing hit path.
        if !matches!(
            self.menu,
            MurdererMenu::Top
                | MurdererMenu::Replay
                | MurdererMenu::Hints
                | MurdererMenu::Accusation
                | MurdererMenu::PurpleByPerson
                | MurdererMenu::PurpleByChapter
        ) && hit.is_none()
            && self.pointer_row.is_none()
        {
            self.pointer(position, held && !was_held);
            return;
        }
        if held && !was_held {
            self.pointer_row = hit;
            if let Some(target) = hit {
                if let PointerTarget::Row(row) = target {
                    if matches!(
                        self.menu,
                        MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter
                    ) {
                        self.select_purple_controller(row);
                    } else if self.menu == MurdererMenu::Accusation {
                        self.select_accusation(row);
                    } else if self.menu == MurdererMenu::Hints {
                        self.select_hint(row, true);
                    } else {
                        self.change_menu_focus(row);
                        self.menu_focus = row;
                    }
                }
                let flash = self.pointer_flash(target);
                flash.enqueue_now(
                    0.7,
                    Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
                );
            }
        } else if !held && was_held {
            if let Some(pressed) = self.pointer_row.take() {
                if let Some(target) = hit {
                    if pressed != target {
                        self.cancel_pointer_flash(pressed);
                    }
                    match target {
                        PointerTarget::Row(row) => {
                            if matches!(
                                self.menu,
                                MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter
                            ) {
                                self.select_purple_controller(row);
                            } else if self.menu == MurdererMenu::Accusation {
                                self.select_accusation(row);
                            } else if self.menu == MurdererMenu::Hints {
                                self.select_hint(row, true);
                            } else {
                                self.change_menu_focus(row);
                                self.menu_focus = row;
                            }
                            if matches!(
                                self.menu,
                                MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter
                            ) {
                                let flash = if self.menu == MurdererMenu::PurpleByPerson {
                                    &mut self.purple_person_flash[row]
                                } else {
                                    &mut self.purple_chapter_flash[row]
                                };
                                flash.enqueue_now(
                                    0.7,
                                    Tween::linear(Ticks::from_f32(
                                        (0.7 - flash.value()).abs() / 0.2,
                                    )),
                                );
                                flash.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
                                self.sounds.push("decide");
                            } else {
                                self.confirm();
                            }
                        }
                        PointerTarget::Heading(heading) => self.release_purple_heading(heading),
                        PointerTarget::Back => self.back_with_sound("decide"),
                    }
                } else {
                    self.cancel_pointer_flash(pressed);
                }
            }
        } else if held && hit.is_none() {
            if let Some(target) = self.pointer_row.take() {
                self.cancel_pointer_flash(target);
            }
        }
        self.last_pointer = Some(position);
    }
    fn pointer_flash(&mut self, target: PointerTarget) -> &mut Tweener {
        match target {
            PointerTarget::Heading(heading) => &mut self.purple_heading_flash[heading],
            PointerTarget::Row(row) if self.menu == MurdererMenu::PurpleByChapter => {
                &mut self.purple_chapter_flash[row]
            }
            PointerTarget::Back => &mut self.back_feedback,
            PointerTarget::Row(row) if self.menu == MurdererMenu::PurpleByPerson => {
                &mut self.purple_person_flash[row]
            }
            PointerTarget::Row(row) if self.menu == MurdererMenu::Accusation => {
                &mut self.accusation_row_flash[row - FIRST_SUSPECT_ENTRY]
            }
            PointerTarget::Row(row) if self.menu == MurdererMenu::Replay => {
                &mut self.replay_row_flash[row]
            }
            PointerTarget::Row(row) if self.menu == MurdererMenu::Hints => {
                &mut self.hint_row_flash[row]
            }
            PointerTarget::Row(row) => &mut self.top_row_flash[row],
        }
    }
    fn cancel_pointer_flash(&mut self, target: PointerTarget) {
        // Native leave/reset (+78/+88) clears the queue and targets0
        // at speed0.2. The +70 callback has distinct Back hover feedback.
        let flash = self.pointer_flash(target);
        flash.enqueue_now(0.0, Tween::linear(Ticks::from_f32(flash.value() / 0.2)));
    }
    pub fn ignore_pointer_state(&mut self, held: bool) {
        self.scroll_pointer.held = held;
        self.scroll_pointer.target = None;
        if let Some(target) = self.pointer_row.take() {
            if self.result.is_none() && !self.menu_input_busy() {
                self.cancel_pointer_flash(target);
            }
        }
        self.pointer_held = held;
    }

    /// HintMenu completion 0x91828 returns the original index plus six.
    pub fn request_hint(&mut self, index: usize) -> bool {
        if self.result.is_some()
            || self.menu_input_busy()
            || self.menu != MurdererMenu::Hints
            || index >= self.available_hints()
        {
            return false;
        }
        self.select_hint(index, false);
        self.sounds.push("decide");
        self.result = Some(index as i32 + 6);
        let flash = std::mem::replace(&mut self.hint_row_flash[index], Tweener::new(0.0));
        let completion = ResponseCompletion::from_flash(self.menu, flash);
        self.hint_row_flash[index] = completion.flash.clone();
        self.response_completion = Some(completion);
        true
    }

    pub fn suspect_selected(&self, entry: usize) -> bool {
        entry
            .checked_sub(FIRST_SUSPECT_ENTRY)
            .and_then(|index| self.suspects.get(index))
            .copied()
            .unwrap_or(false)
    }

    pub fn accusation_pointer(&mut self, position: Vec2, clicked: bool) {
        if self.menu == MurdererMenu::Accusation {
            self.pointer(position, clicked);
        }
    }

    /// Select input 0x9a650 toggles suspect entries and submits entry 45.
    pub fn activate_accusation_entry(&mut self, entry: usize) -> bool {
        if self.result.is_some()
            || self.menu_input_busy()
            || self.menu != MurdererMenu::Accusation
            || !(FIRST_SUSPECT_ENTRY..=SUBMIT_ENTRY).contains(&entry)
        {
            return false;
        }
        self.select_accusation(entry);
        self.flash_accusation(entry);
        if entry == SUBMIT_ENTRY {
            self.sounds.push("attack");
            // Validation 0x9a68c..0x9a700. Every required entry must be
            // selected; any other selected suspect invalidates the accusation.
            let correct = self.suspects.iter().enumerate().all(|(index, selected)| {
                if SUSPECT_FLAGS[index] & 0x40 != 0 && *selected {
                    true
                } else {
                    !(34..37).contains(&(FIRST_SUSPECT_ENTRY + index)) && !selected
                }
            });
            // Completion 0x98700/0x98910. Do not clamp the lower bound:
            // the native code uses signed min(progress,20), including negatives.
            self.completion = Some(AccusationCompletion::SubmitFlash { correct });
            self.result = Some(if correct {
                27
            } else {
                self.hint_progress.min(20).wrapping_add(28)
            });
            return true;
        }
        if let Some(selected) = entry
            .checked_sub(FIRST_SUSPECT_ENTRY)
            .and_then(|index| self.suspects.get_mut(index))
        {
            *selected = !*selected;
            self.sounds.push("decide");
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        let serial = self.wipe_serial;
        *self = Self::new(self.hint_progress);
        self.wipe_serial = serial;
    }
}

#[cfg(test)]
#[path = "murderer_fast_forward_tests.rs"]
mod fast_forward_tests;

#[cfg(test)]
mod tests {

    #[test]
    fn completed_murderer_restore_keeps_artwork_separate_from_parent_response() {
        for answer in 0..6 {
            let mut model = MurdererModel::new(20);
            model.restore_completed_answer(answer);
            let before = model.replay_expansion();
            model.navigate(1);
            model.confirm();
            model.back();
            model.pointer(Vec2::new(400.0, 300.0), true);
            model.update(120.0);
            assert_eq!(model.poll_result(), None);
            assert_eq!(model.replay_expansion(), before);
            assert_eq!(
                before,
                Some((answer as usize, vec4(0.0, 0.0, 1920.0, 1080.0), 1.0))
            );
            assert!(model.take_sounds().is_empty());
            assert!(model.allows_playback_mode_changes());
        }
        let mut hints = MurdererModel::new(20);
        hints.restore_completed_answer(19); // Original row13, not response19.
        assert_eq!(hints.menu, MurdererMenu::Hints);
        assert_eq!(hints.menu_focus, 13);
        assert_eq!(hints.hint_scroll, 810.0);
        assert_eq!(hints.hint_feedback(13), (1.0, 0.0));
        assert_eq!(hints.poll_result(), None);
        assert!(
            hints.request_hint(13),
            "Native restored Hints phase remains input-enabled"
        );
        assert_eq!(hints.poll_result(), None);
        hints.update(7.0);
        hints.update(60.0);
        hints.update(0.0);
        assert_eq!(hints.poll_result(), Some(19));
        for answer in [27, 48] {
            let mut accusation = MurdererModel::new(20);
            accusation.restore_completed_answer(answer);
            assert_eq!(accusation.accusation_focus, Some(SUBMIT_ENTRY));
            assert_eq!(
                accusation.battler_sprite_state(),
                if answer == 27 { 1 } else { 2 }
            );
            assert_eq!(accusation.white_flash_opacity(), 0.0);
            assert_eq!(accusation.poll_result(), None);
            assert!(accusation.take_sounds().is_empty());
            if answer == 27 {
                assert!(accusation.activate_accusation_entry(28));
            } else {
                for entry in 34..37 {
                    assert!(accusation.activate_accusation_entry(entry));
                }
            }
            assert!(accusation.activate_accusation_entry(SUBMIT_ENTRY));
            assert_eq!(
                accusation.battler_sprite_state(),
                if answer == 27 { 1 } else { 2 },
                "Submission retains restored expression until the native completion phase changes it"
            );
            for _ in 0..130 {
                accusation.update(1.0);
            }
            assert_eq!(
                accusation.poll_result(),
                Some(if answer == 27 { 48 } else { 27 })
            );
            assert_eq!(
                accusation.battler_sprite_state(),
                if answer == 27 { 2 } else { 1 }
            );
        }
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "executes guarded original/1.03 Murderer restore callbacks with bounded resource constructors"]
    fn reference_native_completed_murderer_restoration() {
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary = std::env::temp_dir().join(format!(
            "shin-murderer-restore-oracle-{}",
            std::process::id()
        ));
        let compile = Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra"])
            .arg(root.join("reverse-engineering/native-murderer-restoration.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let folder = std::path::Path::new(&rom).parent().unwrap();
        let mut requests = String::new();
        for answer in (-1..=49).chain([99]) {
            for progress in (-1..=20).chain([99]) {
                for prior in [-1, 32] {
                    requests += &format!("{answer} {progress} {prior}\n");
                }
            }
        }
        for name in ["uncompressed_executable", "uncompressed_executable_update"] {
            let exe = folder.join(name);
            let mut child = Command::new(&binary)
                .arg(&exe)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            let input = requests.clone();
            let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
            let output = child.wait_with_output().unwrap();
            writer.join().unwrap();
            assert!(
                output.status.success(),
                "{name}:{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = String::from_utf8(output.stdout).unwrap();
            assert_eq!(output.lines().count(), 2392);
            for line in output.lines() {
                let v: Vec<f32> = line
                    .split_whitespace()
                    .map(|s| s.parse().unwrap())
                    .collect();
                let answer = v[0] as i32;
                let prior = (v[2] >= 0.0).then_some(v[2] as i32);
                let mut model = MurdererModel::new(v[1] as i32);
                model.result = prior;
                model.restore_completed_answer(answer);
                assert_eq!(
                    model.poll_result(),
                    prior,
                    "{name}:parent response preserved {line}"
                );
                assert_eq!(model.poll_result().is_some(), v[3] != 0.0);
                assert_eq!(model.result.unwrap_or(-1), v[4] as i32);
                if v[5] == 0.0 {
                    assert_eq!(model.menu, MurdererMenu::Top);
                    assert_eq!(v[12], 0.0);
                    continue;
                }
                assert_eq!(
                    v[12], 1.0,
                    "Native replaces and destroys previous subview exactly once"
                );
                assert_eq!(model.back_flash(), v[9]);
                assert_eq!(
                    model.allows_playback_mode_changes(),
                    *v.last().unwrap() != 0.0,
                    "{name}:native mode gate {line}"
                );
                assert_eq!(
                    !model.menu_input_busy(),
                    v[8] != 0.0,
                    "{name}:input phase {line}"
                );
                match v[5] as i32 {
                    1 => {
                        assert_eq!(model.menu, MurdererMenu::Replay);
                        assert_eq!(v[6], 3.0);
                        assert_eq!(model.menu_focus, v[7] as usize);
                        assert_eq!(
                            model.replay_expansion(),
                            Some((v[7] as usize, vec4(0.0, 0.0, 1920.0, 1080.0), v[10]))
                        );
                    }
                    2 => {
                        assert_eq!(model.menu, MurdererMenu::Hints);
                        assert_eq!(v[6], 0.0);
                        assert_eq!(model.menu_focus, v[7] as usize);
                        assert_eq!(model.hint_scroll, v[10]);
                    }
                    3 => {
                        assert_eq!(model.menu, MurdererMenu::Accusation);
                        assert_eq!(v[6], 0.0);
                        assert_eq!(
                            model.accusation_focus,
                            Some(FIRST_SUSPECT_ENTRY + v[7] as usize)
                        );
                        assert_eq!(model.battler_sprite_state(), v[11] as usize);
                        assert_eq!(model.white_flash_opacity(), v[10]);
                    }
                    _ => panic!(),
                }
                for row in 0..v[13] as usize {
                    let actual = match model.menu {
                        MurdererMenu::Replay => model.replay_feedback(row),
                        MurdererMenu::Hints => model.hint_feedback(row),
                        MurdererMenu::Accusation => {
                            model.accusation_feedback(FIRST_SUSPECT_ENTRY + row)
                        }
                        _ => unreachable!(),
                    };
                    assert_eq!(
                        actual,
                        (v[14 + row * 3], v[15 + row * 3]),
                        "{name}:row{row} {line}"
                    );
                    assert_eq!(
                        model.suspect_selected(FIRST_SUSPECT_ENTRY + row)
                            && model.menu == MurdererMenu::Accusation,
                        v[16 + row * 3] != 0.0,
                        "{name}:suspect{row} {line}"
                    );
                }
                assert!(model.take_sounds().is_empty());
            }
            for address in [0x9b7a0, 0x8ec20, 0x923b0, 0x99830, 0xbb2e0] {
                let address = if name.ends_with("update") {
                    address - 0xa0
                } else {
                    address
                };
                let mut bytes = std::fs::read(&exe).unwrap();
                bytes[address + 256] ^= 1;
                let altered = std::env::temp_dir().join(format!(
                    "shin-murderer-restore-altered-{}",
                    std::process::id()
                ));
                std::fs::write(&altered, bytes).unwrap();
                let rejected = Command::new(&binary).arg(&altered).output().unwrap();
                std::fs::remove_file(altered).unwrap();
                assert_eq!(rejected.status.code(), Some(2));
                assert!(String::from_utf8_lossy(&rejected.stderr).contains("Unreviewed"));
            }
            eprintln!(
                "{name}:2392 native completed Murderer cases: dispatch/resource replacement, retained response, focus/flash/scroll/expansion/suspects/Battler/input; altered dispatch/children/reset rejected"
            );
        }
        std::fs::remove_file(binary).unwrap();
    }
    #[test]
    fn purple_person_red_portraits_match_native_clear_mask_bits() {
        let red: Vec<_> = (0..17)
            .filter(|&index| purple_person_is_red(index))
            .collect();
        assert_eq!(red, [3, 4, 6, 7, 9, 12]);
        for index in 0..17 {
            assert_eq!(
                purple_person_is_red(index),
                (0x3ed27fffffff_u64 >> (index + 28)) & 1 == 0
            );
        }
    }
    #[test]
    #[ignore = "requires supplied original and updated executables"]
    fn reference_native_purple_portrait_red_mask_and_color() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let folder = std::path::Path::new(&rom).parent().unwrap();
        for (name, offset) in [
            ("uncompressed_executable", 0),
            ("uncompressed_executable_update", 0xa0),
        ] {
            let exe = std::fs::read(folder.join(name)).unwrap();
            for (a, value) in [
                (0x95bd0, 0xb2407bf9u32),
                (0x95bdc, 0xf2c7da59),
                (0x95be0, 0x129e1e1a),
                (0x95cc8, 0x9adc2728),
                (0x95ccc, 0xf240011f),
                (0x95cd0, 0x5a9f0341),
            ] {
                assert_eq!(
                    u32::from_le_bytes(exe[a - offset + 256..a - offset + 260].try_into().unwrap()),
                    value,
                    "{name} {a:x}"
                );
            }
        }
    }

    use super::*;

    fn finish_back(model: &mut MurdererModel) {
        if model.back_completion.is_some() {
            for delta in [7.0, 0.0, 30.0] {
                model.update(delta);
            }
            assert_eq!(model.menu, MurdererMenu::Top);
            assert!(!model.menu_input_busy());
        }
    }
    fn finish_top_opening(model: &mut MurdererModel) {
        for delta in [7.0, 0.0, 30.0, 0.0, 60.0] {
            model.update(delta);
        }
        assert!(!model.menu_input_busy());
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_overlay_playback_policy() {
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
            (0x11190, 0xf9402d08),
            (0x11194, 0xd63f0100),
            (0x11198, 0x37000060),
            (0x1119c, 0x2a1f03f4),
            (0x111a0, 0xb9047e7f),
            (0x9ba08, 0xf9400008),
            (0x9ba0c, 0xf9403908),
            (0x9ba10, 0xd63f0100),
            (0x9ba14, 0x2a2003e8),
            (0x9ba18, 0x12000100),
            (0x9ba70, 0xf9401008),
            (0x9ba74, 0xf9402500),
            (0x9ba78, 0xb4000080),
            (0x9ba80, 0xf9401901),
            (0x9ba84, 0xd61f0020),
            (0x8d4d0, 0x320003e0),
            (0x902e0, 0x320003e0),
            (0x967b0, 0x320003e0),
            (0x8ec00, 0xb9401c08),
            (0x8ec04, 0x121f7908),
            (0x8ec08, 0x7100091f),
            (0x8ec0c, 0x1a9f07e0),
            (0x92390, 0xb9408c08),
            (0x92394, 0x121f7908),
            (0x92398, 0x7100091f),
            (0x9239c, 0x1a9f07e0),
            (0x99810, 0xb9401008),
            (0x99814, 0x51000908),
            (0x99818, 0x7100091f),
            (0x9981c, 0x1a9f97e0),
            (0x8e5cc, 0xb9005928),
            (0x8e5d0, 0xb9401e68),
            (0x8e5d4, 0x11000508),
            (0x91828, 0xb9005928),
            (0x9182c, 0xb9408e68),
            (0x91830, 0x11000508),
            (0x8f1fc, 0xd0000648),
            (0x8f200, 0xbd45a500),
            (0x8f208, 0xbd497508),
            (0x8f218, 0x9400b1fa),
            (0x8f22c, 0x9400b1f5),
            (0x8f230, 0x320003e8),
            (0x8f234, 0xb9001e68),
            (0x8e544, 0x9400b52b),
            (0x8e548, 0x370008c0),
            (0x8e560, 0x1e27d00a),
            (0x8e584, 0x91024260),
            (0x8e588, 0x321903e1),
            (0x8e590, 0x9400b51c),
            (0x8e598, 0xbd479901),
            (0x8e5a0, 0x321f03e1),
            (0x8e5a8, 0x9400b516),
            (0x8e5b4, 0xb9001e68),
            (0x8e5bc, 0x9400b50d),
            (0x8e5c0, 0x37000500),
            (0x929bc, 0xbd45a500),
            (0x929c4, 0xbd497508),
            (0x929d4, 0x9400a40b),
            (0x929ec, 0x9400a405),
            (0x929f4, 0xbd479901),
            (0x92a08, 0x9400a3fe),
            (0x92a10, 0xb9008e68),
            (0x917fc, 0x9400a87d),
            (0x91800, 0x37000640),
            (0x9180c, 0xb9008e68),
            (0x91814, 0x9400a877),
            (0x91818, 0x37000580),
            (0x91828, 0xb9005928),
            (0x8ea1c, 0x51000908),
            (0x8ea20, 0x71001d1f),
            (0x8ea24, 0x54000cc8),
            (0x8ea2c, 0x91024260),
            (0x8ea5c, 0xf8697901),
            (0x8ea84, 0xf8697901),
            (0x8eaac, 0x1e202860),
            (0x8eacc, 0x1e230903),
            (0x8eadc, 0x1e223863),
            (0x8eae0, 0x1e230904),
            (0x8eaf0, 0x1e230903),
            (0x8eafc, 0x1e242863),
            (0x8eb0c, 0x1e240904),
            (0x8eb10, 0x1e252884),
            (0x8eb1c, 0x1e253800),
            (0x8eb20, 0x1e263841),
            (0x8eb84, 0x97ff1eeb),
            (0x8eba0, 0x97ff1eec),
            (0x8ebb8, 0x9400e16e),
            (0xc7258, 0x321f07e0),
            (0xc7270, 0x320003e0),
            (0xc7278, 0x321c03e2),
            (0xc7280, 0x321f03e0),
            (0xc7288, 0x321c03e2),
            (0x8e844, 0x910062a0),
            (0x8e84c, 0x9101c2a0),
            (0x8e850, 0x4ea01c0a),
            (0x8e85c, 0x4ea01c0b),
            (0x8e8f0, 0x4eab1d63),
            (0x8e8f4, 0x97ff1f97),
            (0x8e908, 0x9400e182),
            (0x8e90c, 0x1e202148),
            (0x8e914, 0x321f03e0),
            (0x8e91c, 0x1e2c1000),
            (0x8e920, 0x1e200943),
            (0x8e924, 0x4ea91d20),
            (0x8e930, 0x97ff1f88),
            (0x8e950, 0x910183e0),
            (0x8f184, 0x91006100),
            (0x8f190, 0xbd457101),
            (0x8f1a0, 0x9400b218),
            (0x8f1ac, 0x91006100),
            (0x8f1b8, 0x1e2e1000),
            (0x8f1bc, 0x1e2a1001),
            (0x8f1c8, 0x9400b20e),
            (0x8e560, 0x1e27d00a),
            (0x8e56c, 0x91006100),
            (0x8e578, 0x9400b522),
            (0x8f040, 0xb9401c08),
            (0x8f044, 0x34000068),
            (0x8f074, 0xb9801268),
            (0x8f080, 0x9b097d09),
            (0x8f084, 0xb94013eb),
            (0x8f094, 0x7100091f),
            (0x8f098, 0x0b09052a),
            (0x8f09c, 0x1a89c7ec),
            (0x8f0a0, 0x7207017f),
            (0x8f0a4, 0x1a8c012c),
            (0x8f0ac, 0x71000d1f),
            (0x8f0b0, 0x4b0a010a),
            (0x8f0b4, 0x1a9fa529),
            (0x8f0b8, 0x7208017f),
            (0x8f0bc, 0x1a890189),
            (0x8f0c0, 0x7100055f),
            (0x8f0c4, 0x1a8ac7ec),
            (0x8f0c8, 0x7205017f),
            (0x8f0cc, 0x1a8c014c),
            (0x8f0d0, 0x7100054a),
            (0x8f0d4, 0x321f03ed),
            (0x8f0d8, 0x1a8ab1aa),
            (0x8f0dc, 0x7206017f),
            (0x8f0e0, 0x0b090529),
            (0x8f0e4, 0x1a8a018a),
            (0x8f0e8, 0x0b090154),
            (0x8f0ec, 0x6b14011f),
            (0x8f0f0, 0x54000341),
            (0x8f0f4, 0xb9400fe8),
            (0x8f0f8, 0x37e006c8),
            (0x8f0fc, 0x36e809e8),
            (0x92828, 0xb9408c08),
            (0x9282c, 0x350015a8),
            (0x92838, 0xaa1403e0),
            (0x9283c, 0x9400a46d),
            (0x92840, 0x37001500),
            (0x92854, 0x37c00309),
            (0x92858, 0x36c80369),
            (0x92874, 0x11000515),
            (0x92878, 0x6b0902bf),
            (0x92880, 0x51000535),
            (0x92888, 0x540001e0),
            (0x928a8, 0x320003e2),
            (0x928ac, 0x94000095),
            (0x928b0, 0x1400008c),
            (0x928b4, 0x51000509),
            (0x928b8, 0x0aa97d35),
            (0x928c0, 0x54fffe61),
            (0x928c8, 0x37e00628),
            (0x928cc, 0x37e80a68),
            (0x92b84, 0x360005d4),
            (0x92b90, 0x9400a224),
            (0x92ba4, 0x52800b48),
            (0x92bb0, 0x1e203823),
            (0x92bbc, 0x710002df),
            (0x92bc0, 0x1e21bd00),
            (0x92bd8, 0x1e222863),
            (0x92bdc, 0x1e242060),
            (0x92be0, 0x5400022d),
            (0x92c10, 0x1e222821),
            (0x92c14, 0x1e232821),
            (0x92c18, 0x1e232800),
            (0x92c1c, 0x1e202020),
            (0x92c20, 0x1e21cc00),
            (0x92c24, 0x1e221001),
            (0x92c34, 0x9400a373),
            (0x8d550, 0xb9401808),
            (0x8d554, 0x34000068),
            (0x8d580, 0xb9401268),
            (0x8d584, 0xb94013e9),
            (0x8d588, 0x37c004e9),
            (0x8d58c, 0x36c80569),
            (0x8d590, 0x7100111f),
            (0x8d594, 0x1a88c7f4),
            (0x8d598, 0x6b14011f),
            (0x8d59c, 0x540004e0),
            (0x8d5b8, 0x6b14011f),
            (0x8d5bc, 0x54000780),
            (0x8d5f0, 0xb9001274),
            (0x8d624, 0x71000509),
            (0x8d628, 0x528000aa),
            (0x8d62c, 0x1a89b154),
            (0x8d630, 0x6b14011f),
            (0x8d634, 0x54fffb61),
            (0x8d638, 0x39403fe8),
            (0x8d63c, 0x36200388),
            (0x8d650, 0x97fe6718),
        ] {
            assert_eq!(word(address), expected, "Native policy{address:x}");
        }
        for (address, value) in [(0x1595a4, 0.7f32), (0x159974, 0.2), (0x159798, 60.0)] {
            assert_eq!(word(address), value.to_bits());
        }
        for (address, value) in [
            (0x159624, 960.0f32),
            (0x15954c, 540.0),
            (0x159520, 1376.0),
            (0x159448, 544.0),
            (0x159704, 774.0),
            (0x159a98, 306.0),
        ] {
            assert_eq!(word(address), value.to_bits());
        }
        assert_eq!(word(0x159570), 0.1f32.to_bits());
        for (address, value) in [(0x159980, 720.0f32), (0x1599e0, 90.0), (0x1593a8, -720.0)] {
            assert_eq!(word(address), value.to_bits());
        }
        let relocations: std::collections::HashMap<_, _> = exe[0x11e140..0x152fb8]
            .chunks_exact(24)
            .map(|entry| {
                (
                    u64::from_le_bytes(entry[..8].try_into().unwrap()),
                    u64::from_le_bytes(entry[16..].try_into().unwrap()),
                )
            })
            .collect();
        for (index, chapter) in ["1", "2", "4", "56", "7", "8"].into_iter().enumerate() {
            for (table, suffix) in [(0x1d9730, ""), (0x1d9760, "_s")] {
                let address = relocations[&(table + index as u64 * 8)] as usize + 256;
                let expected = format!("thumb_{chapter}{suffix}");
                assert_eq!(&exe[address..address + expected.len()], expected.as_bytes());
            }
        }
        for (vtable, method, name) in [
            (0x1d95b0, 0x8d4d0, "N4shin13MurdererLayer7TopMenuE"),
            (0x1d9690, 0x8ec00, "N4shin13MurdererLayer10ReplayMenuE"),
            (0x1d9790, 0x902e0, "N4shin13MurdererLayer4RuleE"),
            (0x1d9830, 0x92390, "N4shin13MurdererLayer8HintMenuE"),
            (0x1d98d0, 0x967b0, "N4shin13MurdererLayer5CheckE"),
            (0x1d9a00, 0x99810, "N4shin13MurdererLayer6SelectE"),
        ] {
            assert_eq!(relocations[&(vtable + 0x10 + 0x30)], method);
            let typeinfo = relocations[&(vtable + 8)];
            let name_address = relocations[&(typeinfo + 8)] as usize + 256;
            assert_eq!(
                &exe[name_address..name_address + name.len()],
                name.as_bytes()
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_murderer_top_transition_lifecycle() {
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
            // Bernkastel body rectangles, face variants, and horizontal offset.
            (0x8ce90, 0x2d402aa0),
            (0x8cea0, 0xb9809268),
            (0x8ceac, 0xd37be908),
            (0x8ceb0, 0xf8686ac1),
            (0x8ceb4, 0x1e2c280b),
            (0x8cf34, 0x2d422aa0),
            (0x8cf44, 0xb9809268),
            (0x8cfcc, 0x2d442aa0),
            (0x8cfdc, 0xb9809268),
            (0x8d074, 0xb9809268),
            (0x8d3d8, 0xbd492900),
            (0x8d3e0, 0xbd434508),
            (0x8d3e8, 0xbd475d02),
            (0x8d3f0, 0xbd437d03),
            (0x8d404, 0x2d000500),
            (0x8d408, 0x2d010d02),
            (0x8d410, 0xbd4ad500),
            (0x8d418, 0xbd487509),
            (0x8d420, 0xbd43490a),
            (0x8d43c, 0x2d000500),
            (0x8d440, 0x2d010d02),
            (0x8d448, 0xbd4ad101),
            (0x8d450, 0xbd49a902),
            (0x8d458, 0xbd442503),
            (0x8d46c, 0x2d000500),
            (0x8d470, 0x2d010d02),
            (0x8d478, 0xbd492d00),
            (0x8d490, 0x2d080680),
            (0x8d494, 0x2d090e82),
            // Capture delegates both passes; Top flash fills white after plate and text.
            (0x9b404, 0xf9402680),
            (0x9b410, 0xf9401108),
            (0x9b414, 0x12000262),
            (0x9b41c, 0xd63f0100),
            (0x8cdc0, 0x36001a14),
            (0x8d188, 0x910062c0),
            (0x8d18c, 0x9400b8c5),
            (0x8d190, 0x9101c2c0),
            (0x8d198, 0x9400b8c2),
            (0x8d330, 0x1e202108),
            (0x8d334, 0x540002ad),
            (0x8d350, 0x1e2e1000),
            (0x8d35c, 0x4ea81d03),
            (0x8d384, 0x9400e673),
            // Top phase checks precede tween updates; accusation first fades Top.
            (0x8c674, 0x7100091f),
            (0x8c678, 0x540004e0),
            (0x8c67c, 0x7100051f),
            (0x8c680, 0x54001901),
            (0x8c694, 0x9101c100),
            (0x8c698, 0x9400bcd6),
            (0x8c69c, 0x37001820),
            (0x8c6a4, 0x7100151f),
            (0x8c6b0, 0x1e249000),
            (0x8c6bc, 0x97ff5759),
            (0x8c6c8, 0x94000126),
            (0x8c6ec, 0x1e27d001),
            (0x8c700, 0xb9009268),
            (0x8c704, 0x9400bcbf),
            (0x8c710, 0xb9001a68),
            (0x8c718, 0x91010108),
            (0x8c71c, 0xc8dffd08),
            (0x8c720, 0xb5001408),
            (0x8c728, 0x9400bcb2),
            (0x8c72c, 0x370013a0),
            // Five ordinary children use DefaultWiper30; accusation uses GlassWiper60.
            (0x8c758, 0x1e27d000),
            (0x8c764, 0x97ff572f),
            (0x8c770, 0x940000fc),
            (0x8c7a4, 0x940004f3),
            (0x8c7b0, 0x1e27d000),
            (0x8c7bc, 0x97ff5719),
            (0x8c7c8, 0x940000e6),
            (0x8c7fc, 0x94000fd5),
            (0x8c808, 0x1e27d000),
            (0x8c814, 0x97ff5703),
            (0x8c820, 0x940000d0),
            (0x8c858, 0x94001b1a),
            (0x8c864, 0x1e27d000),
            (0x8c870, 0x97ff56ec),
            (0x8c87c, 0x940000b9),
            (0x8c8b4, 0x94001b03),
            (0x8c8c0, 0x1e27d000),
            (0x8c8cc, 0x97ff56d5),
            (0x8c8d8, 0x940000a2),
            (0x8c90c, 0x94000c15),
            (0x8c92c, 0xbd479900),
            (0x8c93c, 0x97ff7c0d),
            (0x8c948, 0x94000086),
            (0x8c97c, 0x94002c15),
            (0x8c984, 0xf9002a95),
            (0x8c99c, 0xb9001a68),
            (0x8c9a8, 0x9400bc2a),
            // Capture allocates a texture, clears it, renders both passes and stores the wiper.
            (0x8cb78, 0x9100c016),
            (0x8cb88, 0xb5000448),
            (0x8cbb8, 0x94010e7a),
            (0x8cc34, 0x940109a7),
            (0x8cc3c, 0x321f07e0),
            (0x8cc40, 0x32081fe1),
            (0x8cc48, 0x94010dee),
            (0x8cc78, 0x320003e2),
            (0x8cc7c, 0x320003e3),
            (0x8cc88, 0x94003996),
            (0x8cc94, 0x320003e2),
            (0x8cc98, 0x2a1f03e3),
            (0x8cc9c, 0x94003991),
            (0x8cca0, 0x94010a68),
            (0x8cca8, 0x91010288),
            (0x8ccb0, 0xc809fd00),
            // Parent installs pending child before child/wiper update and releases capture at completion.
            (0x9b170, 0xf9402a68),
            (0x9b178, 0xb40000e8),
            (0x9b180, 0xa904fe68),
            (0x9b194, 0xf9402660),
            (0x9b1a0, 0xf9400d08),
            (0x9b1a8, 0xd63f0100),
            (0x9b1ac, 0x91010275),
            (0x9b1c0, 0xf9400908),
            (0x9b1cc, 0xd63f0100),
            (0x9b1d8, 0xf9401908),
            (0x9b1dc, 0xd63f0100),
            (0x9b1e0, 0x370002a0),
            (0x9b1e8, 0xc808febf),
            (0x9b200, 0x9100c268),
            (0x9b208, 0xc809fd1f),
            // Draw passes live+38 as x1, frozen+30 as x2; Default binds frozen to slot0.
            (0x9b4e8, 0x9100e268),
            (0x9b4f4, 0x9100c268),
            (0x9b508, 0xf9402108),
            (0x9b50c, 0xaa0003e2),
            (0x9b514, 0xaa1503e1),
            (0x9b518, 0xd63f0100),
            (0x62718, 0x2a1f03e0),
            (0x6271c, 0xaa0203e1),
            (0x62724, 0x9401b427),
            (0x62728, 0x320003e0),
            (0x6272c, 0xaa1303e1),
            (0x62734, 0x9401b423),
            (0x6276c, 0x9402e295),
            (0x6277c, 0xbd0043e0),
            (0x627b4, 0x52800480),
            (0x627bc, 0x320003e0),
            // Controller dispatch is unavailable while parent has a wiper.
            (0x9bac4, 0x91010108),
            (0x9bac8, 0xc8dffd08),
            (0x9bacc, 0xb4000068),
            (0x9bad0, 0x2a1f03e0),
            (0x9badc, 0xf9402500),
            (0x9bae8, 0xf9402901),
            // Shared wrapper deduplicates frame IDs before dispatching subclass update.
            (0x6aa5c, 0x94024411),
            (0x6aa60, 0x91002289),
            (0x6aa64, 0x885ffd28),
            (0x6aa68, 0x880afd20),
            (0x6aa70, 0x6b00011f),
            (0x6aa74, 0x540000c1),
            (0x6aa78, 0x2a1f03e0),
            (0x6aa90, 0xf9400d08),
            (0x6aa98, 0x12000261),
            (0x6aaa0, 0xd63f0100),
            // Nonpositive duration starts complete; positive duration advances only when permitted.
            (0x11afdc, 0x1e202108),
            (0x11afe0, 0x1e2e1000),
            (0x11afe4, 0x1e2703e1),
            (0x11aff0, 0x1e208c21),
            (0x11aff4, 0x1e208d00),
            (0x11b010, 0xbd001e61),
            (0x11b014, 0xbd002260),
            (0x11b1b0, 0x2d438400),
            (0x11b1b4, 0x1e212000),
            (0x11b1b8, 0x1a9f57e0),
            (0x11b1c4, 0x1e211800),
            (0x11b1e0, 0x360000e1),
            (0x11b1e8, 0x1e202840),
            (0x11b1f4, 0x5400004d),
            (0x11b1f8, 0xbd001c01),
        ] {
            assert_eq!(word(address), expected, "Native transition {address:x}");
        }
        for (address, value) in [
            (0x159928, 259.0f32),
            (0x159344, 256.0),
            (0x15975c, 352.0),
            (0x15937c, 160.0),
            (0x159ad4, 147.0),
            (0x159874, 416.0),
            (0x159348, 208.0),
            (0x159ad0, 624.0),
            (0x1599a8, 672.0),
            (0x159424, 456.0),
            (0x15992c, 355.0),
            (0x159700, 239.0),
        ] {
            assert_eq!(word(address), value.to_bits());
        }
        assert_eq!(word(0x159798), 60.0f32.to_bits());
        let relocations: std::collections::HashMap<_, _> = exe[0x11e140..0x152fb8]
            .chunks_exact(24)
            .map(|entry| {
                (
                    u64::from_le_bytes(entry[..8].try_into().unwrap()),
                    u64::from_le_bytes(entry[16..].try_into().unwrap()),
                )
            })
            .collect();
        for (index, name) in [
            "Ber_Body1",
            "Ber_Body2",
            "Ber_Body3",
            "Ber_Akuwarai2",
            "Ber_Body1",
            "Ber_Body2",
            "Ber_Body3",
            "Ber_AkuwaraiA1",
        ]
        .into_iter()
        .enumerate()
        {
            let address = relocations[&(0x1d9650 + index as u64 * 8)] as usize + 256;
            assert_eq!(&exe[address..address + name.len()], name.as_bytes());
        }
        for (got, vtable, update, draw, name) in [
            (
                0x224bf8,
                0x1d6c70,
                0x11b1e0,
                0x62700,
                "N4shin12DefaultWiperE",
            ),
            (0x224ca0, 0x1d7588, 0x6ca20, 0x6cb80, "N4shin10GlassWiperE"),
        ] {
            assert_eq!(relocations[&got], vtable);
            assert_eq!(relocations[&(vtable + 0x10 + 0x10)], 0x6aa40);
            assert_eq!(relocations[&(vtable + 0x10 + 0x18)], update);
            assert_eq!(relocations[&(vtable + 0x10 + 0x30)], 0x11b1b0);
            assert_eq!(relocations[&(vtable + 0x10 + 0x40)], draw);
            let typeinfo = relocations[&(vtable + 8)];
            let name_address = relocations[&(typeinfo + 8)] as usize + 256;
            assert_eq!(
                &exe[name_address..name_address + name.len()],
                name.as_bytes()
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_murderer_back_transition_lifecycle() {
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
            // Keyboard back: clear/queue0.7->0 atspeed0.2 andsetreturnphase.
            (0x8f104, 0xf943bd08),
            (0x8f114, 0x91008260),
            (0x8f118, 0x9400b0c2),
            (0x8f120, 0xbd45a500),
            (0x8f128, 0xbd497508),
            (0x8f130, 0x321a03e1),
            (0x8f138, 0x9400b232),
            (0x8f13c, 0x1e2703e0),
            (0x8f14c, 0x9400b22d),
            (0x8f150, 0x52800148),
            (0x9039c, 0xf943bd08),
            (0x903ac, 0x91006260),
            (0x903b0, 0x9400ac1c),
            (0x903b8, 0xbd45a500),
            (0x903c0, 0xbd497508),
            (0x903c8, 0x321a03e1),
            (0x903d0, 0x9400ad8c),
            (0x903e4, 0x9400ad87),
            (0x903e8, 0x320003e8),
            (0x903ec, 0xb9001268),
            (0x92a1c, 0xf943bd08),
            (0x92a2c, 0x9103e260),
            (0x92a30, 0x9400a27c),
            (0x92a50, 0x9400a3ec),
            (0x92a64, 0x9400a3e7),
            (0x92a68, 0x52800148),
            (0x92a6c, 0xb9008e68),
            (0x96a90, 0xf943bd08),
            (0x96aa0, 0x91070260),
            (0x96aa4, 0x9400925f),
            (0x96ac4, 0x940093cf),
            (0x96ad8, 0x940093ca),
            (0x96adc, 0xb9401668),
            (0x96ae0, 0x11000508),
            (0x96ae4, 0xb9001668),
            (0x99fd4, 0xf943bd08),
            (0x99fe4, 0x91080260),
            (0x99fe8, 0x9400850e),
            (0x9a008, 0x9400867e),
            (0x9a01c, 0x94008679),
            (0x9a020, 0x52800148),
            // Pointer back usesdecide; finalqueue replaces its duplicated preliminaryqueue.
            (0x8f5a4, 0xf9437108),
            (0x8f5fc, 0xaa1403e0),
            (0x8f600, 0x9400af88),
            (0x8f614, 0x9400b0fb),
            (0x8f628, 0x9400b0f6),
            (0x8f62c, 0x52800148),
            (0x8f630, 0xb9001e68),
            (0x9058c, 0xf9437108),
            (0x905e4, 0xaa1403e0),
            (0x905e8, 0x9400ab8e),
            (0x905fc, 0x9400ad01),
            (0x93000, 0xf9437108),
            (0x93058, 0xaa1403e0),
            (0x9305c, 0x9400a0f1),
            (0x93070, 0x9400a264),
            (0x93084, 0x9400a25f),
            (0x93088, 0x52800148),
            (0x973cc, 0xf9437108),
            (0x97424, 0xaa1403e0),
            (0x97428, 0x94008ffe),
            (0x9743c, 0x94009171),
            (0x9a4f4, 0xf9437108),
            (0x9a54c, 0xaa1403e0),
            (0x9a550, 0x940083b4),
            (0x9a564, 0x94008527),
            // Replay waitsflash then capturesDefault30/stagesTopfocus0 before updatingflash.
            (0x8e518, 0x7100291f),
            (0x8e51c, 0x54000600),
            (0x8e5dc, 0x91008260),
            (0x8e5e0, 0x9400b504),
            (0x8e5e4, 0x370003e0),
            (0x8e5ec, 0x1e27d000),
            (0x8e5f8, 0x97ff4f8a),
            (0x8e604, 0x97fff957),
            (0x8e638, 0x2a1f03e2),
            (0x8e63c, 0x97fff619),
            (0x8e644, 0xf9002a95),
            (0x8e658, 0x12800008),
            (0x8e65c, 0xb9001e68),
            (0x8e668, 0x9400b4fa),
            // Rulesreturnphase1 restoresTopfocus1;bothpurplemodesrestoremode+2.
            (0x8fc74, 0x7100051f),
            (0x8fc78, 0x54000441),
            (0x8fc7c, 0x91006260),
            (0x8fc80, 0x9400af5c),
            (0x8fc84, 0x370003e0),
            (0x8fc8c, 0x1e27d000),
            (0x8fc98, 0x97ff49e2),
            (0x8fca4, 0x97fff3af),
            (0x8fcd8, 0x320003e2),
            (0x8fcdc, 0x97fff071),
            (0x8fce4, 0xf9002a95),
            (0x8fcfc, 0xb9001268),
            (0x8fd08, 0x9400af52),
            (0x94b64, 0x7100051f),
            (0x94b68, 0x54000461),
            (0x94b6c, 0x91070260),
            (0x94b70, 0x94009ba0),
            (0x94b74, 0x37000400),
            (0x94b7c, 0x1e27d000),
            (0x94b88, 0x97ff3626),
            (0x94b94, 0x97ffdff3),
            (0x94bc0, 0xb9401268),
            (0x94bc8, 0x11000902),
            (0x94bd0, 0x97ffdcb4),
            (0x94bd8, 0xf9002a95),
            (0x94bf0, 0xb9001668),
            // Hintphase10->11 falls throughcapture/stageTopfocus4;Accphase10 restoresfocus5.
            (0x91838, 0x9103e260),
            (0x9183c, 0x9400a86d),
            (0x91840, 0x37000440),
            (0x91848, 0x11000508),
            (0x9184c, 0xb9008e68),
            (0x91854, 0x1e27d000),
            (0x91860, 0x97ff42f0),
            (0x9186c, 0x97ffecbd),
            (0x918a0, 0x321e03e2),
            (0x918a4, 0x97ffe97f),
            (0x918ac, 0xf9002a95),
            (0x918c4, 0xb9008e68),
            (0x9888c, 0x91080260),
            (0x98890, 0x94008c58),
            (0x98894, 0x37000500),
            (0x9889c, 0x1e27d000),
            (0x988a8, 0x97ff26de),
            (0x988b4, 0x97ffd0ab),
            (0x988e4, 0x528000a2),
            (0x988ec, 0x97ffcd6d),
            (0x988f4, 0xf9002a95),
            (0x98908, 0x12800008),
            // Backdraw supplieswhiteflash2;sharedhelper supplieszeroflash1 andprogram5.
            (0x8e9e8, 0x91008260),
            (0x8e9ec, 0x9400b2ad),
            (0x8e9f0, 0x4ea01c03),
            (0x8ea14, 0x9400e13f),
            (0x90274, 0x91006260),
            (0x90278, 0x9400ac8a),
            (0x9027c, 0x4ea01c03),
            (0x902a0, 0x9400db1c),
            (0x92324, 0x9103e260),
            (0x92328, 0x9400a45e),
            (0x9232c, 0x4ea01c03),
            (0x92350, 0x9400d2f0),
            (0x962ec, 0x91070260),
            (0x962f0, 0x9400946c),
            (0x962f4, 0x4ea01c03),
            (0x96304, 0x97ff0113),
            (0x99580, 0x91080260),
            (0x99584, 0x940087c7),
            (0x99588, 0x4ea01c03),
            (0x995ac, 0x9400b659),
            (0xc6f20, 0x1e2703e0),
            (0xc6f2c, 0x97fe3e01),
            (0xc6f38, 0xaa1303e2),
            (0xc6f44, 0x94000007),
            (0xc704c, 0x528000a0),
            (0xc7054, 0x940021d3),
            (0xc7058, 0x320003e0),
            (0xc7064, 0x940021f7),
            (0xc7068, 0x321f03e0),
            (0xc7074, 0x940021f3),
        ] {
            assert_eq!(word(address), expected, "Native back {address:x}");
        }
        for (address, value) in [
            (0x1595a4, 0.7f32),
            (0x159974, 0.2),
            (0x159a9c, 1640.0),
            (0x159694, 32.0),
            (0x159930, 1800.0),
            (0x159658, 116.0),
        ] {
            assert_eq!(word(address), value.to_bits());
        }
        assert_eq!(0x917e8 + usize::from(exe[0x171176 + 256 + 9]) * 4, 0x91838);
        assert_eq!(0x917e8 + usize::from(exe[0x171176 + 256 + 10]) * 4, 0x91850);
        assert_eq!(0x98748 + usize::from(exe[0x171181 + 256 + 9]) * 4, 0x9888c);
    }

    #[test]
    fn playback_policy_tracks_murderer_screen_and_completion() {
        let mut model = MurdererModel::new(20);
        assert!(!model.allows_playback_mode_changes());
        for menu in 0..6 {
            model.reset();
            assert!(model.open_menu(menu));
            assert!(!model.allows_playback_mode_changes());
            model.back();
            finish_back(&mut model);
            assert!(!model.allows_playback_mode_changes());
        }
        model.open_menu(0);
        model.replay_chapter(4);
        assert!(!model.allows_playback_mode_changes());
        model.update(7.0);
        model.update(0.0);
        assert!(model.allows_playback_mode_changes());
        model.reset();
        model.open_menu(4);
        model.request_hint(12);
        assert!(!model.allows_playback_mode_changes());
        model.update(7.0);
        model.update(0.0);
        assert!(model.allows_playback_mode_changes());
        for correct in [false, true] {
            model.reset();
            model.open_menu(5);
            if correct {
                for entry in 34..37 {
                    model.activate_accusation_entry(entry);
                }
            }
            assert_eq!(model.battler_sprite_state(), 0);
            model.activate_accusation_entry(SUBMIT_ENTRY);
            assert_eq!(model.battler_sprite_state(), 0);
            assert!(!model.allows_playback_mode_changes());
            model.update(7.0);
            assert!(!model.allows_playback_mode_changes());
            model.update(0.0);
            assert!(model.allows_playback_mode_changes());
            assert!(model.poll_result().is_none());
            assert_eq!(model.battler_sprite_state(), 1);
            model.update(60.0);
            model.update(0.0);
            assert!(model.allows_playback_mode_changes());
            if !correct {
                assert!(model.poll_result().is_none());
                model.update(60.0);
                model.update(0.0);
            }
            assert!(model.poll_result().is_some());
            assert_eq!(model.battler_sprite_state(), if correct { 1 } else { 2 });
            assert!(model.allows_playback_mode_changes());
        }
        model.reset();
        assert!(!model.allows_playback_mode_changes());
    }

    #[test]
    fn replay_and_hint_responses_wait_for_native_confirmation_phases() {
        for menu in [0, 4] {
            let mut model = MurdererModel::new(20);
            model.open_menu(menu);
            if menu == 0 {
                model.replay_chapter(4);
            } else {
                model.request_hint(4);
            }
            assert_eq!(model.poll_result(), None);
            model.update(3.5);
            assert_eq!(
                model.response_completion.as_ref().unwrap().flash.value(),
                0.7
            );
            assert!(!model.allows_playback_mode_changes());
            model.update(3.5);
            assert_eq!(model.poll_result(), None);
            model.update(0.0);
            assert!(model.allows_playback_mode_changes());
            assert_eq!(model.poll_result(), None);
            if menu == 0 {
                model.update(30.0);
                assert_eq!(
                    model.response_completion.as_ref().unwrap().timer.value(),
                    0.0
                );
                model.update(30.0);
                let fade = model.response_completion.as_ref().unwrap().timer.value();
                assert!((fade - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
                model.update(30.0);
            } else {
                model.update(53.0);
            }
            assert_eq!(model.poll_result(), None);
            model.update(0.0);
            assert_eq!(model.poll_result(), Some(if menu == 0 { 4 } else { 10 }));
            model.reset();
            assert_eq!(model.poll_result(), None);
            assert!(!model.allows_playback_mode_changes());
        }
        let mut model = MurdererModel::new(20);
        model.open_menu(0);
        model.replay_chapter(1);
        model.update(1000.0);
        assert_eq!(model.poll_result(), None);
        assert!(!model.allows_playback_mode_changes());
        model.update(0.0);
        assert!(model.allows_playback_mode_changes());
        assert_eq!(
            model.response_completion.as_ref().unwrap().timer.value(),
            0.0
        );
        model.update(90.0);
        assert_eq!(model.poll_result(), None);
        model.update(0.0);
        assert_eq!(model.poll_result(), Some(1));
    }

    #[test]
    fn all_replay_cards_expand_from_their_native_center_to_the_screen() {
        for index in 0..6 {
            let mut model = MurdererModel::new(20);
            model.open_menu(0);
            model.replay_chapter(index);
            assert!(model.replay_expansion().is_none());
            model.update(7.0);
            model.update(0.0);
            assert_eq!(
                model.replay_expansion(),
                Some((index, replay_rect(index).unwrap(), 0.0))
            );
            model.update(30.0);
            assert_eq!(
                model.replay_expansion().unwrap().1,
                replay_rect(index).unwrap()
            );
            model.update(30.0);
            let (_, rect, amount) = model.replay_expansion().unwrap();
            assert!((amount - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
            assert!((rect.z - (544.0 + 1376.0 * amount)).abs() < 1e-4);
            assert!((rect.w - (306.0 + 774.0 * amount)).abs() < 1e-4);
            model.update(30.0);
            assert_eq!(
                model.replay_expansion(),
                Some((index, vec4(0.0, 0.0, 1920.0, 1080.0), 1.0))
            );
            assert!(model.poll_result().is_none());
            model.update(0.0);
            assert_eq!(model.poll_result(), Some(index as i32));
            assert_eq!(
                model.replay_expansion().unwrap().1,
                vec4(0.0, 0.0, 1920.0, 1080.0)
            );
            model.reset();
            assert!(model.replay_expansion().is_none());
        }
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_top_focus_interpolation() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        // Reviewed constructor, controller retarget, update, render/vector
        // lerp, hover eligibility and pointer release retarget instructions.
        for (address, expected) in [
            (0x8c08c, 0xb9401268),
            (0x8c090, 0x6b08039f),
            (0x8c094, 0x1e280dc0),
            (0x8c098, 0xaa1803e0),
            (0x8c09c, 0x9400bc91),
            (0x8d5c4, 0xf9403e69),
            (0x8d5c8, 0x5280190a),
            (0x8d5cc, 0x9b0a2508),
            (0x8d5d0, 0x91006100),
            (0x8d5d4, 0x9400b793),
            (0x8d5d8, 0x90000668),
            (0x8d5dc, 0xbd457101),
            (0x8d5e0, 0x1e2703e0),
            (0x8d5e4, 0x321a03e1),
            (0x8d5e8, 0x2a1f03e2),
            (0x8d5ec, 0x9400b905),
            (0x8d5f0, 0xb9001274),
            (0x8d5f4, 0x37f805d4),
            (0x8d5f8, 0xf9403e68),
            (0x8d5fc, 0x52801909),
            (0x8d600, 0x9b292288),
            (0x8d604, 0x91006100),
            (0x8d608, 0x9400b786),
            (0x8d60c, 0x1e2e1000),
            (0x8d610, 0x1e2a1001),
            (0x8d614, 0x321a03e1),
            (0x8d618, 0x2a1f03e2),
            (0x8d61c, 0x9400b8f9),
            (0x8c9b8, 0x4ea81d00),
            (0x8c9bc, 0x91006280),
            (0x8c9c0, 0x9400bc24),
            (0x8c9c4, 0x4ea81d00),
            (0x8c9c8, 0x9101c280),
            (0x8c9cc, 0x9400bc21),
            (0x8c9d0, 0x91032294),
            (0x8c9d4, 0xeb14027f),
            (0x8c9d8, 0x54ffff01),
            (0x8d188, 0x910062c0),
            (0x8d18c, 0x9400b8c5),
            (0x8d190, 0x9101c2c0),
            (0x8d194, 0x4ea01c0d),
            (0x8d198, 0x9400b8c2),
            (0x8d19c, 0xbc1cc3a0),
            (0x8d1a0, 0x2d40b2cb),
            (0x8d1a4, 0x2d41a6ce),
            (0x8d1a8, 0x4ead1da0),
            (0x8d1ac, 0x97ff2561),
            (0x8d1b0, 0x91004261),
            (0x8d1b4, 0x910203e2),
            (0x8d1b8, 0xaa1303e0),
            (0x8d1bc, 0x2d1007e0),
            (0x8d1c0, 0x2d110fe2),
            (0x8d1c4, 0x97ff2593),
            (0x56810, 0x3dc00000),
            (0x56814, 0x3dc00021),
            (0x56818, 0x3dc00042),
            (0x5681c, 0x4ea0d421),
            (0x56820, 0x6e22dc21),
            (0x56824, 0x4e21d400),
            (0x8d7b0, 0x12800008),
            (0x8d7b4, 0xb9001408),
            (0x8d7b8, 0xb9401808),
            (0x8d7bc, 0x34000068),
            (0x8d7c0, 0x2a1f03e0),
            (0x8d7c4, 0xd65f03c0),
            (0x8d7c8, 0xf81e0ff3),
            (0x8d7cc, 0xa9017bfd),
            (0x8d7d0, 0x910043fd),
            (0x8d7d4, 0xaa0003f3),
            (0x8d7d8, 0x97ffffbe),
            (0x8d7dc, 0xb9001660),
            (0x8d7e0, 0xa9417bfd),
            (0x8d7e4, 0x2a2003e8),
            (0x8d7e8, 0x531f7d00),
            (0x8d7ec, 0xf84207f3),
            (0x8d7f0, 0xd65f03c0),
            (0x8d974, 0x37f80188),
            (0x8d978, 0xf9403e69),
            (0x8d97c, 0x5280190a),
            (0x8d980, 0x9b0a2508),
            (0x8d984, 0x91006100),
            (0x8d988, 0x9400b6a6),
            (0x8d98c, 0x90000668),
            (0x8d990, 0xbd457101),
            (0x8d994, 0x1e2703e0),
            (0x8d998, 0x321a03e1),
            (0x8d99c, 0x2a1f03e2),
            (0x8d9a0, 0x9400b818),
            (0x8d9a4, 0xb9001275),
            (0x8d9a8, 0x37f801b5),
            (0x8d9ac, 0xf9403e69),
            (0x8d9b0, 0x93407ea8),
            (0x8d9b4, 0x5280190a),
            (0x8d9b8, 0x9b0a2508),
            (0x8d9bc, 0x91006100),
            (0x8d9c0, 0x9400b698),
            (0x8d9c4, 0x1e2e1000),
            (0x8d9c8, 0x1e2a1001),
            (0x8d9cc, 0x321a03e1),
            (0x8d9d0, 0x2a1f03e2),
            (0x8d9d4, 0x9400b80b),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        for (address, value) in [
            (0x159570, 0.1f32),
            (0x1599b0, 0.2157),
            (0x1598d8, 0.1059),
            (0x159780, 0.1608),
        ] {
            assert_eq!(word(address), value.to_bits(), "constant{address:x}");
        }
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_top_pointer_feedback() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        // Reviewed press/release, cancellation, keyboard confirm, selected
        // row idle gate, parent forwarders and common ADV callback dispatch.
        for (address, expected) in [
            (0x8d838, 0x9400b6fa),
            (0x8d83c, 0x90000668),
            (0x8d840, 0xbd45a500),
            (0x8d844, 0x90000668),
            (0x8d848, 0xbd497501),
            (0x8d84c, 0x321a03e1),
            (0x8d850, 0x2a1f03e2),
            (0x8d854, 0x9400b86b),
            (0x8d8e0, 0xb9401408),
            (0x8d8e4, 0x37f80a68),
            (0x8d8e8, 0x6dbc23e9),
            (0x8d8ec, 0xf9000bf5),
            (0x8d8f0, 0xa9024ff4),
            (0x8d8f4, 0xa9037bfd),
            (0x8d8f8, 0x9100c3fd),
            (0x8d8fc, 0xf0000ca8),
            (0x8d900, 0xf9437108),
            (0x8d904, 0xaa0003f3),
            (0x8d908, 0xf9400100),
            (0x8d90c, 0x1e2e1000),
            (0x8d910, 0x97fe6668),
            (0x8d914, 0xb9801668),
            (0x8d918, 0xf9403e69),
            (0x8d91c, 0x52801914),
            (0x8d920, 0x9b142508),
            (0x8d924, 0x9101c100),
            (0x8d928, 0x9400b6be),
            (0x8d92c, 0x90000668),
            (0x8d930, 0xbd45a509),
            (0x8d934, 0x90000668),
            (0x8d938, 0xbd497508),
            (0x8d93c, 0x4ea91d20),
            (0x8d940, 0x4ea81d01),
            (0x8d944, 0x321a03e1),
            (0x8d948, 0x2a1f03e2),
            (0x8d94c, 0x9400b82d),
            (0x8d950, 0x1e2703e0),
            (0x8d954, 0x4ea81d01),
            (0x8d958, 0x321a03e1),
            (0x8d95c, 0x2a1f03e2),
            (0x8d960, 0x9400b828),
            (0x8d9e8, 0x9400b68e),
            (0x8d9ec, 0x4ea91d20),
            (0x8d9f0, 0x4ea81d01),
            (0x8d9f4, 0x321a03e1),
            (0x8d9f8, 0x2a1f03e2),
            (0x8d9fc, 0x9400b801),
            (0x8da00, 0x1e2703e0),
            (0x8da04, 0x4ea81d01),
            (0x8da08, 0x321a03e1),
            (0x8da0c, 0x2a1f03e2),
            (0x8da10, 0x9400b7fc),
            (0x8da14, 0xb9401a68),
            (0x8da18, 0x11000508),
            (0x8da1c, 0xb9001a68),
            (0x8da68, 0x9400b66e),
            (0x8da6c, 0x90000668),
            (0x8da70, 0xbd497501),
            (0x8da74, 0x1e2703e0),
            (0x8da78, 0x321a03e1),
            (0x8da7c, 0x2a1f03e2),
            (0x8da80, 0x9400b7e0),
            (0x8da84, 0x12800008),
            (0x8da88, 0xb9001668),
            (0x8db38, 0x9400b63a),
            (0x8db3c, 0x90000668),
            (0x8db40, 0xbd497501),
            (0x8db44, 0x1e2703e0),
            (0x8db48, 0x321a03e1),
            (0x8db4c, 0x2a1f03e2),
            (0x8db50, 0x9400b7ac),
            (0x8db54, 0x12800008),
            (0x8db58, 0xb9001668),
            (0x8d638, 0x39403fe8),
            (0x8d63c, 0x36200388),
            (0x8d640, 0xf0000ca8),
            (0x8d644, 0xf9437108),
            (0x8d648, 0xf9400100),
            (0x8d64c, 0x1e2e1000),
            (0x8d650, 0x97fe6718),
            (0x8d654, 0xb9801268),
            (0x8d658, 0xf9403e69),
            (0x8d65c, 0x5280190a),
            (0x8d660, 0x9b0a2508),
            (0x8d664, 0x9101c100),
            (0x8d668, 0x9400b76e),
            (0x8d66c, 0x90000668),
            (0x8d670, 0xbd45a500),
            (0x8d674, 0x90000668),
            (0x8d678, 0xbd497508),
            (0x8d67c, 0x4ea81d01),
            (0x8d680, 0x321a03e1),
            (0x8d684, 0x2a1f03e2),
            (0x8d688, 0x9400b8de),
            (0x8d68c, 0x1e2703e0),
            (0x8d690, 0x4ea81d01),
            (0x8d694, 0x321a03e1),
            (0x8d698, 0x2a1f03e2),
            (0x8d69c, 0x9400b8d9),
            (0x8d6a0, 0xb9401a68),
            (0x8d6a4, 0x11000508),
            (0x8d6a8, 0xb9001a68),
            (0x8c684, 0xb9801268),
            (0x8c688, 0xf9403e69),
            (0x8c68c, 0x5280190a),
            (0x8c690, 0x9b0a2508),
            (0x8c694, 0x9101c100),
            (0x8c698, 0x9400bcd6),
            (0x8c69c, 0x37001820),
            (0x9bb10, 0xf9401008),
            (0x9bb14, 0xf9402500),
            (0x9bb18, 0xb4000080),
            (0x9bb1c, 0xf9400008),
            (0x9bb20, 0xf9403101),
            (0x9bb24, 0xd61f0020),
            (0x9bb28, 0xd65f03c0),
            (0x9bb2c, 0x00000000),
            (0x9bb30, 0xf9401008),
            (0x9bb34, 0xf9402500),
            (0x9bb38, 0xb4000080),
            (0x9bb3c, 0xf9400008),
            (0x9bb40, 0xf9403501),
            (0x9bb44, 0xd61f0020),
            (0x1573c, 0xc8dffe80),
            (0x15740, 0xf9400008),
            (0x15744, 0xf9405108),
            (0x15748, 0x4ea91d20),
            (0x1574c, 0x4ea81d01),
            (0x15750, 0xd63f0100),
            (0x1598c, 0xc8dffe80),
            (0x15990, 0xf9400008),
            (0x15994, 0xf9405508),
            (0x15998, 0x4ea91d20),
            (0x1599c, 0x4ea81d01),
            (0x159a0, 0xd63f0100),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        assert_eq!(word(0x1595a4), 0.7f32.to_bits());
        assert_eq!(word(0x159974), 0.2f32.to_bits());
        let relocation = |address: u64| {
            exe[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find(|entry| u64::from_le_bytes(entry[..8].try_into().unwrap()) == address)
                .map(|entry| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                .unwrap()
        };
        // ADV +a0/+a8 forwards through Murderer parent +60/+68.
        for (address, target) in [
            (0x1d9d38, 0x9bb10),
            (0x1d9d40, 0x9bb30),
            (0x1d9620, 0x8d810),
            (0x1d9628, 0x8d8e0),
            (0x1d9630, 0x8da40),
            (0x1d9640, 0x8db00),
        ] {
            assert_eq!(relocation(address), target, "pointer slot{address:x}");
        }
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_replay_and_back_pointer_feedback() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        // Reviewed Replay row/Back press and release queues plus shared
        // Back press/leave queues for Rule/Hint/Purple/Accusation.
        for (address, expected) in [
            (0x8f4a4, 0x91008260),
            (0x8f4a8, 0x9400afde),
            (0x8f4ac, 0xd0000648),
            (0x8f4b0, 0xbd45a500),
            (0x8f4b4, 0xd0000648),
            (0x8f4b8, 0xbd497501),
            (0x8f4bc, 0x14000026),
            (0x8f4c0, 0xb9801a68),
            (0x8f4c4, 0x37f804e8),
            (0x8f4c8, 0xf9403e69),
            (0x8f4cc, 0x5280190a),
            (0x8f4d0, 0x9b0a2508),
            (0x8f4d4, 0x9101c100),
            (0x8f4d8, 0x9400afd2),
            (0x8f4dc, 0xd0000648),
            (0x8f4e0, 0xbd45a500),
            (0x8f4e4, 0xd0000648),
            (0x8f4e8, 0xbd497501),
            (0x8f4ec, 0x321a03e1),
            (0x8f4f0, 0x2a1f03e2),
            (0x8f4f4, 0x9400b143),
            (0x8f5a4, 0xf9437108),
            (0x8f5a8, 0xf9400100),
            (0x8f5ac, 0x1e2e1000),
            (0x8f5b0, 0x97fe5f40),
            (0x8f5b4, 0x91008274),
            (0x8f5b8, 0xaa1403e0),
            (0x8f5bc, 0x9400af99),
            (0x8f5c0, 0xd0000648),
            (0x8f5c4, 0xbd45a508),
            (0x8f5c8, 0xd0000648),
            (0x8f5cc, 0xbd497509),
            (0x8f5d0, 0x4ea81d00),
            (0x8f5d4, 0x4ea91d21),
            (0x8f5d8, 0x321a03e1),
            (0x8f5dc, 0x2a1f03e2),
            (0x8f5e0, 0x9400b108),
            (0x8f5e4, 0x1e2703ea),
            (0x8f5e8, 0x4eaa1d40),
            (0x8f5ec, 0x4ea91d21),
            (0x8f5f0, 0x321a03e1),
            (0x8f5f4, 0x2a1f03e2),
            (0x8f5f8, 0x9400b102),
            (0x8f5fc, 0xaa1403e0),
            (0x8f600, 0x9400af88),
            (0x8f604, 0x4ea81d00),
            (0x8f608, 0x4ea91d21),
            (0x8f60c, 0x321a03e1),
            (0x8f610, 0x2a1f03e2),
            (0x8f614, 0x9400b0fb),
            (0x8f618, 0x4eaa1d40),
            (0x8f61c, 0x4ea91d21),
            (0x8f620, 0x321a03e1),
            (0x8f624, 0x2a1f03e2),
            (0x8f628, 0x9400b0f6),
            (0x8f62c, 0x52800148),
            (0x8f630, 0xb9001e68),
            (0x8f66c, 0x9400af6d),
            (0x8f670, 0xd0000648),
            (0x8f674, 0xbd45a509),
            (0x8f678, 0xd0000648),
            (0x8f67c, 0xbd497508),
            (0x8f680, 0x4ea91d20),
            (0x8f684, 0x4ea81d01),
            (0x8f688, 0x321a03e1),
            (0x8f68c, 0x2a1f03e2),
            (0x8f690, 0x9400b0dc),
            (0x8f694, 0x1e2703e0),
            (0x8f698, 0x4ea81d01),
            (0x8f69c, 0x321a03e1),
            (0x8f6a0, 0x2a1f03e2),
            (0x8f6a4, 0x9400b0d7),
            (0x8f720, 0x9400af40),
            (0x8f724, 0x4ea91d20),
            (0x8f728, 0x4ea81d01),
            (0x8f72c, 0x321a03e1),
            (0x8f730, 0x2a1f03e2),
            (0x8f734, 0x9400b0b3),
            (0x8f738, 0x1e2703e0),
            (0x8f73c, 0x4ea81d01),
            (0x8f740, 0x321a03e1),
            (0x8f744, 0x2a1f03e2),
            (0x8f748, 0x9400b0ae),
            (0x8f74c, 0x320003e8),
            (0x8f750, 0xb9001e68),
            (0x8f830, 0x91008260),
            (0x8f834, 0x9400aefb),
            (0x8f838, 0xd0000648),
            (0x8f83c, 0xbd497501),
            (0x8f840, 0x1e2703e0),
            (0x8f844, 0x321a03e1),
            (0x8f848, 0x2a1f03e2),
            (0x8f84c, 0x9400b06d),
            (0x8f850, 0xb900167f),
            (0x8f874, 0x9101c100),
            (0x8f878, 0x9400aeea),
            (0x8f87c, 0xd0000648),
            (0x8f880, 0xbd497501),
            (0x8f884, 0x1e2703e0),
            (0x8f888, 0x321a03e1),
            (0x8f88c, 0x2a1f03e2),
            (0x8f890, 0x9400b05c),
            (0x8f894, 0x12800008),
            (0x8f898, 0xb9001a68),
            (0x90534, 0x91006000),
            (0x90538, 0x9400abba),
            (0x9053c, 0xb0000648),
            (0x90540, 0xbd45a500),
            (0x90544, 0xb0000648),
            (0x90548, 0xbd497501),
            (0x9054c, 0x321a03e1),
            (0x90550, 0x2a1f03e2),
            (0x90554, 0x9400ad2b),
            (0x906c0, 0x91006260),
            (0x906c4, 0x9400ab57),
            (0x906c8, 0xb0000648),
            (0x906cc, 0xbd497501),
            (0x906d0, 0x1e2703e0),
            (0x906d4, 0x321a03e1),
            (0x906d8, 0x2a1f03e2),
            (0x906dc, 0x9400acc9),
            (0x906e0, 0xb900167f),
            (0x92f40, 0x9103e260),
            (0x92f44, 0x9400a137),
            (0x92f48, 0xf0000628),
            (0x92f4c, 0xbd45a500),
            (0x92f50, 0xf0000628),
            (0x92f54, 0xbd497501),
            (0x92f58, 0x321a03e1),
            (0x92f5c, 0x2a1f03e2),
            (0x92f60, 0x9400a2a8),
            (0x93268, 0x9103e260),
            (0x9326c, 0x9400a06d),
            (0x93270, 0xd0000628),
            (0x93274, 0xbd497501),
            (0x93278, 0x1e2703e0),
            (0x9327c, 0x321a03e1),
            (0x93280, 0x2a1f03e2),
            (0x93284, 0x9400a1df),
            (0x972f0, 0x91070260),
            (0x972f4, 0x14000007),
            (0x972f8, 0xb9802668),
            (0x972fc, 0x37f80208),
            (0x97300, 0xf9412669),
            (0x97304, 0x52800d0a),
            (0x97308, 0x9b0a2508),
            (0x9730c, 0x91004100),
            (0x97310, 0x94009044),
            (0x97314, 0xd0000608),
            (0x97318, 0xbd45a500),
            (0x9731c, 0xd0000608),
            (0x97320, 0xbd497501),
            (0x97324, 0x321a03e1),
            (0x97328, 0x2a1f03e2),
            (0x9732c, 0x940091b5),
            (0x976e4, 0x91070260),
            (0x976e8, 0x94008f4e),
            (0x976ec, 0xd0000608),
            (0x976f0, 0xbd497501),
            (0x976f4, 0x1e2703e0),
            (0x976f8, 0x321a03e1),
            (0x976fc, 0x2a1f03e2),
            (0x97700, 0x940090c0),
            (0x97704, 0xb9001e7f),
            (0x9a3e4, 0x91080260),
            (0x9a3e8, 0x9400840e),
            (0x9a3ec, 0xf00005e8),
            (0x9a3f0, 0xbd45a500),
            (0x9a3f4, 0xf00005e8),
            (0x9a3f8, 0xbd497501),
            (0x9a3fc, 0x14000027),
            (0x9a498, 0x321a03e1),
            (0x9a49c, 0x2a1f03e2),
            (0x9a4a0, 0x94008558),
            (0x9a800, 0x91080260),
            (0x9a804, 0x94008307),
            (0x9a808, 0xf00005e8),
            (0x9a80c, 0xbd497501),
            (0x9a810, 0x1e2703e0),
            (0x9a814, 0x321a03e1),
            (0x9a818, 0x2a1f03e2),
            (0x9a81c, 0x94008479),
            (0x9a820, 0xb9001a7f),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        assert_eq!(word(0x1595a4), 0.7f32.to_bits());
        assert_eq!(word(0x159974), 0.2f32.to_bits());
        let relocation = |address: u64| {
            exe[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find(|entry| u64::from_le_bytes(entry[..8].try_into().unwrap()) == address)
                .map(|entry| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                .unwrap()
        };
        for (table, press, release, leave) in [
            (0x1d96a0, 0x8f480, 0x8f570, 0x8f810),
            (0x1d97a0, 0x90520, 0x90560, 0x906a0),
            (0x1d9840, 0x92f20, 0x92fd0, 0x93240),
            (0x1d98e0, 0x972d0, 0x973a0, 0x976c0),
            (0x1d9a10, 0x9a3c0, 0x9a4c0, 0x9a7e0),
        ] {
            for (offset, target) in [(0x60, press), (0x68, release), (0x78, leave)] {
                assert_eq!(relocation(table + offset), target);
            }
        }
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_hint_row_feedback() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        // Reviewed availability/constructor/restore, focus retarget,
        // press/release/leave and row update plus both Button uniform calls.
        for (address, expected) in [
            (0x90a08, 0xb9401268),
            (0x90a0c, 0x6b14011f),
            (0x90a10, 0x5400108b),
            (0x90a14, 0x4ea81d00),
            (0x90a18, 0xaa1b03e0),
            (0x90a1c, 0x9400aa2d),
            (0x90a20, 0x4ea81d00),
            (0x90a24, 0xaa1c03e0),
            (0x90a28, 0x9400aa2a),
            (0x90a2c, 0xb901d3f4),
            (0x90a30, 0xb9401268),
            (0x90a34, 0x6b08029f),
            (0x90a38, 0x54000081),
            (0x90a3c, 0xb901d7ff),
            (0x90a40, 0xb9009274),
            (0x90c2c, 0xb9809274),
            (0x90c30, 0xf9403a76),
            (0x90c34, 0x52801708),
            (0x90c38, 0x9b085a88),
            (0x90c3c, 0x91002115),
            (0x90c40, 0x1e2e1000),
            (0x90c44, 0xaa1503e0),
            (0x90c48, 0x9400a9a6),
            (0x923dc, 0xb9009001),
            (0x923e0, 0xeb1802ff),
            (0x923e4, 0x54000ce0),
            (0x923e8, 0x1e2703e8),
            (0x923ec, 0x1e2e1009),
            (0x92418, 0xb94002e8),
            (0x9241c, 0x6b01011f),
            (0x92420, 0x1e280d20),
            (0x92424, 0x910022f6),
            (0x92428, 0xaa1603e0),
            (0x9242c, 0x9400a3ad),
            (0x92b14, 0xb9809008),
            (0x92b18, 0x6b01011f),
            (0x92b1c, 0x54000900),
            (0x92b20, 0xf9403809),
            (0x92b24, 0x52801716),
            (0x92b28, 0x9b162508),
            (0x92b2c, 0xaa0003f3),
            (0x92b30, 0x91002100),
            (0x92b34, 0x2a0203f4),
            (0x92b38, 0x2a0103f5),
            (0x92b3c, 0x9400a239),
            (0x92b40, 0xf0000628),
            (0x92b44, 0x1e2703e8),
            (0x92b48, 0x4ea81d00),
            (0x92b4c, 0xbd457101),
            (0x92b50, 0x321a03e1),
            (0x92b54, 0x2a1f03e2),
            (0x92b58, 0x9400a3aa),
            (0x92b5c, 0xf9403a68),
            (0x92b60, 0x9b3622a8),
            (0x92b64, 0x91002100),
            (0x92b68, 0xb9009275),
            (0x92b6c, 0x9400a22d),
            (0x92b70, 0x1e2e1000),
            (0x92b74, 0x1e2a1001),
            (0x92b78, 0x321a03e1),
            (0x92b7c, 0x2a1f03e2),
            (0x92b80, 0x9400a3a0),
            (0x92f74, 0x51000514),
            (0x92f78, 0xf9403a68),
            (0x92f7c, 0x52801709),
            (0x92f80, 0x9b292288),
            (0x92f84, 0x91018100),
            (0x92f88, 0x9400a126),
            (0x92f8c, 0xf0000628),
            (0x92f90, 0xbd45a500),
            (0x92f94, 0xf0000628),
            (0x92f98, 0xbd497501),
            (0x92f9c, 0x321a03e1),
            (0x92fa0, 0x2a1f03e2),
            (0x92fa4, 0x9400a297),
            (0x930bc, 0xf9403a68),
            (0x930c0, 0x52801715),
            (0x930c4, 0x9b352288),
            (0x930c8, 0x91018100),
            (0x930cc, 0x9400a0d5),
            (0x930d0, 0xd0000628),
            (0x930d4, 0xbd45a508),
            (0x930d8, 0xd0000628),
            (0x930dc, 0xbd497509),
            (0x930e0, 0x4ea81d00),
            (0x930e4, 0x4ea91d21),
            (0x930e8, 0x321a03e1),
            (0x930ec, 0x2a1f03e2),
            (0x930f0, 0x9400a244),
            (0x930f4, 0x1e2703ea),
            (0x930f8, 0x4eaa1d40),
            (0x930fc, 0x4ea91d21),
            (0x93100, 0x321a03e1),
            (0x93104, 0x2a1f03e2),
            (0x93108, 0x9400a23e),
            (0x9310c, 0xaa1303e0),
            (0x93110, 0x2a1403e1),
            (0x93114, 0x320003e2),
            (0x93118, 0x320003f6),
            (0x9311c, 0x97fffe79),
            (0x93120, 0xb9809268),
            (0x93124, 0xf9403a69),
            (0x93128, 0x9b152508),
            (0x9312c, 0x91018100),
            (0x93130, 0x9400a0bc),
            (0x93134, 0x4ea81d00),
            (0x93138, 0x4ea91d21),
            (0x9313c, 0x321a03e1),
            (0x93140, 0x2a1f03e2),
            (0x93144, 0x9400a22f),
            (0x93148, 0x4eaa1d40),
            (0x9314c, 0x4ea91d21),
            (0x93150, 0x321a03e1),
            (0x93154, 0x2a1f03e2),
            (0x93158, 0x9400a22a),
            (0x9315c, 0xd0000628),
            (0x93160, 0xbd479901),
            (0x93164, 0x4eaa1d40),
            (0x93168, 0x91006260),
            (0x9316c, 0x2a1f03e1),
            (0x93170, 0x2a1f03e2),
            (0x93174, 0x9400a223),
            (0x93178, 0xb9008e76),
            (0x932b4, 0x91018100),
            (0x932b8, 0x9400a05a),
            (0x932bc, 0xd0000628),
            (0x932c0, 0xbd497501),
            (0x932c4, 0x1e2703e0),
            (0x932c8, 0x321a03e1),
            (0x932cc, 0x2a1f03e2),
            (0x932d0, 0x9400a1cc),
            (0x918f0, 0x4ea81d00),
            (0x918f4, 0x910022a0),
            (0x918f8, 0x9400a856),
            (0x918fc, 0x4ea81d00),
            (0x91900, 0x910182a0),
            (0x91904, 0x9400a853),
            (0x91908, 0x9102e2b5),
            (0x9190c, 0xeb1502df),
            (0x91910, 0x54ffff01),
            (0x92088, 0x910022e0),
            (0x9208c, 0x9400a505),
            (0x92090, 0x1e2c1001),
            (0x92094, 0x910182e0),
            (0x92098, 0x1e210808),
            (0x9209c, 0x9400a501),
            (0x92138, 0x4ea91d20),
            (0x9213c, 0x4ea91d21),
            (0x92140, 0x4ea91d22),
            (0x92144, 0x4ea81d03),
            (0x92148, 0x97ff1182),
            (0x9214c, 0x2d0c07e0),
            (0x92150, 0x2d0d0fe2),
            (0x92154, 0x4ea91d20),
            (0x92158, 0x4ea91d21),
            (0x9215c, 0x4ea91d22),
            (0x92160, 0x4eaf1de3),
            (0x92164, 0x97ff117b),
            (0x92168, 0x910383e0),
            (0x9216c, 0x910183e1),
            (0x92170, 0x910083e2),
            (0x92174, 0x2d0407e0),
            (0x92178, 0x2d050fe2),
            (0x9217c, 0x9400d379),
            (0x92210, 0x4ea91d20),
            (0x92214, 0x4ea91d21),
            (0x92218, 0x4ea91d22),
            (0x9221c, 0x4ea81d03),
            (0x92220, 0x97ff114c),
            (0x92224, 0x2d0c07e0),
            (0x92228, 0x2d0d0fe2),
            (0x9222c, 0x4ea91d20),
            (0x92230, 0x4ea91d21),
            (0x92234, 0x4ea91d22),
            (0x92238, 0x4eaf1de3),
            (0x9223c, 0x97ff1145),
            (0x92240, 0x910383e0),
            (0x92244, 0x910183e1),
            (0x92248, 0x910083e2),
            (0x9224c, 0x2d0407e0),
            (0x92250, 0x2d050fe2),
            (0x92254, 0x9400d343),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        for (address, value) in [
            (0x159570, 0.1f32),
            (0x1595a4, 0.7),
            (0x159974, 0.2),
            (0x159798, 60.0),
        ] {
            assert_eq!(word(address), value.to_bits());
        }
    }
    #[test]
    fn hint_focus_pointer_release_and_independent_timer_match_native() {
        let mut model = MurdererModel::new(20);
        model.open_menu(4);
        assert_eq!(model.hint_feedback(20), (1.0, 0.0));
        model.select_hint(19, true);
        assert_eq!(model.hint_scroll, 1170.0);
        model.update(2.0);
        assert!((model.hint_feedback(20).0 - 0.8).abs() < 1e-6);
        assert_eq!(model.hint_feedback(19).0, 0.5);
        model.select_hint(20, true);
        model.update(1.0);
        assert_eq!(model.hint_feedback(20).0, 1.0);
        assert!((model.hint_feedback(19).0 - 0.4).abs() < 1e-6);
        model.update(3.0);
        let point = Vec2::new(330.0, 820.0);
        model.pointer_state(point, true);
        model.update(10.0);
        assert_eq!(model.hint_feedback(20), (1.0, 0.7));
        assert!(model.take_sounds().is_empty());
        assert!(model.result.is_none());
        model.pointer_state(point, false);
        assert_eq!(model.take_sounds(), ["decide"]);
        model.update(3.5);
        assert_eq!(model.hint_feedback(20).1, 0.0);
        assert_eq!(
            model.response_completion.as_ref().unwrap().phase,
            ResponsePhase::Flash
        );
        model.update(0.0);
        assert_eq!(
            model.response_completion.as_ref().unwrap().phase,
            ResponsePhase::Exit
        );
        model.update(56.5);
        assert!(model.poll_result().is_none());
        model.update(0.0);
        assert_eq!(model.poll_result(), Some(26));
    }
    #[test]
    fn hint_pointer_hit_clipping_selection_scroll_and_cancel() {
        let mut model = MurdererModel::new(20);
        model.open_menu(4);
        let point = Vec2::new(330.0, 190.0); // Native row13 in initial viewport.
        model.pointer_state(point, false);
        assert_eq!(model.menu_focus, 20);
        model.pointer_state(point, true);
        assert_eq!(model.menu_focus, 13);
        assert!(!model.hint_scroll_motion.is_idle());
        model.update(2.0);
        assert_eq!(model.hint_feedback(13), (0.5, 0.4));
        assert_eq!(model.hint_scroll, 1170.0); // Still in view:4tick hold.
        model.pointer_state(Vec2::ZERO, true);
        model.update(2.0);
        model.pointer_state(point, false);
        assert_eq!(model.hint_feedback(13), (1.0, 0.0));
        assert!(model.result.is_none());
        assert!(model.take_sounds().is_empty());
        for point in [Vec2::new(300.0, 190.0), Vec2::new(330.0, 900.0)] {
            model.pointer(point, true);
            assert!(model.result.is_none());
        }
        model.select_hint(12, true);
        model.update(2.0);
        assert_eq!(model.hint_scroll, 1125.0);
        model.update(2.0);
        assert_eq!(model.hint_scroll, 1080.0);
        model.reset();
        model.open_menu(4);
        assert_eq!(model.hint_feedback(20), (1.0, 0.0));
        assert_eq!(model.hint_feedback(13), (0.0, 0.0));
    }
    #[test]
    fn hint_pointer_hit_includes_two_pixels_below_artwork() {
        let mut model = MurdererModel::new(20);
        model.open_menu(4);
        model.pointer(Vec2::new(330.0, 269.0), true);
        assert_eq!(model.menu_focus, 13);
        assert_eq!(model.result, Some(19));
        assert_eq!(model.take_sounds(), ["decide"]);
    }
    #[test]
    fn replay_pointer_hold_release_retains_flash_and_original_response() {
        for index in 0..6 {
            for hold in [0.0f32, 2.0, 10.0] {
                let mut model = MurdererModel::new(20);
                model.open_menu(0);
                let rect = replay_rect(index).unwrap();
                let point = Vec2::new(rect.x + 10.0, rect.y + 10.0);
                model.pointer_state(point, false);
                assert_eq!(model.menu_focus, 0);
                model.pointer_state(point, true);
                model.update(hold);
                let brightness = (hold * 0.2).min(0.7);
                assert!((model.replay_feedback(index).1 - brightness).abs() < 1e-6);
                assert!(model.result.is_none());
                assert!(model.take_sounds().is_empty());
                model.pointer_state(point, false);
                assert_eq!(model.take_sounds(), ["decide"]);
                assert_eq!(model.result, Some(index as i32));
                model.update((0.7 - brightness) / 0.2);
                assert!((model.replay_feedback(index).1 - 0.7).abs() < 1e-6);
                model.update(3.5);
                assert_eq!(model.replay_feedback(index).1, 0.0);
                model.update(0.0);
                model.update(90.0);
                assert!(model.poll_result().is_none());
                model.update(0.0);
                assert_eq!(model.poll_result(), Some(index as i32));
            }
        }
    }
    #[test]
    fn all_back_buttons_preserve_held_feedback_and_release_timing() {
        for menu in 0..6 {
            for hold in [0.0f32, 2.0, 10.0] {
                let mut model = MurdererModel::new(20);
                model.open_menu(menu);
                model.take_sounds();
                let child = model.menu;
                let point = Vec2::new(1700.0, 60.0);
                model.pointer_state(point, true);
                model.update(hold);
                let brightness = (hold * 0.2).min(0.7);
                assert!((model.back_flash() - brightness).abs() < 1e-6);
                assert_eq!(model.menu, child);
                assert!(model.back_completion.is_none());
                assert!(model.take_sounds().is_empty());
                model.pointer_state(point, false);
                assert_eq!(model.take_sounds(), ["decide"]);
                assert!(model.back_completion.is_some());
                model.update((0.7 - brightness) / 0.2);
                assert!((model.back_flash() - 0.7).abs() < 1e-6);
                model.update(3.5);
                assert_eq!(model.back_flash(), 0.0);
                assert!(model.wipe.is_none());
                model.update(0.0);
                assert_eq!(model.wipe.as_ref().unwrap().duration, 30.0);
                model.update(30.0);
                assert_eq!(model.menu, MurdererMenu::Top);
                assert_eq!(model.menu_focus, menu);
            }
        }
    }
    #[test]
    fn replay_and_back_cancel_fade_and_keyboard_retargets_current_brightness() {
        let mut model = MurdererModel::new(20);
        model.open_menu(0);
        let point = Vec2::new(700.0, 210.0);
        model.pointer_state(point, true);
        model.update(2.0);
        model.pointer_state(Vec2::ZERO, true);
        model.update(1.0);
        assert!((model.replay_feedback(1).1 - 0.2).abs() < 1e-6);
        model.pointer_state(point, false);
        assert!(model.result.is_none());
        assert!(model.take_sounds().is_empty());
        model.confirm();
        model.update(2.5);
        assert!((model.replay_feedback(1).1 - 0.7).abs() < 1e-6);
        model.reset();
        model.open_menu(1);
        let back = Vec2::new(1700.0, 60.0);
        model.pointer_state(back, true);
        model.update(2.0);
        model.ignore_pointer_state(true);
        model.update(1.0);
        assert!((model.back_flash() - 0.2).abs() < 1e-6);
        model.pointer_state(back, false);
        assert!(model.back_completion.is_none());
        model.back();
        assert_eq!(model.take_sounds(), ["cancel"]);
        model.update(2.5);
        assert!((model.back_flash() - 0.7).abs() < 1e-6);
    }
    #[test]
    fn top_pointer_press_release_uses_current_flash_and_delayed_capture() {
        for hold in [0.0f32, 1.0, 2.0, 3.5, 10.0] {
            let mut model = MurdererModel::new(20);
            let rect = top_menu_rect(2).unwrap();
            let point = Vec2::new(rect.x + 10.0, rect.y + 10.0);
            model.pointer_state(point, false);
            assert_eq!(model.menu_focus, 0);
            model.pointer_state(point, true);
            model.update(hold);
            let flash = (hold * 0.2).min(0.7);
            assert!((model.top_flash(2) - flash).abs() < 1e-6);
            assert_eq!(model.top_phase, TopPhase::Waiting);
            assert!(!model.menu_input_busy());
            assert!(model.take_sounds().is_empty());
            model.pointer_state(point, false);
            assert_eq!(model.take_sounds(), ["decide"]);
            assert_eq!(model.top_phase, TopPhase::Flash);
            model.update((0.7 - flash) / 0.2);
            assert!((model.top_flash(2) - 0.7).abs() < 1e-6);
            model.update(3.5);
            assert_eq!(model.top_flash(2), 0.0);
            assert_eq!(model.top_phase, TopPhase::Flash);
            assert!(model.wipe.is_none());
            model.update(0.0);
            model.update(0.0);
            assert_eq!(model.wipe.as_ref().unwrap().duration, 30.0);
            assert_eq!(model.wipe.as_ref().unwrap().source.menu_focus, 2);
            model.update(30.0);
            assert_eq!(model.menu, MurdererMenu::PurpleByPerson);
        }
    }
    #[test]
    fn top_pointer_cancel_and_modal_edges_do_not_confirm() {
        let point = Vec2::new(1010.0, 450.0);
        let mut model = MurdererModel::new(20);
        model.pointer_state(point, true);
        model.update(2.0);
        model.pointer_state(Vec2::ZERO, true);
        model.update(1.0);
        assert!((model.top_flash(0) - 0.2).abs() < 1e-6);
        model.pointer_state(point, false);
        model.update(1.0);
        assert_eq!(model.top_flash(0), 0.0);
        assert_eq!(model.top_phase, TopPhase::Waiting);
        assert!(model.take_sounds().is_empty());
        model.pointer_state(point, true);
        model.update(1.0);
        model.ignore_pointer_state(true);
        model.update(1.0);
        model.pointer_state(point, true);
        model.pointer_state(point, false);
        assert_eq!(model.top_flash(0), 0.0);
        assert!(model.take_sounds().is_empty());
        model.pointer_state(Vec2::ZERO, true);
        model.pointer_state(point, true);
        model.pointer_state(point, false);
        assert_eq!(model.top_phase, TopPhase::Waiting);
        // New physical presses acquired during completion are consumed even
        // after installation. Their release cannot confirm the new child.
        model.pointer(point, true);
        model.pointer_state(point, true);
        finish_top_opening(&mut model);
        model.pointer_state(Vec2::new(130.0, 200.0), true);
        model.pointer_state(Vec2::new(130.0, 200.0), false);
        assert!(model.result.is_none());
        model.reset();
        model.pointer_state(point, true);
        model.update(2.0);
        model.pointer_state(Vec2::ZERO, false);
        model.pointer_state(point + Vec2::new(0.0, 96.0), true);
        model.update(1.0);
        assert!((model.top_flash(0) - 0.2).abs() < 1e-6);
        assert!((model.top_flash(1) - 0.2).abs() < 1e-6);
        assert_eq!(model.menu_focus, 1);
        assert!(model.take_sounds().is_empty());
    }
    #[test]
    fn top_focus_retargets_from_current_values_and_restores_on_return() {
        let mut model = MurdererModel::new(20);
        assert_eq!(model.top_focus_amount(0), 1.0);
        model.navigate(1);
        assert_eq!(model.top_focus_amount(0), 1.0);
        assert_eq!(model.top_focus_amount(1), 0.0);
        model.update(2.0);
        assert!((model.top_focus_amount(0) - 0.8).abs() < 1e-6);
        assert_eq!(model.top_focus_amount(1), 0.5);
        model.navigate(-1);
        model.update(1.0);
        assert_eq!(model.top_focus_amount(0), 1.0);
        assert!((model.top_focus_amount(1) - 0.4).abs() < 1e-6);
        model.update(4.0);
        assert_eq!(model.top_focus_amount(1), 0.0);
        for index in 0..6 {
            let mut model = MurdererModel::new(20);
            model.navigate(index);
            model.confirm();
            model.update(7.0);
            model.update(0.0);
            model.update(30.0);
            model.update(0.0);
            model.update(60.0);
            model.back();
            finish_back(&mut model);
            for row in 0..6 {
                assert_eq!(
                    model.top_focus_amount(row),
                    if row == index as usize { 1.0 } else { 0.0 }
                );
            }
            model.reset();
            assert_eq!(model.top_focus_amount(0), 1.0);
        }
    }
    #[test]
    fn top_controller_and_pointer_confirmation_retarget_focus() {
        use crate::app::AppAction;
        let mut model = MurdererModel::new(20);
        let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        input[AppAction::AnyDown].is_clicked_or_repeated = true;
        assert!(model.handle_controller_input(&input));
        model.update(2.0);
        assert_eq!(model.top_focus_amount(1), 0.5);
        let rect = top_menu_rect(2).unwrap();
        let point = Vec2::new(rect.x + 10.0, rect.y + 10.0);
        model.pointer(point, false);
        assert_eq!(model.menu_focus, 1);
        model.pointer(point, true);
        assert_eq!(model.menu_focus, 2);
        assert_eq!(model.top_focus_amount(2), 0.0);
        model.update(2.0);
        assert!((model.top_focus_amount(1) - 0.3).abs() < 1e-6);
        assert_eq!(model.top_focus_amount(2), 0.5);
        model.update(5.0);
        model.update(0.0);
        let source = &model.wipe.as_ref().unwrap().source;
        assert_eq!(source.top_focus_amount(2), 1.0);
        let amount = source.top_focus_amount(0);
        model.update(10.0);
        assert_eq!(
            model.wipe.as_ref().unwrap().source.top_focus_amount(0),
            amount
        );
    }
    #[test]
    fn replay_focus_retargets_from_current_values_and_confirmation_fades_it() {
        let mut model = MurdererModel::new(20);
        model.open_menu(0);
        assert_eq!(model.replay_feedback(0), (1.0, 0.0));
        model.navigate(1);
        model.update(2.0);
        assert!((model.replay_feedback(0).0 - 0.8).abs() < 1e-6);
        assert_eq!(model.replay_feedback(1), (0.5, 0.0));
        model.navigate(-1);
        model.update(1.0);
        assert_eq!(model.replay_feedback(0).0, 1.0);
        assert!((model.replay_feedback(1).0 - 0.4).abs() < 1e-6);
        model.confirm();
        model.update(3.5);
        assert_eq!(model.replay_feedback(0), (1.0, 0.7));
        model.update(3.5);
        assert_eq!(model.replay_feedback(0), (1.0, 0.0));
        model.update(0.0);
        model.update(15.0);
        assert_eq!(model.replay_feedback(0), (0.5, 0.0));
        model.update(15.0);
        assert_eq!(model.replay_feedback(0), (0.0, 0.0));
        model.reset();
        model.open_menu(0);
        assert_eq!(model.replay_feedback(0), (1.0, 0.0));
        assert_eq!(model.replay_feedback(1), (0.0, 0.0));
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_accusation_directional_selection() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        // Reviewed input priorities, descriptor walk, aim adjustment,
        // normalization, cosine threshold, ranking, tie and no-target branch.
        for (address, expected) in [
            (0x99ddc, 0x531b6d09),
            (0x99de0, 0x7206011f),
            (0x99de4, 0x5a9f0135),
            (0x99de8, 0x53196509),
            (0x99dec, 0x7208011f),
            (0x99df0, 0x5a9f013a),
            (0x99df4, 0x2a1a02a8),
            (0x99df8, 0x34000e68),
            (0x99dfc, 0xb9801668),
            (0x99e00, 0xf9404269),
            (0x99e04, 0x52801a0a),
            (0x99e08, 0x9b0a2508),
            (0x99e0c, 0x1b1a7f49),
            (0x99e10, 0x1b1526a9),
            (0x99e14, 0x1e220121),
            (0x99e18, 0xb9400519),
            (0x99e1c, 0x1e21c020),
            (0x99e20, 0x1e202000),
            (0x99e24, 0x54000067),
            (0x99e28, 0x4ea11c20),
            (0x99e2c, 0x94020b5d),
            (0x99e30, 0xa9485e76),
            (0x99e34, 0xeb1702df),
            (0x99e38, 0x540017a0),
            (0x99e3c, 0xb9801668),
            (0x99e40, 0x1e2e100a),
            (0x99e44, 0x52801a14),
            (0x99e48, 0x1e2202a1),
            (0x99e4c, 0x9b145908),
            (0x99e50, 0x1e201940),
            (0x99e54, 0xf0000c58),
            (0x99e58, 0xf9476f18),
            (0x99e5c, 0x1e220342),
            (0x99e60, 0xb9800508),
            (0x99e64, 0x8b080b08),
            (0x99e68, 0x39400909),
            (0x99e6c, 0x39400508),
            (0x99e70, 0x531d72aa),
            (0x99e74, 0x4b15014a),
            (0x99e78, 0x7100b73f),
            (0x99e7c, 0x1a95014a),
            (0x99e80, 0x12800015),
            (0x99e84, 0x0b080159),
            (0x99e88, 0x90000608),
            (0x99e8c, 0xbd47710e),
            (0x99e90, 0x90000608),
            (0x99e94, 0x1e21080b),
            (0x99e98, 0x1e22080c),
            (0x99e9c, 0xbd4ab90d),
            (0x99ea0, 0x0b09035a),
            (0x99ea4, 0xb98006c8),
            (0x99ea8, 0xd1007109),
            (0x99eac, 0xf100453f),
            (0x99eb0, 0x54000488),
            (0x99eb4, 0xb9401669),
            (0x99eb8, 0xb94002ca),
            (0x99ebc, 0x6b0a013f),
            (0x99ec0, 0x54000400),
            (0x99ec4, 0x8b080b08),
            (0x99ec8, 0x39400509),
            (0x99ecc, 0x39400908),
            (0x99ed0, 0x4b190129),
            (0x99ed4, 0x4b1a0108),
            (0x99ed8, 0x1e22012f),
            (0x99edc, 0x1e2f09e0),
            (0x99ee0, 0x1e220108),
            (0x99ee4, 0x1e280901),
            (0x99ee8, 0x1e212800),
            (0x99eec, 0x1e21c009),
            (0x99ef0, 0x1e292120),
            (0x99ef4, 0x54000067),
            (0x99ef8, 0x94020b2a),
            (0x99efc, 0x4ea01c09),
            (0x99f00, 0x1e291940),
            (0x99f04, 0x1e2f0801),
            (0x99f08, 0x1e280800),
            (0x99f0c, 0x1e210961),
            (0x99f10, 0x1e200980),
            (0x99f14, 0x1e202820),
            (0x99f18, 0x1e2d2000),
            (0x99f1c, 0x5400012d),
            (0x99f20, 0x1e221001),
            (0x99f24, 0x94020b47),
            (0x99f28, 0x1e201940),
            (0x99f2c, 0x1e200920),
            (0x99f30, 0x1e2021c0),
            (0x99f34, 0x5400006d),
            (0x99f38, 0xb94002d5),
            (0x99f3c, 0x4ea01c0e),
            (0x99f40, 0x910342d6),
            (0x99f44, 0xeb1602ff),
            (0x99f48, 0x54fffae1),
            (0x99f4c, 0x37f80f15),
            (0x99d90, 0xb9401008),
            (0x99d94, 0x34000068),
            (0x99d98, 0x320003e0),
            (0x99d9c, 0xd65f03c0),
            (0x99fc4, 0xb9400fe8),
            (0x99fc8, 0x37e00308),
            (0x99fcc, 0x36e80b08),
            (0x9a12c, 0x6d4623e9),
            (0x9a150, 0x910303ff),
            (0x9a154, 0x320003e0),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        assert_eq!(word(0x159770), f32::MAX.to_bits());
        assert_eq!(word(0x159ab8), f32::EPSILON.to_bits());
        for entry in FIRST_SUSPECT_ENTRY..=SUBMIT_ENTRY {
            let base = 256 + 0x17118b + entry * 4;
            let expected = if entry == SUBMIT_ENTRY {
                (7, 21)
            } else {
                SUSPECT_GRID[entry - FIRST_SUSPECT_ENTRY]
            };
            assert_eq!((exe[base + 1], exe[base + 2]), expected);
        }
        let reloc = exe[0x11e040 + 256..0x152eb8 + 256]
            .chunks_exact(24)
            .find(|r| u64::from_le_bytes(r[..8].try_into().unwrap()) == 0x224ed8)
            .unwrap();
        assert_eq!(
            u64::from_le_bytes(reloc[16..24].try_into().unwrap()),
            0x17118b
        );
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "executes reviewed supplied ARM64 controller with stubbed external calls"]
    fn reference_native_murderer_accusation_executed_targets() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary =
            std::env::temp_dir().join(format!("shin-accusation-oracle-{}", std::process::id()));
        let compile = std::process::Command::new("/usr/bin/clang")
            .args(["-O0"])
            .arg(root.join("reverse-engineering/native-accusation-controller.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let output = std::process::Command::new(&binary)
            .arg(
                std::path::Path::new(&rom)
                    .parent()
                    .unwrap()
                    .join("uncompressed_executable"),
            )
            .output()
            .unwrap();
        std::fs::remove_file(binary).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rows: Vec<Vec<usize>> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| {
                line.trim_matches(|c| c == '[' || c == ']' || c == ',')
                    .split(',')
                    .map(|value| value.parse().unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(rows.len(), 18);
        for (row, expected) in rows.iter().zip(NATIVE_ACCUSATION_TARGETS) {
            assert_eq!(row.as_slice(), expected);
        }
    }
    // Golden tables from the independently executed reviewed ARM64 Purple
    // controller; person rows include legacy empty-transcript starting states.
    const NATIVE_PURPLE_PERSON_TARGETS: [[usize; 16]; 17] = [
        [0, 11, 1, 11, 0, 0, 2, 0, 14, 11, 8, 11, 0, 0, 2, 0],
        [1, 0, 2, 0, 1, 0, 2, 0, 8, 11, 5, 11, 1, 0, 2, 0],
        [2, 1, 2, 1, 2, 0, 2, 0, 5, 13, 8, 13, 2, 0, 2, 0],
        [3, 11, 5, 11, 0, 0, 1, 0, 14, 13, 10, 13, 0, 0, 1, 0],
        [4, 11, 5, 11, 1, 0, 2, 0, 15, 13, 8, 13, 1, 0, 2, 0],
        [5, 11, 5, 11, 2, 1, 2, 1, 8, 14, 10, 14, 2, 1, 2, 1],
        [6, 11, 8, 11, 0, 11, 2, 11, 14, 13, 16, 13, 0, 11, 2, 11],
        [7, 11, 8, 11, 1, 0, 5, 0, 15, 13, 10, 13, 1, 0, 5, 0],
        [8, 11, 8, 11, 5, 0, 2, 0, 10, 14, 16, 14, 5, 0, 2, 0],
        [9, 13, 10, 13, 0, 11, 5, 11, 14, 13, 15, 13, 0, 11, 5, 11],
        [10, 14, 10, 14, 8, 0, 5, 0, 16, 15, 16, 15, 8, 0, 5, 0],
        [11, 11, 5, 11, 0, 11, 0, 11, 13, 13, 10, 13, 0, 11, 0, 11],
        [12, 12, 10, 12, 11, 11, 1, 11, 13, 13, 14, 13, 11, 11, 1, 11],
        [13, 13, 14, 13, 11, 11, 8, 11, 13, 13, 15, 13, 11, 11, 8, 11],
        [14, 13, 15, 13, 0, 11, 8, 11, 14, 13, 16, 13, 0, 11, 8, 11],
        [15, 14, 16, 14, 8, 11, 10, 11, 15, 13, 16, 13, 8, 11, 10, 11],
        [16, 15, 16, 15, 10, 11, 8, 11, 16, 14, 16, 14, 10, 11, 8, 11],
    ];
    const NATIVE_PURPLE_CHAPTER_TARGETS: [[usize; 16]; 23] = [
        [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0],
        [1, 1, 1, 1, 0, 0, 0, 0, 2, 2, 2, 2, 0, 0, 0, 0],
        [2, 2, 2, 2, 1, 1, 1, 1, 3, 3, 3, 3, 1, 1, 1, 1],
        [3, 3, 3, 3, 2, 2, 2, 2, 4, 4, 4, 4, 2, 2, 2, 2],
        [4, 4, 4, 4, 3, 3, 3, 3, 5, 5, 5, 5, 3, 3, 3, 3],
        [5, 5, 5, 5, 4, 4, 4, 4, 6, 6, 6, 6, 4, 4, 4, 4],
        [6, 6, 6, 6, 5, 5, 5, 5, 7, 7, 7, 7, 5, 5, 5, 5],
        [7, 7, 7, 7, 6, 6, 6, 6, 8, 8, 8, 8, 6, 6, 6, 6],
        [8, 8, 8, 8, 7, 7, 7, 7, 9, 9, 9, 9, 7, 7, 7, 7],
        [9, 9, 9, 9, 8, 8, 8, 8, 10, 10, 10, 10, 8, 8, 8, 8],
        [10, 10, 10, 10, 9, 9, 9, 9, 11, 11, 11, 11, 9, 9, 9, 9],
        [
            11, 11, 11, 11, 10, 10, 10, 10, 12, 12, 12, 12, 10, 10, 10, 10,
        ],
        [
            12, 12, 12, 12, 11, 11, 11, 11, 13, 13, 13, 13, 11, 11, 11, 11,
        ],
        [
            13, 13, 13, 13, 12, 12, 12, 12, 14, 14, 14, 14, 12, 12, 12, 12,
        ],
        [
            14, 14, 14, 14, 13, 13, 13, 13, 15, 15, 15, 15, 13, 13, 13, 13,
        ],
        [
            15, 15, 15, 15, 14, 14, 14, 14, 16, 16, 16, 16, 14, 14, 14, 14,
        ],
        [
            16, 16, 16, 16, 15, 15, 15, 15, 17, 17, 17, 17, 15, 15, 15, 15,
        ],
        [
            17, 17, 17, 17, 16, 16, 16, 16, 18, 18, 18, 18, 16, 16, 16, 16,
        ],
        [
            18, 18, 18, 18, 17, 17, 17, 17, 19, 19, 19, 19, 17, 17, 17, 17,
        ],
        [
            19, 19, 19, 19, 18, 18, 18, 18, 20, 20, 20, 20, 18, 18, 18, 18,
        ],
        [
            20, 20, 20, 20, 19, 19, 19, 19, 21, 21, 21, 21, 19, 19, 19, 19,
        ],
        [
            21, 21, 21, 21, 20, 20, 20, 20, 22, 22, 22, 22, 20, 20, 20, 20,
        ],
        [
            22, 22, 22, 22, 21, 21, 21, 21, 22, 22, 22, 22, 21, 21, 21, 21,
        ],
    ];
    #[test]
    fn purple_controller_matches_native_execution_and_cancel_priorities() {
        use crate::app::AppAction;
        for (menu, targets) in [
            (2, NATIVE_PURPLE_PERSON_TARGETS.as_slice()),
            (3, NATIVE_PURPLE_CHAPTER_TARGETS.as_slice()),
        ] {
            for (original, directions) in targets.iter().enumerate() {
                for bits in 0u8..64 {
                    let mut model = MurdererModel::new(20);
                    model.open_menu(menu);
                    model.menu_focus = original;
                    model.purple_text_scroll = 123.0;
                    model.take_sounds();
                    let mut input =
                        enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                    for (bit, action) in [
                        AppAction::Left,
                        AppAction::Right,
                        AppAction::Up,
                        AppAction::AnyDown,
                        AppAction::Enter,
                        AppAction::Cancel,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        input[action] = shin_input::ActionState {
                            is_clicked: bits & (1 << bit) != 0,
                            is_clicked_or_repeated: bits & (1 << bit) != 0,
                            ..Default::default()
                        };
                    }
                    assert!(model.handle_controller_input(&input));
                    let expected = directions[(bits & 15) as usize];
                    assert_eq!(model.menu_focus, expected, "{menu}/{original}/{bits}");
                    let changed = expected != original;
                    let cancel = !changed && (menu == 3 || bits & 15 == 0) && bits & 32 != 0;
                    assert_eq!(model.back_completion.is_some(), cancel);
                    assert_eq!(model.purple_list_scrolling(), menu == 3 && changed);
                    assert_eq!(model.purple_text_scroll, if changed { 0.0 } else { 123.0 });
                    assert_eq!(
                        model.take_sounds(),
                        if changed {
                            vec!["move"]
                        } else if cancel {
                            vec!["cancel"]
                        } else {
                            vec![]
                        }
                    );
                    assert_eq!(model.result, None); // Enter never responds.
                    if menu == 2 && changed {
                        assert!(!PERSON_TEXT[expected].is_empty());
                    }
                }
            }
        }
    }
    #[test]
    fn purple_chapter_scroll_reveals_heading_margin_and_gates_four_ticks() {
        use crate::app::AppAction;
        let mut model = MurdererModel::new(20);
        model.open_menu(3);
        model.select_purple_original_index(7);
        model.purple_list_scroll = 0.0;
        model.purple_list_motion = Tweener::new(0.0);
        let mut down = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        down[AppAction::AnyDown].is_clicked_or_repeated = true;
        model.handle_controller_input(&down);
        assert_eq!(model.purple_original_index(), Some(8));
        assert_eq!(model.purple_list_scroll, 0.0);
        model.update(2.0);
        assert_eq!(model.purple_list_scroll, 45.0);
        down[AppAction::Cancel].is_clicked = true;
        model.take_sounds();
        model.handle_controller_input(&down);
        assert_eq!(model.purple_original_index(), Some(8));
        assert!(model.take_sounds().is_empty());
        assert!(model.back_completion.is_none());
        model.update(2.0);
        assert_eq!(model.purple_list_scroll, 90.0);
        assert!(!model.purple_list_scrolling());
        model.purple_list_scroll = 800.0;
        model.purple_list_motion = Tweener::new(800.0);
        let mut up = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        up[AppAction::Up].is_clicked_or_repeated = true;
        model.handle_controller_input(&up);
        model.update(4.0);
        assert_eq!(model.purple_original_index(), Some(7));
        assert_eq!(model.purple_list_scroll, 540.0); //630top minus90heading margin.
        model.handle_controller_input(&down);
        model.update(4.0);
        assert_eq!(model.purple_list_scroll, 540.0);
        assert!(!model.purple_list_scrolling());
        let mut page = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        page[AppAction::PageDown].is_clicked_or_repeated = true;
        assert!(!model.handle_controller_input(&page)); // Desktop fallback only.
        page[AppAction::Up].is_clicked_or_repeated = true;
        assert!(model.handle_controller_input(&page));
        model.reset();
        assert!(!model.purple_list_scrolling());
        assert_eq!(model.purple_list_scroll, 0.0);
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_purple_directional_selection() {
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
            (0x9680c, 0xb9401408),
            (0x96810, 0x350016c8),
            (0x96814, 0xaa0003f3),
            (0x96818, 0x9100c000),
            (0x9681c, 0x94009475),
            (0x96820, 0x37001640),
            (0x96824, 0x910023e8),
            (0x96828, 0x9400b85e),
            (0x9682c, 0xb9401268),
            (0x96830, 0x34000268),
            (0x96834, 0xb9401a68),
            (0x96838, 0xb94013e9),
            (0x9683c, 0x37c00d09),
            (0x96840, 0x36c80d69),
            (0x96844, 0xf9411e69),
            (0x96848, 0xf9411a6a),
            (0x9684c, 0xcb0a0129),
            (0x96850, 0x528b852a),
            (0x96854, 0x72b851ea),
            (0x96858, 0xd343fd29),
            (0x9685c, 0x1b0a7d29),
            (0x96860, 0x11000514),
            (0x96864, 0x6b09029f),
            (0x96868, 0x54000feb),
            (0x9686c, 0x51000534),
            (0x96870, 0x6b14011f),
            (0x96874, 0x54000bc0),
            (0x96878, 0x1400007b),
            (0x9687c, 0xb94013e8),
            (0x96880, 0x531b6d09),
            (0x96884, 0x7206011f),
            (0x96888, 0x5a9f0134),
            (0x9688c, 0x53196509),
            (0x96890, 0x7208011f),
            (0x96894, 0x5a9f0139),
            (0x96898, 0x2a190288),
            (0x9689c, 0x34000a88),
            (0x968a0, 0x1b197f28),
            (0x968a4, 0x1b142288),
            (0x968a8, 0x1e220101),
            (0x968ac, 0x1e21c020),
            (0x968b0, 0x1e202000),
            (0x968b4, 0x54000067),
            (0x968b8, 0x4ea11c20),
            (0x968bc, 0x940218b9),
            (0x968c0, 0xf9411a75),
            (0x968c4, 0xf9411e76),
            (0x968c8, 0xeb1602bf),
            (0x968cc, 0x540010e0),
            (0x968d0, 0xb9801a68),
            (0x968d4, 0x1e2e100a),
            (0x968d8, 0x52801909),
            (0x968dc, 0x1e220281),
            (0x968e0, 0x1e201940),
            (0x968e4, 0x1e220322),
            (0x968e8, 0x9b095508),
            (0x968ec, 0xd0000c77),
            (0x968f0, 0x1e21080b),
            (0x968f4, 0xb9800508),
            (0x968f8, 0xf9476ef7),
            (0x968fc, 0x8b080ae8),
            (0x96900, 0x39400909),
            (0x96904, 0x39400508),
            (0x96908, 0x0b080298),
            (0x9690c, 0xf0000608),
            (0x96910, 0xbd47710e),
            (0x96914, 0xf0000608),
            (0x96918, 0x1e22080c),
            (0x9691c, 0xb2407bfa),
            (0x96920, 0x0b090339),
            (0x96924, 0x12800014),
            (0x96928, 0xf2c7da5a),
            (0x9692c, 0xbd4ab90d),
            (0x96930, 0xb98006a8),
            (0x96934, 0x9ac82749),
            (0x96938, 0x36000489),
            (0x9693c, 0xb9401a69),
            (0x96940, 0xb94002aa),
            (0x96944, 0x6b0a013f),
            (0x96948, 0x54000400),
            (0x9694c, 0x8b080ae8),
            (0x96950, 0x39400509),
            (0x96954, 0x39400908),
            (0x96958, 0x4b180129),
            (0x9695c, 0x4b190108),
            (0x96960, 0x1e22012f),
            (0x96964, 0x1e2f09e0),
            (0x96968, 0x1e220108),
            (0x9696c, 0x1e280901),
            (0x96970, 0x1e212800),
            (0x96974, 0x1e21c009),
            (0x96978, 0x1e292120),
            (0x9697c, 0x54000067),
            (0x96980, 0x94021888),
            (0x96984, 0x4ea01c09),
            (0x96988, 0x1e291940),
            (0x9698c, 0x1e2f0801),
            (0x96990, 0x1e280800),
            (0x96994, 0x1e210961),
            (0x96998, 0x1e200980),
            (0x9699c, 0x1e202820),
            (0x969a0, 0x1e2d2000),
            (0x969a4, 0x5400012d),
            (0x969a8, 0x1e221001),
            (0x969ac, 0x940218a5),
            (0x969b0, 0x1e201940),
            (0x969b4, 0x1e200920),
            (0x969b8, 0x1e2021c0),
            (0x969bc, 0x5400006d),
            (0x969c0, 0xb94002b4),
            (0x969c4, 0x4ea01c0e),
            (0x969c8, 0x910322b5),
            (0x969cc, 0xeb1502df),
            (0x969d0, 0x54fffb01),
            (0x969d4, 0x36f80494),
            (0x969d8, 0x14000044),
            (0x969dc, 0x51000509),
            (0x969e0, 0x0aa97d34),
            (0x969e4, 0x6b14011f),
            (0x969e8, 0x540003e1),
            (0x969ec, 0x39403fe8),
            (0x969f0, 0x372804e8),
            (0x96a64, 0xd0000c68),
            (0x96a68, 0xf943c108),
            (0x96a6c, 0xf9400100),
            (0x96a70, 0x1e2e1000),
            (0x96a74, 0x97fe420f),
            (0x96a78, 0xaa1303e0),
            (0x96a7c, 0x2a1403e1),
            (0x96a80, 0x320003e2),
            (0x96a84, 0x94000027),
            (0x96a88, 0x14000018),
            (0x96a8c, 0xd0000c68),
            (0x96b34, 0xb9801808),
            (0x96b38, 0x6b01011f),
            (0x96b3c, 0x54000d00),
            (0x96b40, 0xf9411809),
            (0x96b44, 0x52801916),
            (0x96b48, 0x9b162508),
            (0x96b4c, 0xaa0003f3),
            (0x96b50, 0x91002100),
            (0x96b54, 0x2a0203f4),
            (0x96b58, 0x2a0103f5),
            (0x96b5c, 0x94009231),
            (0x96b60, 0xf0000608),
            (0x96b64, 0x1e2703e8),
            (0x96b68, 0x4ea81d00),
            (0x96b6c, 0xbd457101),
            (0x96b70, 0x321a03e1),
            (0x96b74, 0x2a1f03e2),
            (0x96b78, 0x940093a2),
            (0x96b7c, 0xf9411a68),
            (0x96b80, 0x9b3622a8),
            (0x96b84, 0x91002100),
            (0x96b88, 0xb9001a75),
            (0x96b8c, 0x94009225),
            (0x96b90, 0x1e2e1000),
            (0x96b94, 0x1e2a1001),
            (0x96b98, 0x321a03e1),
            (0x96b9c, 0x2a1f03e2),
            (0x96ba0, 0x94009398),
            (0x96ba4, 0xb9401268),
            (0x96ba8, 0x34000488),
            (0x96bac, 0xf9411a69),
            (0x96bb0, 0x93407ea8),
            (0x96bb4, 0x5280190a),
            (0x96bb8, 0x9b0a2508),
            (0x96bbc, 0xd0000c69),
            (0x96bc0, 0x321d07ea),
            (0x96bc4, 0xb9800508),
            (0x96bc8, 0xf9476929),
            (0x96bcc, 0x9b0a2508),
            (0x96bd0, 0xf9400901),
            (0x96bd4, 0xaa1303e0),
            (0x96bd8, 0x97fff652),
            (0x96bdc, 0x36000814),
            (0x96be0, 0x9100c275),
            (0x96be4, 0xaa1503e0),
            (0x96be8, 0x9400920e),
            (0x96bec, 0xb9801a76),
            (0x96bf0, 0xaa0003f4),
            (0x96bf4, 0xaa1503e0),
            (0x96bf8, 0x9400922a),
            (0x96bfc, 0xf9411a68),
            (0x96c00, 0x52801909),
            (0x96c04, 0x9b0922c8),
            (0x96c08, 0xb9401269),
            (0x96c0c, 0x7100013f),
            (0x96c10, 0xf0000609),
            (0x96c14, 0xbd40bd01),
            (0x96c18, 0xbd49e122),
            (0x96c1c, 0x1e203823),
            (0x96c20, 0x1e220d02),
            (0x96c24, 0x1e222060),
            (0x96c28, 0x54000345),
            (0x96c2c, 0x1e223820),
            (0x96c30, 0x1e284800),
            (0x96c34, 0x14000025),
            (0x96c90, 0xbd40c502),
            (0x96c94, 0xf0000608),
            (0x96c98, 0xbd498104),
            (0x96c9c, 0x1e222863),
            (0x96ca0, 0x1e242060),
            (0x96ca4, 0x5400012d),
            (0x96ca8, 0xf0000608),
            (0x96cac, 0xbd408a60),
            (0x96cb0, 0x1e222821),
            (0x96cb4, 0xbd43a903),
            (0x96cb8, 0x1e232800),
            (0x96cbc, 0x1e232821),
            (0x96cc0, 0x1e202020),
            (0x96cc4, 0x1e21cc00),
            (0x96cc8, 0x1e221001),
            (0x96ccc, 0xaa1403e0),
            (0x96cd0, 0x2a1f03e1),
            (0x96cd4, 0x2a1f03e2),
            (0x96cd8, 0x9400934a),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        for (address, value) in [(0x1599e0, 90.0f32), (0x159980, 720.0), (0x1593a8, -720.0)] {
            assert_eq!(word(address), value.to_bits());
        }
        let mask = 0x3ed27fffffff_u64;
        for person in 0..17 {
            assert_eq!(
                mask & (1 << (person + 28)) != 0,
                !PERSON_TEXT[person].is_empty()
            );
            let flags = exe[256 + 0x17118b + (person + 28) * 4 + 3];
            assert_eq!(flags, SUSPECT_FLAGS[person]);
            assert_eq!(flags & 0x80 == 0, !PERSON_TEXT[person].is_empty());
        }
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "executes reviewed supplied ARM64 Purple controller with external calls stubbed"]
    fn reference_native_murderer_purple_executed_targets() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary =
            std::env::temp_dir().join(format!("shin-purple-oracle-{}", std::process::id()));
        let compile = std::process::Command::new("/usr/bin/clang")
            .args(["-O0"])
            .arg(root.join("reverse-engineering/native-purple-controller.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let output = std::process::Command::new(&binary)
            .arg(
                std::path::Path::new(&rom)
                    .parent()
                    .unwrap()
                    .join("uncompressed_executable"),
            )
            .output()
            .unwrap();
        std::fs::remove_file(binary).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rows: Vec<Vec<usize>> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| {
                line.trim_matches(|c| c == '[' || c == ']' || c == ',')
                    .split(',')
                    .map(|value| value.parse().unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(rows.len(), 40);
        for (row, expected) in rows.iter().zip(
            NATIVE_PURPLE_PERSON_TARGETS
                .iter()
                .chain(NATIVE_PURPLE_CHAPTER_TARGETS.iter()),
        ) {
            assert_eq!(row.as_slice(), expected);
        }
    }
    // Captured by executing reviewed ARM64 controller0x99d90 with external
    // input/cue/tween calls stubbed. Reproduce with the audit C harness.
    const NATIVE_ACCUSATION_TARGETS: [[usize; 16]; 18] = [
        [
            28, 39, 29, 39, 28, 28, 30, 28, 31, 39, 32, 39, 28, 28, 30, 28,
        ],
        [
            29, 28, 30, 28, 29, 28, 30, 28, 32, 31, 33, 31, 29, 28, 30, 28,
        ],
        [
            30, 29, 30, 29, 30, 28, 30, 28, 33, 32, 36, 32, 30, 28, 30, 28,
        ],
        [
            31, 39, 32, 39, 28, 28, 29, 28, 34, 40, 35, 40, 28, 28, 29, 28,
        ],
        [
            32, 31, 33, 31, 29, 28, 30, 28, 35, 34, 36, 34, 29, 28, 30, 28,
        ],
        [
            33, 32, 33, 32, 30, 29, 30, 29, 36, 35, 38, 35, 30, 29, 30, 29,
        ],
        [
            34, 39, 35, 39, 31, 39, 32, 39, 37, 40, 44, 40, 31, 39, 32, 39,
        ],
        [
            35, 34, 36, 34, 32, 31, 33, 31, 43, 37, 38, 37, 32, 31, 33, 31,
        ],
        [
            36, 35, 36, 35, 33, 32, 30, 32, 38, 42, 44, 42, 33, 32, 30, 32,
        ],
        [
            37, 40, 38, 40, 34, 39, 35, 39, 42, 41, 43, 41, 34, 39, 35, 39,
        ],
        [
            38, 37, 38, 37, 36, 35, 33, 35, 44, 43, 44, 43, 36, 35, 33, 35,
        ],
        [
            39, 39, 31, 39, 28, 39, 28, 39, 40, 40, 34, 40, 28, 39, 28, 39,
        ],
        [
            40, 40, 37, 40, 39, 39, 34, 39, 41, 41, 42, 41, 39, 39, 34, 39,
        ],
        [
            41, 41, 42, 41, 40, 39, 37, 39, 45, 41, 45, 41, 40, 39, 37, 39,
        ],
        [
            42, 41, 43, 41, 37, 40, 36, 40, 45, 41, 45, 41, 37, 40, 36, 40,
        ],
        [
            43, 42, 44, 42, 35, 37, 38, 37, 45, 45, 44, 45, 35, 37, 38, 37,
        ],
        [
            44, 43, 44, 43, 38, 34, 36, 34, 45, 45, 44, 45, 38, 34, 36, 34,
        ],
        [
            45, 45, 45, 45, 42, 40, 38, 40, 45, 45, 45, 45, 42, 40, 38, 40,
        ],
    ];
    #[test]
    fn accusation_controller_matches_native_execution_and_action_priority() {
        use crate::app::AppAction;
        for (row, targets) in NATIVE_ACCUSATION_TARGETS.iter().enumerate() {
            let original = FIRST_SUSPECT_ENTRY + row;
            for bits in 0u8..64 {
                let mut model = MurdererModel::new(20);
                model.open_menu(5);
                model.accusation_focus = Some(original);
                model.take_sounds();
                let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                for (bit, action) in [
                    AppAction::Left,
                    AppAction::Right,
                    AppAction::Up,
                    AppAction::AnyDown,
                    AppAction::Enter,
                    AppAction::Cancel,
                ]
                .into_iter()
                .enumerate()
                {
                    input[action] = shin_input::ActionState {
                        is_clicked: bits & (1 << bit) != 0,
                        is_clicked_or_repeated: bits & (1 << bit) != 0,
                        ..Default::default()
                    };
                }
                assert!(model.handle_controller_input(&input));
                let expected = targets[(bits & 15) as usize];
                assert_eq!(model.accusation_focus, Some(expected), "{original}/{bits}");
                if bits & 15 != 0 {
                    assert_eq!(
                        model.take_sounds(),
                        if expected == original {
                            vec![]
                        } else {
                            vec!["move"]
                        }
                    );
                    assert!(model.suspects.iter().all(|selected| !selected));
                    assert!(model.completion.is_none());
                    assert!(model.back_completion.is_none());
                } else if bits & 16 != 0 {
                    assert!(model.back_completion.is_none());
                    if original == SUBMIT_ENTRY {
                        assert!(model.completion.is_some());
                        assert_eq!(model.take_sounds(), ["attack"]);
                    } else {
                        assert!(model.suspect_selected(original));
                        assert_eq!(model.take_sounds(), ["decide"]);
                    }
                } else if bits & 32 != 0 {
                    assert!(model.back_completion.is_some());
                    assert_eq!(model.take_sounds(), ["cancel"]);
                } else {
                    assert!(model.take_sounds().is_empty());
                }
            }
        }
    }
    #[test]
    fn accusation_controller_uses_repeat_edges_and_consumes_completion_input() {
        use crate::app::AppAction;
        let mut model = MurdererModel::new(20);
        model.open_menu(5);
        let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        input[AppAction::Right].is_held = true;
        model.handle_controller_input(&input);
        assert_eq!(model.accusation_focus, Some(28));
        input[AppAction::Right].is_clicked_or_repeated = true;
        model.handle_controller_input(&input);
        assert_eq!(model.accusation_focus, Some(29));
        model.accusation_focus = Some(SUBMIT_ENTRY);
        model.confirm();
        model.take_sounds();
        input[AppAction::Up].is_clicked_or_repeated = true;
        input[AppAction::Enter].is_clicked = true;
        input[AppAction::Cancel].is_clicked = true;
        for ticks in [0.0, 7.0, 0.0, 60.0, 0.0, 120.0] {
            model.update(ticks);
            model.take_sounds(); // Completion may emit its independent failure cue.
            assert!(model.handle_controller_input(&input));
            assert_eq!(model.accusation_focus, Some(SUBMIT_ENTRY));
            assert!(model.take_sounds().is_empty());
        }
    }
    #[test]
    fn replay_controller_wraps_grid_and_consumes_simultaneous_actions() {
        use crate::app::AppAction;
        let right = [1, 2, 0, 4, 5, 3];
        let left = [2, 0, 1, 5, 3, 4];
        let vertical = [3, 4, 5, 0, 1, 2];
        for original in 0..6 {
            for bits in 0u8..64 {
                let mut model = MurdererModel::new(20);
                model.open_menu(0);
                model.navigate(original as i32);
                model.take_sounds();
                let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                for (bit, action) in [
                    AppAction::Left,
                    AppAction::Right,
                    AppAction::Up,
                    AppAction::AnyDown,
                    AppAction::Enter,
                    AppAction::Cancel,
                ]
                .into_iter()
                .enumerate()
                {
                    input[action] = shin_input::ActionState {
                        is_clicked: bits & (1 << bit) != 0,
                        is_clicked_or_repeated: bits & (1 << bit) != 0,
                        ..Default::default()
                    };
                }
                assert!(model.handle_replay_controller_input(&input));
                finish_back(&mut model);
                let vertical_target = if bits & 12 != 0 {
                    vertical[original]
                } else {
                    original
                };
                let expected = if bits & 1 != 0 {
                    left[vertical_target]
                } else if bits & 2 != 0 {
                    right[vertical_target]
                } else {
                    vertical_target
                };
                assert_eq!(
                    model.menu_focus,
                    if model.menu == MurdererMenu::Replay {
                        expected
                    } else {
                        0
                    }
                );
                if expected != original {
                    assert_eq!(model.menu, MurdererMenu::Replay);
                    assert_eq!(model.result, None);
                    assert_eq!(model.take_sounds(), ["move"]);
                } else if bits & 16 != 0 {
                    assert_eq!(model.result, Some(original as i32));
                    assert_eq!(model.poll_result(), None);
                    assert_eq!(model.take_sounds(), ["decide"]);
                } else if bits & 32 != 0 {
                    assert_eq!(model.menu, MurdererMenu::Top);
                    assert_eq!(model.take_sounds(), ["cancel"]);
                }
            }
        }
        let mut model = MurdererModel::new(20);
        model.open_menu(0);
        let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        input[AppAction::Right].is_held = true;
        model.handle_replay_controller_input(&input);
        finish_back(&mut model);
        assert_eq!(model.menu_focus, 0);
        input[AppAction::Right].is_clicked_or_repeated = true;
        model.handle_replay_controller_input(&input);
        finish_back(&mut model);
        assert_eq!(model.menu_focus, 1);
    }
    #[test]
    fn hint_controller_clamps_and_prioritizes_movement_then_confirmation() {
        use crate::app::AppAction;
        for progress in 0..21 {
            for original in 0..=progress as usize {
                for bits in 0u8..16 {
                    let mut model = MurdererModel::new(progress);
                    model.open_menu(4);
                    model.navigate(original as i32 - progress);
                    model.take_sounds();
                    let mut input =
                        enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                    for (bit, action) in [
                        AppAction::Up,
                        AppAction::AnyDown,
                        AppAction::Enter,
                        AppAction::Cancel,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        input[action] = shin_input::ActionState {
                            is_clicked: bits & (1 << bit) != 0,
                            is_clicked_or_repeated: bits & (1 << bit) != 0,
                            ..Default::default()
                        };
                    }
                    model.handle_controller_input(&input);
                    finish_back(&mut model);
                    let expected = if bits & 1 != 0 {
                        original.saturating_sub(1)
                    } else if bits & 2 != 0 {
                        (original + 1).min(progress as usize)
                    } else {
                        original
                    };
                    if expected != original {
                        assert_eq!(model.menu_focus, expected);
                        assert_eq!(model.menu, MurdererMenu::Hints);
                        assert_eq!(model.result, None);
                        assert_eq!(model.take_sounds(), ["move"]);
                        assert!(!model.hint_scroll_motion.is_idle());
                    } else if bits & 4 != 0 {
                        assert_eq!(model.result, Some(original as i32 + 6));
                        assert_eq!(model.poll_result(), None);
                        assert_eq!(model.take_sounds(), ["decide"]);
                    } else if bits & 8 != 0 {
                        assert_eq!(model.menu, MurdererMenu::Top);
                        assert_eq!(model.take_sounds(), ["cancel"]);
                    } else {
                        assert_eq!(model.menu_focus, original);
                        assert!(model.take_sounds().is_empty());
                    }
                }
            }
        }
    }
    #[test]
    fn every_submenu_flashes_back_stages_top_and_restores_focus_after_wipe() {
        use crate::app::AppAction;
        for index in 0..6 {
            for pointer in [false, true] {
                let mut model = MurdererModel::new(20);
                model.open_menu(index);
                model.take_sounds();
                let submenu = model.menu;
                if pointer {
                    model.pointer(Vec2::new(1700.0, 60.0), true);
                } else {
                    model.back();
                }
                assert_eq!(
                    model.take_sounds(),
                    [if pointer { "decide" } else { "cancel" }]
                );
                assert!(model.menu_input_busy());
                assert_eq!(model.menu, submenu);
                model.update(3.5);
                assert_eq!(model.back_flash(), 0.7);
                let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                for action in [
                    AppAction::Up,
                    AppAction::AnyDown,
                    AppAction::Enter,
                    AppAction::Cancel,
                ] {
                    input[action].is_clicked = true;
                    input[action].is_clicked_or_repeated = true;
                }
                assert!(model.handle_controller_input(&input));
                model.confirm();
                model.back();
                model.pointer(Vec2::new(1200.0, 500.0), true);
                assert!(model.take_sounds().is_empty());
                assert_eq!(model.result, None);
                model.update(3.5);
                assert_eq!(model.back_flash(), 0.0);
                assert!(model.wipe.is_none());
                model.update(0.0);
                assert_eq!(model.menu, submenu);
                assert!(model.pending_top);
                let source = &model.wipe.as_ref().unwrap().source;
                assert_eq!(source.menu, submenu);
                assert_eq!(source.back_flash(), 0.0);
                assert_eq!(model.wipe.as_ref().unwrap().duration, 30.0);
                model.update(15.0);
                assert_eq!(model.menu, MurdererMenu::Top);
                assert_eq!(model.menu_focus, index);
                assert_eq!(model.back_flash(), 0.0);
                assert!(!model.top_accusation_art);
                assert!(model.menu_input_busy());
                assert_eq!(model.wipe.as_ref().unwrap().source.menu, submenu);
                assert!(model.handle_controller_input(&input));
                assert!(model.take_sounds().is_empty());
                model.update(15.0);
                assert!(!model.menu_input_busy());
                assert!(model.wipe.is_none());
                assert_eq!(model.result, None);
            }
        }
        let mut model = MurdererModel::new(20);
        model.open_menu(4);
        model.back();
        model.update(1000.0);
        assert_eq!(model.menu, MurdererMenu::Hints);
        assert!(model.wipe.is_none());
        model.update(1000.0);
        assert_eq!(model.menu, MurdererMenu::Hints);
        assert!(model.pending_top);
        assert!(model.wipe.is_none());
        model.update(0.0);
        assert_eq!(model.menu, MurdererMenu::Top);
        assert_eq!(model.menu_focus, 4);
        assert!(!model.menu_input_busy());
        model.open_menu(0);
        model.back();
        model.update(3.5);
        model.reset();
        assert_eq!(model.back_flash(), 0.0);
        assert!(!model.pending_top);
        assert!(!model.menu_input_busy());
    }

    #[test]
    fn ordinary_top_entries_flash_stage_then_wipe_live_children() {
        for index in 0..5 {
            let mut model = MurdererModel::new(20);
            model.navigate(index as i32);
            model.take_sounds();
            model.confirm();
            assert_eq!(model.take_sounds(), ["decide"]);
            assert_eq!(model.menu, MurdererMenu::Top);
            model.update(3.5);
            assert_eq!(model.top_flash(index), 0.7);
            model.confirm();
            model.navigate(1);
            model.pointer(Vec2::new(1200.0, 500.0), true);
            assert!(model.take_sounds().is_empty());
            assert_eq!(model.menu_focus, index);
            model.update(3.5);
            assert_eq!(model.top_flash(index), 0.0);
            assert!(model.wipe.is_none());
            model.update(0.0);
            assert_eq!(model.menu, MurdererMenu::Top);
            assert_eq!(model.pending_menu, Some(index));
            let wipe = model.wipe.as_ref().unwrap();
            assert_eq!(wipe.kind, MurdererWipeKind::Default);
            assert_eq!((wipe.elapsed, wipe.duration), (0.0, 30.0));
            assert_eq!(wipe.source.menu, MurdererMenu::Top);
            assert_eq!(wipe.source.menu_focus, index);
            let source = wipe.source.clone();
            model.update(15.0);
            assert_ne!(model.menu, MurdererMenu::Top);
            assert!(model.menu_input_busy());
            assert_eq!(model.wipe.as_ref().unwrap().elapsed, 15.0);
            assert_eq!(
                model.wipe.as_ref().unwrap().source.top_flash(index),
                source.top_flash(index)
            );
            model.confirm();
            model.back();
            finish_back(&mut model);
            assert_eq!(model.result, None);
            assert!(model.take_sounds().is_empty());
            model.update(15.0);
            assert!(model.wipe.is_none());
            assert!(!model.menu_input_busy());
            model.back();
            finish_back(&mut model);
            assert_eq!(model.menu, MurdererMenu::Top);
            assert!(!model.menu_input_busy());
            assert_eq!(model.menu_focus, index);
        }
        let mut model = MurdererModel::new(20);
        model.confirm();
        model.update(1000.0);
        assert!(model.wipe.is_none()); // No carry from flash into later phase.
        assert_eq!(model.menu, MurdererMenu::Top);
        model.update(1000.0);
        assert_eq!(model.menu, MurdererMenu::Top); // Still staged until next update.
        assert!(model.wipe.is_none()); // Parent did finish its wipe this frame.
        model.update(0.0);
        assert_eq!(model.menu, MurdererMenu::Replay);
        assert!(!model.menu_input_busy());
    }

    #[test]
    fn parent_wipe_advances_once_per_frame_even_when_first_update_is_paused() {
        let mut model = MurdererModel::new(20);
        model.confirm();
        model.update(7.0);
        let mut frame = FrameId::default();
        model.update_for_frame(0.0, false, frame); // Create and consume frame0.
        model.update_for_frame(15.0, true, frame); // Install child; no wipe advance.
        assert_eq!(model.menu, MurdererMenu::Replay);
        assert_eq!(model.wipe.as_ref().unwrap().elapsed, 0.0);
        frame.advance();
        model.update_for_frame(15.0, true, frame);
        assert_eq!(model.wipe.as_ref().unwrap().elapsed, 15.0);
        model.update_for_frame(15.0, true, frame);
        assert_eq!(model.wipe.as_ref().unwrap().elapsed, 15.0);
        frame.advance();
        model.update_for_frame(15.0, true, frame);
        assert!(model.wipe.is_none());
    }

    #[test]
    fn accusation_glass_moves_on_unrendered_paused_frames_and_deduplicates_aliases() {
        let mut model = MurdererModel::new(20);
        model.navigate(5);
        model.confirm();
        for delta in [7.0, 0.0, 30.0, 0.0] {
            model.update(delta);
        }
        let initial = model
            .wipe
            .as_ref()
            .unwrap()
            .glass
            .as_ref()
            .unwrap()
            .reference_motion();
        assert_eq!(initial.0, 0.0);
        let mut frame = FrameId::default();
        for delta in [0.25, 0.75, 1.25] {
            frame.advance();
            model.update_for_frame(delta, false, frame);
            let wipe = model.wipe.as_ref().unwrap();
            assert_eq!(wipe.elapsed, 0.0, "Paused progress keeps the menu busy");
            let motion = wipe.glass.as_ref().unwrap().reference_motion();
            model.update_for_frame(40.0, true, frame);
            let wipe = model.wipe.as_ref().unwrap();
            assert_eq!(
                wipe.elapsed, 0.0,
                "A second alias cannot resume the consumed frame"
            );
            assert_eq!(wipe.glass.as_ref().unwrap().reference_motion(), motion);
        }
        let moved = model
            .wipe
            .as_ref()
            .unwrap()
            .glass
            .as_ref()
            .unwrap()
            .reference_motion();
        assert_eq!(
            moved.0, 2.25,
            "Physics integrates every frame without a renderer"
        );
        for (before, after) in initial.1.iter().zip(&moved.1) {
            assert!((after.0.z - before.0.z - 15.0 * 2.25).abs() < 0.0001);
            assert!(
                (after.2 - before.2 + 0.7).abs() < 0.00001,
                "Gravity decrements at both native integer boundaries during pause"
            );
        }
        frame.advance();
        model.update_for_frame(0.5, true, frame);
        let wipe = model.wipe.as_ref().unwrap();
        assert_eq!(wipe.elapsed, 0.5);
        assert_eq!(wipe.glass.as_ref().unwrap().reference_motion().0, 2.75);
    }

    #[test]
    fn accusation_entry_waits_for_preliminary_wipe_hold_then_glass() {
        let mut model = MurdererModel::new(20);
        model.navigate(5);
        model.take_sounds();
        model.confirm();
        assert_eq!(model.take_sounds(), ["decide"]);
        model.update(7.0);
        model.update(0.0);
        assert_eq!(model.top_phase, TopPhase::Prepare);
        let preliminary = model.wipe.as_ref().unwrap();
        assert_eq!(preliminary.kind, MurdererWipeKind::Default);
        assert_eq!(preliminary.duration, 10.0);
        assert!(!preliminary.source.top_accusation_art);
        assert!(model.top_accusation_art);
        model.update_with_wipe_permission(30.0, false);
        assert_eq!(model.wipe.as_ref().unwrap().elapsed, 0.0);
        model.update(0.0);
        assert!(model.pending_menu.is_none()); // Timer idle, parent wipe still busy.
        model.update(10.0);
        assert!(model.wipe.is_none());
        assert!(model.take_sounds().is_empty());
        model.update(0.0);
        assert_eq!(model.take_sounds(), ["execute"]);
        assert_eq!(model.menu, MurdererMenu::Top);
        assert_eq!(model.pending_menu, Some(5));
        let glass = model.wipe.as_ref().unwrap();
        assert_eq!(glass.kind, MurdererWipeKind::Glass);
        assert_eq!(glass.duration, 60.0);
        assert_eq!(glass.source.menu, MurdererMenu::Top);
        assert!(glass.source.top_accusation_art);
        let serial = glass.serial;
        model.update(30.0);
        assert_eq!(model.menu, MurdererMenu::Accusation);
        assert_eq!(model.wipe.as_ref().unwrap().elapsed, 30.0);
        model.confirm();
        assert_eq!(model.result, None);
        model.update(30.0);
        assert!(!model.menu_input_busy());
        model.reset();
        finish_top_opening_after_confirm(&mut model);
        assert!(model.wipe_serial > serial);
    }
    fn finish_top_opening_after_confirm(model: &mut MurdererModel) {
        model.confirm();
        finish_top_opening(model);
    }

    #[test]
    fn top_controller_wraps_up_before_down_and_consumes_confirm_on_movement() {
        use crate::app::AppAction;
        let menus = [
            MurdererMenu::Replay,
            MurdererMenu::Rules,
            MurdererMenu::PurpleByPerson,
            MurdererMenu::PurpleByChapter,
            MurdererMenu::Hints,
            MurdererMenu::Accusation,
        ];
        let up = [5, 0, 1, 2, 3, 4];
        let down = [1, 2, 3, 4, 5, 0];
        for original in 0..6 {
            for bits in 0u8..16 {
                let mut model = MurdererModel::new(20);
                model.navigate(original as i32);
                model.take_sounds();
                let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                for (bit, action) in [
                    AppAction::Up,
                    AppAction::AnyDown,
                    AppAction::Enter,
                    AppAction::Cancel,
                ]
                .into_iter()
                .enumerate()
                {
                    input[action] = shin_input::ActionState {
                        is_clicked: bits & (1 << bit) != 0,
                        is_clicked_or_repeated: bits & (1 << bit) != 0,
                        ..Default::default()
                    };
                }
                assert!(model.handle_controller_input(&input));
                finish_back(&mut model);
                let selected = if bits & 1 != 0 {
                    up[original]
                } else if bits & 2 != 0 {
                    down[original]
                } else {
                    original
                };
                if selected != original {
                    assert_eq!(model.menu, MurdererMenu::Top);
                    assert_eq!(model.menu_focus, selected);
                    assert_eq!(model.take_sounds(), ["move"]);
                } else if bits & 4 != 0 {
                    assert_eq!(model.menu, MurdererMenu::Top);
                    assert!(model.menu_input_busy());
                    finish_top_opening(&mut model);
                    assert_eq!(model.menu, menus[original]);
                    let sounds = model.take_sounds();
                    assert_eq!(sounds.first(), Some(&"decide"));
                } else {
                    assert_eq!(model.menu, MurdererMenu::Top);
                    assert_eq!(model.menu_focus, original);
                    assert!(model.take_sounds().is_empty());
                }
                assert_eq!(model.result, None);
            }
        }
        let mut model = MurdererModel::new(20);
        let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        input[AppAction::Up].is_held = true;
        model.handle_controller_input(&input);
        finish_back(&mut model);
        assert_eq!(model.menu_focus, 0);
        input[AppAction::Up].is_clicked_or_repeated = true;
        model.handle_controller_input(&input);
        finish_back(&mut model);
        assert_eq!(model.menu_focus, 5);
    }
    #[test]
    fn hint_controller_scrolls_only_enough_to_reveal_row_and_blocks_busy_input() {
        use crate::app::AppAction;
        let mut model = MurdererModel::new(20);
        model.open_menu(4);
        model.navigate(-20);
        let mut down = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        down[AppAction::AnyDown].is_clicked_or_repeated = true;
        let mut cancel = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        cancel[AppAction::Cancel].is_clicked = true;
        for selected in 1..=7 {
            model.handle_controller_input(&down);
            assert_eq!(model.menu_focus, selected);
            assert_eq!(model.hint_scroll, 0.0);
            model.handle_controller_input(&cancel);
            assert_eq!(model.menu, MurdererMenu::Hints);
            model.update(4.0);
        }
        model.handle_controller_input(&down);
        assert_eq!(model.menu_focus, 8);
        model.update(2.0);
        assert_eq!(model.hint_scroll, 45.0);
        model.handle_controller_input(&down);
        assert_eq!(model.menu_focus, 8);
        model.update(2.0);
        assert_eq!(model.hint_scroll, 90.0);
        model.handle_controller_input(&down);
        model.update(4.0);
        assert_eq!(model.menu_focus, 9);
        assert_eq!(model.hint_scroll, 180.0);
        model.handle_controller_input(&cancel);
        finish_back(&mut model);
        assert_eq!(model.menu, MurdererMenu::Top);
    }

    #[test]
    fn purple_analog_scroll_uses_native_gain_bounds_and_action_priority() {
        use crate::app::AppAction as A;
        let idle = enum_map::EnumMap::<A, shin_input::ActionState>::default();
        for menu in [2, 3] {
            let mut model = MurdererModel::new(20);
            model.open_menu(menu);
            model.handle_controller_input_with_scroll(&idle, 0.5, 1200.0);
            assert_eq!(model.purple_text_scroll, 16.0);
            model.handle_controller_input_with_scroll(&idle, -0.25, 1200.0);
            assert_eq!(model.purple_text_scroll, 8.0);
            model.handle_controller_input_with_scroll(&idle, -1.0, 1200.0);
            assert_eq!(model.purple_text_scroll, 0.0);
            model.purple_text_scroll = 470.0;
            model.handle_controller_input_with_scroll(&idle, 1.0, 1200.0);
            assert_eq!(model.purple_text_scroll, 480.0);
            for axis in [0.0, f32::NAN, f32::INFINITY] {
                model.handle_controller_input_with_scroll(&idle, axis, 1200.0);
                assert_eq!(model.purple_text_scroll, 480.0);
            }
            model.handle_controller_input_with_scroll(&idle, 1.0, 100.0);
            assert_eq!(model.purple_text_scroll, 0.0);
            model.purple_text_scroll = 100.0;
            let mut down = idle.clone();
            down[A::AnyDown].is_clicked_or_repeated = true;
            model.handle_controller_input_with_scroll(&down, 1.0, 1200.0);
            assert_eq!(model.purple_text_scroll, 0.0); // Selection consumes axis.
            assert_eq!(model.take_sounds(), ["move"]);
            model.purple_text_scroll = 100.0;
            model.update(4.0);
            let mut cancel = idle.clone();
            cancel[A::Cancel].is_clicked = true;
            model.handle_controller_input_with_scroll(&cancel, 1.0, 1200.0);
            assert_eq!(model.purple_text_scroll, 100.0);
            assert_eq!(model.take_sounds(), ["cancel"]);
            model.handle_controller_input_with_scroll(&idle, 1.0, 1200.0);
            assert_eq!(model.purple_text_scroll, 100.0); // Back phase blocks input.
        }
        let mut model = MurdererModel::new(20);
        model.open_menu(3);
        let mut up = idle.clone();
        up[A::Up].is_clicked_or_repeated = true;
        model.handle_controller_input_with_scroll(&up, 0.5, 1200.0);
        assert_eq!(model.purple_text_scroll, 16.0); // Stationary chapter edge falls through.
        model.reset();
        model.open_menu(2);
        model.handle_controller_input_with_scroll(&up, 0.5, 1200.0);
        assert_eq!(model.purple_text_scroll, 0.0); // Person edge consumes direction.
        model.reset();
        model.open_menu(3);
        let mut down = idle.clone();
        down[A::AnyDown].is_clicked_or_repeated = true;
        for _ in 0..8 {
            model.handle_controller_input_with_scroll(&down, 0.0, 1200.0);
            model.update(4.0);
        }
        model.handle_controller_input_with_scroll(&down, 0.0, 1200.0);
        assert!(model.purple_list_scrolling());
        model.handle_controller_input_with_scroll(&idle, 1.0, 1200.0);
        assert_eq!(model.purple_text_scroll, 0.0);
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_purple_analog_transcript_scroll() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x969ec, 0x39403fe8),
            (0x969f0, 0x372804e8),
            (0x969f4, 0xbd4027e0),
            (0x969f8, 0x1e202008),
            (0x969fc, 0x54000760),
            (0x96a00, 0x91024274),
            (0x96a04, 0xaa1403e0),
            (0x96a08, 0x940092a6),
            (0x96a0c, 0xf0000608),
            (0x96a10, 0xbd4027e1),
            (0x96a14, 0xbd469502),
            (0x96a18, 0x1e220821),
            (0x96a1c, 0x1e212801),
            (0x96a20, 0x1e2703e0),
            (0x96a24, 0x1e202028),
            (0x96a28, 0x54000164),
            (0x96a2c, 0xf0000608),
            (0x96a30, 0xbd40ea62),
            (0x96a34, 0xbd43a903),
            (0x96a38, 0x1e232842),
            (0x96a3c, 0x1e202048),
            (0x96a40, 0x540000a9),
            (0x96a44, 0x1e212040),
            (0x96a48, 0x4ea11c20),
            (0x96a4c, 0x54000045),
            (0x96a50, 0x4ea21c40),
            (0x96a54, 0xaa1403e0),
            (0x96a58, 0x94009222),
            (0x96a5c, 0xb900ee7f),
            (0x96a60, 0x14000022),
            (0xc49a0, 0xf0000b29),
            (0xc49a4, 0x9101c129),
            (0xc49a8, 0x29402d2a),
            (0xc49ac, 0x29002d0a),
            (0xc49b0, 0x29412d2a),
            (0xc49b4, 0x29012d0a),
            (0xc49b8, 0x29442d2a),
            (0xc49bc, 0x29022d0a),
            (0xc49c0, 0x29452d2a),
            (0xc49c4, 0x29032d0a),
            (0xc49c8, 0x2946252a),
            (0xc49cc, 0x2904250a),
            (0xc49d0, 0xd65f03c0),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [(0x159694, 32.0f32), (0x1593a8, -720.0)] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
    }

    #[test]
    fn purple_chapter_drag_reconciles_selection_and_flings_without_clicking() {
        let mut model = MurdererModel::new(20);
        model.open_menu(3);
        let tick = 0.016683333333333335;
        let origin = Vec2::new(200.0, 300.0); // Current statement: list capture, no row click.
        model.pointer_state_with_time(origin, true, tick, 1200.0);
        model.pointer_state_with_time(origin - Vec2::Y * 16.0, true, tick, 1200.0);
        assert_eq!(model.purple_list_scroll, 0.0);
        model.pointer_state_with_time(origin - Vec2::Y * 40.0, true, tick, 1200.0);
        assert_eq!(model.purple_list_scroll, 40.0);
        assert_eq!(model.purple_original_index(), Some(2));
        assert!(!model.purple_list_scrolling());
        model.pointer_state_with_time(origin - Vec2::Y * 40.0, false, tick, 1200.0);
        assert_eq!(model.purple_list_velocity, -40.0);
        model.update(2.5);
        assert_eq!(model.purple_list_scroll, 140.0);
        assert_eq!(model.purple_original_index(), Some(3));
        assert!((model.purple_list_velocity + 40.0 * 0.95f32.powf(2.5)).abs() < 0.0001);
        assert!(model.take_sounds().is_empty());
        assert!(model.result.is_none());
        // First press stops momentum rather than selecting a fresh row.
        let previous = model.purple_original_index();
        model.pointer_state_with_time(Vec2::new(200.0, 800.0), true, tick, 1200.0);
        assert_eq!(model.purple_list_velocity, 0.0);
        assert_eq!(model.purple_original_index(), previous);
        model.pointer_state_with_time(Vec2::new(200.0, 800.0), false, tick, 1200.0);
        assert!(model.take_sounds().is_empty());
        model.purple_list_scroll = 1850.0;
        model.purple_list_motion = Tweener::new(1850.0);
        model.purple_list_velocity = -100.0;
        model.update(1.0);
        assert_eq!(model.purple_list_scroll, 1890.0);
        assert_eq!(model.purple_original_index(), Some(22));
        assert_eq!(model.purple_list_velocity, -95.0);
        model.update(1.0);
        assert_eq!(model.purple_list_velocity, 0.0);
    }
    #[test]
    fn purple_text_drag_inertia_axis_and_stationary_release_follow_native_bounds() {
        let tick = 0.016683333333333335;
        for menu in [2, 3] {
            let mut model = MurdererModel::new(20);
            model.open_menu(menu);
            let origin = Vec2::new(1100.0, 400.0);
            let selected = model.purple_original_index();
            model.pointer_state_with_time(origin, true, tick, 1200.0);
            model.pointer_state_with_time(origin - Vec2::Y * 40.0, true, tick, 1200.0);
            assert_eq!(model.purple_text_scroll, 40.0);
            model.pointer_state_with_time(origin - Vec2::Y * 40.0, false, tick, 1200.0);
            model.update(2.5);
            assert_eq!(model.purple_text_scroll, 140.0);
            assert!((model.purple_text_velocity + 40.0 * 0.95f32.powf(2.5)).abs() < 0.0001);
            model.handle_controller_input_with_scroll(&Default::default(), 0.5, 1200.0);
            assert_eq!(model.purple_text_scroll, 156.0);
            assert_eq!(model.purple_text_velocity, 0.0);
            model.pointer_state_with_time(origin, true, tick, 1200.0);
            model.pointer_state_with_time(origin - Vec2::Y * 40.0, true, tick, 1200.0);
            model.pointer_state_with_time(origin - Vec2::Y * 40.0, true, tick, 1200.0);
            model.pointer_state_with_time(origin - Vec2::Y * 40.0, false, tick, 1200.0);
            assert_eq!(model.purple_text_velocity, 0.0);
            assert_eq!(model.purple_original_index(), selected);
            assert!(model.take_sounds().is_empty());
            assert!(model.result.is_none());
            model.purple_text_velocity = 0.1001;
            model.purple_text_scroll = 100.0;
            model.update(1.0);
            assert!((model.purple_text_scroll - 99.8999).abs() < 0.0001);
            assert_eq!(model.purple_text_velocity, 0.0);
            model.purple_text_velocity = 40.0;
            model.update(0.0);
            assert_eq!(model.purple_text_velocity, 0.0);
        }
    }
    #[test]
    fn purple_heading_drag_cancels_flash_and_cannot_scroll_by_click_release() {
        let mut model = MurdererModel::new(20);
        model.open_menu(3);
        model.reference_set_purple_list_scroll(495.0);
        let tick = 0.016683333333333335;
        let origin = Vec2::new(200.0, 200.0);
        model.pointer_state_with_time(origin, true, tick, 1200.0);
        model.update(2.0);
        assert!((model.purple_heading_flash(0) - 0.4).abs() < 0.0001);
        model.pointer_state_with_time(origin + Vec2::Y * 40.0, true, tick, 1200.0);
        assert_eq!(model.purple_list_scroll, 455.0);
        model.update(1.0);
        assert!((model.purple_heading_flash(0) - 0.2).abs() < 0.0001);
        model.pointer_state_with_time(origin + Vec2::Y * 40.0, true, tick, 1200.0);
        model.pointer_state_with_time(origin + Vec2::Y * 40.0, false, tick, 1200.0);
        model.update(4.0);
        assert_eq!(model.purple_list_scroll, 455.0);
        assert!(model.take_sounds().is_empty());
        assert!(!model.purple_list_scrolling());
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_purple_drag_and_inertia_callbacks() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x94bf4, 0xbd408e60),
            (0x94bf8, 0x1e202008),
            (0x94bfc, 0x540009a0),
            (0x94c00, 0x9100c274),
            (0x94c04, 0xaa1403e0),
            (0x94c08, 0x94009a26),
            (0x94c0c, 0xbd408e61),
            (0x94c10, 0x1e280821),
            (0x94c14, 0x1e213801),
            (0x94c18, 0x1e2703e9),
            (0x94c1c, 0x1e202028),
            (0x94c20, 0x54000164),
            (0x94c24, 0xb0000628),
            (0x94c28, 0xbd408a62),
            (0x94c2c, 0xbd43a903),
            (0x94c30, 0x1e232842),
            (0x94c34, 0x1e202048),
            (0x94c38, 0x540000a9),
            (0x94c3c, 0x1e212040),
            (0x94c40, 0x4ea11c29),
            (0x94c44, 0x54000045),
            (0x94c48, 0x4ea21c49),
            (0x94c4c, 0xb0000628),
            (0x94c50, 0x7ea9d400),
            (0x94c54, 0xbd4ab901),
            (0x94c58, 0x1e212000),
            (0x94c5c, 0x54000684),
            (0x94c60, 0xb9401268),
            (0x94c64, 0x7100011f),
            (0x94c68, 0xb0000628),
            (0x94c6c, 0xbd49e100),
            (0x94c70, 0x1e2703e1),
            (0x94c74, 0xb000062a),
            (0x94c78, 0x1e200c20),
            (0x94c7c, 0xbd498141),
            (0x94c80, 0xb9401a68),
            (0x94c84, 0x2a0803ec),
            (0x94c88, 0x1e202920),
            (0x94c8c, 0x1e212921),
            (0x94c90, 0xf9411a69),
            (0x94c94, 0x5280190a),
            (0x94c98, 0x1280000b),
            (0x94c9c, 0x14000003),
            (0x94ca0, 0x320003ec),
            (0x94ca4, 0x0b0c002c),
            (0x94ca8, 0x93407d81),
            (0x94cac, 0x9b2a258c),
            (0x94cb0, 0xbd40bd82),
            (0x94cb4, 0x1e222000),
            (0x94cb8, 0x54ffff4c),
            (0x94cbc, 0x9b0a242c),
            (0x94cc0, 0xbd40c583),
            (0x94cc4, 0x2a0b03ec),
            (0x94cc8, 0x1e232842),
            (0x94ccc, 0x1e222020),
            (0x94cd0, 0x54000065),
            (0x94cd4, 0x0b0c002c),
            (0x94cd8, 0x17fffff4),
            (0x94cdc, 0x6b01011f),
            (0x94ce0, 0x54000080),
            (0x94ce4, 0xaa1303e0),
            (0x94ce8, 0x2a1f03e2),
            (0x94cec, 0x9400078d),
            (0x94cf0, 0x4ea91d20),
            (0x94cf4, 0xaa1403e0),
            (0x94cf8, 0x9400997a),
            (0x94cfc, 0xb0000628),
            (0x94d00, 0xbd45d900),
            (0x94d04, 0xbd408e69),
            (0x94d08, 0x4ea81d01),
            (0x94d0c, 0x94021fcd),
            (0x94d10, 0x1e200920),
            (0x94d14, 0xb0000628),
            (0x94d18, 0xbd457101),
            (0x94d1c, 0xbd008e60),
            (0x94d20, 0x1e20c000),
            (0x94d24, 0x1e212000),
            (0x94d28, 0x54000045),
            (0x94d2c, 0xb9008e7f),
            (0x94d30, 0xbd40ee60),
            (0x94d34, 0x1e202008),
            (0x94d38, 0x54000520),
            (0x94d3c, 0x91024274),
            (0x94d40, 0xaa1403e0),
            (0x94d44, 0x940099d7),
            (0x94d48, 0x4ea01c01),
            (0x94d4c, 0xbd40ee60),
            (0x94d50, 0x1e280800),
            (0x94d54, 0x1e203822),
            (0x94d58, 0x1e2703e0),
            (0x94d5c, 0x1e202048),
            (0x94d60, 0x54000164),
            (0x94d64, 0xb0000628),
            (0x94d68, 0xbd40ea63),
            (0x94d6c, 0xbd43a904),
            (0x94d70, 0x1e242863),
            (0x94d74, 0x1e202068),
            (0x94d78, 0x540000a9),
            (0x94d7c, 0x1e222060),
            (0x94d80, 0x4ea21c40),
            (0x94d84, 0x54000045),
            (0x94d88, 0x4ea31c60),
            (0x94d8c, 0xb0000628),
            (0x94d90, 0x7ea0d421),
            (0x94d94, 0xbd4ab902),
            (0x94d98, 0x1e222020),
            (0x94d9c, 0x540001e4),
            (0x94da0, 0xaa1403e0),
            (0x94da4, 0x9400994f),
            (0x94da8, 0xb0000628),
            (0x94dac, 0xbd45d900),
            (0x94db0, 0xbd40ee69),
            (0x94db4, 0x4ea81d01),
            (0x94db8, 0x94021fa2),
            (0x94dbc, 0x1e200920),
            (0x94dc0, 0xb0000628),
            (0x94dc4, 0xbd457101),
            (0x94dc8, 0xbd00ee60),
            (0x94dcc, 0x1e20c000),
            (0x94dd0, 0x1e212000),
            (0x94dd4, 0x54000045),
            (0x94dd8, 0xb900ee7f),
            (0x97754, 0xb2607fe8),
            (0x97758, 0xf9001268),
            (0x9775c, 0x9100c274),
            (0x97760, 0xaa1403e0),
            (0x97764, 0xb900227f),
            (0x97768, 0x94008f4e),
            (0x9776c, 0x1e293901),
            (0x97770, 0x1e213801),
            (0x97774, 0x1e2703e0),
            (0x97778, 0x4ea01c08),
            (0x9777c, 0x1e202028),
            (0x97780, 0x54000184),
            (0x97784, 0xd0000608),
            (0x97788, 0xbd408a62),
            (0x9778c, 0x4ea01c08),
            (0x97790, 0xbd43a903),
            (0x97794, 0x1e232842),
            (0x97798, 0x1e202048),
            (0x9779c, 0x540000a9),
            (0x977a0, 0x1e212040),
            (0x977a4, 0x4ea11c28),
            (0x977a8, 0x54000045),
            (0x977ac, 0x4ea21c48),
            (0x977b0, 0xb9401268),
            (0x977b4, 0x7100011f),
            (0x977b8, 0xd0000608),
            (0x977bc, 0xbd49e101),
            (0x977c0, 0xd000060a),
            (0x977c4, 0x1e210c00),
            (0x977c8, 0xbd498141),
            (0x977cc, 0xb9401a68),
            (0x977d0, 0x2a0803ec),
            (0x977d4, 0x1e202900),
            (0x977d8, 0x1e212901),
            (0x977dc, 0xf9411a69),
            (0x977e0, 0x5280190a),
            (0x977e4, 0x1280000b),
            (0x977e8, 0x14000003),
            (0x977ec, 0x320003ec),
            (0x977f0, 0x0b0c002c),
            (0x977f4, 0x93407d81),
            (0x977f8, 0x9b2a258c),
            (0x977fc, 0xbd40bd82),
            (0x97800, 0x1e222000),
            (0x97804, 0x54ffff4c),
            (0x97808, 0x9b0a242c),
            (0x9780c, 0xbd40c583),
            (0x97810, 0x2a0b03ec),
            (0x97814, 0x1e232842),
            (0x97818, 0x1e222020),
            (0x9781c, 0x54000065),
            (0x97820, 0x0b0c002c),
            (0x97824, 0x17fffff4),
            (0x97828, 0x6b01011f),
            (0x9782c, 0x54000080),
            (0x97830, 0xaa1303e0),
            (0x97834, 0x2a1f03e2),
            (0x97838, 0x97fffcba),
            (0x9783c, 0xaa1403e0),
            (0x97840, 0x4ea81d00),
            (0x97844, 0x94008ea7),
            (0x97848, 0xa9427bfd),
            (0x9784c, 0xa9414ff4),
            (0x97898, 0x3940a268),
            (0x9789c, 0x34fffd68),
            (0x978a0, 0x91024274),
            (0x978a4, 0xaa1403e0),
            (0x978a8, 0x94008efe),
            (0x978ac, 0x1e293901),
            (0x978b0, 0x1e213801),
            (0x978b4, 0x1e2703e0),
            (0x978b8, 0x1e202028),
            (0x978bc, 0x54000164),
            (0x978c0, 0xd0000608),
            (0x978c4, 0xbd40ea62),
            (0x978c8, 0xbd43a903),
            (0x978cc, 0x1e232842),
            (0x978d0, 0x1e202048),
            (0x978d4, 0x540000a9),
            (0x978d8, 0x1e212040),
            (0x978dc, 0x4ea11c20),
            (0x978e0, 0x54000045),
            (0x978e4, 0x4ea21c40),
            (0x978e8, 0xaa1403e0),
            (0x978ec, 0x17ffffd6),
            (0x978f0, 0xb9401c08),
            (0x978f4, 0x37f80048),
            (0x978f8, 0xd65f03c0),
            (0x978fc, 0xb9402008),
            (0x97900, 0x37f80068),
            (0x97904, 0xbd008c01),
            (0x97908, 0xd65f03c0),
            (0x9790c, 0x3940a008),
            (0x97910, 0x34ffff48),
            (0x97914, 0xbd00ec01),
            (0x97918, 0xd65f03c0),
            (0x971c8, 0xbd408e60),
            (0x971cc, 0xd0000608),
            (0x971d0, 0xbd4ab901),
            (0x971d4, 0x1e20c000),
            (0x971d8, 0x1e212000),
            (0x971dc, 0x540002a4),
            (0x971e0, 0xb2607fe8),
            (0x971e4, 0xf9001268),
            (0x971e8, 0xb9008e7f),
            (0x971ec, 0x14000011),
            (0x971f0, 0x4ea91d20),
            (0x971f4, 0x4ea81d01),
            (0x971f8, 0xaa1303e0),
            (0x971fc, 0x97fffebd),
            (0x97200, 0xb9002260),
            (0x97204, 0x37f80260),
            (0x97208, 0xbd408e60),
            (0x9720c, 0xd0000608),
            (0x97210, 0xbd4ab901),
            (0x97214, 0x1e20c000),
            (0x97218, 0x1e212000),
            (0x9721c, 0x540000a4),
            (0x97220, 0xb900227f),
            (0x97224, 0x320003e0),
            (0x97228, 0xb9008e7f),
            (0x9722c, 0x14000002),
            (0x97250, 0xd0000608),
            (0x97254, 0xbd48c100),
            (0x97258, 0x1e202100),
            (0x9725c, 0x2a1f03e0),
            (0x97260, 0x54fffea5),
            (0x97264, 0xd0000608),
            (0x97268, 0xbd49f900),
            (0x9726c, 0x1e202120),
            (0x97270, 0x54fffe2b),
            (0x97274, 0xd0000608),
            (0x97278, 0xbd470900),
            (0x9727c, 0x1e202120),
            (0x97280, 0x54fffda5),
            (0x97284, 0xd0000608),
            (0x97288, 0xbd481500),
            (0x9728c, 0x1e202100),
            (0x97290, 0x54fffd2b),
            (0x97294, 0x320003e0),
            (0x97298, 0x3900a260),
            (0x9729c, 0xb900ee7f),
            (0x972a0, 0x17ffffe5),
            (0x94234, 0x52a88688),
            (0x94238, 0xb9008a68),
            (0x946dc, 0xb900ee7f),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [
            (0x1595d8, 0.95f32),
            (0x159570, 0.1),
            (0x159ab8, f32::EPSILON),
            (0x1599e0, 90.0),
            (0x159980, 720.0),
            (0x1599f8, 706.0),
            (0x159708, 1774.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
    }

    #[test]
    fn hint_analog_scroll_uses_native_gain_selection_reconciliation_and_priority() {
        use crate::app::AppAction as A;
        let idle = enum_map::EnumMap::<A, shin_input::ActionState>::default();
        let mut model = MurdererModel::new(20);
        model.open_menu(4);
        model.navigate(-20);
        model.take_sounds();
        model.handle_controller_input_with_scroll(&idle, 0.5, 0.0);
        assert_eq!(model.hint_scroll, 16.0);
        assert_eq!(model.menu_focus, 1);
        assert!(model.hint_scroll_motion.is_idle());
        model.handle_controller_input_with_scroll(&idle, -0.25, 0.0);
        assert_eq!(model.hint_scroll, 8.0);
        model.handle_controller_input_with_scroll(&idle, -1.0, 0.0);
        assert_eq!(model.hint_scroll, 0.0);
        assert_eq!(model.menu_focus, 1); // Current visible row is retained.
        assert!(model.take_sounds().is_empty());
        model.hint_scroll_velocity = -40.0;
        model.handle_controller_input_with_scroll(&idle, 0.5, 0.0);
        assert_eq!(model.hint_scroll_velocity, 0.0);
        let mut down = idle.clone();
        down[A::AnyDown].is_clicked_or_repeated = true;
        model.handle_controller_input_with_scroll(&down, 1.0, 0.0);
        assert_eq!(model.menu_focus, 2);
        assert_eq!(model.hint_scroll, 16.0);
        assert_eq!(model.take_sounds(), ["move"]);
        model.handle_controller_input_with_scroll(&idle, 1.0, 0.0);
        assert_eq!(model.hint_scroll, 16.0); // Four-tick queue gates axis.
        model.update(4.0);
        let mut enter = idle.clone();
        enter[A::Enter].is_clicked = true;
        model.handle_controller_input_with_scroll(&enter, 1.0, 0.0);
        assert_eq!(model.hint_scroll, 16.0);
        assert_eq!(model.take_sounds(), ["decide"]);
        for progress in [-1, 0, 7] {
            let mut model = MurdererModel::new(progress);
            model.open_menu(4);
            model.handle_controller_input_with_scroll(&idle, 1.0, 0.0);
            assert_eq!(model.hint_scroll, 0.0);
            assert!(model.result.is_none());
        }
    }
    #[test]
    fn hint_keyboard_scrolling_uses_physical_k_l_bounds_and_native_input_gates() {
        use crate::app::AppAction as A;
        use shin_input::{Action, RawInputState};
        use winit::keyboard::KeyCode;
        let input = |key| {
            let mut raw = RawInputState::new();
            raw.keyboard.insert(key);
            let lowered = A::lower(&raw);
            enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
                is_clicked: lowered[action],
                is_clicked_or_repeated: lowered[action],
                is_held: lowered[action],
                ..Default::default()
            })
        };
        let up = input(KeyCode::KeyK);
        let down = input(KeyCode::KeyL);
        let mut model = MurdererModel::new(20);
        model.open_menu(4);
        model.navigate(-20);
        model.take_sounds();
        model.handle_controller_input_with_scroll(&up, 0., 0.);
        assert_eq!(model.hint_scroll, 0.);
        model.handle_controller_input_with_scroll(&down, 0., 0.);
        assert_eq!(model.hint_scroll, 180.);
        assert_eq!(model.menu_focus, 2);
        model.handle_controller_input_with_scroll(&down, 0., 0.);
        assert_eq!(model.hint_scroll, 360.);
        model.handle_controller_input_with_scroll(&up, 0., 0.);
        assert_eq!(model.hint_scroll, 180.);
        assert!(model.take_sounds().is_empty());
        for _ in 0..20 {
            model.handle_controller_input_with_scroll(&down, 0., 0.);
        }
        assert_eq!(model.hint_scroll, model.hint_scroll_maximum());
        for _ in 0..20 {
            model.handle_controller_input_with_scroll(&up, 0., 0.);
        }
        assert_eq!(model.hint_scroll, 0.);
        let mut raw = RawInputState::new();
        raw.gamepads.unified.buttons[shin_input::inputs::GamepadButton::R] = true;
        let lowered = A::lower(&raw);
        assert!(lowered[A::PageDown]);
        assert!(!lowered[A::KeyboardScrollDown]);
        let shoulder = enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
            is_clicked: lowered[action],
            is_clicked_or_repeated: lowered[action],
            ..Default::default()
        });
        model.handle_controller_input_with_scroll(&shoulder, 0., 0.);
        assert_eq!(model.hint_scroll, 0., "Native shoulder policy unchanged");
        model.scroll_hint_list(f32::NAN);
        assert_eq!(model.hint_scroll, 0.);
        model.scroll_hint_list(180.);
        let mut select = down.clone();
        select[A::Up].is_clicked_or_repeated = true;
        model.handle_controller_input_with_scroll(&select, 0., 0.);
        assert_eq!(
            model.hint_scroll, 180.,
            "Selection takes priority over scroll"
        );
        model.handle_controller_input_with_scroll(&down, 0., 0.);
        assert_eq!(
            model.hint_scroll, 180.,
            "Four-tick focus tween blocks input"
        );
        model.update(4.);
        let mut confirm = down.clone();
        confirm[A::Enter].is_clicked = true;
        model.handle_controller_input_with_scroll(&confirm, 0., 0.);
        let captured = model.hint_scroll;
        model.handle_controller_input_with_scroll(&up, 0., 0.);
        assert_eq!(
            model.hint_scroll, captured,
            "Response phase blocks scrolling"
        );
        for progress in [-1, 0, 7] {
            let mut short = MurdererModel::new(progress);
            short.open_menu(4);
            short.handle_controller_input_with_scroll(&down, 0., 0.);
            assert_eq!(short.hint_scroll, 0., "Short lists have no scroll range");
        }
    }

    #[test]
    fn hint_drag_inertia_updates_focus_after_clocks_and_stops_on_press() {
        let tick = 0.016683333333333335;
        let mut model = MurdererModel::new(20);
        model.open_menu(4);
        let origin = Vec2::new(300.0, 400.0); // Blank left margin captures list.
        model.pointer_state_with_time(origin, true, tick, 0.0);
        model.pointer_state_with_time(origin + Vec2::Y * 40.0, true, tick, 0.0);
        assert_eq!(model.hint_scroll, 1130.0);
        assert_eq!(model.menu_focus, 19);
        assert!(model.hint_scroll_motion.is_idle());
        model.pointer_state_with_time(origin + Vec2::Y * 40.0, false, tick, 0.0);
        assert_eq!(model.hint_scroll_velocity, 40.0);
        model.update(2.5);
        assert_eq!(model.hint_scroll, 1030.0);
        assert_eq!(model.menu_focus, 18);
        assert_eq!(model.hint_feedback(18).0, 0.0); // Reconciled after focus clocks.
        assert_eq!(model.hint_feedback(19).0, 0.625);
        assert!((model.hint_scroll_velocity - 40.0 * 0.95f32.powf(2.5)).abs() < 0.0001);
        let point = Vec2::new(400.0, 200.0);
        model.pointer_state_with_time(point, true, tick, 0.0);
        model.pointer_state_with_time(point, false, tick, 0.0);
        assert_eq!(model.hint_scroll_velocity, 0.0);
        assert_eq!(model.menu_focus, 18); // Stop press did not select a hit row.
        model.update(1.0);
        assert_eq!(model.hint_scroll, 1030.0);
        assert_eq!(model.hint_feedback(18).0, 0.25);
        assert!(model.take_sounds().is_empty());
        assert!(model.result.is_none());
        model.hint_scroll_velocity = 100.0;
        model.update(20.0);
        assert_eq!(model.hint_scroll, 0.0);
        assert_eq!(model.menu_focus, 7);
        model.update(1.0);
        assert_eq!(model.hint_scroll_velocity, 0.0);
    }
    #[test]
    fn hint_row_drag_cancels_hot_flash_response_and_stationary_release_momentum() {
        let mut model = MurdererModel::new(20);
        model.open_menu(4);
        let origin = Vec2::new(400.0, 268.0); // Last2px of native row13 hit strip.
        let tick = 0.016683333333333335;
        model.pointer_state_with_time(origin, true, tick, 0.0);
        model.update(2.0);
        assert!((model.hint_feedback(13).1 - 0.4).abs() < 0.0001);
        model.pointer_state_with_time(origin + Vec2::Y * 40.0, true, tick, 0.0);
        assert_eq!(model.hint_scroll, 1130.0);
        model.update(1.0);
        assert!((model.hint_feedback(13).1 - 0.2).abs() < 0.0001);
        model.pointer_state_with_time(origin + Vec2::Y * 40.0, true, tick, 0.0);
        model.pointer_state_with_time(origin + Vec2::Y * 40.0, false, tick, 0.0);
        model.update(60.0);
        assert_eq!(model.hint_scroll, 1130.0);
        assert_eq!(model.hint_scroll_velocity, 0.0);
        assert!(model.result.is_none());
        assert!(model.take_sounds().is_empty());
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_hint_analog_drag_and_inertia() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x928c4, 0xb9400fe8),
            (0x928c8, 0x37e00628),
            (0x928cc, 0x37e80a68),
            (0x928d0, 0xbd4027e0),
            (0x928d4, 0x1e202008),
            (0x928d8, 0x54001040),
            (0x928dc, 0xaa1403e0),
            (0x928e0, 0x9400a2f0),
            (0x928e4, 0xf0000628),
            (0x928e8, 0xbd4027e1),
            (0x928ec, 0xf000062a),
            (0x928f0, 0xbd469502),
            (0x928f4, 0xb9408a68),
            (0x928f8, 0x510b4108),
            (0x928fc, 0x1e220821),
            (0x92900, 0x1e220102),
            (0x92904, 0x1e212800),
            (0x92908, 0x1e2703e1),
            (0x9290c, 0x1e222000),
            (0x92910, 0x1e20cc40),
            (0x92914, 0x1e214808),
            (0x92918, 0xbd498140),
            (0x9291c, 0xf000062b),
            (0x92920, 0xb9409269),
            (0x92924, 0x93407d28),
            (0x92928, 0x2a0903f5),
            (0x9292c, 0x1e202900),
            (0x92930, 0xbd49e161),
            (0x92934, 0x52800b4a),
            (0x92938, 0x1280000b),
            (0x9293c, 0x1b0a7eac),
            (0x92940, 0x1e220182),
            (0x92944, 0x1e222100),
            (0x92948, 0x540000ed),
            (0x9294c, 0x320003ec),
            (0x92950, 0x0b0c02b5),
            (0x92954, 0x1b0a7eac),
            (0x92958, 0x1e220182),
            (0x9295c, 0x1e222100),
            (0x92960, 0x54ffff6c),
            (0x92964, 0x1e212842),
            (0x92968, 0x1e222000),
            (0x9296c, 0x2a0b03ec),
            (0x92970, 0x54000825),
            (0x92974, 0x0b0c02b5),
            (0x92978, 0x1b0a7eac),
            (0x9297c, 0x1e220182),
            (0x92980, 0x1e222100),
            (0x92984, 0x54ffff0d),
            (0x92988, 0x17fffff1),
            (0x92ad0, 0x4ea81d00),
            (0x92ad4, 0xaa1403e0),
            (0x92ad8, 0x9400a202),
            (0x92adc, 0xb901527f),
            (0x918c8, 0x4ea81d00),
            (0x918cc, 0x91006260),
            (0x918d0, 0x9400a860),
            (0x918d4, 0x4ea81d00),
            (0x918d8, 0x91028274),
            (0x918dc, 0xaa1403e0),
            (0x918e0, 0x9400a85c),
            (0x918e4, 0xa9475a75),
            (0x918e8, 0xeb1602bf),
            (0x918ec, 0x54000140),
            (0x918f0, 0x4ea81d00),
            (0x918f4, 0x910022a0),
            (0x918f8, 0x9400a856),
            (0x918fc, 0x4ea81d00),
            (0x91900, 0x910182a0),
            (0x91904, 0x9400a853),
            (0x91908, 0x9102e2b5),
            (0x9190c, 0xeb1502df),
            (0x91910, 0x54ffff01),
            (0x91914, 0x4ea81d00),
            (0x91918, 0x9103e260),
            (0x9191c, 0x9400a84d),
            (0x91920, 0xbd415260),
            (0x91924, 0x1e202008),
            (0x91928, 0x54000ae0),
            (0x9192c, 0xaa1403e0),
            (0x91930, 0x9400a6dc),
            (0x91934, 0xbd415261),
            (0x91938, 0xb9408a68),
            (0x9193c, 0x510b4108),
            (0x91940, 0x1e280821),
            (0x91944, 0x1e220102),
            (0x91948, 0x90000648),
            (0x9194c, 0x1e213801),
            (0x91950, 0x1e222020),
            (0x91954, 0x1e21cc41),
            (0x91958, 0x1e2703e2),
            (0x9195c, 0x1e224829),
            (0x91960, 0xbd4ab901),
            (0x91964, 0x7ea9d400),
            (0x91968, 0x1e212000),
            (0x9196c, 0x540008a4),
            (0x91970, 0x9000064a),
            (0x91974, 0xbd498140),
            (0x91978, 0x9000064b),
            (0x9197c, 0xb9409269),
            (0x91980, 0x93407d28),
            (0x91984, 0x2a0903f5),
            (0x91988, 0x1e202920),
            (0x9198c, 0xbd49e161),
            (0x91990, 0x52800b4a),
            (0x91994, 0x1280000b),
            (0x91998, 0x1b0a7eac),
            (0x9199c, 0x1e220182),
            (0x919a0, 0x1e222120),
            (0x919a4, 0x540000ed),
            (0x919a8, 0x320003ec),
            (0x919ac, 0x0b0c02b5),
            (0x919b0, 0x1b0a7eac),
            (0x919b4, 0x1e220182),
            (0x919b8, 0x1e222120),
            (0x919bc, 0x54ffff6c),
            (0x919c0, 0x1e212842),
            (0x919c4, 0x1e222000),
            (0x919c8, 0x2a0b03ec),
            (0x919cc, 0x540000e5),
            (0x919d0, 0x0b0c02b5),
            (0x919d4, 0x1b0a7eac),
            (0x919d8, 0x1e220182),
            (0x919dc, 0x1e222120),
            (0x919e0, 0x54ffff0d),
            (0x919e4, 0x17fffff1),
            (0x919e8, 0x6b15013f),
            (0x919ec, 0x540002c0),
            (0x919f0, 0xf9403a69),
            (0x919f4, 0x52801716),
            (0x919f8, 0x9b162508),
            (0x919fc, 0x91002100),
            (0x91a00, 0x9400a688),
            (0x91a04, 0x90000648),
            (0x91a08, 0xbd457101),
            (0x91a0c, 0x1e2703e0),
            (0x91a10, 0x321a03e1),
            (0x91a14, 0x2a1f03e2),
            (0x91a18, 0x9400a7fa),
            (0x91a1c, 0xf9403a68),
            (0x91a20, 0x9b3622a8),
            (0x91a24, 0x91002100),
            (0x91a28, 0xb9009275),
            (0x91a2c, 0x9400a67d),
            (0x91a30, 0x1e2e1000),
            (0x91a34, 0x1e2a1001),
            (0x91a38, 0x321a03e1),
            (0x91a3c, 0x2a1f03e2),
            (0x91a40, 0x9400a7f0),
            (0x91a44, 0x4ea91d20),
            (0x91a48, 0xaa1403e0),
            (0x91a4c, 0x9400a625),
            (0x91a50, 0x90000648),
            (0x91a54, 0xbd45d900),
            (0x91a58, 0xbd415269),
            (0x91a5c, 0x4ea81d01),
            (0x91a60, 0x94022c78),
            (0x91a64, 0x1e200920),
            (0x91a68, 0x90000648),
            (0x91a6c, 0xbd457101),
            (0x91a70, 0xbd015260),
            (0x91a74, 0x1e20c000),
            (0x91a78, 0x1e212000),
            (0x91a7c, 0x54000045),
            (0x91a80, 0xb901527f),
            (0x92ec8, 0xbd415260),
            (0x92ecc, 0xf0000628),
            (0x92ed0, 0xbd4ab901),
            (0x92ed4, 0x1e20c000),
            (0x92ed8, 0x1e212000),
            (0x92edc, 0x540000a4),
            (0x92ee0, 0x320003e0),
            (0x92ee4, 0xb9009a7f),
            (0x92ee8, 0xb901527f),
            (0x92eec, 0x14000002),
            (0x92ef0, 0x320003e0),
            (0x93290, 0xb9409a68),
            (0x93294, 0x37f80aa8),
            (0x93298, 0x4ea31c68),
            (0x9329c, 0x4ea11c29),
            (0x932a0, 0x340001a8),
            (0x932a4, 0xf9403a69),
            (0x932a8, 0x51000508),
            (0x932ac, 0x5280170a),
            (0x932b0, 0x9b2a2508),
            (0x932b4, 0x91018100),
            (0x932b8, 0x9400a05a),
            (0x932bc, 0xd0000628),
            (0x932c0, 0xbd497501),
            (0x932c4, 0x1e2703e0),
            (0x932c8, 0x321a03e1),
            (0x932cc, 0x2a1f03e2),
            (0x932d0, 0x9400a1cc),
            (0x932d4, 0x91028274),
            (0x932d8, 0xaa1403e0),
            (0x932dc, 0x9400a071),
            (0x932e0, 0xb9408a68),
            (0x932e4, 0x510b4108),
            (0x932e8, 0x1e293901),
            (0x932ec, 0x1e220102),
            (0x932f0, 0x1e213800),
            (0x932f4, 0x1e2703e1),
            (0x932f8, 0x1e222000),
            (0x932fc, 0x1e20cc40),
            (0x93300, 0xd000062a),
            (0x93304, 0xd000062b),
            (0x93308, 0xb9409269),
            (0x9330c, 0x93407d28),
            (0x93310, 0x2a0903f5),
            (0x93314, 0x1e214808),
            (0x93318, 0xbd498140),
            (0x9331c, 0x52800b4a),
            (0x93320, 0x1e202900),
            (0x93324, 0xbd49e161),
            (0x93328, 0x1280000b),
            (0x9332c, 0x1b0a7eac),
            (0x93330, 0x1e220182),
            (0x93334, 0x1e222100),
            (0x93338, 0x540000ed),
            (0x9333c, 0x320003ec),
            (0x93340, 0x0b0c02b5),
            (0x93344, 0x1b0a7eac),
            (0x93348, 0x1e220182),
            (0x9334c, 0x1e222100),
            (0x93350, 0x54ffff6c),
            (0x93354, 0x1e212842),
            (0x93358, 0x1e222000),
            (0x9335c, 0x2a0b03ec),
            (0x93360, 0x540000e5),
            (0x93364, 0x0b0c02b5),
            (0x93368, 0x1b0a7eac),
            (0x9336c, 0x1e220182),
            (0x93370, 0x1e222100),
            (0x93374, 0x54ffff0d),
            (0x93378, 0x17fffff1),
            (0x9337c, 0x6b15013f),
            (0x93380, 0x540002c0),
            (0x93384, 0xf9403a69),
            (0x93388, 0x52801716),
            (0x9338c, 0x9b162508),
            (0x93390, 0x91002100),
            (0x93394, 0x9400a023),
            (0x93398, 0xd0000628),
            (0x9339c, 0xbd457101),
            (0x933a0, 0x1e2703e0),
            (0x933a4, 0x321a03e1),
            (0x933a8, 0x2a1f03e2),
            (0x933ac, 0x9400a195),
            (0x933b0, 0xf9403a68),
            (0x933b4, 0x9b3622a8),
            (0x933b8, 0x91002100),
            (0x933bc, 0xb9009275),
            (0x933c0, 0x9400a018),
            (0x933c4, 0x1e2e1000),
            (0x933c8, 0x1e2a1001),
            (0x933cc, 0x321a03e1),
            (0x933d0, 0x2a1f03e2),
            (0x933d4, 0x9400a18b),
            (0x933d8, 0x4ea81d00),
            (0x933dc, 0xaa1403e0),
            (0x933e0, 0x94009fc0),
            (0x933e4, 0xb9009a7f),
            (0x93400, 0xb9409808),
            (0x93404, 0x34000048),
            (0x93408, 0xd65f03c0),
            (0x9340c, 0xbd015001),
            (0x93410, 0xd65f03c0),
            (0x92d08, 0xb9400288),
            (0x92d0c, 0x1b157d08),
            (0x92d10, 0xbc5ec3a1),
            (0x92d14, 0x1e220100),
            (0x92d18, 0x1e213801),
            (0x92d1c, 0xbc5e83a0),
            (0x92d20, 0x4eab1d62),
            (0x92d24, 0x4eac1d83),
            (0x92d28, 0x9400eac6),
            (0x92d2c, 0x1e212865),
            (0x92d30, 0x1e202068),
            (0x92d34, 0x1e202844),
            (0x92d38, 0x1e214ca3),
            (0x92d3c, 0x1e254c2d),
            (0x92d40, 0x1e202048),
            (0x92d44, 0x1e204c81),
            (0x92d48, 0x1e244c0e),
            (0x92d4c, 0x4ea11c20),
            (0x92d50, 0x4ea31c61),
            (0x92d54, 0x9400eaaf),
            (0x92d58, 0x4ea01c0f),
            (0x92d5c, 0x4ea11c2a),
            (0x92d60, 0x4eae1dc0),
            (0x92d64, 0x4ead1da1),
            (0x92d68, 0x9400eaaa),
            (0x92d6c, 0x1e212120),
            (0x92d70, 0x1e294542),
            (0x92d74, 0x1e2895e2),
            (0x92d78, 0x1e209500),
            (0x92d7c, 0x540000c4),
            (0x92d80, 0x9102e294),
            (0x92d84, 0xeb14027f),
            (0x92d88, 0x54fffc01),
            (0x92d8c, 0x2a1f03e0),
            (0x92d90, 0x14000003),
            (0x92d94, 0xb9400288),
            (0x92d98, 0x11000500),
            (0x917e8, 0xb9809268),
            (0x917ec, 0xf9403a69),
            (0x917f0, 0x5280170a),
            (0x917f4, 0x9b0a2508),
            (0x917f8, 0x91018100),
            (0x917fc, 0x9400a87d),
            (0x91800, 0x37000640),
            (0x91804, 0xb9408e68),
            (0x91808, 0x11000508),
            (0x9180c, 0xb9008e68),
            (0x91810, 0x91006260),
            (0x91814, 0x9400a877),
            (0x91818, 0x37000580),
            (0x9181c, 0xb9409268),
            (0x91820, 0xf9400669),
            (0x91824, 0x11001908),
            (0x91828, 0xb9005928),
            (0x9182c, 0xb9408e68),
            (0x91830, 0x11000508),
            (0x91834, 0x14000024),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [
            (0x159694, 32.0f32),
            (0x1595d8, 0.95),
            (0x159570, 0.1),
            (0x159ab8, f32::EPSILON),
            (0x1599e0, 90.0),
            (0x159980, 720.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
    }

    #[test]
    fn hint_confirmation_during_inertia_publishes_current_selection_at_native_phase() {
        let mut model = MurdererModel::new(20);
        model.open_menu(4);
        model.hint_scroll_velocity = 100.0;
        model.request_hint(20);
        model.update(2.0);
        assert_eq!(model.menu_focus, 17);
        assert_eq!(model.hint_scroll, 970.0);
        assert!((model.hint_feedback(20).1 - 0.4).abs() < 0.0001);
        assert_eq!(model.poll_result(), None);
        model.update(0.0); // Current17's flash is idle, even though20 remains hot.
        assert_eq!(
            model.response_completion.as_ref().unwrap().phase,
            ResponsePhase::Exit
        );
        // Zero-delta inertia stops natively; restore momentum to cover late selection.
        model.hint_scroll_velocity = 90.25;
        model.update(58.0);
        assert_eq!(model.menu_focus, 7);
        assert_eq!(model.poll_result(), None);
        model.update(0.0);
        assert_eq!(model.poll_result(), Some(13)); // Current7+6, not requested20+6.
        assert_eq!(model.take_sounds(), ["decide"]);
    }
    #[test]
    fn only_native_three_suspect_combination_succeeds() {
        // Exhaust all 131072 possible selections, including missing required
        // suspects and correct suspects accompanied by any additional person.
        for selection in 0u32..1 << 17 {
            let mut model = MurdererModel::new(9);
            assert!(model.open_menu(5));
            for index in 0..17 {
                if selection & (1 << index) != 0 {
                    assert!(model.activate_accusation_entry(FIRST_SUSPECT_ENTRY + index));
                }
            }
            assert!(model.activate_accusation_entry(SUBMIT_ENTRY));
            assert_eq!(
                model.result,
                Some(if selection == (7 << 6) { 27 } else { 37 }),
                "suspect selection {selection:#x}"
            );
        }
    }

    #[test]
    fn hint_unlocks_and_wrong_accusation_follow_progress() {
        for progress in [i32::MIN, -1, 0, 1, 19, 20, 21, i32::MAX] {
            let available = (i64::from(progress) + 1).clamp(0, 21) as usize;
            let mut model = MurdererModel::new(progress);
            assert_eq!(model.available_hints(), available);
            assert!(model.open_menu(4));
            assert!(!model.request_hint(available));
            if available > 0 {
                assert!(model.request_hint(available - 1));
                assert_eq!(model.result, Some(available as i32 + 5));
            }
            model.reset();
            assert!(model.open_menu(5));
            assert!(model.activate_accusation_entry(SUBMIT_ENTRY));
            assert_eq!(model.result, Some(progress.min(20).wrapping_add(28)));
        }
        for hint in 0..21 {
            let mut model = MurdererModel::new(20);
            assert!(model.open_menu(4));
            assert!(model.request_hint(hint));
            assert_eq!(model.result, Some(hint as i32 + 6));
        }
    }

    #[test]
    fn menu_routes_chapters_and_freezes_completed_response() {
        for (index, menu) in [
            MurdererMenu::Replay,
            MurdererMenu::Rules,
            MurdererMenu::PurpleByPerson,
            MurdererMenu::PurpleByChapter,
            MurdererMenu::Hints,
            MurdererMenu::Accusation,
        ]
        .into_iter()
        .enumerate()
        {
            let mut model = MurdererModel::new(0);
            assert!(!model.open_menu(6));
            assert!(!model.replay_chapter(0));
            assert!(!model.request_hint(0));
            assert!(!model.activate_accusation_entry(34));
            assert!(model.open_menu(index));
            assert_eq!(model.menu, menu);
            assert!(!model.open_menu(index));
            model.back();
            finish_back(&mut model);
            assert_eq!(model.menu, MurdererMenu::Top);
        }
        for chapter in 0..6 {
            let mut model = MurdererModel::new(0);
            assert!(model.open_menu(0));
            assert!(!model.replay_chapter(6));
            assert!(model.replay_chapter(chapter));
            assert_eq!(model.result, Some(chapter as i32));
            model.back();
            finish_back(&mut model);
            assert!(!model.open_menu(5));
            assert!(!model.replay_chapter((chapter + 1) % 6));
            assert_eq!(model.result, Some(chapter as i32));
        }
    }

    #[test]
    fn toggles_cancel_and_reset_clear_accusation() {
        let mut model = MurdererModel::new(12);
        assert!(model.open_menu(5));
        for entry in [0, 27, 46, usize::MAX] {
            assert!(!model.activate_accusation_entry(entry));
            assert!(!model.suspect_selected(entry));
        }
        assert!(model.activate_accusation_entry(34));
        assert!(model.suspect_selected(34));
        assert!(model.activate_accusation_entry(34));
        assert!(!model.suspect_selected(34));
        assert!(model.activate_accusation_entry(35));
        model.back();
        finish_back(&mut model);
        assert!(model.open_menu(5));
        assert!(!model.suspect_selected(35));
        for entry in 34..37 {
            assert!(model.activate_accusation_entry(entry));
        }
        assert!(model.activate_accusation_entry(SUBMIT_ENTRY));
        assert_eq!(model.result, Some(27));
        assert!(!model.activate_accusation_entry(28));
        model.reset();
        assert_eq!(model.menu, MurdererMenu::Top);
        assert_eq!(model.result, None);
        assert_eq!(model.available_hints(), 13);
        assert!((28..45).all(|entry| !model.suspect_selected(entry)));
    }

    #[test]
    fn accusation_press_release_focus_and_cancellation_preserve_native_feedback() {
        let mut model = MurdererModel::new(20);
        model.open_menu(5);
        model.take_sounds();
        let point = Vec2::new(580.0, 240.0); //29
        model.pointer_state(point, false);
        assert_eq!(model.accusation_focus, Some(28));
        model.pointer_state(point, true);
        assert_eq!(model.accusation_focus, Some(29));
        assert!(!model.suspect_selected(29));
        model.update(2.0);
        assert!((model.accusation_feedback(28).0 - 0.8).abs() < 1e-6);
        assert_eq!(model.accusation_feedback(29), (0.5, 0.4));
        assert!(model.take_sounds().is_empty());
        model.pointer_state(point, false);
        assert!(model.suspect_selected(29));
        assert_eq!(model.take_sounds(), ["decide"]);
        model.update(2.0);
        let (focus, flash) = model.accusation_feedback(29);
        assert_eq!(focus, 1.0);
        assert!((flash - 0.6).abs() < 1e-6); // Remaining1.5rise then0.5fall.
        model.update(10.0);
        model.pointer_state(point, true);
        model.update(2.0);
        model.pointer_state(Vec2::ZERO, true);
        model.update(1.0);
        assert!((model.accusation_feedback(29).1 - 0.2).abs() < 1e-6);
        model.pointer_state(point, false);
        assert!(model.suspect_selected(29)); // Canceled release never toggles.
        assert!(model.take_sounds().is_empty());
        model.pointer_state(point, true);
        model.update(10.0);
        assert_eq!(model.accusation_feedback(29), (1.0, 0.7));
        model.ignore_pointer_state(true);
        model.update(3.5);
        assert_eq!(model.accusation_feedback(29).1, 0.0);
        model.pointer_state(point, false);
        assert!(model.suspect_selected(29));
        model.reset();
        model.open_menu(5);
        assert_eq!(model.accusation_feedback(28), (1.0, 0.0));
        assert_eq!(model.accusation_feedback(29), (0.0, 0.0));
    }
    #[test]
    fn accusation_held_submit_waits_for_remaining_flash_before_white_phase() {
        let mut model = MurdererModel::new(20);
        model.open_menu(5);
        for entry in 34..37 {
            model.activate_accusation_entry(entry);
        }
        model.update(10.0);
        model.take_sounds();
        let point = Vec2::new(500.0, 850.0);
        model.pointer_state(point, true);
        model.update(10.0);
        assert_eq!(model.accusation_feedback(45), (1.0, 0.7));
        assert_eq!(model.poll_result(), None);
        assert!(model.take_sounds().is_empty());
        model.pointer_state(point, false);
        assert_eq!(model.take_sounds(), ["attack"]);
        model.update(1.75);
        assert!((model.accusation_feedback(45).1 - 0.35).abs() < 1e-6);
        assert_eq!(model.white_flash_opacity(), 0.0);
        model.update(1.75);
        assert_eq!(model.accusation_feedback(45).1, 0.0);
        assert_eq!(model.white_flash_opacity(), 0.0);
        model.update(0.0);
        assert_eq!(model.white_flash_opacity(), 1.0);
        model.update(30.0);
        assert_eq!(model.white_flash_opacity(), 0.0);
        assert_eq!(model.poll_result(), None);
        model.update(30.0);
        assert_eq!(model.poll_result(), None);
        model.update(0.0);
        assert_eq!(model.poll_result(), Some(27));
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_accusation_row_feedback() {
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
            (0x97d74, 0xb9801676),
            (0x97d78, 0xf9404277),
            (0x97d7c, 0x52801a08),
            (0x97d80, 0x9b085ec8),
            (0x97d84, 0x91004115),
            (0x97d88, 0x1e2e1000),
            (0x97d8c, 0xaa1503e0),
            (0x97d90, 0x94008d54),
            (0x986c8, 0x91004280),
            (0x986cc, 0x94008b91),
            (0x986d0, 0x9101a280),
            (0x986d4, 0x94008b8f),
            (0x986d8, 0x91034294),
            (0x986dc, 0xeb1402bf),
            (0x986e0, 0x54ffff41),
            (0x98748, 0xb9801668),
            (0x9874c, 0xf9404269),
            (0x98750, 0x52801a0a),
            (0x98754, 0x9b0a2508),
            (0x98758, 0x9101a100),
            (0x9875c, 0x94008ca5),
            (0x98760, 0x37000ea0),
            (0x98764, 0x1e2e1000),
            (0x98768, 0x9106a260),
            (0x9876c, 0x94008add),
            (0x98770, 0x1e2703e9),
            (0x98774, 0x1e27d001),
            (0x98778, 0x2a1f03e1),
            (0x9877c, 0x4ea91d20),
            (0x98780, 0x2a1f03e2),
            (0x98784, 0x94008c9f),
            (0x98788, 0x320003e8),
            (0x9878c, 0xb901a268),
            (0x98790, 0xb0000608),
            (0x98794, 0xbd479901),
            (0x98798, 0x4ea91d20),
            (0x9879c, 0x9100a260),
            (0x987a0, 0x2a1f03e1),
            (0x987a4, 0x2a1f03e2),
            (0x987a8, 0x94008c96),
            (0x987ac, 0xb9401268),
            (0x987b0, 0x11000508),
            (0x987b4, 0xb9001268),
            (0x99f64, 0xb9801668),
            (0x99f68, 0x6b15011f),
            (0x99f6c, 0x54000e00),
            (0x99f70, 0xf9404269),
            (0x99f74, 0x9b142508),
            (0x99f78, 0x91004100),
            (0x99f7c, 0x94008529),
            (0x99f80, 0x90000608),
            (0x99f84, 0xbd457101),
            (0x99f88, 0x1e2703e0),
            (0x99f8c, 0x321a03e1),
            (0x99f90, 0x2a1f03e2),
            (0x99f94, 0x9400869b),
            (0x99f98, 0xf9404268),
            (0x99f9c, 0x9b3422a8),
            (0x99fa0, 0x91004100),
            (0x99fa4, 0xb9001675),
            (0x99fa8, 0x9400851e),
            (0x99fac, 0x1e2e1000),
            (0x99fb0, 0x1e2a1001),
            (0x99fb4, 0x321a03e1),
            (0x99fb8, 0x2a1f03e2),
            (0x99fbc, 0x94008691),
            (0x99fc0, 0x1400005b),
            (0x9a028, 0xb9801674),
            (0x9a02c, 0xf9404275),
            (0x9a030, 0x52801a08),
            (0x9a034, 0x9b085696),
            (0x9a038, 0x9101a2c0),
            (0x9a03c, 0x940084f9),
            (0x9a040, 0xf00005e8),
            (0x9a044, 0xbd45a500),
            (0x9a048, 0xf00005e8),
            (0x9a04c, 0xbd497508),
            (0x9a050, 0x4ea81d01),
            (0x9a054, 0x321a03e1),
            (0x9a058, 0x2a1f03e2),
            (0x9a05c, 0x94008669),
            (0x9a060, 0x1e2703e0),
            (0x9a064, 0x4ea81d01),
            (0x9a068, 0x321a03e1),
            (0x9a06c, 0x2a1f03e2),
            (0x9a070, 0x94008664),
            (0x9a074, 0xb94006c8),
            (0x9a29c, 0x92800008),
            (0x9a2a0, 0xf9000c08),
            (0x9a2a4, 0xb9401008),
            (0x9a2a8, 0x34000068),
            (0x9a2ac, 0x2a1f03e0),
            (0x9a2b0, 0x14000037),
            (0x9a400, 0xb9401e68),
            (0x9a404, 0x37f80508),
            (0x9a408, 0x340004e8),
            (0x9a40c, 0x51000514),
            (0x9a410, 0xf9404268),
            (0x9a414, 0x52801a09),
            (0x9a418, 0x9b292288),
            (0x9a41c, 0x9101a100),
            (0x9a420, 0x94008400),
            (0x9a424, 0xf00005e8),
            (0x9a428, 0xbd45a500),
            (0x9a42c, 0xf00005e8),
            (0x9a430, 0xbd497501),
            (0x9a434, 0x321a03e1),
            (0x9a438, 0x2a1f03e2),
            (0x9a43c, 0x94008571),
            (0x9a440, 0xb9801668),
            (0x9a444, 0x6b14011f),
            (0x9a448, 0x540002e0),
            (0x9a44c, 0xf9404269),
            (0x9a450, 0x52801a16),
            (0x9a454, 0x9b162508),
            (0x9a458, 0x91004100),
            (0x9a45c, 0x93407e95),
            (0x9a460, 0x940083f0),
            (0x9a464, 0xf00005e8),
            (0x9a468, 0xbd457101),
            (0x9a46c, 0x1e2703e0),
            (0x9a470, 0x321a03e1),
            (0x9a474, 0x2a1f03e2),
            (0x9a478, 0x94008562),
            (0x9a47c, 0xf9404268),
            (0x9a480, 0x9b1622a8),
            (0x9a484, 0x91004100),
            (0x9a488, 0xb9001674),
            (0x9a48c, 0x940083e5),
            (0x9a490, 0x1e2e1000),
            (0x9a494, 0x1e2a1001),
            (0x9a498, 0x321a03e1),
            (0x9a49c, 0x2a1f03e2),
            (0x9a4a0, 0x94008558),
            (0x9a4a4, 0xa9427bfd),
            (0x9a4a8, 0xa9414ff4),
            (0x9a4ac, 0xa8c357f6),
            (0x9a4b0, 0xd65f03c0),
            (0x9a58c, 0xb9401e68),
            (0x9a590, 0x37f80c08),
            (0x9a594, 0x34000ba8),
            (0x9a598, 0xf9404275),
            (0x9a59c, 0x51000516),
            (0x9a5a0, 0x52801a08),
            (0x9a5a4, 0x9b2856c8),
            (0x9a5a8, 0x9101a100),
            (0x9a5ac, 0x93407ed4),
            (0x9a5b0, 0x9400839c),
            (0x9a5b4, 0xf00005e8),
            (0x9a5b8, 0xbd45a500),
            (0x9a5bc, 0xf00005e8),
            (0x9a5c0, 0xbd497508),
            (0x9a5c4, 0x4ea81d01),
            (0x9a5c8, 0x321a03e1),
            (0x9a5cc, 0x2a1f03e2),
            (0x9a5d0, 0x9400850c),
            (0x9a5d4, 0x1e2703e0),
            (0x9a5d8, 0x4ea81d01),
            (0x9a5dc, 0x321a03e1),
            (0x9a5e0, 0x2a1f03e2),
            (0x9a5e4, 0x94008507),
            (0x9a5e8, 0xb9801668),
            (0x9a5ec, 0x6b16011f),
            (0x9a5f0, 0x540002c0),
            (0x9a5f4, 0xf9404269),
            (0x9a5f8, 0x52801a17),
            (0x9a5fc, 0x9b172508),
            (0x9a600, 0x91004100),
            (0x9a604, 0x94008387),
            (0x9a608, 0xf00005e8),
            (0x9a60c, 0xbd457101),
            (0x9a610, 0x1e2703e0),
            (0x9a614, 0x321a03e1),
            (0x9a618, 0x2a1f03e2),
            (0x9a61c, 0x940084f9),
            (0x9a620, 0xf9404268),
            (0x9a624, 0x9b172288),
            (0x9a628, 0x91004100),
            (0x9a62c, 0xb9001676),
            (0x9a630, 0x9400837c),
            (0x9a634, 0x1e2e1000),
            (0x9a638, 0x1e2a1001),
            (0x9a63c, 0x321a03e1),
            (0x9a640, 0x2a1f03e2),
            (0x9a644, 0x940084ef),
            (0x9a648, 0x52801a08),
            (0x9a64c, 0x9b085688),
            (0x9a650, 0xb9400508),
            (0x9a654, 0x7100b51f),
            (0x9a658, 0x540003e1),
            (0x9a6d4, 0xd0000c48),
            (0x9a6d8, 0xf9437108),
            (0x9a6dc, 0xf9400100),
            (0x9a6e0, 0x1e2e1000),
            (0x9a6e4, 0x97fe32f3),
            (0x9a6e8, 0x52801a08),
            (0x9a6ec, 0x9b085688),
            (0x9a6f0, 0x39402109),
            (0x9a6f4, 0x52000129),
            (0x9a6f8, 0x39002109),
            (0x9a6fc, 0x14000003),
            (0x9a700, 0x3900827f),
            (0x9a704, 0xb9001268),
            (0x9a708, 0x12800008),
            (0x9a70c, 0xb9001e68),
            (0x9a830, 0xb9401e68),
            (0x9a834, 0x37f80208),
            (0x9a838, 0x340001a8),
            (0x9a83c, 0xf9404269),
            (0x9a840, 0x51000508),
            (0x9a844, 0x52801a0a),
            (0x9a848, 0x9b2a2508),
            (0x9a84c, 0x9101a100),
            (0x9a850, 0x940082f4),
            (0x9a854, 0xf00005e8),
            (0x9a858, 0xbd497501),
            (0x9a85c, 0x1e2703e0),
            (0x9a860, 0x321a03e1),
            (0x9a864, 0x2a1f03e2),
            (0x9a868, 0x94008466),
            (0x9a86c, 0x12800008),
            (0x9a870, 0xb9001e68),
            (0x99210, 0xb980073c),
            (0x99214, 0x91004320),
            (0x99218, 0x940088a2),
            (0x9921c, 0x1e2c1001),
            (0x99220, 0x9101a320),
            (0x99224, 0x1e21080e),
            (0x99228, 0x9400889e),
            (0x9922c, 0x4ea01c0d),
            (0x992dc, 0x4eac1d80),
            (0x992e0, 0x4eac1d81),
            (0x992e4, 0x4eac1d82),
            (0x992e8, 0x4eae1dc3),
            (0x992ec, 0x97fef519),
            (0x992f0, 0x2d0507e0),
            (0x992f4, 0x2d060fe2),
            (0x992f8, 0x4eac1d80),
            (0x992fc, 0x4eac1d81),
            (0x99300, 0x4eac1d82),
            (0x99304, 0x4ead1da3),
            (0x99308, 0x97fef512),
            (0x9930c, 0x9100e3e0),
            (0x99310, 0x9100a3e1),
            (0x99314, 0x910063e2),
            (0x99318, 0x2d0307e0),
            (0x9931c, 0x2d040fe2),
            (0x99320, 0x9400b710),
            (0x99470, 0x1e2021c8),
            (0x99474, 0x540002ad),
            (0x99478, 0x4eac1d80),
            (0x9947c, 0x4eac1d81),
            (0x99480, 0x4eac1d82),
            (0x99484, 0x4eae1dc3),
            (0x99488, 0x97fef4b2),
            (0x9948c, 0x9100a3e0),
            (0x99490, 0x2d0507e0),
            (0x99494, 0x2d060fe2),
            (0x99498, 0x97fef4ea),
            (0x9949c, 0x2a0003e1),
            (0x994a0, 0x9100e3e0),
            (0x994a4, 0x9400b623),
            (0x994a8, 0x9100e3e0),
            (0x994ac, 0x9400b629),
            (0x994b0, 0x1e2021a8),
            (0x994b4, 0x540000ec),
            (0x994b8, 0x91034339),
            (0x994bc, 0xeb19035f),
            (0x994c0, 0x54ffea81),
            (0x994c4, 0x14000014),
            (0x994c8, 0x1e2021a8),
            (0x994cc, 0x54ffff6d),
            (0x994d0, 0x4eac1d80),
            (0x994d4, 0x4eac1d81),
            (0x994d8, 0x4eac1d82),
            (0x994dc, 0x4ead1da3),
            (0x994e0, 0x97fef49c),
            (0x994e4, 0x9100a3e0),
            (0x994e8, 0x2d0507e0),
            (0x994ec, 0x2d060fe2),
            (0x994f0, 0x97fef4d4),
            (0x994f4, 0x2a0003e1),
            (0x994f8, 0x9100e3e0),
            (0x994fc, 0x9400b60d),
            (0x99500, 0x9100e3e0),
            (0x99504, 0x9400b613),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        for (address, value) in [(0x159570, 0.1f32), (0x1595a4, 0.7), (0x159974, 0.2)] {
            assert_eq!(word(address), value.to_bits());
        }
        let reloc = |target| {
            let r = exe[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find(|r| u64::from_le_bytes(r[..8].try_into().unwrap()) == target)
                .unwrap();
            u64::from_le_bytes(r[16..24].try_into().unwrap())
        };
        for (offset, target) in [
            (0x50, 0x9a280),
            (0x60, 0x9a3c0),
            (0x68, 0x9a4c0),
            (0x78, 0x9a7e0),
        ] {
            assert_eq!(reloc(0x1d9a10 + offset), target);
        }
    }
    #[test]
    fn native_accusation_rectangles_and_pointer_submit() {
        assert_eq!(accusation_rect(34), Some(vec4(450.0, 453.5, 88.0, 99.0)));
        assert_eq!(accusation_rect(35), Some(vec4(562.0, 453.5, 88.0, 99.0)));
        assert_eq!(accusation_rect(36), Some(vec4(674.0, 453.5, 88.0, 99.0)));
        assert_eq!(accusation_hit_test(Vec2::new(500.0, 899.0)), Some(45));
        assert_eq!(accusation_hit_test(Vec2::new(500.0, 900.0)), None);
        for entry in 28..=45 {
            let rect = accusation_rect(entry).unwrap();
            assert_eq!(
                accusation_hit_test(Vec2::new(rect.x, rect.y) + Vec2::new(1.0, 1.0)),
                Some(entry)
            );
            assert_eq!(
                accusation_hit_test(Vec2::new(rect.x, rect.y) - Vec2::new(1.0, 0.0)),
                None
            );
        }
        for point in [
            Vec2::ZERO,
            Vec2::new(780.0, 500.0),
            Vec2::NAN,
            Vec2::INFINITY,
        ] {
            assert_eq!(accusation_hit_test(point), None);
        }
        let mut model = MurdererModel::new(0);
        assert!(model.open_menu(5));
        for entry in 34..37 {
            let rect = accusation_rect(entry).unwrap();
            let position = Vec2::new(rect.x, rect.y) + Vec2::splat(20.0);
            let selected = model.accusation_focus;
            model.accusation_pointer(position, false);
            assert_eq!(model.accusation_focus, selected);
            assert!(!model.suspect_selected(entry));
            model.accusation_pointer(position, true);
            assert!(model.suspect_selected(entry));
            assert_eq!(model.result, None);
        }
        model.accusation_pointer(Vec2::new(500.0, 900.0), true);
        assert_eq!(model.result, None);
        model.accusation_pointer(Vec2::new(500.0, 850.0), true);
        assert_eq!(model.result, Some(27));
        model.accusation_pointer(Vec2::new(500.0, 480.0), true);
        assert!(model.suspect_selected(34));
        model.reset();
        assert_eq!(model.accusation_focus, Some(28));
        assert_eq!(model.last_pointer, None);
    }

    #[test]
    fn menu_pointer_and_keyboard_preserve_native_indexes_and_back_focus() {
        let mut model = MurdererModel::new(20);
        model.navigate(-1);
        assert_eq!(model.menu_focus, 5);
        model.confirm();
        finish_top_opening(&mut model);
        assert_eq!(model.menu, MurdererMenu::Accusation);
        model.pointer(Vec2::new(1700.0, 60.0), true);
        finish_back(&mut model);
        assert_eq!(model.menu, MurdererMenu::Top);
        assert_eq!(model.menu_focus, 5);
        model.pointer(Vec2::new(1200.0, 500.0), true);
        finish_top_opening(&mut model);
        assert_eq!(model.menu, MurdererMenu::Replay);
        let rect = replay_rect(4).unwrap();
        model.pointer(Vec2::new(rect.x + 10.0, rect.y + 10.0), true);
        assert_eq!(model.result, Some(4));
        model.pointer(Vec2::new(1700.0, 60.0), true);
        finish_back(&mut model);
        assert_eq!(model.result, Some(4));
        model.reset();
        model.pointer(Vec2::new(1200.0, 600.0), true);
        finish_top_opening(&mut model);
        assert_eq!(model.menu, MurdererMenu::Rules);
        assert_eq!(model.result, None);
        model.confirm();
        assert_eq!(model.result, None);
        model.back();
        finish_back(&mut model);
        assert_eq!(model.menu_focus, 1);
    }

    #[test]
    fn hint_scroll_and_text_rows_keep_unlocked_original_indexes() {
        let mut model = MurdererModel::new(20);
        assert!(model.open_menu(4));
        assert_eq!(model.menu_focus, 20);
        assert_eq!(model.hint_scroll, 1170.0);
        assert_eq!(model.hint_texture_row(20), Some(0));
        assert_eq!(model.hint_texture_row(0), Some(1));
        assert_eq!(model.hint_texture_row(21), None);
        assert_eq!(model.hint_rect(13), Some(vec4(320.0, 180.0, 1280.0, 88.0)));
        model.pointer(Vec2::new(500.0, 179.0), true);
        assert_eq!(model.result, None);
        model.pointer(Vec2::new(500.0, 180.0), true);
        assert_eq!(model.result, Some(19));
        model.reset();
        assert!(model.open_menu(4));
        model.navigate(1);
        assert_eq!(model.menu_focus, 0);
        assert_eq!(model.hint_scroll, 0.0);
        model.confirm();
        assert_eq!(model.result, Some(6));
        for progress in 0..21 {
            model = MurdererModel::new(progress);
            assert!(model.open_menu(4));
            assert_eq!(model.menu_focus, progress as usize);
            let rect = model.hint_rect(progress as usize).unwrap();
            assert!(rect.y >= 180.0 && rect.y + rect.w <= 900.0);
            model.confirm();
            assert_eq!(model.result, Some(progress + 6));
        }
    }

    #[test]
    fn purple_browsers_preserve_native_indexes_without_finishing_quiz() {
        let mut model = MurdererModel::new(0);
        assert!(model.open_menu(3));
        assert_eq!(model.purple_original_index(), Some(1));
        assert_eq!(model.purple_text(), Some(CHAPTERS[1].text));
        for index in 0..CHAPTERS.len() {
            let selectable = CHAPTERS[index].heading().is_none();
            assert_eq!(model.select_purple_original_index(index), selectable);
            if selectable {
                assert_eq!(model.purple_original_index(), Some(index));
                assert_eq!(model.purple_text(), Some(CHAPTERS[index].text));
                model.confirm();
                assert_eq!(model.result, None);
            }
        }
        assert!(!model.select_purple_original_index(29));
        model.navigate(1);
        assert_eq!(model.purple_original_index(), Some(1));
        model.navigate(-1);
        assert_eq!(model.purple_original_index(), Some(28));
        model.back();
        finish_back(&mut model);
        assert_eq!(model.menu_focus, 3);
        assert_eq!(model.purple_text(), None);
        assert!(model.open_menu(2));
        for index in 0..17 {
            assert!(model.select_purple_original_index(index));
            assert_eq!(model.purple_original_index(), Some(index));
            assert_eq!(model.purple_text(), Some(PERSON_TEXT[index]));
            model.confirm();
            assert_eq!(model.result, None);
        }
        assert!(!model.select_purple_original_index(usize::MAX));
        model.navigate(1);
        assert_eq!(model.purple_original_index(), Some(0));
        model.back();
        finish_back(&mut model);
        assert_eq!(model.menu_focus, 2);
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_purple_chapter_pointer_feedback() {
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
            (0x971ac, 0x3700044a),
            (0x971b0, 0x4ea91d20),
            (0x971b4, 0x4ea81d01),
            (0x971b8, 0xaa1303e0),
            (0x971bc, 0x97ffff79),
            (0x971c0, 0xb9002660),
            (0x971c4, 0x37f80160),
            (0x971c8, 0xbd408e60),
            (0x971cc, 0xd0000608),
            (0x971d0, 0xbd4ab901),
            (0x971d4, 0x1e20c000),
            (0x971d8, 0x1e212000),
            (0x971dc, 0x540002a4),
            (0x971e0, 0xb2607fe8),
            (0x971e4, 0xf9001268),
            (0x971e8, 0xb9008e7f),
            (0x971ec, 0x14000011),
            (0x971f0, 0x4ea91d20),
            (0x971f4, 0x4ea81d01),
            (0x971f8, 0xaa1303e0),
            (0x971fc, 0x97fffebd),
            (0x97200, 0xb9002260),
            (0x97204, 0x37f80260),
            (0x97208, 0xbd408e60),
            (0x9720c, 0xd0000608),
            (0x97210, 0xbd4ab901),
            (0x97214, 0x1e20c000),
            (0x97218, 0x1e212000),
            (0x9721c, 0x540000a4),
            (0x97220, 0xb900227f),
            (0x97224, 0x320003e0),
            (0x97228, 0xb9008e7f),
            (0x9722c, 0x14000002),
            (0x97230, 0x320003e0),
            (0x9700c, 0xbd400e80),
            (0x97010, 0xbc5ec3a1),
            (0x97014, 0x1e212801),
            (0x97018, 0xbc5e83a0),
            (0x9701c, 0x4eab1d62),
            (0x97020, 0x4eac1d83),
            (0x97024, 0x9400da07),
            (0x97028, 0x1e212865),
            (0x9702c, 0x1e202068),
            (0x97030, 0x1e202844),
            (0x97034, 0x1e214ca3),
            (0x97038, 0x1e254c2d),
            (0x9703c, 0x1e202048),
            (0x97040, 0x1e204c81),
            (0x97044, 0x1e244c0e),
            (0x97048, 0x4ea11c20),
            (0x9704c, 0x4ea31c61),
            (0x97050, 0x9400d9f0),
            (0x97054, 0x4ea01c0f),
            (0x97058, 0x4ea11c2a),
            (0x9705c, 0x4eae1dc0),
            (0x97060, 0x4ead1da1),
            (0x97064, 0x9400d9eb),
            (0x97068, 0x1e292140),
            (0x9706c, 0x1e2895e2),
            (0x97070, 0x1e209500),
            (0x97074, 0x54000065),
            (0x97078, 0x1e212120),
            (0x9707c, 0x540000e4),
            (0x972f8, 0xb9802668),
            (0x972fc, 0x37f80208),
            (0x97300, 0xf9412669),
            (0x97304, 0x52800d0a),
            (0x97308, 0x9b0a2508),
            (0x9730c, 0x91004100),
            (0x97310, 0x94009044),
            (0x97314, 0xd0000608),
            (0x97318, 0xbd45a500),
            (0x9731c, 0xd0000608),
            (0x97320, 0xbd497501),
            (0x97324, 0x321a03e1),
            (0x97328, 0x2a1f03e2),
            (0x9732c, 0x940091b5),
            (0x97468, 0xb9802668),
            (0x9746c, 0x37f806a8),
            (0x97470, 0xf9412669),
            (0x97474, 0x52800d14),
            (0x97478, 0x9b142508),
            (0x9747c, 0x91004100),
            (0x97480, 0x94008fe8),
            (0x97484, 0xd0000608),
            (0x97488, 0xbd45a500),
            (0x9748c, 0xd0000608),
            (0x97490, 0xbd497509),
            (0x97494, 0x4ea91d21),
            (0x97498, 0x321a03e1),
            (0x9749c, 0x2a1f03e2),
            (0x974a0, 0x94009158),
            (0x974a4, 0x1e2703e8),
            (0x974a8, 0x4ea81d00),
            (0x974ac, 0x4ea91d21),
            (0x974b0, 0x321a03e1),
            (0x974b4, 0x2a1f03e2),
            (0x974b8, 0x94009152),
            (0x974bc, 0xb0000c68),
            (0x974c0, 0xf9437108),
            (0x974c4, 0xf9400100),
            (0x974c8, 0x1e2e1000),
            (0x974cc, 0x97fe3f79),
            (0x974d0, 0x9100c260),
            (0x974d4, 0x94008fd3),
            (0x974d8, 0xb9802668),
            (0x974dc, 0xf9412669),
            (0x974e0, 0x9b142508),
            (0x974e4, 0xbd400500),
            (0x974e8, 0x1e202008),
            (0x974ec, 0x54000164),
            (0x974f0, 0xd0000608),
            (0x974f4, 0xbd408a61),
            (0x974f8, 0xbd43a902),
            (0x974fc, 0x1e222821),
            (0x97500, 0x1e202028),
            (0x97504, 0x540000a9),
            (0x97508, 0x1e202020),
            (0x9750c, 0x4ea01c08),
            (0x97510, 0x54000045),
            (0x97514, 0x4ea11c28),
            (0x97518, 0x1e221001),
            (0x9751c, 0x4ea81d00),
            (0x97520, 0x321f03e1),
            (0x97524, 0x2a1f03e2),
            (0x97528, 0x94009136),
            (0x97624, 0xb9802668),
            (0x97628, 0x37f80228),
            (0x9762c, 0xf9412669),
            (0x97630, 0x52800d0a),
            (0x97634, 0x9b0a2508),
            (0x97638, 0x91004100),
            (0x9763c, 0x94008f79),
            (0x97640, 0xd0000608),
            (0x97644, 0xbd497501),
            (0x97648, 0x1e2703e0),
            (0x9764c, 0x321a03e1),
            (0x97650, 0x2a1f03e2),
            (0x97654, 0x940090eb),
            (0x97658, 0xb2607fe8),
            (0x9765c, 0xf9001268),
            (0x97660, 0xa9417bfd),
            (0x97664, 0xf84207f3),
            (0x97668, 0xd65f03c0),
            (0x9733c, 0xb9402268),
            (0x97340, 0x37ffff88),
            (0x97344, 0x34ffff68),
            (0x97348, 0x51000514),
            (0x9734c, 0xf9411a68),
            (0x97350, 0x52801909),
            (0x97354, 0x9b292288),
            (0x97358, 0x91018100),
            (0x9735c, 0x94009031),
            (0x97360, 0xd0000608),
            (0x97364, 0xbd45a500),
            (0x97368, 0xd0000608),
            (0x9736c, 0xbd497501),
            (0x97370, 0x321a03e1),
            (0x97374, 0x2a1f03e2),
            (0x97378, 0x940091a2),
            (0x9737c, 0xa9417bfd),
            (0x97380, 0xaa1303e0),
            (0x97384, 0x2a1403e1),
            (0x97388, 0x320003e2),
            (0x9738c, 0xa8c24ff4),
            (0x97390, 0x17fffde4),
            (0x97540, 0xb9402268),
            (0x97544, 0x37ffff48),
            (0x97548, 0x34ffff28),
            (0x9754c, 0x51000514),
            (0x97550, 0xb0000c68),
            (0x97554, 0xf9437108),
            (0x97558, 0xf9400100),
            (0x9755c, 0x1e2e1000),
            (0x97560, 0x97fe3f54),
            (0x97564, 0xf9411a68),
            (0x97568, 0x52801909),
            (0x9756c, 0x9b292288),
            (0x97570, 0x91018100),
            (0x97574, 0x94008fab),
            (0x97578, 0xd0000608),
            (0x9757c, 0xbd45a500),
            (0x97580, 0xd0000608),
            (0x97584, 0xbd497508),
            (0x97588, 0x4ea81d01),
            (0x9758c, 0x321a03e1),
            (0x97590, 0x2a1f03e2),
            (0x97594, 0x9400911b),
            (0x97598, 0x1e2703e0),
            (0x9759c, 0x4ea81d01),
            (0x975a0, 0x321a03e1),
            (0x975a4, 0x2a1f03e2),
            (0x975a8, 0x94009116),
            (0x975ac, 0x6d4123e9),
            (0x975b0, 0xaa1303e0),
            (0x975b4, 0x2a1403e1),
            (0x975b8, 0xa9437bfd),
            (0x975bc, 0xa9424ff4),
            (0x975c0, 0x320003e2),
            (0x975c4, 0xfc4407ea),
            (0x975c8, 0x17fffd56),
            (0x957a0, 0x1e2c1000),
            (0x957a4, 0x1e20090f),
            (0x957a8, 0x4eab1d60),
            (0x957ac, 0x4eab1d61),
            (0x957b0, 0x4eab1d62),
            (0x957b4, 0x4eaf1de3),
            (0x957b8, 0x97ff03e6),
            (0x957bc, 0x2d0607e0),
            (0x957c0, 0x2d070fe2),
            (0x957c4, 0x4eab1d60),
            (0x957c8, 0x4eab1d61),
            (0x957cc, 0x4eab1d62),
            (0x957d0, 0x4eae1dc3),
            (0x957d4, 0x97ff03df),
            (0x957d8, 0x2d0e07e0),
            (0x957dc, 0x2d0f0fe2),
            (0x957e0, 0xd103e3a0),
            (0x957e4, 0x9100c3e1),
            (0x957e8, 0x9101c3e2),
            (0x957ec, 0x9400c5dd),
            (0x95894, 0x4eab1d60),
            (0x95898, 0x4eab1d61),
            (0x9589c, 0x4eab1d62),
            (0x958a0, 0x4eaf1de3),
            (0x958a4, 0x97ff03ab),
            (0x958a8, 0x2d0607e0),
            (0x958ac, 0x2d070fe2),
            (0x958b0, 0x4eab1d60),
            (0x958b4, 0x4eab1d61),
            (0x958b8, 0x4eab1d62),
            (0x958bc, 0x4eae1dc3),
            (0x958c0, 0x97ff03a4),
            (0x958c4, 0x2d0e07e0),
            (0x958c8, 0x2d0f0fe2),
            (0x958cc, 0xd103e3a0),
            (0x958d0, 0x9100c3e1),
            (0x958d4, 0x9101c3e2),
            (0x958d8, 0x9400c5a2),
            (0x959e4, 0x91004280),
            (0x959e8, 0x940096ae),
            (0x959ec, 0x4ea01c03),
            (0x959f0, 0x4eab1d60),
            (0x959f4, 0x4eab1d61),
            (0x959f8, 0x4eab1d62),
            (0x959fc, 0x97ff0355),
            (0x95a00, 0xd103e3a0),
            (0x95a04, 0x910203e1),
            (0x95a08, 0x2d1007e0),
            (0x95a0c, 0x2d110fe2),
            (0x95a10, 0x9400c540),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        for (address, value) in [
            (0x1599e0, 90.0f32),
            (0x159940, 88.0),
            (0x1595a4, 0.7),
            (0x159974, 0.2),
            (0x1593a8, -720.0),
        ] {
            assert_eq!(word(address), value.to_bits());
        }
    }
    #[test]
    fn purple_chapter_heading_press_has_priority_and_release_scrolls_without_selecting() {
        let mut model = MurdererModel::new(20);
        model.open_menu(3);
        model.reference_set_purple_list_scroll(495.0);
        model.purple_text_scroll = 100.0;
        let point = Vec2::new(200.0, 200.0); // Outgoing heading0 over statement5.
        model.pointer_state(point, true);
        model.update(2.0);
        assert_eq!(model.purple_original_index(), Some(1));
        assert_eq!(model.purple_heading_flash(0), 0.4);
        assert_eq!(model.purple_list_scroll, 495.0);
        assert!(model.take_sounds().is_empty());
        model.pointer_state(point, false);
        assert_eq!(model.take_sounds(), ["decide"]);
        assert_eq!(model.purple_text_scroll, 100.0);
        model.update(2.0);
        assert!(
            (model.purple_list_scroll - 495.0 * (1.0 - std::f32::consts::FRAC_1_SQRT_2)).abs()
                < 0.001
        );
        assert!((model.purple_heading_flash(0) - 0.6).abs() < 1e-6);
        model.update(2.0);
        assert_eq!(model.purple_list_scroll, 0.0);
        assert!(!model.purple_list_scrolling());
        assert_eq!(model.result, None);
        model.reference_set_purple_list_scroll(1890.0);
        model.pointer(Vec2::new(200.0, 640.0), true); // Last heading26, clamp2340 to1890.
        assert_eq!(model.purple_original_index(), Some(1));
        model.update(4.0);
        assert_eq!(model.purple_list_scroll, 1890.0);
        model.reset();
        model.open_menu(3);
        model.pointer_state(Vec2::new(200.0, 200.0), true);
        model.update(2.0);
        model.pointer_state(Vec2::ZERO, true);
        model.update(1.0);
        assert!((model.purple_heading_flash(0) - 0.2).abs() < 1e-6);
        model.pointer_state(Vec2::new(200.0, 200.0), false);
        assert!(model.take_sounds().is_empty());
    }
    #[test]
    fn purple_chapter_row_press_selects_silently_and_release_preserves_hot_flash() {
        let mut model = MurdererModel::new(20);
        model.open_menu(3);
        let point = Vec2::new(200.0, 390.0); // Original2, packed1.
        model.pointer_state(point, false);
        assert_eq!(model.purple_original_index(), Some(1));
        model.pointer_state(point, true);
        model.update(2.0);
        assert_eq!(model.purple_original_index(), Some(2));
        assert_eq!(model.purple_chapter_feedback(2), (0.5, 0.4));
        assert!((model.purple_chapter_feedback(1).0 - 0.8).abs() < 1e-6);
        assert!(model.take_sounds().is_empty());
        model.update(8.0);
        assert_eq!(model.purple_chapter_feedback(2), (1.0, 0.7));
        model.pointer_state(point, false);
        assert_eq!(model.take_sounds(), ["decide"]);
        model.update(1.75);
        assert!((model.purple_chapter_feedback(2).1 - 0.35).abs() < 1e-6);
        model.update(1.75);
        assert_eq!(model.purple_chapter_feedback(2).1, 0.0);
        model.purple_text_scroll = 50.0;
        model.pointer(point, true);
        assert!(model.take_sounds().is_empty());
        assert_eq!(model.purple_text_scroll, 50.0); // Fresh current row is ineligible.
        assert_eq!(model.result, None);
    }
    #[test]
    fn purple_headings_pin_then_slide_out_before_the_next_section() {
        let mut model = MurdererModel::new(20);
        model.open_menu(3);
        for (scroll, y0, y1) in [
            (0.0, 180.0, 720.0),
            (45.0, 180.0, 675.0),
            (450.0, 180.0, 270.0),
            (495.0, 135.0, 225.0),
            (539.0, 91.0, 181.0),
            (540.0, 90.0, 180.0),
            (585.0, 45.0, 180.0),
        ] {
            model.purple_list_scroll = scroll;
            assert_eq!(
                model.purple_heading_rect(0),
                Some(vec4(166.0, y0, 480.0, 88.0))
            );
            assert_eq!(
                model.purple_heading_rect(1),
                Some(vec4(166.0, y1, 480.0, 88.0))
            );
        }
        model.purple_list_scroll = 1125.0;
        assert_eq!(model.purple_heading_rect(1).unwrap().y, 180.0);
        model.purple_list_scroll = 1215.0;
        assert_eq!(model.purple_heading_rect(1).unwrap().y, 135.0);
        assert_eq!(model.purple_heading_rect(2).unwrap().y, 225.0);
        assert_eq!(model.purple_heading_rect(6), None);
        model.reset();
        assert_eq!(model.purple_heading_rect(0), None);
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_purple_heading_geometry() {
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
            (0x94edc, 0x540001e0),
            (0x94ee0, 0xb000062a),
            (0x94ee4, 0xbd486541),
            (0x94ee8, 0x1e2703e2),
            (0x94eec, 0x2d409103),
            (0x94ef0, 0x1e203884),
            (0x94ef4, 0x1e203863),
            (0x94ef8, 0x1e212884),
            (0x94efc, 0x1e202068),
            (0x94f00, 0x1e227884),
            (0x94f04, 0x1e234c83),
            (0x94f08, 0xbd000d03),
            (0x94f0c, 0x9101a108),
            (0x94f10, 0xeb08013f),
            (0x94f14, 0x54fffec1),
            (0x95928, 0xf9412674),
            (0x9592c, 0xf9412a75),
            (0x95930, 0xeb15029f),
            (0x95934, 0x54000760),
            (0x95938, 0x90000628),
            (0x9593c, 0xbd48150c),
            (0x95940, 0x90000628),
            (0x95944, 0xbd49410d),
            (0x95948, 0x90000628),
            (0x9594c, 0xbd47fd08),
            (0x95950, 0x90000628),
            (0x95954, 0xbd49ad09),
            (0x95958, 0x90000628),
            (0x9595c, 0xbd45410f),
            (0x95960, 0x1e23100e),
            (0x95964, 0x1e2703ea),
            (0x95968, 0xbd400e80),
            (0x9596c, 0x1e2b2800),
            (0x95970, 0x1e2c2801),
            (0x95974, 0x4ea81d00),
            (0x95978, 0x1e2d2823),
            (0x9597c, 0x4ea91d22),
            (0x95980, 0xd103e3a0),
            (0x95984, 0x9400c4cb),
            (0x95988, 0xbd400280),
            (0x9598c, 0x5e21d800),
            (0x95990, 0x1e2e1801),
            (0x95994, 0x4eaa1d40),
            (0x95998, 0x1e2f2823),
            (0x9599c, 0x4eab1d62),
            (0x959a0, 0xd103e3a0),
            (0x959a4, 0x9400c4d7),
            (0x939b0, 0xf9412a69),
            (0x939b4, 0xf9412668),
            (0x939b8, 0xeb080129),
            (0x939bc, 0x54000060),
            (0x939c0, 0x8b090108),
            (0x939c4, 0xbc1a010c),
            (0x958ec, 0x910322d6),
            (0x958f0, 0xeb16039f),
            (0x958f4, 0x54ffeee1),
            (0x958f8, 0x910523e1),
            (0x958fc, 0x2a1f03e0),
            (0x95900, 0x321a03e2),
            (0x95904, 0x9400e7cf),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        for (address, value) in [
            (0x159864, -90.0f32),
            (0x159940, 88.0),
            (0x1599e0, 90.0),
            (0x159814, 180.0),
            (0x1597fc, 166.0),
            (0x1599ac, 646.0),
        ] {
            assert_eq!(word(address), value.to_bits());
        }
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "executes reviewed supplied ARM64 sticky-heading arithmetic"]
    fn reference_native_murderer_purple_executed_heading_positions() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary =
            std::env::temp_dir().join(format!("shin-heading-oracle-{}", std::process::id()));
        let compile = std::process::Command::new("/usr/bin/clang")
            .arg("-O0")
            .arg(root.join("reverse-engineering/native-purple-heading.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let output = std::process::Command::new(&binary)
            .arg(
                std::path::Path::new(&rom)
                    .parent()
                    .unwrap()
                    .join("uncompressed_executable"),
            )
            .output()
            .unwrap();
        std::fs::remove_file(binary).unwrap();
        assert!(output.status.success());
        let mut model = MurdererModel::new(20);
        model.open_menu(3);
        let mut count = 0;
        for line in String::from_utf8(output.stdout).unwrap().lines() {
            let values: Vec<_> = line.split(',').collect();
            let heading: usize = values[0].parse().unwrap();
            model.purple_list_scroll = values[1].parse().unwrap();
            let offset: f32 = values[2].parse().unwrap();
            assert_eq!(
                model.purple_heading_rect(heading).unwrap().y,
                180.0 + offset
            );
            count += 1;
        }
        assert_eq!(count, 11346);
    }
    #[test]
    fn purple_person_pointer_eligibility_release_and_cancel_keep_native_selection() {
        let mut model = MurdererModel::new(20);
        model.open_menu(2);
        let point = Vec2::new(440.0, 210.0); // Person1.
        for invalid in [
            Vec2::new(310.0, 210.0),
            Vec2::new(310.0, 360.0),
            Vec2::ZERO,
            Vec2::NAN,
        ] {
            model.pointer(invalid, true);
            assert_eq!(model.menu_focus, 0); // Selected/empty/outside rejected.
            assert!(model.take_sounds().is_empty());
        }
        model.purple_text_scroll = 100.0;
        model.pointer_state(point, false);
        assert_eq!(model.menu_focus, 0);
        model.pointer_state(point, true);
        assert_eq!(model.menu_focus, 1);
        assert_eq!(model.purple_text_scroll, 0.0);
        assert_eq!(model.purple_text(), Some(PERSON_TEXT[1]));
        model.update(2.0);
        assert!((model.purple_person_feedback(0).0 - 0.8).abs() < 1e-6);
        assert_eq!(model.purple_person_feedback(1), (0.5, 0.4));
        assert!(model.take_sounds().is_empty());
        model.pointer_state(point, false);
        assert_eq!(model.take_sounds(), ["decide"]);
        model.update(2.0);
        assert!((model.purple_person_feedback(1).1 - 0.6).abs() < 1e-6);
        model.update(10.0);
        model.purple_text_scroll = 180.0;
        model.pointer(point, true); // Fresh selected hit does nothing.
        assert_eq!(model.purple_text_scroll, 180.0);
        assert!(model.take_sounds().is_empty());
        let next = Vec2::new(570.0, 210.0); // Person2.
        model.pointer_state(next, true);
        model.update(2.0);
        model.pointer_state(Vec2::ZERO, true);
        model.update(1.0);
        assert!((model.purple_person_feedback(2).1 - 0.2).abs() < 1e-6);
        model.pointer_state(next, false);
        assert_eq!(model.menu_focus, 2); // Press selection survives cancellation.
        assert!(model.take_sounds().is_empty());
        assert_eq!(model.result, None);
        model.reset();
        model.open_menu(2);
        assert_eq!(model.purple_person_feedback(0), (1.0, 0.0));
        assert_eq!(model.purple_person_feedback(1), (0.0, 0.0));
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_murderer_purple_person_row_feedback() {
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
            (0x93fe4, 0xb9425be8),
            (0x93fe8, 0xbd031fe3),
            (0x93fec, 0xbd031be2),
            (0x93ff0, 0xb9401a69),
            (0x93ff4, 0x6b09011f),
            (0x93ff8, 0x54000ee1),
            (0x93ffc, 0x1e2e1000),
            (0x94000, 0xaa1703e0),
            (0x94004, 0x94009cb7),
            (0x94e64, 0xf9411a75),
            (0x94e68, 0xf9411e76),
            (0x94e6c, 0xeb1602bf),
            (0x94e70, 0x54000140),
            (0x94e74, 0x4ea81d00),
            (0x94e78, 0x910022a0),
            (0x94e7c, 0x94009af5),
            (0x94e80, 0x4ea81d00),
            (0x94e84, 0x910182a0),
            (0x94e88, 0x94009af2),
            (0x94e8c, 0x910322b5),
            (0x94e90, 0xeb1502df),
            (0x94e94, 0x54ffff01),
            (0x96b34, 0xb9801808),
            (0x96b38, 0x6b01011f),
            (0x96b3c, 0x54000d00),
            (0x96b40, 0xf9411809),
            (0x96b44, 0x52801916),
            (0x96b48, 0x9b162508),
            (0x96b4c, 0xaa0003f3),
            (0x96b50, 0x91002100),
            (0x96b54, 0x2a0203f4),
            (0x96b58, 0x2a0103f5),
            (0x96b5c, 0x94009231),
            (0x96b60, 0xf0000608),
            (0x96b64, 0x1e2703e8),
            (0x96b68, 0x4ea81d00),
            (0x96b6c, 0xbd457101),
            (0x96b70, 0x321a03e1),
            (0x96b74, 0x2a1f03e2),
            (0x96b78, 0x940093a2),
            (0x96b7c, 0xf9411a68),
            (0x96b80, 0x9b3622a8),
            (0x96b84, 0x91002100),
            (0x96b88, 0xb9001a75),
            (0x96b8c, 0x94009225),
            (0x96b90, 0x1e2e1000),
            (0x96b94, 0x1e2a1001),
            (0x96b98, 0x321a03e1),
            (0x96b9c, 0x2a1f03e2),
            (0x96ba0, 0x94009398),
            (0x96ba4, 0xb9401268),
            (0x96ba8, 0x34000488),
            (0x96c38, 0xb9801a68),
            (0x96c3c, 0xf9411a69),
            (0x96c40, 0x5280190a),
            (0x96c44, 0x9b0a2508),
            (0x96c48, 0xd0000c69),
            (0x96c4c, 0xb9800508),
            (0x96c50, 0xf9476d29),
            (0x96c54, 0x8b080928),
            (0x96c58, 0x39400d08),
            (0x96c5c, 0x92401514),
            (0x96c60, 0xaa1303e0),
            (0x96c64, 0x2a1403e1),
            (0x96c68, 0x97fff6b6),
            (0x96c6c, 0xd0000c68),
            (0x96c70, 0xf9477108),
            (0x96c74, 0xaa1303e0),
            (0x96c78, 0xa9437bfd),
            (0x96ed8, 0xb2407bf6),
            (0x96edc, 0xf2c7da56),
            (0x96ee0, 0xb9800688),
            (0x96ee4, 0x9ac826c8),
            (0x96ee8, 0x360003c8),
            (0x96eec, 0xb9401a68),
            (0x96ef0, 0xb9400289),
            (0x96ef4, 0x6b09011f),
            (0x96ef8, 0x54000340),
            (0x96efc, 0x2d580680),
            (0x9733c, 0xb9402268),
            (0x97340, 0x37ffff88),
            (0x97344, 0x34ffff68),
            (0x97348, 0x51000514),
            (0x9734c, 0xf9411a68),
            (0x97350, 0x52801909),
            (0x97354, 0x9b292288),
            (0x97358, 0x91018100),
            (0x9735c, 0x94009031),
            (0x97360, 0xd0000608),
            (0x97364, 0xbd45a500),
            (0x97368, 0xd0000608),
            (0x9736c, 0xbd497501),
            (0x97370, 0x321a03e1),
            (0x97374, 0x2a1f03e2),
            (0x97378, 0x940091a2),
            (0x9737c, 0xa9417bfd),
            (0x97380, 0xaa1303e0),
            (0x97384, 0x2a1403e1),
            (0x97388, 0x320003e2),
            (0x9738c, 0xa8c24ff4),
            (0x97390, 0x17fffde4),
            (0x97540, 0xb9402268),
            (0x97544, 0x37ffff48),
            (0x97548, 0x34ffff28),
            (0x9754c, 0x51000514),
            (0x97550, 0xb0000c68),
            (0x97554, 0xf9437108),
            (0x97558, 0xf9400100),
            (0x9755c, 0x1e2e1000),
            (0x97560, 0x97fe3f54),
            (0x97564, 0xf9411a68),
            (0x97568, 0x52801909),
            (0x9756c, 0x9b292288),
            (0x97570, 0x91018100),
            (0x97574, 0x94008fab),
            (0x97578, 0xd0000608),
            (0x9757c, 0xbd45a500),
            (0x97580, 0xd0000608),
            (0x97584, 0xbd497508),
            (0x97588, 0x4ea81d01),
            (0x9758c, 0x321a03e1),
            (0x97590, 0x2a1f03e2),
            (0x97594, 0x9400911b),
            (0x97598, 0x1e2703e0),
            (0x9759c, 0x4ea81d01),
            (0x975a0, 0x321a03e1),
            (0x975a4, 0x2a1f03e2),
            (0x975a8, 0x94009116),
            (0x975ac, 0x6d4123e9),
            (0x975b0, 0xaa1303e0),
            (0x975b4, 0x2a1403e1),
            (0x975b8, 0xa9437bfd),
            (0x975bc, 0xa9424ff4),
            (0x975c0, 0x320003e2),
            (0x975c4, 0xfc4407ea),
            (0x975c8, 0x17fffd56),
            (0x9766c, 0xb9402268),
            (0x97670, 0x37f801e8),
            (0x97674, 0x340001a8),
            (0x97678, 0xf9411a69),
            (0x9767c, 0x51000508),
            (0x97680, 0x5280190a),
            (0x97684, 0x9b2a2508),
            (0x97688, 0x91018100),
            (0x9768c, 0x94008f65),
            (0x97690, 0xd0000608),
            (0x97694, 0xbd497501),
            (0x97698, 0x1e2703e0),
            (0x9769c, 0x321a03e1),
            (0x976a0, 0x2a1f03e2),
            (0x976a4, 0x940090d7),
            (0x976a8, 0xb900227f),
            (0x976ac, 0xa9417bfd),
            (0x976b0, 0xf84207f3),
            (0x976b4, 0xd65f03c0),
            (0x95be4, 0x910022c0),
            (0x95be8, 0x9400962e),
            (0x95bec, 0x1e2c1001),
            (0x95bf0, 0x910182c0),
            (0x95bf4, 0x1e210808),
            (0x95bf8, 0x9400962a),
            (0x95bfc, 0x4ea01c09),
            (0x95d04, 0x4eac1d80),
            (0x95d08, 0x4eac1d81),
            (0x95d0c, 0x4eac1d82),
            (0x95d10, 0x4ea81d03),
            (0x95d14, 0x97ff028f),
            (0x95d18, 0x910203e0),
            (0x95d1c, 0x2d1007e0),
            (0x95d20, 0x2d110fe2),
            (0x95d24, 0x97ff02c7),
            (0x95d28, 0x2a0003e1),
            (0x95d2c, 0xd103e3a0),
            (0x95d30, 0x9400c400),
            (0x95d34, 0xd103e3a0),
            (0x95d38, 0x9400c406),
            (0x95d3c, 0x1e202128),
            (0x95d40, 0x54fffdad),
            (0x95d44, 0x4eac1d80),
            (0x95d48, 0x4eac1d81),
            (0x95d4c, 0x4eac1d82),
            (0x95d50, 0x4ea91d23),
            (0x95d54, 0x97ff027f),
            (0x95d58, 0x910203e0),
            (0x95d5c, 0x2d1007e0),
            (0x95d60, 0x2d110fe2),
            (0x95d64, 0x97ff02b7),
            (0x95d68, 0x2a0003e1),
            (0x95d6c, 0xd103e3a0),
            (0x95d70, 0x9400c3f0),
            (0x95d74, 0xd103e3a0),
            (0x95d78, 0x9400c3f6),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        for (address, value) in [(0x159570, 0.1f32), (0x1595a4, 0.7), (0x159974, 0.2)] {
            assert_eq!(word(address), value.to_bits());
        }
        let reloc = |target| {
            let r = exe[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find(|r| u64::from_le_bytes(r[..8].try_into().unwrap()) == target)
                .unwrap();
            u64::from_le_bytes(r[16..24].try_into().unwrap())
        };
        for (offset, target) in [
            (0x50, 0x970c0),
            (0x60, 0x972d0),
            (0x68, 0x973a0),
            (0x78, 0x976c0),
        ] {
            assert_eq!(reloc(0x1d98e0 + offset), target);
        }
    }
    #[test]
    fn scroll_pointer_and_scroll_clip_and_reset_on_new_transcript() {
        let mut model = MurdererModel::new(0);
        model.open_menu(2);
        for index in 0..17 {
            let rect = purple_person_rect(index).unwrap();
            let original = model.purple_original_index();
            model.pointer(Vec2::new(rect.x + 48.0, rect.y + 54.0), true);
            assert_eq!(
                model.purple_original_index(),
                if PERSON_TEXT[index].is_empty() {
                    original
                } else {
                    Some(index)
                }
            );
            assert_eq!(model.result, None);
        }
        model.scroll_purple_text(180.0, 1200.0);
        assert_eq!(model.purple_text_scroll, 180.0);
        model.scroll_purple_text(10000.0, 1200.0);
        assert_eq!(model.purple_text_scroll, 480.0);
        model.scroll_purple_text(f32::NAN, 1200.0);
        assert_eq!(model.purple_text_scroll, 480.0);
        model.navigate(1);
        assert_eq!(model.purple_text_scroll, 0.0);
        model.scroll_purple_text(10000.0, 100.0);
        assert_eq!(model.purple_text_scroll, 0.0);
        model.back();
        finish_back(&mut model);
        model.open_menu(3);
        model.pointer(Vec2::new(200.0, 200.0), true); // Heading.
        assert_eq!(model.purple_original_index(), Some(1));
        model.pointer(Vec2::new(200.0, 380.0), true);
        assert_eq!(model.purple_original_index(), Some(2));
        model.scroll_purple_text(300.0, 1200.0);
        model.pointer(Vec2::new(200.0, 470.0), false);
        assert_eq!(model.purple_original_index(), Some(2));
        assert_eq!(model.purple_text_scroll, 300.0); // Passive hover retains transcript.
        model.select_purple_original_index(28);
        assert_eq!(model.purple_list_scroll, 1890.0);
        model.pointer(Vec2::new(200.0, 850.0), true);
        assert_eq!(model.purple_original_index(), Some(28));
        model.pointer(Vec2::new(1650.0, 60.0), true);
        finish_back(&mut model);
        assert_eq!(model.menu, MurdererMenu::Top);
        assert_eq!(model.result, None);
    }
    #[test]
    fn native_menu_sounds_distinguish_keyboard_cancel_pointer_back_and_accusation() {
        let mut model = MurdererModel::new(0);
        model.navigate(0);
        assert!(model.take_sounds().is_empty());
        model.navigate(5);
        model.confirm();
        assert_eq!(model.take_sounds(), ["move", "decide"]);
        finish_top_opening(&mut model);
        assert_eq!(model.take_sounds(), ["execute"]);
        model.activate_accusation_entry(34);
        model.activate_accusation_entry(35);
        model.activate_accusation_entry(36);
        assert_eq!(model.take_sounds(), ["decide", "decide", "decide"]);
        model.activate_accusation_entry(SUBMIT_ENTRY);
        assert_eq!(model.take_sounds(), ["attack"]);
        model.back();
        finish_back(&mut model);
        model.confirm();
        model.navigate(1);
        assert!(model.take_sounds().is_empty());
        model.reset();
        model.open_menu(0);
        model.back();
        finish_back(&mut model);
        assert_eq!(model.take_sounds(), ["cancel"]);
        model.open_menu(1);
        model.pointer(Vec2::new(1700.0, 60.0), true);
        finish_back(&mut model);
        assert_eq!(model.take_sounds(), ["decide"]);
        model.back();
        finish_back(&mut model);
        assert!(model.take_sounds().is_empty());
    }
    #[test]
    fn pointer_hover_is_silent_and_replay_hint_purple_clicks_have_native_decide() {
        for menu in [0, 2, 3, 4] {
            let mut model = MurdererModel::new(0);
            model.open_menu(menu);
            assert!(model.take_sounds().is_empty());
            let point = match menu {
                0 => Vec2::new(130.0, 200.0),
                2 => Vec2::new(440.0, 210.0), // Nonselected native person1.
                3 => Vec2::new(200.0, 390.0), // Fresh nonselected chapter2.
                _ => Vec2::new(500.0, 200.0),
            };
            model.pointer(point, false);
            assert!(model.take_sounds().is_empty());
            model.pointer(point, true);
            assert_eq!(model.take_sounds(), ["decide"]);
        }
    }
    #[test]
    fn accusation_poll_waits_for_native_flash_and_wrong_answer_laugh_timer() {
        for correct in [false, true] {
            let mut model = MurdererModel::new(0);
            model.open_menu(5);
            model.take_sounds();
            if correct {
                for entry in 34..37 {
                    model.activate_accusation_entry(entry);
                }
                model.take_sounds();
            }
            model.activate_accusation_entry(SUBMIT_ENTRY);
            assert_eq!(model.take_sounds(), ["attack"]);
            assert_eq!(model.poll_result(), None);
            model.update(7.0);
            assert_eq!(model.white_flash_opacity(), 0.0);
            model.update(0.0);
            assert_eq!(model.white_flash_opacity(), 1.0);
            model.update(15.0);
            assert_eq!(model.white_flash_opacity(), 0.5);
            model.update(15.0);
            assert_eq!(model.white_flash_opacity(), 0.0);
            model.update(30.0);
            assert_eq!(model.poll_result(), None);
            model.back();
            finish_back(&mut model);
            model.confirm();
            model.navigate(1);
            assert_eq!(model.menu, MurdererMenu::Accusation);
            model.update(0.0);
            if correct {
                assert_eq!(model.poll_result(), Some(27));
                assert!(model.take_sounds().is_empty());
            } else {
                assert_eq!(model.take_sounds(), ["ahaha"]);
                assert_eq!(model.poll_result(), None);
                model.update(60.0);
                assert_eq!(model.poll_result(), None);
                model.update(0.0);
                assert_eq!(model.poll_result(), Some(28));
                assert!(model.take_sounds().is_empty());
            }
        }
    }
    #[test]
    fn negative_hint_response_does_not_change_wrong_accusation_timing_or_reset_state() {
        let mut model = MurdererModel::new(-1);
        model.open_menu(5);
        model.take_sounds();
        model.activate_accusation_entry(SUBMIT_ENTRY);
        assert_eq!(model.result, Some(27));
        assert_eq!(model.poll_result(), None);
        model.update(f32::NAN);
        model.update(-1.0);
        model.update(f32::INFINITY);
        assert_eq!(model.white_flash_opacity(), 0.0);
        for _ in 0..68 {
            model.update(1.0);
        }
        assert_eq!(model.take_sounds(), ["attack", "ahaha"]);
        assert_eq!(model.poll_result(), None);
        model.reset();
        assert_eq!(model.poll_result(), None);
        assert_eq!(model.white_flash_opacity(), 0.0);
        assert!(model.take_sounds().is_empty());
    }
}
