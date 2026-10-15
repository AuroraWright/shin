mod blocks;
mod interpolators;
mod layout;
mod messagebox;
pub(crate) mod outline;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::reference_authored_truth_color_render;
#[cfg(test)]
pub(crate) use tests::reference_live_frame_guard;
#[cfg(test)]
pub(crate) use tests::reference_live_msgclose;
#[cfg(test)]
pub(crate) use tests::reference_live_msgset;
#[cfg(test)]
pub(crate) use tests::reference_live_msgsync;
#[cfg(test)]
pub(crate) use tests::reference_saved_sentence_state;

use std::sync::Arc;

use bitflags::bitflags;
use glam::{Mat4, Vec2, vec2, vec3, vec4};
use interpolators::{Countdown, HeightInterpolator, SlideInterpolator, SlideInterpolatorDirection};
use itertools::{Either, Itertools};
use shin_core::{
    format::{
        save::Settings,
        scenario::{Scenario, instruction_elements::MessageId},
    },
    layout::{
        LayoutParams, MessageLayerLayouter, MessageTextLayouterDefaults,
        commands::{CharFontType, Command},
    },
    primitives::color::FloatColor4,
    time::Ticks,
    vm::command::types::{AudioWaitStatus, MessageTextLayout, MessageboxType, Volume},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{
        buffer::{OwnedVertexBuffer, VertexSource},
        vertices::TextVertex,
    },
};
use tracing::debug;

use crate::{
    adv::assets::AdvFonts,
    asset::{font::GpuFontLazy, texture_archive::TextureArchive},
    audio::{VoicePlayFlags, VoicePlayer},
    layer::{
        DrawableLayer, Layer,
        message_layer::{
            blocks::{Block, BlockType},
            messagebox::Messagebox,
        },
        properties::LayerProperties,
        render_params::TransformParams,
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
};

#[derive(TextureArchive)]
pub struct MessageboxTextures {
    #[txa(name = "keywait")]
    pub keywait: GpuTexture,
    #[txa(name = "select")]
    pub select: GpuTexture,
    #[txa(name = "select_cur")]
    pub select_cursor: GpuTexture,

    #[txa(name = "msgwnd1")]
    pub message_window_1: GpuTexture,
    #[txa(name = "msgwnd2")]
    pub message_window_2: GpuTexture,
    #[txa(name = "msgwnd3")]
    pub message_window_3: GpuTexture,
}

bitflags! {
    #[derive(Copy, Clone, Debug, Default)]
    pub struct MessageFlags: u32 {
        const READ = 0x1;
        const IGNORE_INPUT = 0x2;
    }
}

/// Messagebox, but it has an interpolator for sliding out
///
/// This is used to store additional fake messageboxes for properly animating messagebox type changes
#[derive(Debug, Copy, Clone)]
struct SlidingOutMessagebox {
    pub ty: MessageboxType,
    pub slide_out: SlideInterpolator,
    pub height: f32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum WaitKind {
    Regular,
    Last,
    AutoClick,
}

#[derive(Debug, Copy, Clone)]
pub struct MsgsetParams {
    pub flags: MessageFlags,
    pub messagebox_type: MessageboxType,
    pub text_layout: MessageTextLayout,
    pub message_id: MessageId,
}

const VERTICES_PER_CHARACTER: usize = 4;

fn voice_sync_waiting(
    total_voices: u32,
    current: i32,
    requested: i32,
    status: AudioWaitStatus,
    position_millis: u64,
    sync_time: i32,
) -> bool {
    // Native b19b0: the message's parsed voice count gates the command.
    // Unequal indices compare signed values even without an active player.
    if total_voices == 0 {
        return false;
    }
    if current != requested {
        return current < requested;
    }
    // 57ec sign-extends the target; b1a4c then compares it unsigned.
    status.contains(AudioWaitStatus::PLAYING) && (i64::from(sync_time) as u64) > position_millis
}

pub struct MessageLayer {
    update_tracker: shin_core::primitives::update::UpdateTracker,
    messagebox_textures: Arc<MessageboxTextures>,
    props: LayerProperties,
    voice_player: VoicePlayer,
    adv_fonts: AdvFonts,
    // TODO: maybe split it into smaller structs to reduce complexity somewhat
    /// Slide that is happening as a result of showing/hiding the messagebox with commands
    natural_slide: SlideInterpolator,
    /// Slide that is happening as a result of opening a modal window
    modal_slide: SlideInterpolator,

    autoplay_requested: bool,
    // NB: there is another `_requested`-like field in the original engine, but it's never set, so don't bother
    scenario: Option<Arc<Scenario>>,

    current_block_index: usize,
    current_time: f32,

    height: HeightInterpolator,

    wait_kind: Option<WaitKind>,
    time_to_skip_wait: Countdown,
    autoplay_voice_delay: Countdown,
    is_voice_playing: bool,
    disable_voice: bool,
    completed_sections: u32,
    received_syncs: u32,
    ticks_since_last_wait: Ticks,

    cursor_position: Vec2,

    current_line_index: usize,
    total_voices_count: u32,
    voice_block_index: usize,
    voice_counter: i32,
    // NB: "some_unused_mode"
    //
    message_flags: MessageFlags,
    settings: Settings,

    messagebox_type: MessageboxType,
    text_layout: MessageTextLayout,
    message_id: MessageId,
    completed_messages: Vec<MessageId>,
    chars: Vec<layout::Char>,
    lines: Vec<layout::LineInfo>,
    blocks: Vec<Block>,

    char_name_width: f32,
    message_size: Vec2,

    vertex_buffer: Option<OwnedVertexBuffer<TextVertex>>,
    sliding_out_messageboxes: Vec<SlidingOutMessagebox>,
    transform: Mat4,
    // font_atlas: Arc<FontAtlas>,
    // message: Option<Message>,
    // messagebox: Messagebox,
}

impl MessageLayer {
    pub fn voice_player(&self) -> &VoicePlayer {
        &self.voice_player
    }

    pub fn voice_player_mut(&mut self) -> &mut VoicePlayer {
        &mut self.voice_player
    }

    pub fn new(
        adv_fonts: AdvFonts,
        messagebox_textures: Arc<MessageboxTextures>,
        voice_player: VoicePlayer,
    ) -> Self {
        Self {
            update_tracker: shin_core::primitives::update::UpdateTracker::new(),
            messagebox_textures,
            props: LayerProperties::new(),
            voice_player,
            adv_fonts,

            natural_slide: SlideInterpolator::new(0.0, SlideInterpolatorDirection::Decreasing),
            modal_slide: SlideInterpolator::new(1.0, SlideInterpolatorDirection::Increasing),

            autoplay_requested: false,
            scenario: None,
            current_block_index: 0,
            current_time: 0.0,
            height: HeightInterpolator::new(357.0),
            wait_kind: None,
            time_to_skip_wait: Countdown::new(0.0),
            autoplay_voice_delay: Countdown::new(0.0),
            is_voice_playing: false,
            disable_voice: false,
            completed_sections: 0,
            received_syncs: 0,
            ticks_since_last_wait: Ticks::ZERO,
            cursor_position: Vec2::ZERO,
            current_line_index: 0,
            total_voices_count: 0,
            voice_block_index: 0,
            voice_counter: 0,
            message_flags: MessageFlags::default(),
            settings: Settings::default(),
            messagebox_type: MessageboxType::Neutral,
            text_layout: MessageTextLayout::Justify,
            message_id: MessageId(0),
            completed_messages: Vec::new(),
            chars: vec![],
            lines: vec![],
            blocks: vec![],
            char_name_width: 0.0,
            message_size: Vec2::ZERO,
            vertex_buffer: None,
            sliding_out_messageboxes: vec![],
            transform: Mat4::from_translation(vec3(-960.0, -540.0, 0.0)),
        }
    }

    fn reset_message(&mut self) {
        self.vertex_buffer = None;
        self.blocks.clear();
        self.lines.clear();
        self.chars.clear();
        self.total_voices_count = 0;
    }

    fn rebuild_vertices(&mut self, ctx: &PreRenderContext) {
        self.vertex_buffer = None;

        let mut vertices = Vec::with_capacity(VERTICES_PER_CHARACTER * self.chars.len());

        for char in &self.chars {
            let glyph = char.glyph.info();

            let scaled_size = char.scale() * glyph.actual_size_f32();

            // this adds an overdraw of 2 pixels on all sides of the character
            let pos_to_top_left = char.scale() * glyph.bearing_screenspace_f32() - 2.0;
            let pos_to_bottom_right = pos_to_top_left + scaled_size + 4.0;

            let [screen_left, screen_top] = (char.position + pos_to_top_left).to_array();
            let [screen_right, screen_bottom] = (char.position + pos_to_bottom_right).to_array();

            let tex_overdraw_ratio =
                ((pos_to_bottom_right - pos_to_top_left) / scaled_size - 1.0) / 2.0;

            let [tex_left, tex_top] =
                (-glyph.actual_size_normalized() * tex_overdraw_ratio).to_array();
            let [tex_right, tex_bottom] =
                (glyph.actual_size_normalized() * (tex_overdraw_ratio + 1.0)).to_array();

            let (color_top, color_bottom) = if char.is_rubi {
                (0.0, 0.0)
            } else {
                let line = &self.lines[char.line_index];
                // I don't think the formulas in the original engine are right (it doesn't take scale into account),
                // but umineko doesn't rely on this feature (both tints are set to the same color),
                // so I ain't gonna fix it

                // NB: this bearing is NOT in screenspace, so we need to flip the Y
                let effective_ascent = line.baseline_ascent - glyph.bearing_y as f32;
                let effective_descent = effective_ascent + glyph.actual_height as f32;

                (
                    effective_ascent / line.line_height,
                    effective_descent / line.line_height,
                )
            };

            let char_vertices: [_; VERTICES_PER_CHARACTER] = [
                TextVertex {
                    position: vec4(screen_left, screen_top, tex_left, tex_top),
                    color: color_top,
                },
                TextVertex {
                    position: vec4(screen_right, screen_top, tex_right, tex_top),
                    color: color_top,
                },
                TextVertex {
                    position: vec4(screen_left, screen_bottom, tex_left, tex_bottom),
                    color: color_bottom,
                },
                TextVertex {
                    position: vec4(screen_right, screen_bottom, tex_right, tex_bottom),
                    color: color_bottom,
                },
            ];

            vertices.extend(char_vertices);
        }

        self.vertex_buffer = Some(OwnedVertexBuffer::allocate_vertex(
            ctx.device,
            &vertices,
            Some("MessageLayer/vtxbuf"),
        ));
    }

    #[cfg(test)]
    pub(crate) fn reference_modal_clock(&self) -> (f32, f32) {
        (self.modal_slide.value(), self.current_time)
    }
    #[cfg(test)]
    pub(crate) fn reference_update_ticks(&self) -> Ticks {
        self.ticks_since_last_wait
    }
    pub fn set_modal_hidden(&mut self, hidden: bool) {
        self.modal_slide
            .set_direction(SlideInterpolatorDirection::from_is_increasing(!hidden));
    }

    pub fn is_interested_in_input(&self) -> bool {
        // messagebox is in the process of being hidden or is not fully shown yet
        // TODO: can this be a function on SimpleInterpolator?
        if self.natural_slide.direction() == SlideInterpolatorDirection::Decreasing {
            return false;
        }
        if self.natural_slide.value() < 1.0 {
            return false;
        }

        // All the blocks that we can wait were already executed
        if self.current_block_index >= self.blocks.len() {
            return false;
        }

        if self.message_flags.contains(MessageFlags::IGNORE_INPUT) {
            return false;
        }
        if self.messagebox_type == MessageboxType::NoText
            && self.wait_kind == Some(WaitKind::AutoClick)
        {
            return false;
        }

        true
    }

    fn play_voice(&mut self, voice_index: usize, segment_start: u32, segment_duration: u32) {
        // NB: original game passes the actual voice block reference in here,
        // but it's annoying to do with borrow checker, as we need &mut for VoicePlayer::play
        let Block {
            ty: BlockType::Voice(voice),
            ..
        } = &self.blocks[voice_index]
        else {
            panic!(
                "Expected a voice block, but got {:?}",
                self.blocks[voice_index].ty
            )
        };
        let voicevol = self.settings.v2_voicevol;
        if self.disable_voice == true || voicevol == 0 {
            return;
        }

        let flags = if voice.lipsync_enabled {
            VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC | VoicePlayFlags::ENABLE_CHARACTER_MUTING
        } else {
            VoicePlayFlags::ENABLE_CHARACTER_MUTING
        };

        debug!(
            "Playing voice: {:?} (segment_start: {}, segment_duration: {})",
            voice.filename, segment_start, segment_duration
        );

        self.is_voice_playing = self.voice_player.play(
            self.scenario.as_ref().unwrap(),
            &voice.filename,
            segment_start,
            segment_duration,
            flags,
            voice.volume,
        );

        self.autoplay_voice_delay
            .set_time_left(if self.is_voice_playing { 0.5 } else { 0.0 });
    }

    pub fn on_msgset(
        &mut self,
        ctx: &PreRenderContext,
        scenario: &Arc<Scenario>,
        // NB: the original engine accepted a number here,
        // but it's always set to -1 when parsing MSGSET and the (clamped) number is never used later, so unimplemented
        message: &str,
        params: MsgsetParams,
        dont_ff_slide: bool,
    ) {
        // if the user can currently see a messagebox of a different type, take care to slide it out before (visibly) changing the type
        if params.messagebox_type != self.messagebox_type && self.natural_slide.value() > 0.0 {
            self.sliding_out_messageboxes.push(SlidingOutMessagebox {
                ty: self.messagebox_type,
                // it should always be sliding out
                slide_out: SlideInterpolator::new(
                    self.natural_slide.value(),
                    SlideInterpolatorDirection::Decreasing,
                ),
                height: self.height.value(),
            });
            self.natural_slide.set_value(0.0);
        }
        self.reset_message();

        self.message_flags = params.flags;
        self.messagebox_type = params.messagebox_type;
        self.text_layout = params.text_layout;
        self.message_id = params.message_id;

        self.scenario = Some(scenario.clone());

        self.natural_slide
            .set_direction(SlideInterpolatorDirection::Increasing);

        self.current_block_index = 0;
        self.current_time = 0.0;

        self.wait_kind = None;
        self.time_to_skip_wait.set_time_left(0.0);
        self.autoplay_voice_delay.set_time_left(0.0);
        self.is_voice_playing = false;
        self.disable_voice = false;

        self.completed_sections = 0;
        self.received_syncs = 0;
        self.ticks_since_last_wait = Ticks::ZERO;

        self.cursor_position = Vec2::ZERO;

        self.current_line_index = 0;
        self.total_voices_count = 0;
        self.voice_block_index = 0;
        self.voice_counter = -1;

        if self.messagebox_type == MessageboxType::Novel {
            self.height.set_target(1080.0);
            self.height.set_value(1080.0);
        } else {
            if self.natural_slide.value() == 0.0 {
                self.height.set_value(357.0);
            }
            self.height.set_target(357.0);
        }

        self.set_message(ctx, message);
        self.rebuild_vertices(ctx);

        if !dont_ff_slide {
            self.natural_slide.set_value(1.0);
        }
    }

    /// Restore already displayed text without playing its voice again. Retain
    /// the final key wait so loading a message boundary cannot advance the VM.
    pub fn restore_message(
        &mut self,
        ctx: &PreRenderContext,
        scenario: &Arc<Scenario>,
        message: &str,
        params: MsgsetParams,
        received_syncs: u32,
    ) {
        self.on_msgset(ctx, scenario, message, params, false);
        self.received_syncs = received_syncs;
        let completed_count = self.completed_messages.len();
        self.fast_forward();
        self.completed_messages.truncate(completed_count);
        self.disable_voice = true;
        if self.current_block_index == self.blocks.len()
            && !params.flags.contains(MessageFlags::IGNORE_INPUT)
        {
            if let Some(index) = self.blocks.iter().rposition(|block| {
                matches!(&block.ty, BlockType::Wait(wait) if wait.is_last_wait && !wait.is_auto_click)
            }) {
                self.current_block_index = index;
                self.wait_kind = Some(WaitKind::Last);
                self.ticks_since_last_wait = Ticks::ZERO;
            }
        }
    }

    fn set_message(&mut self, ctx: &PreRenderContext, message: &str) {
        let layout_params = LayoutParams {
            layout_width: 1500.0,
            text_alignment: self.text_layout,
            line_padding_above: 0.0,
            line_padding_below: 0.0,
            line_padding_between: 4.0,
            rubi_size: 20.0,
            text_size: 50.0,
            base_font_horizontal_scale: 0.9697,
            follow_kinsoku_shori_rules: true,
            always_leave_space_for_rubi: true, // < I am not sure if this should be true
            perform_soft_breaks: true,
        };
        let defaults = MessageTextLayouterDefaults {
            color: 999,
            draw_speed: if self.messagebox_type == MessageboxType::NoText {
                100
            } else {
                i32::from(self.settings.v9_msgspeed)
            },
            fade: 200,
        };

        let (commands, lines, size) = MessageLayerLayouter::<&GpuFontLazy>::new(
            self.adv_fonts.medium_font.as_ref(),
            self.adv_fonts.bold_font.as_ref(),
            self.messagebox_type,
            layout_params,
            defaults,
        )
        .parse(message);

        let (regular_chars, bold_chars) = commands
            .iter()
            .filter_map(|c| {
                if let Command::Char(c) = c {
                    Some((c.codepoint, c.font))
                } else {
                    None
                }
            })
            .partition_map::<Vec<_>, Vec<_>, _, _, _>(|(c, font)| match font {
                CharFontType::Regular => Either::Left(c),
                CharFontType::Bold => Either::Right(c),
            });
        let mut regular_glyphs = self
            .adv_fonts
            .medium_font
            .clone()
            .load_glyphs(ctx.device.clone(), ctx.queue.clone(), &regular_chars)
            .into_iter();
        let mut bold_glyphs = self
            .adv_fonts
            .bold_font
            .clone()
            .load_glyphs(ctx.device.clone(), ctx.queue.clone(), &bold_chars)
            .into_iter();

        let mut wait_auto_delay = 0.0;
        for command in commands {
            match command {
                Command::Char(char) => {
                    let glyph = match char.font {
                        // rely on the order of the glyphs returned from `load_glyphs`
                        CharFontType::Regular => regular_glyphs.next().unwrap(),
                        CharFontType::Bold => bold_glyphs.next().unwrap(),
                    };
                    let info = glyph.info();

                    let border_distances = outline::distances(
                        vec2(char.horizontal_scale, char.scale),
                        info.actual_size_f32(),
                        info.actual_size_normalized(),
                        self.settings.v8,
                    );

                    if !char.is_rubi {
                        wait_auto_delay += 0.05;
                    }

                    self.chars.push(layout::Char {
                        time: char.time,
                        line_index: char.line_index,
                        is_rubi: char.is_rubi,
                        position: char.position,
                        width: char.width,
                        height: char.height,
                        horizontal_scale: char.horizontal_scale,
                        vertical_scale: char.scale,
                        color_rgba: char.color,
                        progress_rate: {
                            if char.fade == 0.0 {
                                1.0
                            } else {
                                1.0 / char.fade / 60.0
                            }
                        },
                        current_progress: 0.0,
                        block_index: self.blocks.len(),
                        vertex_buffer_offset: self.chars.len() * VERTICES_PER_CHARACTER,
                        border_distances,
                        glyph,
                    });
                }
                Command::Section(section) => self.blocks.push(Block {
                    time: section.time,
                    ty: BlockType::Section(blocks::Section {
                        index: section.index,
                    }),
                }),
                Command::Sync(sync) => self.blocks.push(Block {
                    time: sync.time,
                    ty: BlockType::Sync(blocks::Sync { index: sync.index }),
                }),
                Command::Voice(voice) => {
                    self.total_voices_count += 1;
                    self.blocks.push(Block {
                        time: voice.time,
                        ty: BlockType::Voice(blocks::Voice {
                            filename: voice.filename,
                            volume: Volume(voice.volume),
                            lipsync_enabled: voice.lipsync_enabled,
                            segment_duration: voice.segment_duration,
                        }),
                    });
                }
                Command::VoiceSync(voice_sync) => self.blocks.push(Block {
                    time: voice_sync.time,
                    ty: BlockType::VoiceSync(blocks::VoiceSync {
                        segment_start: voice_sync.segment_start,
                        segment_duration: voice_sync.segment_duration,
                    }),
                }),
                Command::VoiceWait(voice_wait) => self.blocks.push(Block {
                    time: voice_wait.time,
                    ty: BlockType::VoiceWait(blocks::VoiceWait),
                }),
                Command::Wait(wait) => {
                    self.blocks.push(Block {
                        time: wait.time,
                        ty: BlockType::Wait(blocks::Wait {
                            wait_auto_delay,
                            is_last_wait: wait.is_last_wait,
                            is_auto_click: wait.is_auto_click,
                        }),
                    });

                    wait_auto_delay = 0.0;
                }
            }
        }

        for line in &lines {
            self.lines.push(layout::LineInfo {
                y_position: line.y_position,
                baseline_ascent: line.baseline_ascent,
                line_height: line.line_height,
                rubi_height: line.rubi_height,
                is_visible: 0.0,
            });
        }

        self.char_name_width = lines[0].width;
        self.message_size = size;
    }

    pub fn close(&mut self, dont_ff_slide: bool) {
        self.natural_slide
            .set_direction(SlideInterpolatorDirection::Decreasing);
        if !dont_ff_slide {
            self.natural_slide.set_value(0.0);
        }
        self.reset_message();
    }

    pub fn close_is_waiting(&self) -> bool {
        // Native b1a60..b1acc checks the natural slide and every retained
        // outgoing window. The independent modal slide is not a close clock.
        self.natural_slide.value() > 0.0
            || self
                .sliding_out_messageboxes
                .iter()
                .any(|window| window.slide_out.value() > 0.0)
    }

    pub fn set_settings(&mut self, settings: &Settings) {
        self.settings = settings.clone();
    }

    #[cfg(test)]
    pub(crate) fn final_wait_time(&self) -> f32 {
        self.blocks
            .iter()
            .find_map(|block| match &block.ty {
                BlockType::Wait(wait) if wait.is_last_wait => Some(block.time),
                _ => None,
            })
            .expect("Message has no final wait")
    }

    pub fn take_completed_messages(&mut self) -> Vec<MessageId> {
        std::mem::take(&mut self.completed_messages)
    }

    pub fn request_autoplay(&mut self) {
        self.autoplay_requested = true;
    }

    pub fn try_advance(&mut self) -> bool {
        if !self
            .modal_slide
            .is_fully_at(SlideInterpolatorDirection::Increasing)
            || !self
                .natural_slide
                .is_fully_at(SlideInterpolatorDirection::Increasing)
            || self.current_block_index >= self.blocks.len()
            || self.message_flags.contains(MessageFlags::IGNORE_INPUT)
        {
            // we should not accept any input in this case
            return false;
        }

        // try to fast-forward any non-complete chars in this section
        let any_char_ff = self.chars.iter_mut().fold(false, |acc, char| {
            if char.block_index > self.current_block_index {
                return acc;
            }

            if char.current_progress < 1.0 {
                char.current_progress = 1.0;
                return true;
            }

            acc
        });

        if any_char_ff {
            if self.current_block_index < self.blocks.len() {
                self.current_time = self.blocks[self.current_block_index].time;
            }
            return true;
        }

        // no chars needed to be fast-forwarded, try to advance wait
        if let Some(wait_kind) = self.wait_kind {
            match wait_kind {
                WaitKind::Regular => {
                    self.voice_player.stop();
                    self.is_voice_playing = false;
                    self.autoplay_voice_delay.set_time_left(0.0);
                }
                WaitKind::Last | WaitKind::AutoClick => {
                    if !self.settings.v6 {
                        self.voice_player.stop();
                        self.is_voice_playing = false;
                        self.autoplay_voice_delay.set_time_left(0.0);
                    }
                    self.completed_messages.push(self.message_id);
                }
            }

            self.wait_kind = None;
            self.current_block_index += 1;
            return true;
        }

        // if all else fails - try to stop/advance the voice
        if self.current_block_index >= self.blocks.len() {
            return true;
        }
        match &self.blocks[self.current_block_index].ty {
            BlockType::Voice(_) | BlockType::VoiceWait(_) => {
                // just stop the voice so the wait can begin
                self.voice_player.stop();
                self.is_voice_playing = false;
                self.autoplay_voice_delay.set_time_left(0.0);
            }
            BlockType::VoiceSync(voice_sync) => {
                // start playing next voice segment
                //
                // NB: we do not check if `self.voice_block_index` actually points to a voice block
                // surely it'll be fine (and we'll just crash)
                self.play_voice(
                    self.voice_block_index,
                    voice_sync.segment_start,
                    voice_sync.segment_duration,
                );
                self.current_block_index += 1;
            }

            BlockType::Wait(_) | BlockType::Section(_) | BlockType::Sync(_) => {}
        }

        true
    }

    pub fn voice_sync_is_waiting(&self, voice_index: i32, sync_time: i32) -> bool {
        voice_sync_waiting(
            self.total_voices_count,
            self.voice_counter,
            voice_index,
            self.voice_player.get_wait_status(),
            self.voice_player.position_millis(),
            sync_time,
        )
    }

    pub fn recv_sync_is_waiting(&self, signal_index: i32) -> bool {
        // all blocks are done, so nothing should be waiting
        if self.current_block_index >= self.blocks.len() {
            return false;
        }
        // negative `section_index` -> wait for the full message
        if signal_index < 0 {
            return true;
        }
        let signal_index = signal_index as u32;

        self.completed_sections <= signal_index
    }

    pub fn send_sync(&mut self) {
        self.received_syncs += 1;
    }
}

impl AdvUpdatable for MessageLayer {
    fn update(&mut self, context: &AdvUpdateContext) {
        // Native virtual +10 dispatches through 6aa40 before b2090. Front and
        // back roots can own the same message, so guard the entire body before
        // consuming input requests, advancing voices or moving message clocks.
        if !self.update_tracker.update(context.frame_id) {
            return;
        }
        self.voice_player.update(context.delta_ticks);
        let dt = context.delta_ticks;

        let autoplay_requested = self.autoplay_requested;
        self.autoplay_requested = false;

        self.props.update(context);

        // if we are (semi)-transparent, don't update anything else
        // this is used for pausing
        if self.modal_slide.update(dt) < 1.0 {
            return;
        }

        self.natural_slide.update(dt);

        self.sliding_out_messageboxes.retain_mut(|messagebox| {
            // NB: technically not the same as the original game (it doesn't clamp and checks < 0.0),
            // but it's not a big difference
            messagebox.slide_out.update(dt) > 0.0
        });

        if self.natural_slide.value() < 1.0 {
            return;
        }

        if self.is_voice_playing {
            if self
                .voice_player
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
            {
                if self.settings.v2_voicevol == 0 {
                    self.voice_player.stop();
                    self.is_voice_playing = false;
                    self.autoplay_voice_delay.set_time_left(0.0);
                }
            } else {
                // voice player no longer playing, auto mode can proceed
                self.is_voice_playing = false;
            }
        }

        // TODO: this condition structure is kinda ugly...
        if let Some(wait_kind) = self.wait_kind {
            self.time_to_skip_wait.update(dt);

            let autoplay_effective = wait_kind == WaitKind::AutoClick
                || autoplay_requested && !self.message_flags.contains(MessageFlags::IGNORE_INPUT);

            if !autoplay_effective {
                if !self.is_voice_playing {
                    self.autoplay_voice_delay.set_time_left(0.5);
                }
            } else if self.time_to_skip_wait.is_done()
                && self.is_voice_playing == false
                && self.autoplay_voice_delay.update(dt)
            {
                if matches!(wait_kind, WaitKind::Last | WaitKind::AutoClick) {
                    self.completed_messages.push(self.message_id);
                }
                self.wait_kind = None;
                self.current_block_index += 1;
            }
        };

        if self.wait_kind.is_none() {
            // using a while loop, as for is annoying for borrow checker reasons:
            // we want to call &mut self methods while iterating
            while self.current_block_index < self.blocks.len() {
                let block = &self.blocks[self.current_block_index];
                if block.time > self.current_time {
                    break;
                }
                match &block.ty {
                    BlockType::Voice(voice) => {
                        if self.is_voice_playing {
                            break;
                        }
                        // NB: the original game checks !unknown_requested here, but it's never requested
                        // this is probably an older way to do fast forwarding
                        self.play_voice(self.current_block_index, 0, voice.segment_duration);
                        self.voice_block_index = self.current_block_index;
                        self.voice_counter += 1;
                    }
                    BlockType::Wait(wait) => {
                        if self.height.is_interpolating() {
                            break;
                        }

                        let has_incomplete_char = self.chars.iter().any(|c| {
                            // char is not complete
                            c.current_progress < 1.0
                                    // and is in our block or earlier
                                    && c.block_index <= self.current_block_index
                        });
                        if has_incomplete_char || self.wait_kind.is_some() {
                            break;
                        }

                        if !self.message_flags.contains(MessageFlags::IGNORE_INPUT) {
                            self.wait_kind = Some(match (wait.is_last_wait, wait.is_auto_click) {
                                (_, true) => WaitKind::AutoClick,
                                (true, _) => WaitKind::Last,
                                _ => WaitKind::Regular,
                            });

                            self.ticks_since_last_wait = Ticks::ZERO;
                            let skip_speed = i32::from(self.settings.v10_skipspeed);
                            self.time_to_skip_wait.set_time_left(
                                wait.wait_auto_delay * (100 - skip_speed) as f32 * 0.01,
                            );
                            debug!("Waiting on block {}", self.current_block_index);
                            break;
                        }
                        if self
                            .voice_player
                            .get_wait_status()
                            .contains(AudioWaitStatus::PLAYING)
                        {
                            break;
                        }
                        if wait.is_last_wait {
                            self.completed_messages.push(self.message_id);
                        }
                    }
                    BlockType::Section(section) => {
                        self.completed_sections = section.index;
                    }
                    BlockType::Sync(sync) => {
                        if self.received_syncs <= sync.index {
                            break;
                        }
                    }
                    BlockType::VoiceSync(voice_sync) => {
                        self.play_voice(
                            self.voice_block_index,
                            voice_sync.segment_start,
                            voice_sync.segment_duration,
                        );
                    }
                    BlockType::VoiceWait(_) => {
                        if self.is_voice_playing {
                            break;
                        }
                    }
                }
                self.current_block_index += 1;
                debug!("Advanced to block {}", self.current_block_index);
            }
        }

        // surely nobody would need more than 64 lines, right?
        // NB: the original game uses an std::vector<bool> here, but I don't wanna
        let mut line_mask = 0u64;

        for char in &mut self.chars {
            if char.block_index > self.current_block_index || char.time > self.current_time {
                continue;
            }

            char.current_progress =
                1.0f32.min(char.current_progress + char.progress_rate * dt.as_f32());

            let line = &self.lines[char.line_index];
            self.height
                .set_min_target(char.position.y + line.line_height);

            let candidate_cursor_position = char.position + vec2(char.width, 0.0);
            if self.cursor_position.y == candidate_cursor_position.y {
                if self.cursor_position.x < candidate_cursor_position.x {
                    self.cursor_position = candidate_cursor_position;
                }
            } else if self.cursor_position.y < candidate_cursor_position.y {
                self.cursor_position = candidate_cursor_position;
            }

            assert!(char.line_index < 64);
            line_mask |= 1 << char.line_index as u64;
        }

        for (i, line) in (0..).zip(&mut self.lines) {
            if line_mask & (1 << i as u64) == 0 {
                continue;
            }

            line.is_visible = 1.0;
        }

        self.height.update(dt);

        if self.current_block_index < self.blocks.len() {
            let block_time = self.blocks[self.current_block_index].time;

            self.current_time = block_time.min(self.current_time + dt.as_seconds());
        }

        self.ticks_since_last_wait += dt;
    }
}

impl Layer for MessageLayer {
    fn fast_forward(&mut self) {
        if self.modal_slide.direction() != SlideInterpolatorDirection::Increasing {
            return;
        }
        self.natural_slide.fast_forward();
        self.sliding_out_messageboxes.clear();

        while self.current_block_index < self.blocks.len() {
            let block = &self.blocks[self.current_block_index];
            self.current_time = block.time;
            match &block.ty {
                BlockType::Voice(_) => {
                    self.voice_block_index = self.current_block_index;
                    self.voice_counter += 1;
                }
                BlockType::Wait(wait) => {
                    if wait.is_last_wait {
                        self.completed_messages.push(self.message_id);
                    }
                }
                BlockType::Section(section) => {
                    self.completed_sections = section.index;
                }
                BlockType::Sync(sync) => {
                    if self.received_syncs <= sync.index {
                        break;
                    }
                }
                BlockType::VoiceSync(_) | BlockType::VoiceWait(_) => {}
            }
            self.current_block_index += 1;
            debug!("[ff] Advanced to block {}", self.current_block_index);
        }

        // surely nobody would need more than 64 lines, right?
        // NB: the original game uses an std::vector<bool> here, but I don't wanna
        let mut line_mask = 0u64;

        for char in &mut self.chars {
            if char.block_index > self.current_block_index || char.time > self.current_time {
                continue;
            }

            char.current_progress = 1.0;

            let line = &self.lines[char.line_index];
            self.height
                .set_min_target(char.position.y + line.line_height);

            assert!(char.line_index < 64);
            line_mask |= 1 << char.line_index as u64;
        }

        for (i, line) in (0..).zip(&mut self.lines) {
            if line_mask & (1 << i as u64) == 0 {
                continue;
            }

            line.is_visible = 1.0;
        }

        self.voice_player.stop();

        self.height.fast_forward();
        self.is_voice_playing = false;
        self.autoplay_voice_delay.set_time_left(0.0);
        self.wait_kind = None;
    }

    fn get_stencil_bump(&self) -> u8 {
        3
    }

    #[tracing::instrument(skip_all)]
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if pass_kind != PassKind::Transparent {
            return;
        }

        pass.push_debug("MessageLayer");

        let transform = transform.compute_final_transform() * self.transform;

        pass.push_debug("MessageLayer/messageboxes");

        let builder = RenderRequestBuilder::new().depth_stencil_shorthand(stencil_ref, true, false);
        for &messagebox in &self.sliding_out_messageboxes {
            self.render_messagebox(pass, builder, transform, messagebox.into());
        }
        self.render_messagebox(
            pass,
            builder,
            transform,
            Messagebox {
                ty: self.messagebox_type,
                natural_slide: self.natural_slide.value(),
                height: self.height.value(),
            },
        );
        pass.pop_debug();

        // if the messagebox is not fully shown - don't try to render the message and keywait
        if self.natural_slide.value() * self.modal_slide.value() < 1.0 {
            pass.pop_debug();
            return;
        }

        // no vertex buffer = no message and keywait
        let Some(vertex_buffer) = &self.vertex_buffer else {
            pass.pop_debug();
            return;
        };

        let position_y = match self.messagebox_type {
            MessageboxType::Neutral
            | MessageboxType::WitchSpace
            | MessageboxType::Ushiromiya
            | MessageboxType::Transparent
            | MessageboxType::NoText => {
                (1.0 - self.natural_slide.value()) * 64.0 + (1080.0 - self.height.value()) - 32.0
            }

            MessageboxType::Novel => 32.0f32.max((1080.0 - self.message_size.y) * 0.35),
        };

        // render the keywait
        match self.wait_kind {
            Some(wait_kind @ (WaitKind::Regular | WaitKind::Last)) => {
                pass.push_debug("MessageLayer/keywait");

                let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
                let texture = self.messagebox_textures.keywait.as_source();

                let atlas_pos_x = match wait_kind {
                    WaitKind::Regular => 0.0,
                    WaitKind::Last => 0.5,
                    WaitKind::AutoClick => unreachable!(),
                };

                let alpha_wiggle =
                    (self.ticks_since_last_wait.as_seconds() * std::f32::consts::PI).sin();

                let position = match self.messagebox_type {
                    MessageboxType::Neutral
                    | MessageboxType::WitchSpace
                    | MessageboxType::Ushiromiya
                    | MessageboxType::Transparent
                    | MessageboxType::Novel => {
                        self.cursor_position + vec2(210.0, position_y - 30.0)
                    }
                    MessageboxType::NoText => vec2(1870.0, 1030.0),
                };

                let transform = transform * Mat4::from_translation(position.extend(0.0));

                QuadVertices::new()
                    .with_box(0.0, 0.0, 30.0, 30.0)
                    .with_tex_box(atlas_pos_x, 0.0, atlas_pos_x + 0.5, 1.0)
                    .with_color(
                        FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha_wiggle * 0.4 + 0.6)
                            .into_unorm(),
                    )
                    .render_sprite(pass, builder, texture, transform);

                pass.pop_debug();
            }
            Some(WaitKind::AutoClick) | None => {
                // do not render the keywait
            }
        }

        if self.messagebox_type == MessageboxType::NoText {
            pass.pop_debug();
            return;
        }

        // render the text

        let transform = transform * Mat4::from_translation(vec3(210.0, position_y, 0.0));

        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil_shorthand(stencil_ref + 2, true, true);

        pass.push_debug("MessageLayer/text");
        pass.push_debug("MessageLayer/text/border");
        // draw the borders...
        for char in &self.chars {
            let Ok(glyph_texture) = char.glyph.as_texture_source() else {
                continue;
            };
            pass.run(builder.build(
                RenderProgramWithArguments::FontBorder {
                    vertices: VertexSource::VertexBuffer {
                        vertices: vertex_buffer.as_sliced_buffer_ref(
                            char.vertex_buffer_offset,
                            VERTICES_PER_CHARACTER,
                        ),
                    },
                    glyph: glyph_texture,
                    transform,
                    distances: char.border_distances,
                    color: FloatColor4::from_rgba(0.0, 0.0, 0.0, char.current_progress),
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
        pass.pop_debug();

        pass.push_debug("MessageLayer/text/normal");
        // and the characters themselves
        for char in &self.chars {
            let Ok(glyph_texture) = char.glyph.as_texture_source() else {
                continue;
            };
            pass.run(builder.build(
                RenderProgramWithArguments::Font {
                    vertices: VertexSource::VertexBuffer {
                        vertices: vertex_buffer.as_sliced_buffer_ref(
                            char.vertex_buffer_offset,
                            VERTICES_PER_CHARACTER,
                        ),
                    },
                    glyph: glyph_texture,
                    transform,
                    color1:
                        FloatColor4::from_unorm(char.color_rgba).with_alpha(char.current_progress),
                    color2:
                        FloatColor4::from_unorm(char.color_rgba).with_alpha(char.current_progress),
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
        pass.pop_debug();

        pass.pop_debug(); // MessageLayer/text
        pass.pop_debug(); // MessageLayer
    }
}

impl DrawableLayer for MessageLayer {
    fn properties(&self) -> &LayerProperties {
        &self.props
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        &mut self.props
    }
}

#[cfg(test)]
impl MessageLayer {
    pub(crate) fn assert_reference_outline(
        &mut self,
        context: &mut PreRenderContext,
        scenario: &Arc<Scenario>,
    ) {
        use shin_core::primitives::color::UnormColor;
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Message outline reference"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut frames = Vec::new();
        for mode in 0..4 {
            let mut settings = Settings::default();
            settings.v8 = mode;
            self.set_settings(&settings);
            self.on_msgset(
                context,
                scenario,
                "右代宮　金蔵　ABC",
                MsgsetParams {
                    flags: MessageFlags::empty(),
                    messagebox_type: MessageboxType::Neutral,
                    text_layout: MessageTextLayout::Justify,
                    message_id: MessageId(0),
                },
                false,
            );
            assert!(self.chars.len() > 5);
            for char in &mut self.chars {
                char.current_progress = 1.0;
            }
            self.rebuild_vertices(context);
            let buffer = self.vertex_buffer.as_ref().unwrap();
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Native message outline",
                );
                pass.clear(
                    Some(UnormColor::from_rgba(128, 128, 128, 255)),
                    Some(0),
                    Some(1.0),
                );
                let builder = RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::Layer1)
                    .depth_stencil_shorthand(1, true, true);
                let transform = crate::render::top_left_projection_matrix()
                    * Mat4::from_translation(vec3(128.0, 128.0, 0.0));
                for char in &self.chars {
                    let Ok(glyph_texture) = char.glyph.as_texture_source() else {
                        continue;
                    };
                    pass.run(builder.build(
                        RenderProgramWithArguments::FontBorder {
                            vertices: VertexSource::VertexBuffer {
                                vertices: buffer.as_sliced_buffer_ref(
                                    char.vertex_buffer_offset,
                                    VERTICES_PER_CHARACTER,
                                ),
                            },
                            glyph: glyph_texture,
                            transform,
                            distances: char.border_distances,
                            color: FloatColor4::from_rgba(0.0, 0.0, 0.0, 1.0),
                        },
                        DrawPrimitive::TrianglesStrip,
                    ));
                }
                for char in &self.chars {
                    let Ok(glyph_texture) = char.glyph.as_texture_source() else {
                        continue;
                    };
                    pass.run(builder.build(
                        RenderProgramWithArguments::Font {
                            vertices: VertexSource::VertexBuffer {
                                vertices: buffer.as_sliced_buffer_ref(
                                    char.vertex_buffer_offset,
                                    VERTICES_PER_CHARACTER,
                                ),
                            },
                            glyph: glyph_texture,
                            transform,
                            color1: FloatColor4::WHITE,
                            color2: FloatColor4::WHITE,
                        },
                        DrawPrimitive::TrianglesStrip,
                    ));
                }
            }
            frames.push(crate::layer::user::readback_test_texture(context, &target));
        }
        assert_eq!(frames[0], frames[2], "Onlymode1 uses the shifted outline");
        assert_eq!(frames[0], frames[3]);
        let changed = frames[0]
            .chunks_exact(4)
            .zip(frames[1].chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 100,
            "Saved outline mode must alter visible glyph border pixels: {changed}"
        );
        let fill = |pixels: &[u8]| {
            pixels
                .chunks_exact(4)
                .filter(|pixel| pixel[..3].iter().all(|&channel| channel > 250))
                .count()
        };
        assert!(fill(&frames[0]) > 100);
        assert_eq!(
            fill(&frames[0]),
            fill(&frames[1]),
            "Glyph fill remains intact while the border changes"
        );
        println!(
            "Native saved message outline:{} real font glyphs,mode1 changes{changed} pixels;mode0/2/3 retain identical geometry",
            self.chars.len()
        );
    }
}
