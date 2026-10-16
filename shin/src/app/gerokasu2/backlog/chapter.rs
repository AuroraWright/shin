//! Native ChapterSelector85710 GPU host; parent owns cues, voice and dialogs.
use std::sync::Arc;

use anyhow::{Context, Result};
use glam::{Mat4, Vec2, Vec3, Vec4};
use shin_core::{
    layout::{LayoutParams, gerokasu2::generic_layouter},
    primitives::color::UnormColor,
};
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
    RenderRequestBuilder, StencilFunction, StencilOperation, StencilPipelineState, StencilState,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

use crate::{
    adv::{
        character_view::CharacterTextures,
        gerokasu2::backlog::{
            self,
            chapter::{
                Controller, Event, Input,
                artwork::{self, Marker, Quad},
            },
            history::{History, Item},
        },
        input_area::{InputArea, InputAreaTarget, PointerSampler, desktop_gesture_vector},
    },
    app::gerokasu2::load_menu::text::Text,
    asset::{font::gerokasu2::GpuGerokasu2Font, system::AssetServer},
};

#[cfg(test)]
mod tests;

struct Label {
    text: Text,
    /// D4980 includes the three-pixel border on each side in anchor extent.
    extent: Vec2,
}
enum MarkerText {
    Chapter(String),
    Choice { title: String, text: String },
    Other,
}
impl MarkerText {
    fn borrowed(&self) -> Marker<'_> {
        match self {
            Self::Chapter(title) => Marker::Chapter(title),
            Self::Choice { title, text } => Marker::Choice { title, text },
            Self::Other => Marker::Other,
        }
    }
}
impl Label {
    fn prepare(assets: &AssetServer, font: &Arc<GpuGerokasu2Font>, label: &str) -> Result<Self> {
        let (commands, lines, extent) =
            generic_layouter(font.metrics(), layout_params()).parse(label);
        Ok(Self {
            text: Text::prepare(
                &assets.context().wgpu_device,
                &assets.context().wgpu_queue,
                font,
                commands,
                lines,
                artwork::FONT_SIZE,
            )?,
            extent: extent + Vec2::splat(6.),
        })
    }
}

/// Immutable descriptor strings are cheap; GPU text owners are created only
/// while visible and retained in native insertion order until they leave view.
pub(in crate::app::gerokasu2) struct ChapterHost {
    model: Controller,
    assets: Arc<AssetServer>,
    atlas: Arc<CharacterTextures>,
    font: Arc<GpuGerokasu2Font>,
    markers: Vec<MarkerText>,
    labels: Vec<String>,
    text: Vec<Option<Label>>,
    visible: Vec<usize>,
    first: usize,
    last: usize,
    pointer: PointerSampler,
    area: InputArea,
}
impl ChapterHost {
    pub fn load(
        assets: Arc<AssetServer>,
        history: &History,
        selected: usize,
        current: usize,
    ) -> Result<Self> {
        let model = Controller::new(history.entries.len(), &history.chapters, selected, current)
            .map_err(anyhow::Error::msg)?;
        let atlas = assets.load_sync::<CharacterTextures>(backlog::ATLAS)?;
        atlas.require_names(backlog::ATLAS, &["chapter"])?;
        // Native bootstrap5C6D4..5C728 publishes this distinct GenericText font.
        let font = assets.load_sync::<GpuGerokasu2Font>("/system.fnt")?;
        let markers = history
            .entries
            .iter()
            .map(|entry| match &entry.item {
                Item::Choice { title, .. } => MarkerText::Chapter(title.clone()),
                Item::Checkpoint { title, text } => MarkerText::Choice {
                    title: title.clone(),
                    text: text.clone(),
                },
                Item::Message { .. } => MarkerText::Other,
            })
            .collect::<Vec<_>>();
        let labels = model
            .entries()
            .iter()
            .map(|entry| {
                let marker = usize::try_from(entry.marker_index)
                    .ok()
                    .and_then(|i| markers.get(i))
                    .map_or(Marker::Other, MarkerText::borrowed);
                // Chapter and choice titles are scenario text; the fixed
                // position and fallback markers are code-defined UI labels.
                let authored =
                    entry.kind == 1 && matches!(marker, Marker::Chapter(_) | Marker::Choice { .. });
                let text = artwork::row_text(*entry, marker).0;
                if authored {
                    text
                } else {
                    font.localize(&text).into_owned()
                }
            })
            .collect::<Vec<_>>();
        let text = std::iter::repeat_with(|| None).take(labels.len()).collect();
        let mut pointer = PointerSampler::default();
        // Opening a child under an inherited desktop hold cannot invent Down.
        pointer.cancel_contact(true);
        Ok(Self {
            model,
            assets,
            atlas,
            font,
            markers,
            labels,
            text,
            visible: vec![],
            first: 0,
            last: history.entries.len() - 1,
            pointer,
            area: InputArea::canvas(),
        })
    }
    pub fn controller(&self) -> &Controller {
        &self.model
    }
    pub fn controller_mut(&mut self) -> &mut Controller {
        &mut self.model
    }
    ///857CC takes fresh bounds for drawing, independently of84FC4 input.
    pub fn set_parent_bounds(&mut self, first: usize, last: usize) {
        self.first = first;
        self.last = last;
    }
    /// Parent passes mapped1920x1080 coordinates and elapsed seconds. Native
    /// phase ->62B44 contact -> keyboard -> inertia -> literal1 scroll order
    /// is kept in the pure controller hook. Parent applies returned events
    /// before advancing its own scroll/flash and private voice transport.
    pub fn update(
        &mut self,
        delta: f32,
        input: Input,
        point: Vec2,
        held: bool,
        seconds: f64,
    ) -> Vec<Event> {
        self.set_parent_bounds(input.first_row, input.last_row);
        self.pointer.sample(
            if point.is_finite() {
                point
            } else {
                Vec2::splat(-1.)
            },
            held,
            seconds,
        );
        let area = &mut self.area;
        let sampler = &self.pointer;
        let mut events = self.model.update_with_contact(delta, input, |model| {
            let mut events = vec![];
            let mut contact = Contact {
                model,
                events: &mut events,
            };
            let captured = area.update_with_mouse_release(
                &mut contact,
                &sampler.frame(),
                sampler.mouse_release(),
            );
            (captured, events)
        });
        if events.contains(&Event::DisableInput) {
            self.area.disable(&mut Contact {
                model: &mut self.model,
                events: &mut events,
            });
        }
        events
    }
    /// Contact retirement is a desktop owner boundary. Returned flash events
    /// still belong to the parent; this host never changes its private tweens.
    pub fn cancel_input(&mut self) -> Vec<Event> {
        self.pointer.cancel_contact(true);
        let mut events = vec![];
        self.area.disable(&mut Contact {
            model: &mut self.model,
            events: &mut events,
        });
        self.model.reset();
        events
    }
    pub fn prepare(&mut self) -> Result<()> {
        let frame = self.frame();
        for row in &frame.rows {
            if self.text[row.index].is_none() {
                self.text[row.index] = Some(Label::prepare(
                    &self.assets,
                    &self.font,
                    &self.labels[row.index],
                )?);
                self.visible.push(row.index);
            }
        }
        self.visible.retain(|&index| {
            let keep = frame.rows.iter().any(|row| row.index == index);
            if !keep {
                self.text[index] = None;
            }
            keep
        });
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let frame = self.frame();
        for q in &frame.panel {
            self.packet(pass, q);
        }
        // Original flags4 clears stencil0 and preserves scene depth/color.
        pass.clear(None, Some(0), None);
        self.packet(pass, &frame.clip);
        let builder = builder(artwork::CLIP_TEXT, ColorBlendType::Layer1);
        for &index in &self.visible {
            let Some(row) = frame.rows.iter().find(|row| row.index == index) else {
                continue;
            };
            let label = self.text[index]
                .as_ref()
                .context("Ger chapter visible label is unprepared")?;
            label.text.render_colored(
                pass,
                builder,
                text_transform(
                    frame.text_matrix,
                    row.position,
                    row.anchor,
                    row.scale,
                    label.extent,
                ),
                Vec4::from_array(row.color),
            )?;
        }
        // Each request supplies its own default/Always state, so the clip does
        // not leak into header, borders, scrollbar or the parent's later chrome.
        self.packet(pass, &frame.header);
        for q in &frame.borders {
            self.packet(pass, q);
        }
        if let Some(scrollbar) = &frame.scrollbar {
            for q in scrollbar {
                self.packet(pass, q);
            }
        }
        Ok(())
    }
    fn frame(&self) -> artwork::Frame {
        artwork::frame(
            self.model.progress(),
            self.model.scroll(),
            self.model.entries(),
            self.model.selected(),
            self.first,
            self.last,
            |index| {
                usize::try_from(index)
                    .ok()
                    .and_then(|i| self.markers.get(i))
                    .map_or(Marker::Other, MarkerText::borrowed)
            },
        )
    }
    fn packet(&self, pass: &mut RenderPass, q: &Quad) {
        let builder = builder(
            q.stencil,
            match q.blend {
                0 => ColorBlendType::NoColor,
                1 => ColorBlendType::Opaque,
                3 => ColorBlendType::Layer2,
                _ => ColorBlendType::Layer1,
            },
        );
        let transform = Mat4::from_cols_array(&q.matrix);
        let vertices = vertices(q);
        if let Some(texture) = q.texture {
            pass.run(builder.build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: self.atlas.get(texture).as_source(),
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        } else {
            let vertices = vertices.map(PosColTexVertex::pos_col);
            pass.run(builder.build(
                RenderProgramWithArguments::Fill {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}
fn layout_params() -> LayoutParams {
    LayoutParams {
        layout_width: artwork::TEXT_WIDTH - 6.,
        text_size: artwork::FONT_SIZE,
        rubi_size: 0.,
        perform_soft_breaks: false,
        always_leave_space_for_rubi: false,
        ..Default::default()
    }
}
fn text_transform(
    mut matrix: [f32; 16],
    position: [f32; 2],
    anchor: [f32; 2],
    scale: [f32; 2],
    extent: Vec2,
) -> Mat4 {
    // D4B20..D4BC0: anchor translation, scale, position, then parent. Chapter
    // uses unit scale, but retain the explicit rounded multiply/add sequence.
    let x = (-(extent.x * anchor[0])) * scale[0] + position[0];
    let y = (-(extent.y * anchor[1])) * scale[1] + position[1];
    matrix[12] = matrix[0] * x + matrix[12];
    matrix[13] = matrix[5] * y + matrix[13];
    matrix[0] *= scale[0];
    matrix[5] *= scale[1];
    Mat4::from_cols_array(&matrix)
}
fn vertices(q: &Quad) -> [PosColTexVertex; 4] {
    std::array::from_fn(|i| {
        let color = q.colors[i].to_le_bytes();
        PosColTexVertex {
            position: Vec3::new(
                q.rect[if i & 1 == 0 { 0 } else { 2 }],
                q.rect[if i & 2 == 0 { 1 } else { 3 }],
                0.,
            ),
            color: UnormColor::from_rgba(color[2], color[1], color[0], color[3]),
            texture_position: Vec2::new(
                if i & 1 == 0 { 0. } else { 1. },
                if i & 2 == 0 { 0. } else { 1. },
            ),
        }
    })
}
fn builder(stencil: [u32; 5], blend: ColorBlendType) -> RenderRequestBuilder {
    RenderRequestBuilder::new()
        .color_blend_type(blend)
        .depth_stencil(stencil_state(stencil))
}
fn stencil_state(stencil: [u32; 5]) -> DepthStencilState {
    let pipeline = if stencil == artwork::CLIP_WRITE {
        StencilPipelineState {
            pass_operation: StencilOperation::Increment,
            ..Default::default()
        }
    } else if stencil == artwork::CLIP_TEXT {
        StencilPipelineState {
            function: StencilFunction::Equal,
            ..Default::default()
        }
    } else {
        StencilPipelineState::default()
    };
    DepthStencilState {
        stencil: StencilState {
            pipeline,
            stencil_reference: (stencil[4] >> 16) as u8,
        },
        ..Default::default()
    }
}
struct Contact<'a> {
    model: &'a mut Controller,
    events: &'a mut Vec<Event>,
}
impl InputAreaTarget for Contact<'_> {
    fn probe(&mut self, p: Vec2) -> bool {
        p.is_finite()
            && p.x >= 0.
            && p.y >= 0.
            && p.x < 1920.
            && p.y < 1080.
            && self.model.probe(p.x, p.y)
    }
    fn reset(&mut self) {
        self.model.reset();
    }
    fn press(&mut self, _: Vec2) {
        self.events.extend(self.model.press());
    }
    fn release(&mut self, _: Vec2) {
        self.events.extend(self.model.release());
    }
    fn long_press(&mut self, _: Vec2) {} // original86920
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        self.events
            .extend(self.model.drag(previous.to_array(), current.to_array()));
    }
    fn gesture(&mut self, velocity: Vec2) {
        self.events.extend(self.model.fling(velocity.to_array()));
    }
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        self.gesture(desktop_gesture_vector(displacement, velocity));
    }
    fn cancel(&mut self) {
        self.events.extend(self.model.cancel());
    }
}
