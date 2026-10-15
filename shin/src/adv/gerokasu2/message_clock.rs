//! Later-engine message clock, visible glyph fades, windows and block effects.
//! Native8e6f0..8ef94; effects service owns actual voice and completion callbacks.
use anyhow::{Result, ensure};
use glam::{Vec2, vec2};
use shin_core::{
    layout::{LineInfo, commands::Command},
    primitives::update::{FrameId, UpdateTracker},
    time::Ticks,
};

use super::message_text::Gerokasu2MessageText;

const SECOND_PER_TICK: f32 = f32::from_bits(0x3c888889);

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceRequest {
    pub filename: String,
    pub volume: f32,
    pub lipsync: bool,
    pub segment_start: u64,
    pub segment_duration: u64,
}
pub trait MessageEffects {
    fn voice_is_playing(&self) -> bool;
    fn stop_voice(&mut self);
    fn play_voice(&mut self, request: VoiceRequest) -> bool;
    fn observe_voice(&mut self, filename: &str);
    fn completed(&mut self, message_id: u32);
}

/// Snapshot of the native voice service's playing bit and unsigned position.
/// MSGSYNC compares native microseconds directly with its sign-extended operand.
#[derive(Debug, Clone, Copy)]
pub struct VoiceTimeline {
    pub playing: bool,
    pub position: u64,
}

#[derive(Debug, Clone)]
pub(super) enum BlockKind {
    Voice {
        filename: String,
        volume: f32,
        lipsync: bool,
        duration: u32,
    },
    Wait {
        last: bool,
        auto: bool,
        delay: f32,
    },
    Section(u32),
    Sync(u32),
    VoiceSync {
        start: u32,
        duration: u32,
    },
    VoiceWait,
}
#[derive(Debug, Clone)]
pub(super) struct Block {
    pub time: f32,
    pub kind: BlockKind,
}
#[derive(Debug, Clone)]
pub(super) struct Character {
    pub time: f32,
    pub line: usize,
    pub block: usize,
    pub position: Vec2,
    pub width: f32,
    pub rate: f32,
    pub progress: f32,
}
#[derive(Debug, Clone)]
pub struct OutgoingWindow {
    pub mode: i32,
    pub slide: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct MessageParams {
    pub flags: u32,
    pub mode: i32,
    pub message_id: u32,
    pub message_arg: i32,
    pub animate_open: bool,
}

pub struct MessageClock {
    tracker: UpdateTracker,
    pub(super) blocks: Vec<Block>,
    pub(super) characters: Vec<Character>,
    pub(super) lines: Vec<[f32; 5]>,
    pub params: MessageParams,
    pub open: bool,
    pub slide: f32,
    pub background: f32,
    pub modal_hidden: bool,
    pub modal_slide: f32,
    pub outgoing: Vec<OutgoingWindow>,
    pub height: f32,
    pub height_target: f32,
    pub first_line_width: f32,
    pub text_size: Vec2,
    pub block_index: usize,
    pub time: f32,
    pub wait_kind: i32,
    pub skip_delay: f32,
    pub voice_delay: f32,
    pub voice_active: bool,
    pub voice_disabled: bool,
    pub sections: u32,
    pub syncs: u32,
    pub ticks_since_wait: f32,
    pub cursor: Vec2,
    pub cursor_line: usize,
    pub total_voices: u32,
    pub voice_block: usize,
    pub voice_counter: i32,
    autoplay_requested: bool,
    force_skip_requested: bool,
    pub skip_speed: u8,
}

impl MessageClock {
    /// Original8BFC0/8C3C0 for reconstruction mode0. Fields not written by
    /// those bodies (message IDs, clocks, voice association and cursor) remain.
    pub(crate) fn clear_for_reconstruction(&mut self) {
        self.characters.clear();
        self.lines.clear();
        self.blocks.clear();
        self.total_voices = 0;
        self.params.mode = 0;
        self.open = false;
        self.slide = 0.0;
        self.background = 0.0;
        self.modal_hidden = false;
        self.modal_slide = 1.0;
        self.autoplay_requested = false;
        self.force_skip_requested = false;
        self.height = 259.0;
        self.height_target = 259.0;
        self.outgoing.clear();
    }
    pub fn new(
        commands: &[Command],
        lines: &[LineInfo],
        layout_size: Vec2,
        params: MessageParams,
    ) -> Result<Self> {
        let mut result = Self {
            tracker: UpdateTracker::new(),
            blocks: Vec::new(),
            characters: Vec::new(),
            lines: Vec::new(),
            params,
            open: false,
            slide: 0.0,
            background: 0.0,
            modal_hidden: false,
            modal_slide: 1.0,
            outgoing: Vec::new(),
            height: 259.0,
            height_target: 259.0,
            first_line_width: 0.0,
            text_size: Vec2::ZERO,
            block_index: 0,
            time: 0.0,
            wait_kind: -1,
            skip_delay: 0.0,
            voice_delay: 0.0,
            voice_active: false,
            voice_disabled: false,
            sections: 0,
            syncs: 0,
            ticks_since_wait: 0.0,
            cursor: Vec2::ZERO,
            cursor_line: 0,
            total_voices: 0,
            voice_block: 0,
            voice_counter: -1,
            autoplay_requested: false,
            force_skip_requested: false,
            skip_speed: 50,
        };
        result.install(commands, lines, layout_size, params)?;
        Ok(result)
    }

    pub fn install(
        &mut self,
        commands: &[Command],
        lines: &[LineInfo],
        layout_size: Vec2,
        params: MessageParams,
    ) -> Result<()> {
        let mut blocks = Vec::new();
        let mut characters = Vec::new();
        let mut delay = 0.0f32;
        let mut voices = 0u32;
        for command in commands {
            match command {
                Command::Char(c) => {
                    ensure!(
                        c.line_index < lines.len(),
                        "Message character line is missing"
                    );
                    if !c.is_rubi {
                        delay += 0.05;
                    }
                    characters.push(Character {
                        time: c.time,
                        line: c.line_index,
                        block: blocks.len(),
                        position: c.position,
                        width: c.width,
                        rate: if c.fade == 0.0 {
                            1.0
                        } else {
                            (1.0 / c.fade) * SECOND_PER_TICK
                        },
                        progress: 0.0,
                    });
                }
                Command::Voice(v) => {
                    voices = voices.wrapping_add(1);
                    blocks.push(Block {
                        time: v.time,
                        kind: BlockKind::Voice {
                            filename: v.filename.clone(),
                            volume: v.volume,
                            lipsync: v.lipsync_enabled,
                            duration: v.segment_duration,
                        },
                    });
                }
                Command::Wait(v) => {
                    blocks.push(Block {
                        time: v.time,
                        kind: BlockKind::Wait {
                            last: v.is_last_wait,
                            auto: v.is_auto_click,
                            delay,
                        },
                    });
                    delay = 0.0;
                }
                Command::Section(v) => blocks.push(Block {
                    time: v.time,
                    kind: BlockKind::Section(v.index),
                }),
                Command::Sync(v) => blocks.push(Block {
                    time: v.time,
                    kind: BlockKind::Sync(v.index),
                }),
                Command::VoiceSync(v) => blocks.push(Block {
                    time: v.time,
                    kind: BlockKind::VoiceSync {
                        start: v.segment_start,
                        duration: v.segment_duration,
                    },
                }),
                Command::VoiceWait(v) => blocks.push(Block {
                    time: v.time,
                    kind: BlockKind::VoiceWait,
                }),
            }
        }
        if params.mode != self.params.mode
            && self.slide > 0.0
            && !matches!((self.params.mode, params.mode), (0, 3) | (3, 0))
        {
            self.outgoing.push(OutgoingWindow {
                mode: self.params.mode,
                slide: self.slide,
                height: self.height,
            });
            self.slide = 0.0;
        }
        self.params = params;
        if params.message_arg < 0 {
            self.params.message_arg = 9;
        }
        self.blocks = blocks;
        self.characters = characters;
        self.lines = lines
            .iter()
            .map(|l| {
                [
                    l.y_position,
                    l.baseline_ascent,
                    l.line_height,
                    l.rubi_height,
                    0.0,
                ]
            })
            .collect();
        self.open = true;
        self.first_line_width = lines.first().map_or(0.0, |l| l.width);
        // Nameplate width is padded after the parser updates its size. Keep
        // the parser's size explicitly; max(runtime line widths) differs.
        self.text_size = layout_size;
        self.block_index = 0;
        self.time = 0.0;
        self.wait_kind = -1;
        self.skip_delay = 0.0;
        self.voice_delay = 0.0;
        self.voice_active = false;
        self.voice_disabled = false;
        self.sections = 0;
        self.syncs = 0;
        self.ticks_since_wait = 0.0;
        self.cursor = Vec2::ZERO;
        self.cursor_line = 0;
        self.total_voices = voices;
        self.voice_block = 0;
        self.voice_counter = -1;
        if matches!(params.mode, 0 | 2 | 3) {
            if self.slide == 0.0 {
                self.height = 259.0;
            }
            self.height_target = 259.0;
        } else if matches!(params.mode, 1 | 4) {
            self.height = 1080.0;
            self.height_target = 1080.0;
        }
        if !params.animate_open {
            self.slide = 1.0;
            self.background = if params.mode == 3 { 1.0 } else { 0.0 };
        }
        Ok(())
    }

    pub fn request_autoplay(&mut self) {
        self.autoplay_requested = true;
    }
    pub fn send_sync(&mut self) {
        self.syncs = self.syncs.wrapping_add(1);
    }
    pub fn is_finished(&self) -> bool {
        self.block_index >= self.blocks.len()
    }
    pub fn sync_is_waiting(&self, index: i32) -> bool {
        !self.is_finished() && (index < 0 || self.sections as i32 <= index)
    }
    /// Native8e220: current window, unfinished open text, then outgoing windows.
    /// Background and modal animation do not participate in command waits.
    pub fn busy(&self) -> bool {
        if self.open {
            if self.slide < 1.0 || !self.is_finished() {
                return true;
            }
        } else if self.slide > 0.0 {
            return true;
        }
        self.outgoing.iter().any(|window| window.slide > 0.0)
    }
    /// Native8e1a0; voice indices compare as signed values, positions as u64.
    pub fn voice_sync_is_waiting(
        &self,
        index: i32,
        position: u64,
        voice: Option<VoiceTimeline>,
    ) -> bool {
        if self.total_voices == 0 {
            return false;
        }
        if self.voice_counter != index {
            return self.voice_counter < index;
        }
        voice.is_some_and(|voice| voice.playing && voice.position < position)
    }
    /// Native8d874 leaves glyphs, blocks, voice and outgoing windows owned.
    pub fn close(&mut self, animate: bool) {
        self.open = false;
        if !animate {
            self.slide = 0.0;
            self.background = 0.0;
        }
    }
    pub fn progress(&self) -> impl Iterator<Item = f32> + '_ {
        self.characters.iter().map(|c| c.progress)
    }
    pub fn apply_to_text(&self, text: &mut Gerokasu2MessageText) -> Result<()> {
        ensure!(
            text.character_count() == self.characters.len(),
            "Message clock and prepared text disagree"
        );
        for (index, progress) in self.progress().enumerate() {
            text.set_progress(index, progress)?;
        }
        Ok(())
    }

    pub fn update(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        animations_allowed: bool,
        effects: &mut impl MessageEffects,
    ) {
        if self.tracker.update(frame) {
            self.tick(delta.as_f32(), animations_allowed, effects);
        }
    }

    /// Native8e330: reveal unfinished glyphs before advancing a wait. The
    /// setting at Settings+0xb6 preserves the voice at a final click wait.
    pub fn try_advance(
        &mut self,
        keep_voice_after_message: bool,
        effects: &mut impl MessageEffects,
    ) -> bool {
        if self.modal_hidden || self.modal_slide < 1.0 || !self.open {
            return false;
        }
        if self.is_finished() || self.params.flags & 2 != 0 {
            return false;
        }
        if self.slide < 1.0 {
            return true;
        }
        let mut revealed = false;
        for c in &mut self.characters {
            if c.block <= self.block_index && c.progress < 1.0 {
                c.progress = 1.0;
                revealed = true;
            }
        }
        if revealed {
            self.time = self.blocks[self.block_index].time;
            return true;
        }
        if self.wait_kind >= 0 {
            if self.wait_kind == 0 || (self.wait_kind == 1 && !keep_voice_after_message) {
                self.stop_voice(effects);
            }
            if self.wait_kind == 1 {
                effects.completed(self.params.message_id);
            }
            self.wait_kind = -1;
            self.block_index += 1;
        } else if self.voice_active {
            match self.blocks[self.block_index].kind {
                BlockKind::Voice { .. } | BlockKind::VoiceWait => self.stop_voice(effects),
                BlockKind::VoiceSync { start, duration } => {
                    if matches!(
                        self.blocks.get(self.voice_block).map(|b| &b.kind),
                        Some(BlockKind::Voice { .. })
                    ) {
                        self.play_voice(self.voice_block, start, duration, effects);
                        self.block_index += 1;
                    }
                }
                _ => {}
            }
        }
        true
    }

    /// Native8dc30: stop at an unsatisfied sync, retaining its block/time;
    /// glyph eligibility still uses both the current block and its time.
    pub fn fast_forward(&mut self, effects: &mut impl MessageEffects) {
        if self.modal_hidden {
            return;
        }
        self.modal_slide = 1.0;
        self.slide = if self.open { 1.0 } else { 0.0 };
        self.background = if self.open && self.params.mode == 3 {
            1.0
        } else {
            0.0
        };
        self.outgoing.clear();
        while let Some(block) = self.blocks.get(self.block_index) {
            self.time = block.time;
            match &block.kind {
                BlockKind::Voice { filename, .. } => {
                    effects.observe_voice(filename);
                    self.voice_block = self.block_index;
                    self.voice_counter = self.voice_counter.wrapping_add(1);
                }
                BlockKind::Wait { last: true, .. } => effects.completed(self.params.message_id),
                BlockKind::Section(index) => self.sections = *index,
                BlockKind::Sync(index) if (self.syncs as i32) <= (*index as i32) => break,
                _ => {}
            }
            self.block_index += 1;
        }
        for c in &mut self.characters {
            if c.block > self.block_index || c.time > self.time {
                continue;
            }
            c.progress = 1.0;
            let line = &mut self.lines[c.line];
            let target = line[0] + line[2];
            if self.height_target < target {
                self.height_target = target;
            }
            line[4] = 1.0;
        }
        self.stop_voice(effects);
        self.wait_kind = -1;
        self.height = self.height_target;
    }

    fn stop_voice(&mut self, effects: &mut impl MessageEffects) {
        effects.stop_voice();
        self.voice_active = false;
        self.voice_delay = 0.0;
    }

    pub(super) fn tick(&mut self, ticks: f32, animations: bool, effects: &mut impl MessageEffects) {
        let autoplay = self.autoplay_requested;
        let force = self.force_skip_requested;
        self.autoplay_requested = false;
        self.force_skip_requested = false;
        self.modal_slide = slide(self.modal_slide, ticks, !self.modal_hidden);
        if self.modal_slide < 1.0 {
            return;
        }
        if animations {
            self.slide = slide(self.slide, ticks, self.open);
            self.background = slide(self.background, ticks, self.open && self.params.mode == 3);
        }
        for window in &mut self.outgoing {
            window.slide = ticks.mul_add(-0.1, window.slide);
        }
        self.outgoing.retain(|w| w.slide > 0.0);
        if self.slide < 1.0 {
            return;
        }
        let force = force && self.params.flags & 2 == 0;
        let autoplay = autoplay && self.params.flags & 2 == 0;
        if force {
            effects.stop_voice();
            self.voice_active = false;
            self.voice_delay = 0.0;
            if self.wait_kind >= 0 {
                if self.wait_kind == 1 {
                    effects.completed(self.params.message_id);
                }
                self.wait_kind = -1;
                self.block_index += 1;
            }
        }
        if self.voice_active && !effects.voice_is_playing() {
            self.voice_active = false;
        }
        if self.wait_kind >= 0 {
            if self.skip_delay > 0.0 {
                self.skip_delay = ticks.mul_add(-SECOND_PER_TICK, self.skip_delay);
            }
            if !autoplay {
                if self.voice_active {
                    self.voice_delay = 0.5;
                }
            } else if self.skip_delay <= 0.0 && !self.voice_active {
                self.voice_delay = ticks.mul_add(-SECOND_PER_TICK, self.voice_delay);
                if self.voice_delay <= 0.0 {
                    if self.wait_kind == 1 {
                        effects.completed(self.params.message_id);
                    }
                    self.wait_kind = -1;
                    self.block_index += 1;
                }
            }
        }
        if self.wait_kind < 0 {
            while self.block_index < self.blocks.len() {
                let block = self.blocks[self.block_index].clone();
                if block.time > self.time {
                    break;
                }
                match block.kind {
                    BlockKind::Voice { filename, .. } => {
                        if self.voice_active {
                            break;
                        }
                        effects.observe_voice(&filename);
                        if !force {
                            self.play_voice(
                                self.block_index,
                                0,
                                self.voice_duration(self.block_index),
                                effects,
                            );
                        }
                        self.voice_block = self.block_index;
                        self.voice_counter = self.voice_counter.wrapping_add(1);
                    }
                    BlockKind::Wait { last, auto, delay } => {
                        if self.height != self.height_target
                            || self
                                .characters
                                .iter()
                                .any(|c| c.block <= self.block_index && c.progress < 1.0)
                        {
                            break;
                        }
                        if force {
                            if last {
                                effects.completed(self.params.message_id);
                            }
                        } else if auto || self.params.flags & 2 != 0 {
                            if effects.voice_is_playing() {
                                break;
                            }
                        } else {
                            self.wait_kind = i32::from(last);
                            self.ticks_since_wait = 0.0;
                            let rate = (100 - i32::from(self.skip_speed)) as f32 * 0.01;
                            self.skip_delay = delay * rate;
                            break;
                        }
                    }
                    BlockKind::Section(index) => self.sections = index,
                    BlockKind::Sync(index) => {
                        if (self.syncs as i32) <= (index as i32) {
                            break;
                        }
                    }
                    BlockKind::VoiceSync { start, duration } => {
                        if self.voice_active {
                            break;
                        }
                        if self.voice_counter >= 0 && !force {
                            self.play_voice(self.voice_block, start, duration, effects);
                        }
                    }
                    BlockKind::VoiceWait => {
                        if self.voice_active {
                            break;
                        }
                    }
                }
                self.block_index += 1;
            }
        }
        for c in &mut self.characters {
            if c.block > self.block_index || c.time > self.time {
                continue;
            }
            let progress = ticks.mul_add(c.rate, c.progress);
            c.progress = if progress > 1.0 { 1.0 } else { progress };
            let line = &mut self.lines[c.line];
            let target = line[0] + line[2];
            if self.height_target < target {
                self.height_target = target;
            }
            let candidate = c.position + vec2(c.width, 0.0);
            if self.cursor.y == candidate.y {
                if self.cursor.x < candidate.x {
                    self.cursor.x = candidate.x;
                }
            } else if self.cursor.y < candidate.y {
                self.cursor = candidate;
                self.cursor_line = c.line;
            }
            line[4] = 1.0;
        }
        if self.height < self.height_target {
            self.height = ticks.mul_add(18.0, self.height);
            if self.height > self.height_target {
                self.height = self.height_target;
            }
        } else if self.height > self.height_target {
            self.height = ticks.mul_add(-18.0, self.height);
            if self.height < self.height_target {
                self.height = self.height_target;
            }
        }
        if let Some(next) = self.blocks.get(self.block_index) {
            self.time = ticks.mul_add(SECOND_PER_TICK, self.time);
            if self.time > next.time {
                self.time = next.time;
            }
        }
        self.ticks_since_wait += ticks;
    }

    fn voice_duration(&self, index: usize) -> u32 {
        match &self.blocks[index].kind {
            BlockKind::Voice { duration, .. } => *duration,
            _ => 0,
        }
    }
    fn play_voice(
        &mut self,
        index: usize,
        start: u32,
        duration: u32,
        effects: &mut impl MessageEffects,
    ) {
        if self.voice_disabled {
            return;
        }
        if let BlockKind::Voice {
            filename,
            volume,
            lipsync,
            ..
        } = &self.blocks[index].kind
        {
            self.voice_active = effects.play_voice(VoiceRequest {
                filename: filename.clone(),
                volume: *volume,
                lipsync: *lipsync,
                segment_start: u64::from(start) * 1000,
                segment_duration: u64::from(duration) * 1000,
            });
        }
        self.voice_delay = if self.voice_active { 0.5 } else { 0.0 };
    }
}

fn slide(value: f32, ticks: f32, increasing: bool) -> f32 {
    let value = ticks.mul_add(if increasing { 0.1 } else { -0.1 }, value);
    if !increasing && value < 0.0 {
        0.0
    } else if increasing && value > 1.0 {
        1.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod install_tests;
