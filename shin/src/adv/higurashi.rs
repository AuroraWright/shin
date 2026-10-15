//! Older ADV components. Higurashi's layer records and selectors do not use
//! Umineko's four-plane command state.
pub(crate) mod adv_cache;
pub mod audio;
pub mod autosave;
pub mod browser_content;
pub mod browser_controls;
pub mod browser_decorations;
pub mod browser_glyphs;
pub mod browser_history;
pub mod browser_lifecycle;
pub mod browser_navigation;
pub mod browser_shell;
pub mod browser_worker;
pub mod character_selection;
pub mod chart_progress;
pub mod checkpoints;
pub mod choice;
pub mod choice_screen;
pub mod descriptors;
pub mod effect_commands;
pub mod effects;
pub mod exit;
pub(crate) mod fake_select;
pub mod forward_search;
pub mod frame;
pub(crate) mod frame_renderer;
pub(crate) mod framebuffer;
pub mod group;
pub mod hidden_window;
pub mod host;
pub mod input;
pub mod kakera;
pub mod scenario_select;
pub mod layers;
pub mod live_reconstruction;
pub mod message_glyphs;
pub mod message_owner;
pub mod message_playback;
pub mod message_ui;
pub mod modal;
pub mod mode_stack;
pub mod music_commands;
pub mod navigation;
pub(crate) mod notification_command;
pub mod progress;
pub mod question;
pub mod quiz;
pub mod read_history;
pub mod read_state;
pub mod reconstruction_mode;
pub mod replay;
pub mod root_ui;
pub mod runner;
pub mod save_game;
pub mod save_menu;
pub mod save_metadata;
pub mod scene;
pub(crate) mod scene_history;
pub mod scene_textures;
pub mod system_menu;
pub(crate) mod temporal;
pub(crate) mod temporary_wrapper;
pub mod tips_get;
pub mod touch;
pub mod ui_state;
pub(crate) mod unlock;
pub mod variables;
pub mod voice;
pub mod voice_commands;
pub mod wipe;

#[cfg(test)]
pub(crate) fn reference_gpu_assets()
-> (wgpu::Device, wgpu::Queue, crate::asset::system::AssetServer) {
    message_glyphs::tests::gpu::gpu_assets()
}
#[cfg(test)]
pub(crate) fn reference_render_to_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    render: impl FnOnce(&mut crate::render::PreRenderContext, &wgpu::TextureView),
) -> Vec<u8> {
    frame::tests::gpu::render_to_texture(device, queue, size, render)
}
