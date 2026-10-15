//! Updated older message owner clocks and input. Audio is supplied by the
//! caller; no Umineko messagebox, storage or title state is used here.
use super::message_glyphs::HigurashiMessageGlyphs;
use glam::{Vec2, vec2};
use shin_core::layout::{
    LineInfo,
    commands::{Char, Command},
};

const SECONDS_PER_TICK: f32 = 1.0 / 60.0;

pub trait MessageVoice {
    fn is_playing(&self) -> bool;
    fn position_micros(&self) -> u64;
    fn play(
        &mut self,
        filename: &str,
        volume: f32,
        lipsync: bool,
        start: u32,
        duration: u32,
    ) -> bool;
    fn stop(&mut self);
    /// Nativeeb740 pans matching speakers and queries their mouth feedback.
    fn character_lipsync(&self, _character: i32, _pan: f32) -> Option<u8> {
        None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MessageSettings {
    pub voice_volume: u8,
    pub skip_speed: u8,
    pub continue_voice_on_click: bool,
}
impl Default for MessageSettings {
    fn default() -> Self {
        Self {
            voice_volume: 100,
            skip_speed: 50,
            continue_voice_on_click: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitKind {
    Regular,
    Last,
}

struct Character {
    time: f32,
    block: usize,
    line: usize,
    position: Vec2,
    width: f32,
    rate: f32,
    progress: f32,
}
enum BlockKind {
    Voice {
        filename: String,
        volume: f32,
        lipsync: bool,
        duration: u32,
    },
    Wait {
        delay: f32,
        last: bool,
        auto: bool,
    },
    Section(u32),
    Sync(u32),
    VoiceSync {
        start: u32,
        duration: u32,
    },
    VoiceWait,
}
struct Block {
    time: f32,
    kind: BlockKind,
}
struct Line {
    bottom: f32,
    visible: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct OutgoingWindow {
    pub style: u32,
    pub slide: f32,
    pub height: f32,
}

pub struct HigurashiMessagePlayback {
    pub shown: bool,
    pub hidden: bool,
    pub natural_slide: f32,
    pub modal_slide: f32,
    pub height: f32,
    pub height_target: f32,
    pub flags: u32,
    pub style: u32,
    pub alignment: u32,
    pub options: [i32; 2],
    pub argument: i32,
    pub message_id: u32,
    pub outgoing: Vec<OutgoingWindow>,
    pub current_block: usize,
    pub current_time: f32,
    pub wait: Option<WaitKind>,
    pub wait_delay: f32,
    pub voice_delay: f32,
    pub voice_playing: bool,
    pub voice_disabled: bool,
    pub sections: u32,
    pub syncs: u32,
    pub ticks_since_wait: f32,
    pub cursor: Vec2,
    pub current_line: usize,
    pub total_voices: u32,
    pub voice_block: usize,
    pub voice_counter: i32,
    pub input_blocked: bool,
    pub completed_messages: Vec<u32>,
    characters: Vec<Character>,
    blocks: Vec<Block>,
    lines: Vec<Line>,
}
impl Default for HigurashiMessagePlayback {
    fn default() -> Self {
        Self {
            shown: false,
            hidden: false,
            natural_slide: 0.0,
            modal_slide: 1.0,
            height: 357.0,
            height_target: 357.0,
            flags: 0,
            style: 0,
            alignment: 0,
            options: [0; 2],
            argument: 0,
            message_id: 0,
            outgoing: Vec::new(),
            current_block: 0,
            current_time: 0.0,
            wait: None,
            wait_delay: 0.0,
            voice_delay: 0.0,
            voice_playing: false,
            voice_disabled: false,
            sections: 0,
            syncs: 0,
            ticks_since_wait: 0.0,
            cursor: Vec2::ZERO,
            current_line: 0,
            total_voices: 0,
            voice_block: 0,
            voice_counter: 0,
            input_blocked: false,
            completed_messages: Vec::new(),
            characters: Vec::new(),
            blocks: Vec::new(),
            lines: Vec::new(),
        }
    }
}

impl HigurashiMessagePlayback {
    /// Native840b8 stops transport and blocks input. Retain glyph/layout clocks
    /// and cached voice flags until the resumed message command updates them.
    pub fn prepare_reconstruction(&mut self, voice: &mut impl MessageVoice) {
        voice.stop();
        self.flags = 0;
        self.voice_disabled = false;
        self.input_blocked = true;
    }
    /// Install82564's state reset; retain an outgoing window when changing style.
    pub fn install(
        &mut self,
        raw_style: i32,
        options: [i32; 2],
        flags: u32,
        argument: i32,
        message_id: u32,
        animate_slide: bool,
    ) {
        let mode = raw_style as u32 & 15;
        let style = if mode <= 4 { mode } else { 0 };
        if self.style != style && self.natural_slide > 0.0 {
            self.outgoing.push(OutgoingWindow {
                style: self.style,
                slide: self.natural_slide,
                height: self.height,
            });
            self.natural_slide = 0.0;
        }
        self.characters.clear();
        self.blocks.clear();
        self.lines.clear();
        self.flags = flags;
        self.style = style;
        let alignment = (raw_style as u32 >> 4) & 15;
        self.alignment = if alignment <= 3 { alignment } else { 0 };
        self.options = options;
        self.argument = if argument < 0 { 9 } else { argument };
        self.message_id = message_id;
        self.current_block = 0;
        self.current_time = 0.0;
        self.wait = None;
        self.wait_delay = 0.0;
        self.voice_delay = 0.0;
        self.voice_playing = false;
        self.voice_disabled = false;
        self.sections = 0;
        self.syncs = 0;
        self.ticks_since_wait = 0.0;
        self.cursor = Vec2::ZERO;
        self.current_line = 0;
        self.total_voices = 0;
        self.voice_block = 0;
        self.voice_counter = -1;
        self.input_blocked = false;
        self.shown = true;
        if matches!(style, 1 | 3 | 4) {
            self.height = 1080.0;
            self.height_target = 1080.0;
        } else {
            if self.natural_slide == 0.0 {
                self.height = 357.0;
            }
            self.height_target = 357.0;
        }
        if !animate_slide {
            self.natural_slide = 1.0;
        }
    }

    pub fn set_layout(&mut self, commands: &[Command], lines: &[LineInfo]) {
        let mut chars = Vec::new();
        let mut blocks = Vec::new();
        for command in commands {
            match command {
                Command::Char(c) => chars.push((blocks.len(), c)),
                other => blocks.push(other),
            }
        }
        self.set_records(chars.into_iter(), blocks.into_iter(), lines);
    }

    pub fn set_glyphs(&mut self, glyphs: &HigurashiMessageGlyphs) {
        self.set_records(
            glyphs.glyphs.iter().map(|g| (g.block_index, &g.character)),
            glyphs.blocks.iter(),
            &glyphs.lines,
        );
    }

    fn set_records<'a>(
        &mut self,
        characters: impl Iterator<Item = (usize, &'a Char)>,
        blocks: impl Iterator<Item = &'a Command>,
        lines: &[LineInfo],
    ) {
        let mut counts = Vec::<usize>::new();
        self.characters = characters
            .map(|(block, c)| {
                counts.resize(counts.len().max(block + 1), 0);
                if !c.is_rubi {
                    counts[block] += 1;
                }
                Character {
                    time: c.time,
                    block,
                    line: c.line_index,
                    position: c.position,
                    width: c.width,
                    rate: if c.fade == 0.0 {
                        1.0
                    } else {
                        (1.0 / c.fade) * SECONDS_PER_TICK
                    },
                    progress: 0.0,
                }
            })
            .collect();
        let mut delay = 0.0;
        self.total_voices = 0;
        self.blocks = blocks
            .enumerate()
            .map(|(i, command)| {
                // Preserve the native serial accumulation across non-wait blocks.
                // Recompute each character increment rather than summing block totals.
                for _ in 0..counts.get(i).copied().unwrap_or(0) {
                    delay += 0.05;
                }
                let kind = match command {
                    Command::Voice(v) => {
                        self.total_voices += 1;
                        BlockKind::Voice {
                            filename: v.filename.clone(),
                            volume: v.volume,
                            lipsync: v.lipsync_enabled,
                            duration: v.segment_duration,
                        }
                    }
                    Command::Wait(w) => {
                        let result = BlockKind::Wait {
                            delay,
                            last: w.is_last_wait,
                            auto: w.is_auto_click,
                        };
                        delay = 0.0;
                        result
                    }
                    Command::Section(s) => BlockKind::Section(s.index),
                    Command::Sync(s) => BlockKind::Sync(s.index),
                    Command::VoiceSync(s) => BlockKind::VoiceSync {
                        start: s.segment_start,
                        duration: s.segment_duration,
                    },
                    Command::VoiceWait(_) => BlockKind::VoiceWait,
                    Command::Char(_) => unreachable!(),
                };
                Block {
                    time: command.time(),
                    kind,
                }
            })
            .collect();
        self.lines = lines
            .iter()
            .map(|l| Line {
                bottom: l.y_position + l.line_height,
                visible: false,
            })
            .collect();
    }

    pub fn progress(&self) -> Vec<f32> {
        self.characters.iter().map(|c| c.progress).collect()
    }
    pub fn waiting(&self, section: i32) -> bool {
        self.current_block < self.blocks.len() && (section < 0 || (self.sections as i32) <= section)
    }
    pub fn voice_waiting(&self, index: i32, milliseconds: i32, voice: &impl MessageVoice) -> bool {
        if self.total_voices == 0 {
            return false;
        }
        if self.voice_counter != index {
            return self.voice_counter < index;
        }
        // Callback53d8 multiplies the signed target by1000.84008 divides
        // the ec318 microsecond clock by1000 before the unsigned comparison.
        // Preserve this older scaling, including negative targets.
        voice.is_playing()
            && (i64::from(milliseconds).wrapping_mul(1000) as u64) > voice.position_micros() / 1000
    }
    pub fn busy(&self) -> bool {
        (if self.shown {
            self.natural_slide < 1.0 || self.current_block < self.blocks.len()
        } else {
            self.natural_slide > 0.0
        }) || self.outgoing.iter().any(|w| w.slide > 0.0)
    }
    pub fn interested_in_input(&self) -> bool {
        self.shown
            && self.natural_slide >= 1.0
            && self.current_block < self.blocks.len()
            && !self.input_blocked
    }
    pub fn send_sync(&mut self) {
        self.syncs = self.syncs.wrapping_add(1);
    }
    pub fn block_input(&mut self) {
        if (3..=4).contains(&self.style) && self.current_block < self.blocks.len() {
            self.input_blocked = true;
        }
    }
    pub fn close(&mut self, animate: bool) {
        self.shown = false;
        if !animate {
            self.natural_slide = 0.0;
        }
        self.characters.clear();
        self.blocks.clear();
        self.lines.clear();
        self.total_voices = 0;
    }
    fn complete(&mut self) {
        self.completed_messages.push(self.message_id);
    }
    fn stop_voice(&mut self, voice: &mut impl MessageVoice) {
        voice.stop();
        self.voice_playing = false;
        self.voice_delay = 0.0;
    }
    fn play_voice(
        &mut self,
        index: usize,
        start: u32,
        duration: u32,
        settings: MessageSettings,
        voice: &mut impl MessageVoice,
    ) {
        if self.voice_disabled || settings.voice_volume == 0 {
            return;
        }
        let BlockKind::Voice {
            filename,
            volume,
            lipsync,
            ..
        } = &self.blocks[index].kind
        else {
            return;
        };
        self.voice_playing = voice.play(filename, *volume, *lipsync, start, duration);
        self.voice_delay = if self.voice_playing { 0.5 } else { 0.0 };
    }

    /// Update84694 consumes per-frame auto/skip requests before modal gating.
    pub fn advance(
        &mut self,
        ticks: f32,
        autoplay: bool,
        skip_request: bool,
        settings: MessageSettings,
        voice: &mut impl MessageVoice,
    ) {
        let slide = ticks * 0.1;
        self.modal_slide = if self.hidden {
            (self.modal_slide - slide).max(0.0)
        } else {
            (self.modal_slide + slide).min(1.0)
        };
        if self.modal_slide < 1.0 {
            return;
        }
        self.natural_slide = if self.shown {
            (self.natural_slide + slide).min(1.0)
        } else {
            (self.natural_slide - slide).max(0.0)
        };
        self.outgoing.retain_mut(|w| {
            w.slide -= slide;
            w.slide > 0.0
        });
        if self.natural_slide < 1.0 {
            return;
        }
        let allow_input = self.flags & 2 == 0;
        let skip = skip_request && allow_input;
        if skip {
            self.stop_voice(voice);
            if let Some(wait) = self.wait.take() {
                if wait == WaitKind::Last {
                    self.complete();
                }
                self.current_block += 1;
            }
        }
        if self.voice_playing {
            if !voice.is_playing() {
                self.voice_playing = false;
            } else if settings.voice_volume == 0 {
                self.stop_voice(voice);
            }
        }
        if self.wait.is_some() {
            if self.wait_delay > 0.0 {
                self.wait_delay += ticks * -SECONDS_PER_TICK;
            }
            if autoplay && allow_input {
                if self.wait_delay <= 0.0 && !self.voice_playing {
                    self.voice_delay += ticks * -SECONDS_PER_TICK;
                    if self.voice_delay <= 0.0 {
                        if self.wait == Some(WaitKind::Last) {
                            self.complete();
                        }
                        self.wait = None;
                        self.current_block += 1;
                    }
                }
            } else if self.voice_playing {
                self.voice_delay = 0.5;
            }
        }
        if self.wait.is_none() {
            while self.current_block < self.blocks.len() {
                let index = self.current_block;
                let block = &self.blocks[index];
                if block.time > self.current_time {
                    break;
                }
                match &block.kind {
                    BlockKind::Voice { duration, .. } => {
                        if self.voice_playing {
                            break;
                        }
                        if !skip {
                            self.play_voice(index, 0, *duration, settings, voice);
                        }
                        self.voice_block = index;
                        self.voice_counter = self.voice_counter.wrapping_add(1);
                    }
                    BlockKind::Wait { delay, last, auto } => {
                        if self.height != self.height_target
                            || self
                                .characters
                                .iter()
                                .any(|c| c.progress < 1.0 && c.block <= index)
                        {
                            break;
                        }
                        if skip {
                            if *last {
                                self.complete();
                            }
                        } else if *auto || !allow_input {
                            if voice.is_playing() {
                                break;
                            }
                        } else {
                            self.wait = Some(if *last {
                                WaitKind::Last
                            } else {
                                WaitKind::Regular
                            });
                            self.ticks_since_wait = 0.0;
                            self.wait_delay =
                                *delay * ((100 - i32::from(settings.skip_speed)) as f32 * 0.01);
                            break;
                        }
                    }
                    BlockKind::Section(s) => self.sections = *s,
                    BlockKind::Sync(s) => {
                        if (self.syncs as i32) <= *s as i32 {
                            break;
                        }
                    }
                    BlockKind::VoiceSync { start, duration } => {
                        if !skip && self.voice_counter >= 0 {
                            self.play_voice(self.voice_block, *start, *duration, settings, voice);
                        }
                    }
                    BlockKind::VoiceWait => {
                        if self.voice_playing {
                            break;
                        }
                    }
                }
                self.current_block += 1;
                self.input_blocked = false;
            }
        }
        for c in &mut self.characters {
            if c.block > self.current_block || c.time > self.current_time {
                continue;
            }
            if c.progress == 0.0 {
                self.input_blocked = false;
            }
            c.progress = (c.progress + c.rate * ticks).min(1.0);
            self.lines[c.line].visible = true;
            self.height_target = self.height_target.max(self.lines[c.line].bottom);
            let cursor = c.position + vec2(c.width, 0.0);
            if self.cursor.y == cursor.y {
                self.cursor.x = self.cursor.x.max(cursor.x);
            } else if self.cursor.y < cursor.y {
                self.cursor = cursor;
                self.current_line = c.line;
            }
        }
        if self.height < self.height_target {
            self.height = (self.height + ticks * 18.0).min(self.height_target);
        } else if self.height > self.height_target {
            self.height = (self.height + ticks * -18.0).max(self.height_target);
        }
        if let Some(block) = self.blocks.get(self.current_block) {
            self.current_time = (self.current_time + ticks * SECONDS_PER_TICK).min(block.time);
        }
        self.ticks_since_wait += ticks;
    }

    pub fn fast_forward(&mut self, voice: &mut impl MessageVoice) {
        if self.hidden {
            return;
        }
        self.modal_slide = 1.0;
        self.natural_slide = if self.shown { 1.0 } else { 0.0 };
        self.outgoing.clear();
        while self.current_block < self.blocks.len() {
            let block = &self.blocks[self.current_block];
            self.current_time = block.time;
            match &block.kind {
                BlockKind::Voice { .. } => {
                    self.voice_block = self.current_block;
                    self.voice_counter = self.voice_counter.wrapping_add(1);
                }
                BlockKind::Wait { last: true, .. } => self.complete(),
                BlockKind::Section(s) => self.sections = *s,
                BlockKind::Sync(s) if (self.syncs as i32) <= *s as i32 => break,
                _ => {}
            }
            self.current_block += 1;
            self.input_blocked = false;
        }
        for c in &mut self.characters {
            if c.block <= self.current_block && c.time <= self.current_time {
                c.progress = 1.0;
                self.lines[c.line].visible = true;
                self.height_target = self.height_target.max(self.lines[c.line].bottom);
            }
        }
        self.stop_voice(voice);
        self.height = self.height_target;
        self.wait = None;
    }

    pub fn click(&mut self, settings: MessageSettings, voice: &mut impl MessageVoice) -> bool {
        if self.hidden
            || self.modal_slide < 1.0
            || !self.shown
            || self.current_block >= self.blocks.len()
            || self.flags & 2 != 0
        {
            return false;
        }
        if self.natural_slide < 1.0 {
            return true;
        }
        let mut changed = false;
        for c in &mut self.characters {
            if c.block <= self.current_block && c.progress < 1.0 {
                c.progress = 1.0;
                changed = true;
            }
        }
        if changed {
            if let Some(block) = self.blocks.get(self.current_block) {
                self.current_time = block.time;
            }
            return true;
        }
        if let Some(wait) = self.wait.take() {
            if wait == WaitKind::Regular || !settings.continue_voice_on_click {
                self.stop_voice(voice);
            }
            if wait == WaitKind::Last {
                self.complete();
            }
            self.current_block += 1;
        } else if self.voice_playing {
            let next = &self.blocks[self.current_block].kind;
            match next {
                BlockKind::Voice { .. } | BlockKind::VoiceWait => self.stop_voice(voice),
                BlockKind::VoiceSync { start, duration } => {
                    let (start, duration) = (*start, *duration);
                    self.play_voice(self.voice_block, start, duration, settings, voice);
                    self.current_block += 1;
                }
                _ => {}
            }
        }
        true
    }

    #[cfg(test)]
    fn snapshot(&self) -> Vec<u32> {
        let mut words = vec![
            u32::from(self.shown),
            u32::from(self.hidden),
            self.natural_slide.to_bits(),
            self.modal_slide.to_bits(),
            self.current_block as u32,
            self.current_time.to_bits(),
            self.height.to_bits(),
            self.height_target.to_bits(),
            self.wait
                .map_or(u32::MAX, |w| if w == WaitKind::Last { 1 } else { 0 }),
            self.wait_delay.to_bits(),
            self.voice_delay.to_bits(),
            u32::from(self.voice_playing),
            u32::from(self.voice_disabled),
            self.sections,
            self.syncs,
            self.ticks_since_wait.to_bits(),
            self.cursor.x.to_bits(),
            self.cursor.y.to_bits(),
            self.current_line as u32,
            self.total_voices,
            self.voice_block as u32,
            self.voice_counter as u32,
            u32::from(self.input_blocked),
            self.flags,
            self.style,
            self.alignment,
            self.options[0] as u32,
            self.options[1] as u32,
            self.argument as u32,
            self.message_id,
            self.outgoing.len() as u32,
        ];
        for w in &self.outgoing {
            words.extend([w.style, w.slide.to_bits(), w.height.to_bits()]);
        }
        words.push(self.characters.len() as u32);
        words.extend(self.characters.iter().map(|c| c.progress.to_bits()));
        words.push(self.lines.len() as u32);
        words.extend(self.lines.iter().map(|l| u32::from(l.visible)));
        words.extend([
            u32::from(self.busy()),
            u32::from(self.interested_in_input()),
        ]);
        words
    }
}

#[cfg(test)]
mod tests;
