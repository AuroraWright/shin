//! Application resources and startup for the older engine dialect. SceneSlot
//! owns its concrete ADV replacement; the title/progress adapter follows here.
pub(super) mod cg_gallery;
pub(super) mod chart;
pub(super) mod config;
pub(super) mod first_boot;
mod gallery_view;
pub(super) mod gameplay;
pub(super) mod input_bridge;
pub(super) mod kakera_menu;
pub(super) mod load_game;
pub(super) mod main_title;
pub(super) mod manual;
pub(super) mod movie_gallery;
pub(super) mod movie_player;
pub(super) mod music_gallery;
#[cfg(test)]
mod native_shader_replay;
pub(super) mod persistence;
pub(super) mod quiz_scene;
mod random;
pub(super) mod runtime;
pub(super) mod runtime_game_data;
pub(super) mod runtime_saves;
pub(super) mod save_menu;
pub(super) mod scenario_select;
pub(super) mod scene_host;
pub(super) mod shutdown;
pub(super) mod startup;
pub(super) mod tips_menu;
pub(super) mod title_demo;
pub(super) mod window_app;
#[cfg(feature = "higurashi-window-probe")]
pub(super) mod window_probe;
