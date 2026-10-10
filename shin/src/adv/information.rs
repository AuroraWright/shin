//! Character-grid and acquired-tip state, native VM words0x9118..0x9138.
//! These are route-local fields reconstructed by command replay, not gallery unlock bitsets.
use crate::asset::{system::AssetServer, texture_archive::TextureArchive};
use shin_core::{
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tween, Tweener},
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, gpu_texture::GpuTexture, quad_vertices::QuadVertices,
    render_pass::RenderPass,
};
use std::{collections::VecDeque, sync::Arc};

#[derive(Debug, TextureArchive)]
pub struct NotificationTextures {
    #[txa(name = "notify")]
    pub notify: GpuTexture,
    #[txa(name = "adv")]
    _adv: GpuTexture,
    #[txa(name = "info")]
    _info: GpuTexture,
    #[txa(name = "navi")]
    _navi: GpuTexture,
    #[txa(name = "skip")]
    _skip: GpuTexture,
    #[txa(name = "sysmenu")]
    _sysmenu: GpuTexture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InformationState {
    pub character_grid: i32,
    /// Initial character-screen page; native construction masks this word with3
    /// and searches cyclically for an existing page.
    pub character_grid_argument: i32,
    pub acquired_tips: Vec<i32>,
    pub notification_style: i32,
}
impl Default for InformationState {
    fn default() -> Self {
        // Native value-state constructor0xe698/0xea08 stores zero-extended w21=-1
        // as a64-bit word at0x6368, i.e. VM grid0x9118=-1 and argument0x911c=0.
        Self {
            character_grid: -1,
            character_grid_argument: 0,
            acquired_tips: Vec::new(),
            notification_style: 0,
        }
    }
}
impl InformationState {
    pub fn select_character_grid(&mut self, grid: i32, argument: i32) -> bool {
        let changed = self.character_grid != grid || self.character_grid_argument != argument;
        self.character_grid = grid;
        self.character_grid_argument = argument;
        changed
    }

    pub fn acquire_tips(&mut self, tips: &[i32]) -> bool {
        let mut changed = false;
        for &tip in tips {
            if tip >= 0 && !self.acquired_tips.contains(&tip) {
                self.acquired_tips.push(tip);
                changed = true;
            }
        }
        changed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Characters = 0,
    Tips = 1,
}

/// Native0x148f0: at most one queued notification of each kind, preserving queue order.
/// Queued entries are bytes; the active notification comparison uses the full encoded word.
#[derive(Debug, Clone)]
pub struct NotificationQueue {
    active: Option<u8>,
    queued: VecDeque<u8>,
    slide: Tweener,
    opacity: Tweener,
    textures: Option<Arc<NotificationTextures>>,
}
impl Default for NotificationQueue {
    fn default() -> Self {
        Self {
            active: None,
            queued: VecDeque::new(),
            slide: Tweener::new(0.0),
            opacity: Tweener::new(0.0),
            textures: None,
        }
    }
}
impl NotificationQueue {
    #[cfg(test)]
    pub(super) fn reference_motion(&self) -> (Option<u8>, Vec<u8>, f32, f32, bool) {
        (
            self.active,
            self.queued.iter().copied().collect(),
            self.slide.value(),
            self.opacity.value(),
            self.opacity.is_idle(),
        )
    }
    /// Ordinary native loads discard pending notices but let the active
    /// artwork/slide exit through a ten-tick opacity replacement. The reset
    /// also gates new notices when no toast is currently visible.
    pub(super) fn cancel_for_scene_load(&mut self) {
        self.queued.clear();
        self.opacity
            .enqueue_now(0.0, Tween::linear(Ticks::from_f32(10.0)));
    }

    pub fn enqueue(&mut self, kind: NotificationKind, style: i32) {
        if style < 1 {
            return;
        }
        let code = (style.wrapping_sub(1).wrapping_shl(4)) | kind as i32;
        if self.active.map(i32::from) == Some(code) {
            return;
        }
        if let Some(existing) = self
            .queued
            .iter_mut()
            .find(|v| (**v & 1) == (code as u8 & 1))
        {
            *existing = code as u8;
        } else {
            self.queued.push_back(code as u8);
        }
    }

    /// Returns a newly-started notification for the ADV sound player.
    pub fn update(&mut self, delta: Ticks) -> Option<u8> {
        let started = if self.opacity.is_idle() {
            self.begin_next().map(|code| {
                self.slide = Tweener::new(0.0);
                self.opacity = Tweener::new(0.0);
                self.slide.enqueue(
                    1.0,
                    Tween {
                        duration: Ticks::from_f32(30.0),
                        easing: Easing::SineOut,
                    },
                );
                self.slide
                    .enqueue(1.0, Tween::linear(Ticks::from_f32(30.0)));
                self.slide
                    .enqueue(1.0, Tween::linear(Ticks::from_f32(180.0)));
                for (value, duration) in [(1.0, 30.0), (1.0, 30.0), (1.0, 120.0), (0.0, 60.0)] {
                    self.opacity
                        .enqueue(value, Tween::linear(Ticks::from_f32(duration)));
                }
                code
            })
        } else {
            None
        };
        self.slide.update(delta);
        self.opacity.update(delta);
        started
    }

    pub fn load_textures(&mut self, server: &AssetServer) {
        if self.textures.is_none() {
            self.textures = Some(
                server
                    .load_sync("/adv.txa")
                    .expect("Failed to load ADV notification textures"),
            );
        }
    }

    pub fn render(&self, pass: &mut RenderPass) {
        let (Some(code), Some(textures)) = (self.active, &self.textures) else {
            return;
        };
        if self.opacity.is_idle() {
            return;
        }
        let x = 1380.0 - 100.0 * self.slide.value();
        let top = f32::from(code & 1) * 0.5;
        QuadVertices::new()
            .with_box(x, 100.0, x + 640.0, 164.0)
            .with_tex_box(0.0, top, 1.0, top + 0.5)
            .with_color(FloatColor4::from_rgba(1.0, 1.0, 1.0, self.opacity.value()).into_unorm())
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                textures.notify.as_source(),
                crate::render::top_left_projection_matrix(),
            );
    }

    /// The notification renderer calls this when the previous opacity animation is idle.
    pub fn begin_next(&mut self) -> Option<u8> {
        self.active = self.queued.pop_front();
        self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::{
        format::scenario::{instruction_elements::NumberSpec, types::U8SmallNumberList},
        vm::{IntoRuntimeForm, VmCtx, command},
    };

    #[test]
    fn scene_load_clears_old_queue_fades_current_toast_and_gates_new_notices() {
        let mut original = NotificationQueue::default();
        original.enqueue(NotificationKind::Characters, 1);
        assert_eq!(original.update(Ticks::from_f32(15.0)), Some(0));
        original.enqueue(NotificationKind::Tips, 2);
        let before = original.reference_motion();
        let mut restored = original.clone();
        restored.cancel_for_scene_load();
        assert_eq!(
            original.reference_motion(),
            before,
            "candidate reset must preserve failed-restore source"
        );
        assert!(restored.queued.is_empty());
        assert_eq!(restored.active, Some(0));
        assert_eq!(restored.opacity.value(), 0.5);
        assert_eq!(restored.slide.value(), before.2);
        restored.enqueue(NotificationKind::Characters, 1);
        assert!(
            restored.queued.is_empty(),
            "retained active full code still suppresses duplicates"
        );
        restored.enqueue(NotificationKind::Tips, 1);
        assert_eq!(restored.update(Ticks::from_f32(5.0)), None);
        assert_eq!(restored.opacity.value(), 0.25);
        assert!(
            restored.slide.value() > before.2,
            "slide continues through the exit"
        );
        assert_eq!(restored.update(Ticks::from_f32(5.0)), None);
        assert_eq!(restored.opacity.value(), 0.0);
        assert!(restored.opacity.is_idle());
        assert_eq!(
            restored.update(Ticks::ZERO),
            Some(1),
            "only new post-load notice may start"
        );
        assert_eq!(restored.active, Some(1));
        let mut blank = NotificationQueue::default();
        blank.cancel_for_scene_load();
        blank.enqueue(NotificationKind::Characters, 1);
        assert_eq!(blank.update(Ticks::from_f32(10.0)), None);
        assert_eq!(blank.update(Ticks::ZERO), Some(0));
        // The full request code may differ while its stored byte matches.
        let mut truncated = original.clone();
        truncated.cancel_for_scene_load();
        truncated.enqueue(NotificationKind::Characters, 17);
        assert_eq!(
            truncated.queued.iter().copied().collect::<Vec<_>>(),
            vec![0]
        );
    }

    #[test]
    fn commands_rebuild_grid_tips_and_style_without_persistent_side_effects() {
        let mut state = crate::adv::VmState::new();
        assert_eq!(state.information.character_grid, -1);
        assert_eq!(state.information.character_grid_argument, 0);
        assert_eq!(state.information.notification_style, 0);
        state.persistent_commits_enabled = false;
        let ctx = VmCtx::new(0, 42);
        let chars = |grid, arg| {
            command::CompiletimeCommand::CHARS(command::compiletime::CHARS {
                arg1: NumberSpec::constant(grid),
                arg2: NumberSpec::constant(arg),
            })
            .into_runtime_form(&ctx)
        };
        let notify = |style| {
            command::CompiletimeCommand::NOTIFYSET(command::compiletime::NOTIFYSET {
                arg: NumberSpec::constant(style),
            })
            .into_runtime_form(&ctx)
        };
        for (grid, arg) in [(8, 0), (8, 1), (-1, i32::MAX), (169, 0)] {
            crate::adv::command::apply_command_state(chars(grid, arg), &mut state);
            assert_eq!(
                (
                    state.information.character_grid,
                    state.information.character_grid_argument
                ),
                (grid, arg)
            );
        }
        for style in [1, -1, i32::MAX, 2] {
            crate::adv::command::apply_command_state(notify(style), &mut state);
            assert_eq!(state.information.notification_style, style);
        }
        let tips = command::CompiletimeCommand::TIPSGET(command::compiletime::TIPSGET {
            tip_ids: U8SmallNumberList::from_iter(
                [-1, 4, 2, 4, 0, -7, i32::MAX].map(NumberSpec::constant),
            ),
        })
        .into_runtime_form(&ctx);
        crate::adv::command::apply_command_state(tips, &mut state);
        assert_eq!(state.information.acquired_tips, [4, 2, 0, i32::MAX]);
        assert!(!state.information.acquire_tips(&[-1, 0, 4, 2]));
        assert!(!state.information.select_character_grid(169, 0));
        assert!(state.information.select_character_grid(169, 1));
        assert_eq!(state.save_revision, 0);
        assert_eq!(state.save_position, 0);
    }

    #[test]
    fn notification_queue_replaces_by_kind_and_suppresses_active_exact_code() {
        let mut queue = NotificationQueue::default();
        queue.enqueue(NotificationKind::Characters, 0);
        queue.enqueue(NotificationKind::Tips, -1);
        assert_eq!(queue.begin_next(), None);
        queue.enqueue(NotificationKind::Characters, 1);
        queue.enqueue(NotificationKind::Tips, 1);
        queue.enqueue(NotificationKind::Characters, 2);
        assert_eq!(queue.begin_next(), Some(16));
        queue.enqueue(NotificationKind::Characters, 2);
        queue.enqueue(NotificationKind::Characters, 1);
        assert_eq!(queue.begin_next(), Some(1));
        assert_eq!(queue.begin_next(), Some(0));
        assert_eq!(queue.begin_next(), None);
        // The full word257 is not equal to active byte1 even though it truncates to1 in the queue.
        queue.enqueue(NotificationKind::Tips, 1);
        assert_eq!(queue.begin_next(), Some(1));
        queue.enqueue(NotificationKind::Tips, 17);
        assert_eq!(queue.begin_next(), Some(1));
        queue.enqueue(NotificationKind::Characters, i32::MAX);
        assert_eq!(queue.begin_next(), Some(224));
    }

    #[test]
    fn notification_animation_matches_native_slide_fade_and_queue_timing() {
        let mut queue = NotificationQueue::default();
        queue.enqueue(NotificationKind::Characters, 1);
        queue.enqueue(NotificationKind::Tips, 2);
        assert_eq!(queue.update(Ticks::ZERO), Some(0));
        assert_eq!(queue.slide.value(), 0.0);
        assert_eq!(queue.opacity.value(), 0.0);
        assert_eq!(queue.update(Ticks::from_f32(15.0)), None);
        assert!((queue.slide.value() - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.00001);
        assert_eq!(queue.opacity.value(), 0.5);
        queue.update(Ticks::from_f32(15.0));
        assert_eq!(queue.slide.value(), 1.0);
        assert_eq!(queue.opacity.value(), 1.0);
        queue.update(Ticks::from_f32(150.0));
        assert_eq!(queue.opacity.value(), 1.0);
        queue.update(Ticks::from_f32(30.0));
        assert_eq!(queue.opacity.value(), 0.5);
        queue.update(Ticks::from_f32(30.0));
        assert!(queue.opacity.is_idle());
        assert_eq!(queue.active, Some(0));
        assert_eq!(queue.update(Ticks::ZERO), Some(17));
        assert_eq!(queue.opacity.value(), 0.0);
        queue.update(Ticks::from_f32(300.0));
        assert!(queue.opacity.is_idle());
        assert_eq!(queue.update(Ticks::ZERO), None);
        assert_eq!(queue.active, None);
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_information_commands_match_native_state_and_queue() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, value) in [
            (0x159814, 180.0f32),
            (0x159a18, 120.0),
            (0x159a60, 100.0),
            (0x15959c, 1380.0),
            (0x1598f8, 640.0),
            (0x1596c4, 164.0),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                value.to_bits()
            );
        }
        assert_eq!(&exe[0x15c675 + 256..0x15c675 + 263], b"notify1");
        assert_eq!(&exe[0x1609a5 + 256..0x1609a5 + 263], b"notify2");
        // CHARS state words, unchanged-pair comparison, TIPSGET negative guard,
        // NOTIFYSET style field, and queue full-code/kind/byte operations.
        for (address, opcode) in [
            (0x1ab8c, 0xf900015f),
            (0x1abe0, 0x91262100),
            (0x1abe4, 0x9402820f),
            (0x1abe8, 0x1e2703e0),
            (0x1abec, 0x1e249001),
            (0x1abf0, 0x2a1f03e1),
            (0x1abf4, 0x2a1f03e2),
            (0x1abf8, 0x94028382),
            (0x117a0, 0x91262276),
            (0x117a8, 0x9402a892),
            (0x117ac, 0x37000aa0),
            (0x117b4, 0xb4000a28),
            (0x1182c, 0xb9092a77),
            (0x118f8, 0x12800008),
            (0x118fc, 0xb9092a68),
            (0x1190c, 0x9402a851),
            (0x11918, 0x9402a84e),
            (0xe698, 0x12800015u32),
            (0xea08, 0xf931b675),
            (0xea14, 0xb8296a7f),
            (0x4490, 0x52922308),
            (0x44bc, 0x540000a1),
            (0x44cc, 0x540000c0),
            (0x44d0, 0xb900012b),
            (0x44d8, 0xb8296a6c),
            (0xdbb8, 0x37f802fc),
            (0x4910, 0x5292270b),
            (0x14904, 0x6b01011f),
            (0x14994, 0x4a130210),
            (0x149d0, 0x39000173),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                opcode,
                "native {address:x}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); executes episode with other commands using dummy responses"]
    fn reference_episode_rebuilds_information_from_script() {
        use shin_core::{
            format::{rom::RomReader, scenario::Scenario},
            primitives::stateless_reader::StatelessFile,
            vm::Scripter,
        };
        use std::{fs::File, io::Read};
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let mut rom =
            RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
        let entry = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let mut scripter = Scripter::new(&scenario, 0, 42);
        let mut state = crate::adv::VmState::new();
        state.persistent_commits_enabled = false;
        let mut result = shin_core::vm::command::CommandResult::None;
        let mut counts = [0; 3];
        let mut stopped = false;
        for _ in 0..500_000 {
            let command = scripter.run(result).unwrap();
            result = match command {
                command::RuntimeCommand::CHARS(cmd) => {
                    cmd.apply_state(&mut state);
                    counts[0] += 1;
                    assert!(state.information.character_grid >= 0);
                    assert!(
                        (state.information.character_grid as usize)
                            < scenario.info_tables().chars_grid_info.len()
                    );
                    cmd.token.finish()
                }
                command::RuntimeCommand::TIPSGET(cmd) => {
                    cmd.apply_state(&mut state);
                    counts[1] += 1;
                    cmd.token.finish()
                }
                command::RuntimeCommand::NOTIFYSET(cmd) => {
                    cmd.apply_state(&mut state);
                    counts[2] += 1;
                    cmd.token.finish()
                }
                command::RuntimeCommand::EXIT(cmd) if cmd.arg1 == 0 => {
                    cmd.token.finish();
                    stopped = true;
                    break;
                }
                other => match other.execute_dummy() {
                    Some(r) => r,
                    None => {
                        stopped = true;
                        break;
                    }
                },
            };
        }
        assert!(stopped);
        assert!(counts.iter().all(|&c| c > 0), "counts={counts:?}");
        assert!(state.information.character_grid >= 8);
        assert!(
            state
                .information
                .acquired_tips
                .iter()
                .all(|&t| t >= 0 && (t as usize) < scenario.info_tables().tips_info.len())
        );
        assert_eq!(state.save_revision, 0);
    }
    use crate::adv::StartableCommand;
}
