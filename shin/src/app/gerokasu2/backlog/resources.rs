//! Atlas/font ownership, measured history and native row packet submission.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::text::Text;
use crate::{
    adv::{
        characters::view::CharacterTextures,
        gerokasu2::backlog::{
            self,
            history::{Geometry, History},
            view::{self, Packet, Presentation, ScrollRange},
        },
    },
    asset::{font::gerokasu2::GpuGerokasu2Font, system::AssetServer},
};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec2, Vec3, Vec4};
use shin_core::{
    format::{
        save::gerokasu2::{GameData, SaveData},
        scenario::gerokasu2::Gerokasu2Scenario,
    },
    primitives::color::UnormColor,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

#[derive(Default)]
pub(in crate::app::gerokasu2) struct Options {
    #[cfg(test)]
    pub gate: Option<Arc<super::measurement::Gate>>,
}

pub(in crate::app::gerokasu2) struct Resources {
    pub history: Arc<History>,
    pub geometry: Geometry,
    assets: Arc<AssetServer>,
    pub font: Arc<GpuGerokasu2Font>,
    atlas: Arc<CharacterTextures>,
    system: Arc<CharacterTextures>,
    text: Vec<Option<Text>>,
    visible: Vec<usize>,
    measurement: super::measurement::Worker,
}

#[cfg(test)]
mod tests;
impl Resources {
    pub fn game(&self) -> crate::cli::Game {
        self.assets.context().game
    }
    pub fn refresh_geometry(&mut self, controller: &mut backlog::Controller) -> Result<()> {
        if let Some(geometry) = self.measurement.snapshot()? {
            controller
                .update_geometry(&geometry)
                .map_err(anyhow::Error::msg)?;
            self.geometry = geometry;
        }
        Ok(())
    }
    pub fn cancel_measurement(&mut self) {
        self.measurement.cancel();
    }
    pub fn resume_measurement(&mut self) {
        self.measurement.resume();
    }
    pub fn retire_measurement(&mut self) {
        self.measurement.cancel_and_wait();
    }
    #[cfg(test)]
    pub fn measurement_reference(&self) -> (usize, bool, bool) {
        self.measurement.reference()
    }
    pub fn cached_rows(&self) -> &[usize] {
        &self.visible
    }
    #[cfg(test)]
    pub async fn load(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        source: GameData,
        globals: SaveData,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Option<Self>> {
        Self::load_with_options(
            assets,
            scenario,
            source,
            globals,
            cancelled,
            Options::default(),
        )
        .await
    }
    pub async fn load_with_options(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        source: GameData,
        globals: SaveData,
        cancelled: Arc<AtomicBool>,
        options: Options,
    ) -> Result<Option<Self>> {
        let worker_cancelled = cancelled.clone();
        let history = shin_tasks::compute::spawn(move || {
            History::collect(scenario, source, &globals, 10_000_000, || {
                worker_cancelled.load(Ordering::Acquire)
            })
        })
        .await?;
        ensure!(
            !cancelled.load(Ordering::Acquire),
            "Ger backlog preparation cancelled"
        );
        Self::from_history_with_options(assets, history, cancelled, options).await
    }
    #[cfg(test)]
    pub async fn from_history(
        assets: Arc<AssetServer>,
        history: History,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Option<Self>> {
        Self::from_history_with_options(assets, history, cancelled, Options::default()).await
    }
    async fn from_history_with_options(
        assets: Arc<AssetServer>,
        history: History,
        cancelled: Arc<AtomicBool>,
        _options: Options,
    ) -> Result<Option<Self>> {
        if history.entries.is_empty() {
            return Ok(None);
        }
        let (atlas, system, font) = futures::try_join!(
            assets.load::<CharacterTextures, _>(backlog::ATLAS),
            assets.load::<CharacterTextures, _>("/systex.txa"),
            assets.load::<GpuGerokasu2Font, _>(
                crate::adv::gerokasu2::message_ui::medium_font_path(assets.context().game),
            ),
        )?;
        atlas.require_names(
            backlog::ATLAS,
            backlog::view::portraits(assets.context().game),
        )?;
        atlas.require_names(
            backlog::ATLAS,
            &[
                "bg",
                "window_focus",
                "window_normal",
                "top",
                "caption",
                "button_shadow",
                "close",
                "help_logview",
                "key",
            ],
        )?;
        if assets.context().game == crate::cli::Game::Gerokasu {
            atlas.require_names(backlog::ATLAS, &["bottom"])?;
        }
        system.require_names("/systex.txa", &["scroll"])?;
        let history = Arc::new(history);
        let worker_history = history.clone();
        let metrics = font.metrics();
        let mut measurement = super::measurement::Worker::start(
            &history,
            cancelled,
            Arc::new(move |row| {
                let layout =
                    worker_history.entries[row].layout(metrics.clone(), metrics.clone(), 999);
                Ok((layout.size.y, layout.voice_count))
            }),
            #[cfg(test)]
            _options.gate,
        )
        .await?;
        let geometry = measurement
            .snapshot()?
            .ok_or_else(|| anyhow::anyhow!("Ger measurement published no rows"))?;
        let text = std::iter::repeat_with(|| None)
            .take(history.entries.len())
            .collect();
        Ok(Some(Self {
            history,
            geometry,
            assets,
            font,
            atlas,
            system,
            text,
            visible: vec![],
            measurement,
        }))
    }
    pub fn rows(&self, flashes: &[f32]) -> Vec<view::Row> {
        self.history
            .entries
            .iter()
            .zip(&self.geometry.rows)
            .enumerate()
            .map(|(index, (entry, row))| view::Row {
                index,
                top: row.top,
                height: row.height,
                kind: entry.kind(),
                voice_count: row.voice_count,
                message_arg: entry.message_arg(),
                flash: flashes.get(index).copied().unwrap_or(0.),
            })
            .collect()
    }
    pub fn controller(&self) -> Result<backlog::Controller> {
        let g = &self.geometry;
        backlog::Controller::new(
            g.rows
                .iter()
                .map(|r| backlog::Row {
                    marker: r.marker,
                    voiced: r.voice_count != 0,
                    top: r.top,
                    height: r.height,
                })
                .collect(),
            g.selected,
            g.allowed.clone(),
            g.initial_scroll,
            g.minimum,
            g.maximum,
        )
        .map_err(anyhow::Error::msg)
    }
    pub fn jump_data(&self, row: usize) -> Result<GameData> {
        let entry = self
            .history
            .entries
            .get(row)
            .ok_or_else(|| anyhow::anyhow!("Ger backlog jump row is absent"))?;
        let mut game = self.history.jump_source.clone();
        let active = game.progress.active_entry_mut();
        // This is an in-memory native payload: disk serialization deliberately
        // normalizes these positions and would corrupt a backlog jump.
        let (saved, replay) =
            backlog::jump_positions(true, 0, active.save_position, entry.boundary).unwrap();
        active.save_position = saved;
        active.replay_target = replay;
        Ok(game)
    }
    pub fn prepare(&mut self, scroll: f32) -> Result<()> {
        //80F98 builds one row before upper_bound(scroll-160), retaining native
        // overscan beyond the central832px viewport.887C0 prunes the cache.
        // Retained display owners keep insertion order when scrolling reverses.
        let g = &self.geometry;
        let rows = self.rows(&[]);
        let start = view::cache_start(&rows, g.allowed.clone(), scroll);
        for index in start..g.allowed.end {
            if !view::should_create(&rows[index], scroll) {
                break;
            }
            if self.text[index].is_none() {
                let entry = &self.history.entries[index];
                let layout = entry.layout(
                    self.font.metrics(),
                    self.font.metrics(),
                    view::row_color(entry.kind()),
                );
                self.text[index] = Some(Text::prepare(
                    &self.assets.context().wgpu_device,
                    &self.assets.context().wgpu_queue,
                    &self.font,
                    layout.commands,
                    &layout.lines,
                )?);
                self.visible.push(index);
            }
        }
        self.visible.retain(|&index| {
            let keep = view::retain(&rows[index], scroll);
            if !keep {
                self.text[index] = None;
            }
            keep
        });
        Ok(())
    }
    pub fn render_rows(
        &self,
        pass: &mut RenderPass,
        view: &Presentation,
        flashes: &[f32],
    ) -> Result<()> {
        self.packet(pass, &view::background(view.progress));
        let rows = self.rows(flashes);
        for &index in &self.visible {
            let row = view::row_packets(view, rows[index]);
            for p in &row.frame {
                self.packet(pass, p);
            }
            if let Some(text) = self.text[index].as_ref() {
                text.render(pass, row.text_matrix)?;
            }
            if let Some(portrait) = row.portrait.as_ref() {
                self.packet(pass, portrait);
            }
        }
        let range =
            ScrollRange::from_document(view.scroll, self.geometry.minimum, self.geometry.bottom);
        for strip in view::scrollbar(view.progress, range) {
            let vertices = strip.vertices.map(|v| PosColTexVertex {
                position: Vec3::from_array(v.position),
                color: UnormColor::WHITE,
                texture_position: Vec2::from_array(v.uv),
            });
            pass.run(
                RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::Layer1)
                    .build(
                        RenderProgramWithArguments::Sprite {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            sprite: self.system.get("scroll").as_source(),
                            transform: Mat4::from_cols_array(&strip.matrix),
                        },
                        DrawPrimitive::TrianglesStrip,
                    ),
            );
        }
        Ok(())
    }
    pub fn render_chrome(&self, pass: &mut RenderPass, view: &Presentation) {
        let selected = &self.geometry.rows[view.selected];
        for packet in view::chrome(view, !selected.marker && selected.voice_count != 0) {
            self.packet(pass, &packet);
        }
    }
    fn packet(&self, pass: &mut RenderPass, p: &Packet) {
        let c = p.color.to_le_bytes();
        let quad = QuadVertices::new()
            .with_box(p.rect[0], p.rect[1], p.rect[2], p.rect[3])
            .with_tex_box(p.uv[0], p.uv[1], p.uv[2], p.uv[3])
            .with_color(UnormColor::from_rgba(c[2], c[1], c[0], c[3]));
        let builder = RenderRequestBuilder::new().color_blend_type(match p.blend {
            1 => ColorBlendType::Opaque,
            3 => ColorBlendType::Layer2,
            _ => ColorBlendType::Layer1,
        });
        let matrix = Mat4::from_cols_array(&p.matrix);
        let texture = self.atlas.get(p.texture).as_source();
        if p.flash > 0. {
            quad.render_button(
                pass,
                builder,
                texture,
                matrix,
                Vec4::ZERO,
                Vec4::new(1., 1., 1., p.flash),
            );
        } else {
            quad.render_sprite(pass, builder, texture, matrix);
        }
    }
}
