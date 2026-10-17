//! Gerokasu (first game) prisoner/pierrot CardSelect, native layer type 9
//! (constructor 42A60, vtable 17C3F8). See `gerokasu/CARD-SELECT.md`.
//!
//! The mask holds the pairings already played (`$v(90+i)`); QUIZ returns the
//! new pairing index `T[prisoner*4 + pierrot]`.
use shin_core::time::{Easing, Ticks, Tweener};

use crate::layer::wobbler::Wobbler;

/// 122F2C: screen slot -> heroine (fu, do, ka, mi).
pub const SLOT_HEROINE: [usize; 4] = [0, 3, 1, 2];
/// 122F3C: slot X in layer space.
pub const SLOT_X: [f32; 4] = [-700.0, -300.0, 300.0, 700.0];
/// 122F4C: pairing index for prisoner p and pierrot q (diagonal -1).
pub const PAIRING: [i32; 16] = [-1, 0, 1, 2, 3, -1, 4, 5, 6, 7, -1, 8, 9, 10, 11, -1];
/// 17C4D0: voice romaji per heroine.
pub const ROMAJI: [&str; 4] = ["fuuka", "karin", "mina", "doremi"];
/// 17C5B0: Japanese names per heroine.
pub const NAMES: [&str; 4] = ["風華", "火凛", "水無", "土麗美"];

pub const PRISONER: u8 = 0;
pub const PIERROT: u8 = 1;

/// Rejection and retry messages (46B20 callers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    /// 484C0: `これ以上{name}に＜死刑囚/ピエロ＞を渡すことはできません。`
    Exhausted { heroine: usize, card: u8 },
    /// 11E60A (prisoner placed second).
    PairDoneForPrisoner,
    /// 11F724 (pierrot placed second, without 既に).
    PairDoneForPierrot,
    /// 11C400: phase 7 retry.
    AlreadyPlayed,
}
impl Message {
    pub const ALL: [Message; 11] = [
        Message::Exhausted { heroine: 0, card: 0 },
        Message::Exhausted { heroine: 1, card: 0 },
        Message::Exhausted { heroine: 2, card: 0 },
        Message::Exhausted { heroine: 3, card: 0 },
        Message::Exhausted { heroine: 0, card: 1 },
        Message::Exhausted { heroine: 1, card: 1 },
        Message::Exhausted { heroine: 2, card: 1 },
        Message::Exhausted { heroine: 3, card: 1 },
        Message::PairDoneForPrisoner,
        Message::PairDoneForPierrot,
        Message::AlreadyPlayed,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap()
    }
    pub fn text(self) -> String {
        self.localized_text(|text| text.to_owned())
    }
    /// The message with `localize` applied to the name and card label first,
    /// then to the whole sentence, so `localized.toml` can translate each part.
    pub fn localized_text(self, localize: impl Fn(&str) -> String) -> String {
        let sentence = match self {
            Message::Exhausted { heroine, card } => format!(
                "これ以上{}に{}を渡すことはできません。",
                localize(NAMES[heroine]),
                localize(if card == PRISONER {
                    "＜死刑囚＞"
                } else {
                    "＜ピエロ＞"
                })
            ),
            Message::PairDoneForPrisoner => {
                "この＜死刑囚＞と＜ピエロ＞の組み合わせは既に実行済みです。".into()
            }
            Message::PairDoneForPierrot => {
                "この＜死刑囚＞と＜ピエロ＞の組み合わせは実行済みです。".into()
            }
            Message::AlreadyPlayed => "この組み合わせは既に実行済みです。".into(),
        };
        localize(&sentence)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardEvent {
    /// System SE by name (25AF0): decide, move, cancel, error.
    Sound(&'static str),
    /// E3180 on the layer's own voice handle, e.g. `sys/fuuka_card05`.
    Voice(String),
    /// E3830.
    StopVoice,
}

fn control(t: &mut Tweener, v: f32, d: f32, flags: u32) {
    let easing = match flags & 63 {
        0 => Easing::Linear,
        1 => Easing::SineIn,
        2 => Easing::SineOut,
        3 => Easing::SineInOut,
        _ => unreachable!("unused Ger CardSelect easing"),
    };
    t.gerokasu2_control(v, Ticks::from_f32(d), flags, easing, false);
}
/// 9B900: set immediately, clearing the queue.
fn set(t: &mut Tweener, v: f32) {
    t.fast_forward_to(v);
}
/// 9BA40: freeze the current value, clearing the queue.
fn stop(t: &mut Tweener) {
    let v = t.value();
    t.fast_forward_to(v);
}
fn active(t: &Tweener) -> bool {
    !t.is_idle()
}
fn flash(t: &mut Tweener) {
    stop(t);
    control(t, 0.7, 0.2, 0x40);
    control(t, 0.0, 0.2, 0x40);
}
fn tween(v: f32) -> Tweener {
    Tweener::new_gerokasu2(v)
}

/// Card 0x278 (constructor 43E40).
#[derive(Clone, Debug)]
pub struct Card {
    pub kind: u8,
    pub id: u8,
    /// X, Y, scale, rotX/π, rotY/π, alpha, flash.
    pub t: [Tweener; 7],
}
impl Card {
    fn new(kind: u8, id: u8) -> Self {
        Self {
            kind,
            id,
            t: std::array::from_fn(|_| tween(0.0)),
        }
    }
    pub fn value(&self, i: usize) -> f32 {
        self.t[i].value()
    }
    fn moving(&self) -> bool {
        self.t[..6].iter().any(active)
    }
    fn advance(&mut self, dt: Ticks) {
        // 45290: the flash freezes the other tweens while it runs.
        self.t[6].update(dt);
        if !active(&self.t[6]) {
            for t in &mut self.t[..6] {
                t.update(dt);
            }
        }
    }
    fn fast_forward(&mut self) {
        for t in &mut self.t {
            t.fast_forward();
        }
    }
}
const X: usize = 0;
const Y: usize = 1;
const SCALE: usize = 2;
const ROT_X: usize = 3;
const ROT_Y: usize = 4;
const ALPHA: usize = 5;
const FLASH: usize = 6;

/// One keyboard step chosen by the debug autoplayer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoAction {
    Navigate(i32),
    Confirm,
    Cancel,
}

#[derive(Clone, Debug)]
pub struct Placed {
    pub slot: usize,
    pub card: Card,
}

#[derive(Clone, Debug)]
pub struct GerokasuCardModel {
    pub phase: i32,
    pub success: bool,
    pub mask: u32,
    /// Done counts as prisoner / pierrot per heroine.
    pub done_prisoner: [u32; 4],
    pub done_pierrot: [u32; 4],
    pub elapsed: f32,
    pub selected: i32,
    pub slot: i32,
    /// -1 none, 0..3 card/slot, 1000 cancel button.
    pub hover: i32,
    pub result: i32,
    pub hand: Vec<Card>,
    pub placed: Vec<Placed>,
    pub slot_flash: [Tweener; 4],
    pub cancel_flash: Tweener,
    pub cancel_alpha: Tweener,
    pub icon_alpha: Tweener,
    pub message_alpha: Tweener,
    pub message: Option<Message>,
    pub wobble: [Wobbler; 2],
    pub cutin_slide: Tweener,
    pub cutin_band: Tweener,
    pub cutin_heroine: usize,
    pub effect: Tweener,
    pub fade: Tweener,
    pub voice_playing: bool,
    pub listener: bool,
    pub events: Vec<CardEvent>,
}

fn minstd(state: &mut u32) -> u32 {
    *state = (u64::from(*state) * 48271 % 2147483647) as u32;
    *state
}
/// libc++ `uniform_int_distribution` over `minstd_rand` (range 1..2^31-2).
fn uniform(state: &mut u32, last: usize) -> usize {
    if last == 0 {
        return 0;
    }
    let range = last + 1;
    let bits = usize::BITS - (range - 1).leading_zeros();
    let mask = (1u32 << bits) - 1;
    let threshold = (2147483646u32 >> bits) << bits;
    loop {
        let v = minstd(state) - 1;
        if v < threshold {
            let v = (v & mask) as usize;
            if v < range {
                return v;
            }
        }
    }
}

impl GerokasuCardModel {
    pub fn new(raw_mask: i32, seed: u32) -> Self {
        let mask = raw_mask as u32 & 0xfff;
        let mut done_prisoner = [0; 4];
        let mut done_pierrot = [0; 4];
        for p in 0..4 {
            for q in 0..4 {
                let index = PAIRING[p * 4 + q];
                if index >= 0 && mask & (1 << index) != 0 {
                    done_prisoner[p] += 1;
                    done_pierrot[q] += 1;
                }
            }
        }
        // 42EA8: libc++ std::shuffle with minstd_rand seeded from MT.
        let mut order = [0u8, 1, 2, 3];
        let mut state = (seed % 0x7fffffff).max(1);
        for i in 0..3 {
            let j = uniform(&mut state, 3 - i);
            order.swap(i, i + j);
        }
        Self {
            phase: 0,
            success: false,
            mask,
            done_prisoner,
            done_pierrot,
            elapsed: 0.0,
            selected: 0,
            slot: 0,
            hover: -1,
            result: -1,
            hand: (0..4).map(|i| Card::new(order[i], i as u8)).collect(),
            placed: Vec::new(),
            slot_flash: std::array::from_fn(|_| tween(0.0)),
            cancel_flash: tween(0.0),
            cancel_alpha: tween(0.0),
            icon_alpha: tween(0.0),
            message_alpha: tween(0.0),
            message: None,
            wobble: [Wobbler::new(), Wobbler::new()],
            cutin_slide: tween(0.0),
            cutin_band: tween(0.0),
            cutin_heroine: 0,
            effect: tween(0.0),
            fade: tween(0.0),
            voice_playing: false,
            listener: false,
            events: Vec::new(),
        }
    }
    /// `popcount(mask)`: archive and voice number.
    pub fn progress(&self) -> u32 {
        self.mask.count_ones()
    }
    pub fn render_clone_model(&self) -> Self {
        let mut copy = self.clone();
        // 44E60: the clone gets a new voice handle and no listener.
        copy.listener = false;
        copy.voice_playing = false;
        copy.events.clear();
        copy
    }
    /// 48A20: the first listener query deals the hand.
    pub fn attach_listener(&mut self) {
        if !self.listener {
            self.listener = true;
            self.deal();
        }
    }
    pub fn result(&self) -> Option<i32> {
        (self.phase < 0).then_some(self.result)
    }
    /// 48BA0.
    pub fn restore_result(&mut self, result: i32) {
        self.phase = -1;
        self.success = true;
        self.result = result;
    }
    /// 49990: the listener takes input only in phases 1 and 3; phase 5 takes
    /// A/click to stop the voice.
    pub fn ready(&self) -> bool {
        self.listener && matches!(self.phase, 1 | 3 | 5)
    }
    fn sound(&mut self, name: &'static str) {
        self.events.push(CardEvent::Sound(name));
    }
    fn busy(&self) -> bool {
        self.hand.iter().any(Card::moving)
            || self.placed.iter().any(|p| p.card.moving())
            || [
                &self.icon_alpha,
                &self.cutin_slide,
                &self.cutin_band,
                &self.effect,
                &self.fade,
            ]
            .into_iter()
            .any(active)
            || self.voice_playing
    }
    fn placed_slot(&self, slot: usize) -> bool {
        self.placed.iter().any(|p| p.slot == slot)
    }
    /// Debug autoplay: prisoner and pierrot slots for an unfinished pairing
    /// that agree with the cards already placed.
    fn autoplay_plan(&self) -> Option<(usize, usize)> {
        let fixed = |kind: u8| {
            self.placed
                .iter()
                .find(|p| p.card.kind == kind)
                .map(|p| p.slot)
        };
        let (prisoner, pierrot) = (fixed(PRISONER), fixed(PIERROT));
        let usable = |slot: usize, fixed: Option<usize>| {
            fixed.map_or(!self.placed_slot(slot), |fixed| fixed == slot)
        };
        let done = |index: i32| index >= 0 && self.mask & (1 << index) != 0;
        (0..4)
            .flat_map(|p| (0..4).map(move |q| (p, q)))
            .find(|&(p, q)| {
                let (hp, hq) = (SLOT_HEROINE[p], SLOT_HEROINE[q]);
                p != q
                    && usable(p, prisoner)
                    && usable(q, pierrot)
                    && self.done_prisoner[hp] < 3
                    && self.done_pierrot[hq] < 3
                    && !done(PAIRING[hp * 4 + hq])
            })
    }
    /// Debug autoplay: the next keyboard step towards an unfinished pairing,
    /// taking cards back when the placed ones allow none.
    pub fn autoplay_action(&self) -> Option<AutoAction> {
        if !self.ready() {
            return None;
        }
        let plan = self.autoplay_plan();
        let toward = |current: i32, target: usize| match current.cmp(&(target as i32)) {
            std::cmp::Ordering::Less => AutoAction::Navigate(1),
            std::cmp::Ordering::Greater => AutoAction::Navigate(-1),
            std::cmp::Ordering::Equal => AutoAction::Confirm,
        };
        match self.phase {
            1 => {
                if plan.is_none() {
                    return (!self.placed.is_empty()).then_some(AutoAction::Cancel);
                }
                let target = [PRISONER, PIERROT]
                    .iter()
                    .find_map(|&kind| self.hand.iter().position(|c| c.kind == kind))
                    .unwrap_or(0);
                Some(toward(self.selected, target))
            }
            3 => {
                let Some((prisoner, pierrot)) = plan else {
                    return Some(AutoAction::Cancel);
                };
                let kind = self.hand.get(usize::try_from(self.selected).ok()?)?.kind;
                let target = match kind {
                    PRISONER => prisoner,
                    PIERROT => pierrot,
                    _ => (0..4).find(|&s| !self.placed_slot(s) && s != prisoner && s != pierrot)?,
                };
                Some(toward(self.slot, target))
            }
            _ => None,
        }
    }
    fn lowest_free_slot(&self) -> i32 {
        (0..4).find(|&s| !self.placed_slot(s)).map_or(-1, |s| s as i32)
    }

    /// 46300.
    fn deal(&mut self) {
        let n = self.hand.len();
        for (i, card) in self.hand.iter_mut().enumerate() {
            let fi = i as f32;
            set(&mut card.t[X], (fi - (n as f32 - 1.0) / 2.0) * 300.0);
            set(&mut card.t[Y], 800.0);
            control(&mut card.t[Y], 0.0, 10.0 * fi, 0x80);
            control(&mut card.t[Y], 300.0, 30.0, 2);
            set(&mut card.t[SCALE], 1.0);
            set(&mut card.t[ROT_X], 0.0);
            set(&mut card.t[ROT_Y], -1.0);
            control(
                &mut card.t[ROT_Y],
                0.0,
                10.0 * n as f32 + 30.0 + 10.0 * fi,
                0x80,
            );
            control(&mut card.t[ROT_Y], 0.0, 10.0, 0);
            set(&mut card.t[ALPHA], 1.0);
            set(&mut card.t[FLASH], 0.0);
        }
        self.phase = 0;
    }
    /// 46B20.
    fn show_message(&mut self, message: Message) {
        self.message = Some(message);
        stop(&mut self.message_alpha);
        control(&mut self.message_alpha, 1.0, 0.05, 0x40);
        control(&mut self.message_alpha, 0.0, 120.0, 0x1080);
        control(&mut self.message_alpha, 0.0, 0.05, 0x40);
    }
    fn fade_cancel(&mut self, target: f32, speed: f32) {
        stop(&mut self.cancel_alpha);
        control(&mut self.cancel_alpha, target, speed, 0x40);
    }
    fn hide_message(&mut self) {
        stop(&mut self.message_alpha);
        control(&mut self.message_alpha, 0.0, 0.05, 0x40);
    }

    /// 481D0: a hand index, or -1 to undo the last placement.
    pub fn select_card(&mut self, index: i32) -> bool {
        if index < 0 {
            flash(&mut self.cancel_flash);
            let Some(last) = self.placed.last_mut() else {
                return false;
            };
            control(&mut last.card.t[ALPHA], 0.0, 20.0, 0);
            control(&mut last.card.t[Y], 100.0, 20.0, 1);
            for card in &mut self.hand {
                control(&mut card.t[Y], 800.0, 20.0, 0);
            }
            self.selected = -1;
        } else {
            for (i, card) in self.hand.iter_mut().enumerate() {
                if i as i32 == index {
                    control(&mut card.t[Y], 100.0, 20.0, 3);
                    control(&mut card.t[X], 0.0, 20.0, 3);
                    control(&mut card.t[ROT_Y], 2.0, 20.0, 0);
                    control(&mut card.t[SCALE], 1.2, 20.0, 0);
                    flash(&mut card.t[FLASH]);
                } else {
                    control(&mut card.t[Y], 800.0, 15.0, 0);
                }
            }
            control(&mut self.icon_alpha, 0.0, 20.0, 0x80);
            control(&mut self.icon_alpha, 1.0, 10.0, 0);
        }
        // 483EC common tail.
        self.fade_cancel(0.0, 0.1);
        self.phase = 2;
        self.slot = self.lowest_free_slot();
        self.hide_message();
        true
    }

    /// The pair test shared by 484C0 and the bust dimming in render 46C60.
    pub fn rejection(&self, kind: u8, slot: usize) -> Option<Message> {
        let h = SLOT_HEROINE[slot];
        let partner = |wanted: u8| {
            self.placed
                .iter()
                .find(|p| p.card.kind == wanted)
                .map(|p| SLOT_HEROINE[p.slot])
        };
        let done = |index: i32| index >= 0 && self.mask & (1 << index) != 0;
        match kind {
            PRISONER => {
                if self.done_prisoner[h] >= 3 {
                    return Some(Message::Exhausted {
                        heroine: h,
                        card: PRISONER,
                    });
                }
                if let Some(q) = partner(PIERROT)
                    && done(PAIRING[h * 4 + q])
                {
                    return Some(Message::PairDoneForPrisoner);
                }
            }
            PIERROT => {
                if self.done_pierrot[h] >= 3 {
                    return Some(Message::Exhausted {
                        heroine: h,
                        card: PIERROT,
                    });
                }
                if let Some(p) = partner(PRISONER)
                    && done(PAIRING[p * 4 + h])
                {
                    return Some(Message::PairDoneForPierrot);
                }
            }
            _ => {}
        }
        None
    }

    /// 484C0: a slot, or -1 to put the selected card back.
    pub fn assign(&mut self, slot: i32) -> bool {
        let Some(card_index) = usize::try_from(self.selected)
            .ok()
            .filter(|&i| i < self.hand.len())
        else {
            return false;
        };
        if slot < 0 {
            flash(&mut self.cancel_flash);
            control(&mut self.hand[card_index].t[Y], 800.0, 15.0, 0);
            self.slot = -1;
            self.hide_message();
            control(&mut self.icon_alpha, 0.0, 10.0, 0);
            self.fade_cancel(0.0, 0.1);
            self.phase = 4;
            return true;
        }
        let slot = slot as usize;
        flash(&mut self.slot_flash[slot]);
        if let Some(message) = self.rejection(self.hand[card_index].kind, slot) {
            self.show_message(message);
            return false;
        }
        self.hide_message();
        let card = &mut self.hand[card_index];
        control(&mut card.t[X], SLOT_X[slot], 30.0, 0);
        control(&mut card.t[Y], -300.0, 15.0, 0x82);
        control(&mut card.t[Y], 100.0, 15.0, 0x81);
        control(&mut card.t[ROT_X], 0.6, 30.0, 3);
        control(&mut card.t[SCALE], 0.55, 30.0, 3);
        control(&mut card.t[ALPHA], 0.0, 20.0, 0x80);
        control(&mut card.t[ALPHA], 0.0, 10.0, 0);
        control(&mut self.icon_alpha, 0.0, 10.0, 0);
        self.fade_cancel(0.0, 0.1);
        self.phase = 4;
        true
    }

    /// 45290 phase handlers.
    fn run_phase(&mut self) {
        if !matches!(self.phase, 1 | 3) && self.busy() {
            return;
        }
        match self.phase {
            0 => {
                if self.hand.len() > 3 {
                    self.fade_cancel(0.0, 0.1);
                } else {
                    self.fade_cancel(1.0, 0.25);
                }
                self.phase = 1;
            }
            2 => {
                if self.selected < 0 {
                    // 45AD0: the last placed card returns at lower_bound(id).
                    if let Some(Placed { card, .. }) = self.placed.pop() {
                        let at = self.hand.partition_point(|c| c.id < card.id);
                        self.hand.insert(at, card);
                        self.selected = at as i32;
                    }
                    self.deal();
                } else {
                    self.phase = 3;
                    self.fade_cancel(1.0, 0.25);
                }
            }
            4 => {
                if self.slot < 0 {
                    self.deal();
                    return;
                }
                let slot = self.slot as usize;
                if self.hand[self.selected as usize].kind == PRISONER {
                    self.cutin_heroine = SLOT_HEROINE[slot];
                    set(
                        &mut self.cutin_slide,
                        if slot <= 1 { -400.0 } else { 400.0 },
                    );
                    control(&mut self.cutin_slide, 0.0, 20.0, 2);
                    control(&mut self.cutin_band, 1.0, 10.0, 2);
                    self.events.push(CardEvent::Voice(format!(
                        "sys/{}_card{:02}",
                        ROMAJI[self.cutin_heroine],
                        self.progress()
                    )));
                    // The voice starts this frame; busy() waits for it.
                    self.voice_playing = true;
                }
                self.phase = 5;
            }
            5 => self.place(),
            6 => {
                if self.mask & (1 << self.result) == 0 {
                    self.success = true;
                    control(&mut self.effect, 1.0, 90.0, 0);
                    control(&mut self.fade, 0.0, 60.0, 0x80);
                    control(&mut self.fade, 1.0, 30.0, 0);
                } else {
                    control(&mut self.icon_alpha, 0.0, 20.0, 0);
                    for p in &mut self.placed {
                        control(&mut p.card.t[Y], 100.0, 20.0, 1);
                        control(&mut p.card.t[ALPHA], 0.0, 20.0, 0);
                    }
                }
                self.phase = 7;
            }
            7 => {
                if self.success {
                    self.phase = -1;
                } else {
                    self.show_message(Message::AlreadyPlayed);
                    let placed = std::mem::take(&mut self.placed);
                    self.hand.extend(placed.into_iter().map(|p| p.card));
                    self.deal();
                }
            }
            _ => {}
        }
    }
    /// Phase 5.
    fn place(&mut self) {
        let slot = self.slot as usize;
        let mut card = self.hand.remove(self.selected as usize);
        if card.kind == PRISONER {
            control(&mut self.cutin_band, 0.0, 10.0, 2);
        }
        set(&mut card.t[ALPHA], 0.0);
        control(&mut card.t[ALPHA], 1.0, 20.0, 0);
        set(&mut card.t[X], SLOT_X[slot]);
        set(&mut card.t[Y], 100.0);
        control(&mut card.t[Y], 0.0, 20.0, 2);
        set(&mut card.t[SCALE], 0.7);
        self.placed.push(Placed { slot, card });
        if !self.hand.is_empty() {
            self.selected = 0;
            self.deal();
            return;
        }
        let heroine_of = |kind: u8| {
            self.placed
                .iter()
                .find(|p| p.card.kind == kind)
                .map_or(0, |p| SLOT_HEROINE[p.slot])
        };
        self.result = PAIRING[heroine_of(PRISONER) * 4 + heroine_of(PIERROT)].max(0);
        self.phase = 6;
    }

    pub fn update(&mut self, dt: Ticks) {
        if (0..=7).contains(&self.phase) {
            self.run_phase();
        }
        for t in &mut self.slot_flash {
            t.update(dt);
        }
        for card in &mut self.hand {
            card.advance(dt);
        }
        for p in &mut self.placed {
            p.card.advance(dt);
        }
        self.cancel_flash.update(dt);
        if !active(&self.cancel_flash) {
            self.cancel_alpha.update(dt);
        }
        self.icon_alpha.update(dt);
        self.message_alpha.update(dt);
        self.cutin_slide.update(dt);
        self.cutin_band.update(dt);
        for w in &mut self.wobble {
            w.update(dt, 1.0, Ticks::from_f32(2.0));
        }
        self.effect.update(dt);
        self.fade.update(dt);
        self.elapsed += dt.as_f32();
    }
    /// 44ED0.
    pub fn fast_forward(&mut self) {
        for card in &mut self.hand {
            card.fast_forward();
        }
        for p in &mut self.placed {
            p.card.fast_forward();
        }
        for t in self.slot_flash.iter_mut().chain([
            &mut self.cancel_flash,
            &mut self.cancel_alpha,
            &mut self.icon_alpha,
            &mut self.message_alpha,
            &mut self.cutin_slide,
            &mut self.cutin_band,
            &mut self.effect,
            &mut self.fade,
        ]) {
            t.fast_forward();
        }
    }

    // 49A00 buttons.
    pub fn navigate(&mut self, direction: i32) -> bool {
        match self.phase {
            1 => {
                let next = self.selected + direction.signum();
                if (0..self.hand.len() as i32).contains(&next) {
                    self.selected = next;
                    self.sound("move");
                }
                true
            }
            3 => {
                let mut s = self.slot + direction.signum();
                while (0..4).contains(&s) {
                    if !self.placed_slot(s as usize) {
                        self.slot = s;
                        self.sound("move");
                        break;
                    }
                    s += direction.signum();
                }
                true
            }
            _ => false,
        }
    }
    pub fn confirm(&mut self) -> bool {
        match self.phase {
            1 => {
                let ok = self.select_card(self.selected);
                if ok {
                    self.sound("decide");
                }
                true
            }
            3 => {
                let ok = self.assign(self.slot);
                self.sound(if ok { "decide" } else { "error" });
                true
            }
            5 => {
                self.stop_voice();
                true
            }
            _ => false,
        }
    }
    pub fn cancel(&mut self) -> bool {
        match self.phase {
            1 => {
                let ok = self.select_card(-1);
                self.sound(if ok { "cancel" } else { "error" });
                true
            }
            3 => {
                self.sound("cancel");
                self.assign(-1);
                true
            }
            _ => false,
        }
    }
    fn stop_voice(&mut self) {
        if self.voice_playing {
            self.events.push(CardEvent::StopVoice);
        }
    }

    // 49C40 hover; `position` is in screen pixels.
    pub fn pointer_move(&mut self, position: glam::Vec2) -> bool {
        if !self.ready() || !position.is_finite() {
            return false;
        }
        let inside = |x: f32, y: f32, w: f32, h: f32, p: glam::Vec2| {
            p.x >= x && p.x < x + w && p.y >= y && p.y < y + h
        };
        let local = position - glam::vec2(960.0, 540.0);
        let cancel = inside(1534.0, 46.0, 340.0, 108.0, position);
        self.hover = match self.phase {
            1 if cancel && self.hand.len() <= 3 => 1000,
            1 => self
                .hand
                .iter()
                .position(|c| {
                    inside(c.value(X) - 138.0, c.value(Y) - 196.0, 276.0, 392.0, local)
                })
                .map_or(-1, |i| i as i32),
            3 if cancel => 1000,
            3 => (0..4)
                .find(|&s| {
                    !self.placed_slot(s) && inside(SLOT_X[s] - 200.0, -540.0, 400.0, 1080.0, local)
                })
                .map_or(-1, |s| s as i32),
            5 => 0,
            _ => -1,
        };
        self.hover >= 0
    }
    /// 4A280.
    pub fn pointer_press(&mut self) -> bool {
        if !self.ready() || self.hover < 0 {
            return false;
        }
        match (self.phase, self.hover) {
            (1 | 3, 1000) => control(&mut self.cancel_flash, 0.7, 0.2, 0x40),
            (1, i) => {
                self.selected = i;
                control(&mut self.hand[i as usize].t[FLASH], 0.7, 0.2, 0x40);
            }
            (3, s) => {
                self.slot = s;
                control(&mut self.slot_flash[s as usize], 0.7, 0.2, 0x40);
            }
            _ => {}
        }
        true
    }
    /// 4A330 click.
    pub fn pointer_release(&mut self) -> bool {
        if !self.ready() || self.hover < 0 {
            return false;
        }
        let hover = self.hover;
        let ok = match (self.phase, hover) {
            (1, 1000) => self.select_card(-1),
            (1, i) => {
                self.selected = i;
                self.select_card(i)
            }
            (3, 1000) => self.assign(-1),
            (3, s) => {
                self.slot = s;
                self.assign(s)
            }
            _ => {
                self.stop_voice();
                return true;
            }
        };
        self.sound(if ok || hover == 1000 { "decide" } else { "error" });
        self.pointer_cancel();
        true
    }
    /// 4A4A0.
    pub fn pointer_cancel(&mut self) {
        let hover = std::mem::replace(&mut self.hover, -1);
        let t = match (self.phase, hover) {
            (_, -1) => return,
            (1 | 3, 1000) => &mut self.cancel_flash,
            (1, i) => match self.hand.get_mut(i as usize) {
                Some(card) => &mut card.t[FLASH],
                None => return,
            },
            (3, s) => &mut self.slot_flash[s as usize],
            _ => return,
        };
        control(t, 0.0, 0.2, 0x40);
    }
}

#[cfg(test)]
mod tests;
