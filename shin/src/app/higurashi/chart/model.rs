//! Older4687c/48858/4a3d4 Chart state and private tasks. The concrete owner
//! supplies one shared scope for these tasks and the actual Question child.
use super::catalog::Catalog;
use crate::{
    adv::{
        higurashi::{
            autosave::{ProgressEntry, SaveProgress},
            browser_controls::Scrollbar,
            input::Buttons,
        },
        input_area::InputAreaTarget,
    },
    app::higurashi::runtime_game_data::RuntimeGameData,
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::{
    format::{save::higurashi::SaveDate, text::read_sjis_string},
    time::Ticks,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Input {
    pub buttons: Buttons,
    pub axes: [i8; 4],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Node {
    pub group: i32,
    pub index: i32,
    pub x: i32,
    pub y: i32,
    pub source_id: u32,
    pub current: bool,
    pub record: usize,
}
pub(super) struct Group {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub selected: i32,
    pub x: HigurashiTweener,
    pub y: HigurashiTweener,
    pub scroll: HigurashiTweener,
}
#[derive(Clone, Debug)]
pub(super) enum Action {
    Cue(&'static str),
    DisableInput,
    Question { index: i32, prompt: String },
    ReturnSource(u32),
    StartAdv { data: RuntimeGameData, flags: u32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    First,
    Open,
    Input,
    CloseWait,
    Exit,
}
pub(super) struct Task {
    pub kind: Kind,
    pub done: bool,
}
pub(super) trait Services {
    fn input(&mut self, model: &mut Model, permission: bool);
    /// Store owned requests before any fallible platform handoff. The model
    /// has already advanced the native callback and must not resample a seed
    /// or reconstruct a selected payload on retry.
    fn event(&mut self, event: Action) -> Result<()>;
    fn next_seed(&mut self) -> u32;
}
pub(super) struct Model {
    pub catalog: Arc<Catalog>,
    pub nodes: Vec<Node>,
    pub available: Vec<i32>,
    #[allow(dead_code, reason = "Preserve the native initial group separately from live navigation.")]
    pub initial_group: i32,
    pub current_group: i32,
    pub available_index: i32,
    pub title: bool,
    pub progress: f32,
    pub choice: i32,
    pub complete: bool,
    pub closing: bool,
    pub frame: u32,
    pub tooltip: i32,
    pub speed: f32,
    pub target: i32,
    pub pointed: i32,
    pub pressed: i32,
    pub groups: [Group; 5],
    pub flashes: [HigurashiTweener; 3],
    pub completion_text: String,
    pub tooltip_text: String,
    labels: Vec<Option<String>>,
    pending: Vec<Kind>,
    actions: Vec<Action>,
}
fn control(t: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    t.control(value, Ticks::from_f32(time), flags)
        .expect("Native Chart control flags");
}
fn clear(t: &mut HigurashiTweener) {
    t.fast_forward_to(t.value());
}
fn flash(t: &mut HigurashiTweener, release: bool) {
    clear(t);
    control(
        t,
        f32::from_bits(0x3f333333),
        f32::from_bits(0x3e4ccccd),
        0x40,
    );
    if release {
        control(t, 0., f32::from_bits(0x3e4ccccd), 0x40);
    }
}
fn clear_flash(t: &mut HigurashiTweener) {
    clear(t);
    control(t, 0., f32::from_bits(0x3e4ccccd), 0x40);
}
impl Model {
    pub fn new(
        catalog: Arc<Catalog>,
        group: i32,
        mask: i32,
        unlocked: &[u32],
        route: &[u32],
    ) -> Result<Self> {
        ensure!(
            group < 5,
            "Chart raw constructor group outside native five-group storage"
        );
        let title = group < 0;
        let available = if title {
            (0..5).filter(|&i| i == 0 || mask & (1 << i) != 0).collect()
        } else {
            vec![group]
        };
        let mut groups = std::array::from_fn(|_| Group {
            min_x: 32767,
            max_x: -32768,
            min_y: 32767,
            max_y: -32768,
            origin_x: 0,
            origin_y: 0,
            selected: -1,
            x: HigurashiTweener::new(0.),
            y: HigurashiTweener::new(0.),
            scroll: HigurashiTweener::new(0.),
        });
        let mut nodes = vec![];
        let mut active_group = 0;
        let mut base = None;
        let mut seen = false;
        let mut current = false;
        let mut total = 0i32;
        for (index, record) in catalog.records.iter().enumerate() {
            match record.kind {
                5 => {
                    active_group = i32::from(record.value);
                    ensure!(
                        (0..5).contains(&active_group),
                        "Chart catalog group outside five native records"
                    );
                }
                0 => {
                    base = Some(index);
                    seen = unlocked
                        .get(usize::from(record.value) / 32)
                        .is_some_and(|w| w & (1 << (record.value % 32)) != 0);
                    current = route.contains(&u32::from(record.value));
                }
                1..=4 => {
                    total += 1;
                    if !seen && !current {
                        continue;
                    }
                    let b = &catalog.records[base.ok_or_else(|| {
                        anyhow::anyhow!("Chart node before its native base record")
                    })?];
                    let n = nodes.len() as i32;
                    if record.kind == 1 && groups[active_group as usize].selected < 0 {
                        groups[active_group as usize].selected = n;
                    }
                    nodes.push(Node {
                        group: active_group,
                        index: n,
                        x: i32::from(b.x) + i32::from(record.x),
                        y: i32::from(b.y) + i32::from(record.y),
                        source_id: u32::from(b.value),
                        current,
                        record: index,
                    });
                }
                _ => anyhow::bail!("Unsupported older Chart kind{}", record.kind),
            }
        }
        if let Some(&first) = route.first() {
            for node in &mut nodes {
                if node.source_id == first && (2..=4).contains(&catalog.records[node.record].kind) {
                    node.current = false;
                }
            }
        }
        for (id, g) in groups.iter_mut().enumerate() {
            if let Some(node) = route.iter().rev().find_map(|id0| {
                nodes.iter().find(|n| {
                    n.group == id as i32
                        && n.source_id == *id0
                        && catalog.records[n.record].kind == 1
                })
            }) {
                g.selected = node.index;
            }
            for n in nodes.iter().filter(|n| n.group == id as i32) {
                let r = &catalog.records[n.record];
                let (lo_x, hi_x, hi_y) = match r.kind {
                    1 => (n.x - 1, n.x + 1, n.y),
                    3 => (n.x, n.x, n.y + i32::from(r.value)),
                    4 => (n.x, n.x + i32::from(r.value), n.y),
                    _ => (n.x, n.x, n.y),
                };
                g.min_x = g.min_x.min(lo_x);
                g.max_x = g.max_x.max(hi_x);
                g.min_y = g.min_y.min(n.y);
                g.max_y = g.max_y.max(hi_y);
            }
            if g.max_x < g.min_x {
                g.min_x = 0;
                g.max_x = 46;
                g.min_y = 0;
                g.max_y = 14;
            } else {
                let span = g.max_x - g.min_x;
                if span <= 45 {
                    let pad = (46 - span) / 2;
                    g.min_x -= pad;
                    g.max_x += pad;
                }
                g.max_x += 1;
                let lo = g.min_y;
                let hi = g.max_y;
                g.min_y = lo - 1;
                g.max_y = if (hi - lo) + 2 < 14 { lo + 13 } else { hi + 2 };
            }
            g.origin_x = 84 + (-36 * g.min_x) + (1632 - 36 * (g.max_x - g.min_x)) / 2;
            g.origin_y = -56 * g.min_y;
            let scroll = if g.selected >= 0 {
                let y = (nodes[g.selected as usize].y - g.min_y) * 56;
                if y < 381 {
                    0
                } else {
                    (y - 381).min(((g.max_y - g.min_y) * 56).max(819) - 819)
                }
            } else {
                0
            };
            g.scroll.fast_forward_to(scroll as f32);
            //47c94/47d7c center the selected node. With no selection,
            //47ec0/47f94 initialize the viewport cursor to834/409.
            let (x, y) = if g.selected >= 0 {
                let node = &nodes[g.selected as usize];
                ((node.x * 36) as f32 + 18., (node.y * 56) as f32 + 28.)
            } else {
                (834., 409.)
            };
            g.x.fast_forward_to(x);
            g.y.fast_forward_to(y);
        }
        let rate = if total > 0 {
            (nodes.len() as i32 * 1000) / total
        } else {
            0
        };
        let percentage = if rate >= 1000 {
            "100".into()
        } else {
            format!("{}.{}", rate / 10, rate % 10)
        };
        let labels = catalog
            .records
            .iter()
            .map(|r| {
                r.label
                    .as_ref()
                    .map(|l| read_sjis_string(&mut &l.c_string_bytes()[..], None))
                    .transpose()
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(Self {
            catalog,
            nodes,
            available,
            initial_group: if title { 0 } else { group },
            current_group: if title { 0 } else { group },
            available_index: 0,
            title,
            progress: 0.,
            choice: -1,
            complete: false,
            closing: false,
            frame: 0,
            tooltip: -1,
            speed: 0.,
            target: -1,
            pointed: -1,
            pressed: -1,
            groups,
            flashes: std::array::from_fn(|_| HigurashiTweener::new(0.)),
            completion_text: format!("達成率:{percentage}％"),
            tooltip_text: String::new(),
            labels,
            pending: vec![],
            actions: vec![],
        })
    }
    fn group(&self) -> &Group {
        &self.groups[self.current_group as usize]
    }
    fn group_mut(&mut self) -> &mut Group {
        &mut self.groups[self.current_group as usize]
    }
    fn cue(&mut self, name: &'static str) {
        self.actions.push(Action::Cue(name));
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    pub fn take_pending_tasks(&mut self) -> Vec<Task> {
        std::mem::take(&mut self.pending)
            .into_iter()
            .map(|kind| Task { kind, done: false })
            .collect()
    }
    fn set_tooltip(&mut self, index: i32) {
        let valid = index >= 0
            && self
                .nodes
                .get(index as usize)
                .is_some_and(|n| self.catalog.records[n.record].kind == 1);
        if !valid {
            self.tooltip = -1;
            return;
        }
        if self.tooltip != index {
            self.tooltip_text = self.labels[self.nodes[index as usize].record]
                .as_ref()
                .expect("Chart label-kind string")
                .clone();
            self.tooltip = index;
        }
    }
    fn clamp_scroll(&self, value: f32) -> f32 {
        value
            .max(0.)
            .min((((self.group().max_y - self.group().min_y) * 56) as f32 - 819.).max(0.))
    }
    fn keep_scroll(&mut self, next: f32) {
        let old = self.group().scroll.value();
        if next != old {
            let g = self.group_mut();
            g.scroll.fast_forward_to(next);
            g.y.fast_forward_to((next - old) + g.y.value());
        }
    }
    pub fn advance_clocks(&mut self, delta: Ticks, ready: bool, scrollbar: &mut Scrollbar) {
        if self.speed != 0. {
            let next = self.clamp_scroll(self.group().scroll.value() - self.speed);
            self.keep_scroll(next);
            self.speed *= f32::from_bits(0x3f733333).powf(delta.as_f32());
            if self.speed.abs() < f32::from_bits(0x3dcccccd) {
                self.speed = 0.;
            }
        }
        if ready {
            *scrollbar = Scrollbar {
                position: self.group().scroll.value() as i32,
                visible: 819,
                first: 0,
                end: ((self.group().max_y - self.group().min_y) * 56).max(819),
            };
        }
        self.frame = self.frame.wrapping_add(1);
        for g in &mut self.groups {
            g.scroll.update(delta);
            g.x.update(delta);
            g.y.update(delta);
        }
        if self.group().x.is_idle() && self.group().y.is_idle() {
            let p = Vec2::new(self.group().x.value(), self.group().y.value());
            let picked = self.pick_node(p);
            self.group_mut().selected = picked;
            self.set_tooltip(picked);
        }
        for f in &mut self.flashes {
            f.update(delta);
        }
    }
    fn pick_node(&self, p: Vec2) -> i32 {
        self.nodes
            .iter()
            .find(|n| {
                n.group == self.current_group
                    && self.catalog.records[n.record].kind == 1
                    && p.y >= (n.y * 56 - 7) as f32
                    && p.y < (n.y * 56 + 63) as f32
                    && p.x >= (n.x * 36 - 45) as f32
                    && p.x < (n.x * 36 + 81) as f32
            })
            .map_or(-1, |n| n.index)
    }
    fn change_group(&mut self, step: i32, which: usize) {
        self.cue("move");
        flash(&mut self.flashes[which], true);
        self.available_index =
            (self.available_index + step).rem_euclid(self.available.len() as i32);
        self.current_group = self.available[self.available_index as usize];
        self.tooltip = -1;
    }
    pub fn begin_close(&mut self) {
        flash(&mut self.flashes[0], true);
        self.pending.push(Kind::CloseWait);
    }
    fn begin_exit(&mut self) {
        self.closing = true;
        if self.choice < 0 {
            self.actions.push(Action::ReturnSource(0));
        }
        self.pending.push(Kind::Exit);
    }
    fn question(&mut self, index: i32) {
        let text = self.labels[self.nodes[index as usize].record]
            .as_ref()
            .expect("Chart label-kind text");
        self.actions.push(Action::Question {
            index,
            prompt: format!("『{text}』へのショートカットを実行します。@rよろしいですか？"),
        });
    }
    pub fn question_response(&mut self, index: i32, stage: u32, response: i32) {
        if stage == 1 && response == 0 {
            self.choice = index;
            self.begin_exit();
        }
    }
    pub fn poll_controls(&mut self, permission: bool, captured: bool, input: Input) -> bool {
        if !permission {
            self.actions.push(Action::DisableInput);
            return !self.closing;
        }
        if captured
            || self.group().scroll.is_waiting()
            || self.group().x.is_waiting()
            || self.group().y.is_waiting()
        {
            return !self.closing;
        }
        let b = input.buttons;
        if b.pressed & (1 << 10) != 0 {
            if self.title && self.available.len() >= 2 {
                self.change_group(-1, 1);
                self.speed = 0.;
            }
            return !self.closing;
        }
        if b.pressed & (1 << 11) != 0 {
            if self.title && self.available.len() >= 2 {
                self.change_group(1, 2);
                self.speed = 0.;
            }
            return !self.closing;
        }
        if b.pressed & (1 << 25) != 0 {
            self.speed = 0.;
            self.cue("cancel");
            self.begin_close();
            return !self.closing;
        }
        if b.pressed & (1 << 24) != 0 {
            self.speed = 0.;
            let i = self.group().selected;
            if i >= 0 && (self.catalog.records[self.nodes[i as usize].record].value as i16) >= 0 {
                self.cue("decide");
                self.scroll_to(i);
                self.question(i);
            } else {
                self.cue("error");
            }
            return !self.closing;
        }
        let mut dx = if b.held & (1 << 7) != 0 {
            -10
        } else if b.held & (1 << 5) != 0 {
            10
        } else {
            0
        };
        let mut dy = if b.held & (1 << 4) != 0 {
            -10
        } else if b.held & (1 << 6) != 0 {
            10
        } else {
            0
        };
        if input.axes[0] != 0 {
            dx = i32::from(input.axes[0]) / 10;
        }
        if input.axes[1] != 0 {
            dy = i32::from(input.axes[1]) / 10;
        }
        let axis = i32::from(input.axes[3]);
        if !(-15..=15).contains(&axis) && axis != 0 {
            let old = self.group().scroll.value();
            let next = self.clamp_scroll(old + ((axis / 16) * 4) as f32);
            self.keep_scroll(next);
            self.speed = 0.;
        }
        // Native4a9bc processes XY after the right-Y scrollbar path too.
        if dx | dy != 0 {
            let g = self.group_mut();
            let x =
                (g.x.value() + dx as f32).clamp((g.min_x * 36) as f32, (g.max_x * 36 - 1) as f32);
            let y =
                (g.y.value() + dy as f32).clamp((g.min_y * 56) as f32, (g.max_y * 56 - 1) as f32);
            g.x.fast_forward_to(x);
            g.y.fast_forward_to(y);
            let top = (g.min_y * 56) as f32;
            let v = y - top;
            let old = g.scroll.value();
            let next = if v - old < 0. {
                v.max(0.)
            } else if ((y + 1.) - top) - old > 819. {
                (((y + 1.) - top) - 819.).min((((g.max_y - g.min_y) * 56) as f32 - 819.).max(0.))
            } else {
                old
            };
            g.scroll.fast_forward_to(next);
            self.speed = 0.;
        }
        !self.closing
    }
    pub fn scroll_to(&mut self, index: i32) -> bool {
        let n = &self.nodes[index as usize];
        let old = self.group().scroll.value();
        let y = (n.y as f32) * 56.;
        let top = (self.group().min_y * 56) as f32;
        let relative = y - top;
        let next = if relative - old < 0. {
            relative.max(0.)
        } else if ((y + 56.) - top) - old > 819. {
            (((y + 56.) - top) - 819.)
                .min((((self.group().max_y - self.group().min_y) * 56) as f32 - 819.).max(0.))
        } else {
            old
        };
        if next == old {
            return false;
        }
        let g = self.group_mut();
        clear(&mut g.y);
        let target = (next - old) + g.y.value();
        control(&mut g.y, target, 4., 0);
        clear(&mut g.scroll);
        control(&mut g.scroll, next, 4., 0);
        true
    }
    pub fn contact(&mut self, p: Vec2) -> bool {
        self.speed = 0.;
        self.pointed = -1;
        if p.y >= 30. && p.y < 114. && p.x >= 1764. && p.x < 1848. {
            self.target = 1;
            return true;
        }
        if self.title && self.available.len() >= 2 && p.y >= 54. && p.y < 126. {
            if p.x >= 828. && p.x < 900. {
                self.target = 2;
                return true;
            }
            if p.x >= 1428. && p.x < 1500. {
                self.target = 3;
                return true;
            }
        }
        self.target = if p.y >= 165. && p.y < 984. { -1 } else { 0 };
        if self.target == 0 {
            return true;
        }
        if p.x <= 84. || p.x >= 1752. {
            return false;
        }
        let g = self.group();
        let selected = g.selected;
        let q = Vec2::new(
            p.x - g.origin_x as f32,
            (p.y - g.origin_y as f32) + (g.scroll.value() as i32 - 165) as f32,
        );
        self.pointed = self.pick_node(q);
        if self.pointed >= 0 && self.pointed == selected {
            self.pressed = self.pointed;
        }
        true
    }
    pub fn reset_contact(&mut self) {
        self.target = -1;
        self.pointed = -1;
        self.pressed = -1;
    }
    fn move_cursor(&mut self, p: Vec2) {
        let g = self.group_mut();
        g.x.fast_forward_to(p.x - g.origin_x as f32);
        let y = (p.y - g.origin_y as f32) + (g.scroll.value() + f32::from_bits(0xc3250000));
        g.y.fast_forward_to(y);
    }
    pub fn press_contact(&mut self, p: Vec2) {
        if self.target >= 0 {
            if (1..=3).contains(&self.target) {
                flash(&mut self.flashes[(self.target - 1) as usize], false);
            }
            return;
        }
        self.move_cursor(p);
        if self.pointed >= 0 && self.scroll_to(self.pointed) {
            self.pointed = -1;
            self.pressed = -1;
        }
    }
    pub fn release_contact(&mut self, p: Vec2) {
        match self.target {
            1 => {
                self.cue("decide");
                self.begin_close();
            }
            2 => self.change_group(-1, 1),
            3 => self.change_group(1, 2),
            t if t < 0 => {
                self.move_cursor(p);
                if self.pointed >= 0 {
                    if self.scroll_to(self.pointed) {
                        self.pointed = -1;
                        self.pressed = -1;
                    } else if self.pressed >= 0 {
                        self.cue("decide");
                        self.question(self.pointed);
                    }
                }
            }
            _ => {}
        }
    }
    pub fn drag_contact(&mut self, previous: Vec2, current: Vec2) {
        if self.target >= 0 {
            self.cancel_contact();
            return;
        }
        self.pointed = -1;
        self.pressed = -1;
        let next = self.clamp_scroll(self.group().scroll.value() - (current.y - previous.y));
        self.keep_scroll(next);
    }
    pub fn inertia(&mut self, speed: f32) {
        if self.target < 0 && self.pointed < 0 {
            self.speed = speed;
        }
    }
    pub fn cancel_contact(&mut self) {
        if self.target >= 0 {
            for f in &mut self.flashes {
                clear_flash(f);
            }
            self.target = 0;
        }
    }
}
impl InputAreaTarget for Model {
    fn probe(&mut self, p: Vec2) -> bool {
        self.contact(p)
    }
    fn reset(&mut self) {
        self.reset_contact();
    }
    fn press(&mut self, p: Vec2) {
        self.press_contact(p);
    }
    fn release(&mut self, p: Vec2) {
        self.release_contact(p);
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, a: Vec2, b: Vec2) {
        self.drag_contact(a, b);
    }
    fn gesture(&mut self, s: Vec2) {
        self.inertia(s.y);
    }
    fn cancel(&mut self) {
        self.cancel_contact();
    }
}
impl Task {
    pub fn poll_native(
        &mut self,
        m: &mut Model,
        status: u32,
        delta: Ticks,
        permission: bool,
        s: &mut dyn Services,
    ) -> Result<Vec<Task>> {
        let mut spawned = vec![];
        match self.kind {
            Kind::First => {
                if status == 2 {
                    m.closing = false;
                    spawned.push(Task {
                        kind: Kind::Open,
                        done: false,
                    });
                    self.done = true;
                }
            }
            Kind::Open => {
                m.progress += delta.as_f32() * f32::from_bits(0x3e4ccccd);
                if m.progress >= 1. {
                    m.progress = 1.;
                    spawned.push(Task {
                        kind: Kind::Input,
                        done: false,
                    });
                    self.done = true;
                }
            }
            Kind::Input => {
                s.input(m, permission);
                self.done = m.closing;
            }
            Kind::CloseWait => {
                if m.flashes[0].is_idle() {
                    m.begin_exit();
                    self.done = true;
                }
            }
            Kind::Exit => {
                m.progress += delta.as_f32() * f32::from_bits(0xbe4ccccd);
                if m.progress <= 0. {
                    m.progress = 0.;
                    if m.choice < 0 {
                        m.actions.push(Action::ReturnSource(1));
                        m.complete = true;
                    } else {
                        let scenario = i32::from(
                            m.catalog.records[m.nodes[m.choice as usize].record].value as i16,
                        );
                        let seed = s.next_seed();
                        let empty = ProgressEntry {
                            scenario_id: 0,
                            random_seed: 0,
                            save_position: 0,
                            replay_target: 0,
                            response_data: vec![],
                        };
                        let data = RuntimeGameData::from_progress(
                            SaveProgress::new(
                                0,
                                m.current_group as u32,
                                [
                                    ProgressEntry {
                                        scenario_id: scenario,
                                        random_seed: seed,
                                        ..empty.clone()
                                    },
                                    empty,
                                ],
                            )?,
                            // Native4c728..4c78c initializes the active header and
                            // all owned vectors, leaving Date and inactive
                            // scalar storage unwritten. The desktop complete
                            // owner gives those unused fields defined zeros;
                            // original comparisons exclude uninitialized bytes.
                            SaveDate {
                                year: 0,
                                month: 0,
                                day: 0,
                                hour: 0,
                                minute: 0,
                                second: 0,
                            },
                            vec![],
                            vec![],
                        );
                        m.actions.push(Action::StartAdv { data, flags: 0 });
                    }
                    self.done = true;
                }
            }
        }
        spawned.extend(m.take_pending_tasks());
        for event in m.take_actions() {
            s.event(event)?;
        }
        Ok(spawned)
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
