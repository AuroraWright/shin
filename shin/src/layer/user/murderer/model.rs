//! Script-facing behavior of interactive mode 2, recovered from MurdererLayer.
//! Shared by the menus, purple-statement browsers, and accusation view.
//! Replay/hint response phases and accusation completion timing follow native
//! polling. Top opening and submenu return capture/staging follow the native
//! parent lifecycle. Replay and Accusation use native directional selection;
//! Purple uses native person/chapter selection and held pointer feedback.
//! Purple pointer drag/inertia follows native callbacks; platform event arbitration remains pending.

use crate::layer::user::purple_text::{CHAPTERS, PERSON_TEXT};
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
pub(crate) fn purple_person_is_red(index: usize) -> bool {
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
    /// The purple page last sent to the text stream: (by chapter, index).
    streamed_purple: Option<(bool, usize)>,
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
            streamed_purple: None,
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
    #[cfg(test)]
    pub fn update(&mut self, delta: f32) {
        self.update_with_wipe_permission(delta, true);
    }
    #[cfg(test)]
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
    /// The newly shown purple-statement page text, once per page change.
    pub(crate) fn take_purple_page_change(&mut self) -> Option<&'static str> {
        let chapter = self.menu == MurdererMenu::PurpleByChapter;
        let shown = self.purple_original_index().map(|index| (chapter, index));
        if shown == self.streamed_purple {
            return None;
        }
        self.streamed_purple = shown;
        let (chapter, index) = shown?;
        // The same pages `render_purple` draws.
        if chapter {
            CHAPTERS.get(index).map(|entry| entry.text)
        } else {
            PERSON_TEXT.get(index).copied()
        }
    }

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
    #[cfg(test)]
    pub fn purple_text(&self) -> Option<&'static str> {
        let index = self.purple_original_index()?;
        match self.menu {
            MurdererMenu::PurpleByChapter => Some(CHAPTERS[index].text),
            MurdererMenu::PurpleByPerson => Some(PERSON_TEXT[index]),
            _ => None,
        }
    }
    #[cfg(test)]
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
    pub(crate) fn purple_heading_offset(&self, heading: usize) -> Option<f32> {
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
    #[cfg(test)]
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
                self.scroll_purple_text(scroll_axis * 32.0, text_height);
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
        } else if scroll_axis.is_finite() && scroll_axis != 0.0 {
            let next =
                (self.hint_scroll + scroll_axis * 32.0).clamp(0.0, self.hint_scroll_maximum());
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

    #[cfg(test)]
    pub fn reset(&mut self) {
        let serial = self.wipe_serial;
        *self = Self::new(self.hint_progress);
        self.wipe_serial = serial;
    }
}

#[cfg(test)]
mod fast_forward_tests;

#[cfg(test)]
mod tests;
