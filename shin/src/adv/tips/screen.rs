//! Native TipsMode asynchronous atlas startup and SystemMenu child lifetime.
use crate::adv::{
    characters::view::CharacterTextures,
    input_area::{InputArea, PointerFrame},
    tips::TipsModel,
    tips::text::{TipsText, layout_tip},
    tips::view,
};
use crate::{
    app::AppAction,
    asset::{font::GpuFontLazy, system::AssetServer},
};
use enum_map::EnumMap;
use shin_core::{
    format::scenario::{Scenario, info::TipsInfoItem},
    time::Ticks,
};
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

type Atlases = (Arc<CharacterTextures>, Arc<CharacterTextures>);
pub struct TipsScreen {
    loading: Option<shin_tasks::AsyncTask<anyhow::Result<Atlases>>>,
    atlases: Option<Atlases>,
    model: TipsModel,
    entries: Vec<TipsInfoItem>,
    text: TipsText,
    text_row: usize,
    area: InputArea,
    server: Arc<AssetServer>,
}

#[cfg(test)]
mod tests {
    use crate::adv::input_area::PointerSampler;
    use super::*;
    use crate::asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache};

    #[test]
    #[ignore = "requires supplied original/1.03 ROM and Metal; no audio or save writes"]
    fn reference_long_authored_tip_keyboard_scroll_uses_live_input_owner() {
        crate::init_test_task_pools();
        let adapter =
            shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
                .unwrap();
        let (device, queue) =
            shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
                    adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
                } else {
                    wgpu::Features::empty()
                },
                ..Default::default()
            }))
            .unwrap();
        let mut io = LayeredAssetIo::new();
        if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
            io.try_with_rom(patch).unwrap();
        }
        io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device,
                wgpu_queue: queue,
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let scenario: Arc<Scenario> = server.load_sync("/main.snr").unwrap();
        let font: Arc<GpuFontLazy> = server.load_sync("/system.fnt").unwrap();
        let (index, item) = scenario
            .info_tables()
            .tips_info
            .iter()
            .enumerate()
            .find(|(_, item)| layout_tip(item, font.as_ref()).height > 910.0)
            .expect("Supplied scenario must contain an authored tip with scrollable text");
        let mut screen = TipsScreen::new(server, &scenario, i32::from(item.episode), None).unwrap();
        screen.set_pointer_extent((1280, 720));
        screen.wait_reference_assets().unwrap();
        let row = screen
            .model
            .rows()
            .iter()
            .position(|row| row.scenario_index == index)
            .unwrap();
        let mut pointer = PointerSampler::default();
        fn input(
            screen: &mut TipsScreen,
            pointer: &mut PointerSampler,
            action: Option<AppAction>,
            scroll_key: Option<winit::keyboard::KeyCode>,
            held: bool,
            repeat: bool,
        ) {
            let mut state = EnumMap::default();
            if let Some(action) = action {
                state[action] = ActionState {
                    is_clicked: !repeat,
                    is_clicked_or_repeated: true,
                    ..Default::default()
                };
            }
            let axis = if let Some(key) = scroll_key {
                let mut raw = shin_input::RawInputState::new();
                raw.keyboard.insert(key);
                crate::app::text_scroll_axis(&raw)
            } else {
                0.0
            };
            pointer.sample(glam::Vec2::new(480.0, 300.0), held, 1.0 / 60.0);
            screen.input_frame(&pointer.frame(), &state, axis);
            assert_eq!(screen.update(Ticks::ZERO).unwrap(), None);
        }
        // Opening is input-gated, including the desktop keys.
        input(
            &mut screen,
            &mut pointer,
            None,
            Some(winit::keyboard::KeyCode::KeyL),
            false,
            false,
        );
        assert_eq!(screen.model.scroll(), 0.0);
        screen.update(Ticks::from_f32(10.0)).unwrap();
        for _ in 0..row {
            input(
                &mut screen,
                &mut pointer,
                Some(AppAction::AnyDown),
                None,
                false,
                false,
            );
        }
        assert_eq!(screen.text_row, row);
        let maximum = screen.model.content_height() - 730.0;
        assert!(maximum > 180.0);
        input(
            &mut screen,
            &mut pointer,
            None,
            Some(winit::keyboard::KeyCode::KeyL),
            false,
            false,
        );
        assert_eq!(
            screen.model.scroll(),
            32.0,
            "L must scroll long authored tips"
        );
        input(
            &mut screen,
            &mut pointer,
            None,
            Some(winit::keyboard::KeyCode::KeyL),
            false,
            true,
        );
        assert_eq!(
            screen.model.scroll(),
            maximum.min(64.0),
            "Keyboard repeat must scroll"
        );
        assert_eq!(
            screen.model.selected(),
            row,
            "K/L scroll text without switching tips"
        );
        assert_eq!(screen.text_row, row);
        for _ in 0..1000 {
            input(
                &mut screen,
                &mut pointer,
                None,
                Some(winit::keyboard::KeyCode::KeyL),
                false,
                true,
            );
        }
        assert_eq!(
            screen.model.scroll(),
            maximum,
            "Clamp at the last text page"
        );
        // The physical body press consumes keys before they reach the model.
        input(
            &mut screen,
            &mut pointer,
            None,
            Some(winit::keyboard::KeyCode::KeyK),
            true,
            false,
        );
        assert_eq!(screen.model.scroll(), maximum);
        input(&mut screen, &mut pointer, None, None, false, false);
        for _ in 0..1000 {
            input(
                &mut screen,
                &mut pointer,
                None,
                Some(winit::keyboard::KeyCode::KeyK),
                false,
                true,
            );
        }
        assert_eq!(screen.model.scroll(), 0.0);
        input(
            &mut screen,
            &mut pointer,
            None,
            Some(winit::keyboard::KeyCode::KeyK),
            false,
            true,
        );
        assert_eq!(screen.model.scroll(), 0.0, "Clamp at the first text page");
        let short = screen
            .model
            .rows()
            .iter()
            .position(|row| {
                layout_tip(
                    &scenario.info_tables().tips_info[row.scenario_index],
                    font.as_ref(),
                )
                .height
                    <= 730.0
            })
            .expect("Supplied episode contains a short tip");
        let action = if short < row {
            AppAction::Up
        } else {
            AppAction::AnyDown
        };
        for _ in 0..short.abs_diff(row) {
            input(&mut screen, &mut pointer, Some(action), None, false, false);
        }
        input(
            &mut screen,
            &mut pointer,
            None,
            Some(winit::keyboard::KeyCode::KeyL),
            false,
            false,
        );
        assert_eq!(
            screen.model.scroll(),
            0.0,
            "Short text must not move beyond its bounds"
        );
        eprintln!(
            "Authored long tip: index={index}, title={}, height={}, short row={short}",
            item.title,
            maximum + 730.0
        );
    }
}
impl TipsScreen {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: &Scenario,
        episode: i32,
        acquired: Option<&[i32]>,
    ) -> anyhow::Result<Self> {
        let items = &scenario.info_tables().tips_info;
        let mut model = TipsModel::new(items, episode, acquired)
            .ok_or_else(|| anyhow::anyhow!("Information screen has no matching tips"))?;
        let entries: Vec<_> = model
            .rows()
            .iter()
            .map(|row| {
                let item = &items[row.scenario_index];
                TipsInfoItem {
                    episode: item.episode,
                    title_index: item.title_index,
                    title: item.title.clone(),
                    content: item.content.clone(),
                }
            })
            .collect();
        let font: Arc<GpuFontLazy> = server.load_sync("/system.fnt")?;
        for (row, item) in entries.iter().enumerate() {
            model.set_row_height(row, layout_tip(item, font.as_ref()).height);
        }
        let text = TipsText::load(&server, &entries[0]);
        let assets = server.clone();
        Ok(Self {
            loading: Some(shin_tasks::async_io::spawn(async move {
                futures::try_join!(assets.load("/tips.txa"), assets.load("/systex.txa"))
            })),
            atlases: None,
            model,
            entries,
            text,
            text_row: 0,
            area: InputArea::canvas(),
            server,
        })
    }
    pub fn model(&self) -> Option<&TipsModel> {
        self.atlases.as_ref().map(|_| &self.model)
    }
    pub fn set_pointer_extent(&mut self, extent: (u16, u16)) {
        self.area.disable(&mut self.model);
        self.area = InputArea::new(extent.0, extent.1);
    }
    fn refresh_text(&mut self) {
        let row = self.model.selected();
        if self.text_row != row {
            self.text = TipsText::load(&self.server, &self.entries[row]);
            self.text_row = row;
        }
    }
    pub fn input_frame(
        &mut self,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axis: f32,
    ) {
        if self.atlases.is_none() {
            return;
        }
        self.model.set_axis(axis);
        self.area.input_frame(&mut self.model, frame, state);
        self.refresh_text();
    }
    /// Resource completion frame begins at progress0. Callback0 retains owner;
    /// callback1 releases the child after the final closing slide.
    pub fn update(&mut self, delta: Ticks) -> anyhow::Result<Option<i32>> {
        if let Some(task) = &mut self.loading {
            if let Some(result) = task.poll_naive() {
                self.loading = None;
                let (tips, system) = result?;
                tips.require_names(
                    "/tips.txa",
                    &[
                        "bg",
                        "obi",
                        "window",
                        "separator",
                        "menu",
                        "items",
                        "bar",
                        "caption",
                        "close",
                        "help_tips",
                        "key",
                    ],
                )?;
                system.require_names("/systex.txa", &["scroll"])?;
                self.atlases = Some((tips, system));
            }
            return Ok(None);
        }
        self.model.update(delta.as_f32());
        self.refresh_text();
        Ok(self.model.take_responses().into_iter().next())
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        self.model.take_sounds()
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if let Some((atlas, system)) = &self.atlases {
            view::render(
                &self.model,
                &self.text,
                atlas,
                system,
                pass,
                crate::render::top_left_projection_matrix(),
            );
        }
    }
    #[cfg(test)]
    pub(crate) fn wait_reference_assets(&mut self) -> anyhow::Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.atlases.is_none() {
            assert_eq!(self.update(Ticks::from_f32(1000.0))?, None);
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "Tips atlas load timed out"
            );
            std::thread::yield_now();
        }
        assert_eq!(self.model.progress(), 0.0);
        Ok(())
    }
}
