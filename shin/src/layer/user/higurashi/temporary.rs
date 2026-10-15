//! Live older temporary layers11..14. The private UI clocks are independent
//! of inherited65-property clocks; opening/input owners poll before scene ticks.
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            character_selection::view::View as CharacterView,
            input::{Buttons, OverlayInput},
            message_ui::HigurashiMessageTextures,
        },
    },
    asset::system::AssetServer,
    layer::{
        higurashi_properties::{HigurashiProperties, HigurashiTweener},
        render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
    },
};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use parking_lot::Mutex;
use shin_core::{
    format::scenario::higurashi::HigurashiScenario,
    primitives::color::{FloatColor4, UnormColor},
    time::Ticks,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex},
    },
};
use std::sync::Arc;
#[cfg(test)]
pub(crate) mod wrapper_fixture;
/// a9fe0/a9f2c operate on live settings acquired bits (index2*tipID), not VM
/// GameData. Read then acquire in row order; duplicate rows observe prior writes.
pub(crate) trait AcquiredBits {
    fn acquired(&self, id: usize) -> bool;
    fn acquire(&mut self, id: usize) -> Result<()>;
}
#[derive(Debug, Clone)]
struct TipDescriptor {
    id: usize,
    atlas_word: u16,
}
#[derive(Debug, Clone)]
enum Content {
    Tips {
        rows: Vec<TipDescriptor>,
        alternate: bool,
    },
    Otsu {
        argument: i32,
        append: bool,
    },
    Kakera {
        ids: Vec<i32>,
    },
}
impl Content {
    fn archive(&self) -> &'static str {
        match self {
            Self::Tips { .. } => "/tipsget.txa",
            Self::Otsu { .. } => "/otsuget.txa",
            Self::Kakera { .. } => "/kakeraget.txa",
        }
    }
    fn native_id(&self) -> i32 {
        match self {
            Self::Tips { .. } => 12,
            Self::Otsu { .. } => 13,
            Self::Kakera { .. } => 14,
        }
    }
}
#[derive(Debug, Clone)]
pub(crate) enum Prepared {
    Character(CharacterView),
    Notification {
        atlas: Arc<CharacterTextures>,
        content: Content,
    },
}
impl Prepared {
    pub fn requires_settings(&self) -> bool {
        matches!(self, Self::Notification { content: Content::Tips { .. }, .. })
    }
    pub async fn load(
        server: &AssetServer,
        scenario: &HigurashiScenario,
        kind: i32,
        params: &[i32; 8],
    ) -> Result<Self> {
        if kind == 11 {
            let message = HigurashiMessageTextures::load(server).await?;
            return Ok(Self::Character(
                CharacterView::load(server, message, params[0] as u32).await?,
            ));
        }
        let content = match kind {
            12 => {
                let count = usize::try_from(params[1])?;
                ensure!(count <= 6, "Live TipsGet exceeds six compact parameter IDs");
                let rows = params[2..2 + count]
                    .iter()
                    .map(|&id| {
                        let id = usize::try_from(id)?;
                        let info = scenario.tips_info.get(id).ok_or_else(|| {
                            anyhow::anyhow!("Live TipsGet ID outside original scenario catalog")
                        })?;
                        Ok(TipDescriptor {
                            id,
                            atlas_word: info.id,
                        })
                    })
                    .collect::<Result<_>>()?;
                Content::Tips {
                    rows,
                    alternate: params[0] != 0,
                }
            }
            13 => Content::Otsu {
                argument: params[0],
                append: server.contains("/append.txt"),
            },
            14 => {
                let count = params[0].max(0) as usize;
                ensure!(
                    count <= 7,
                    "Live KakeraGet exceeds seven compact parameter IDs"
                );
                Content::Kakera {
                    ids: params[1..1 + count].to_vec(),
                }
            }
            _ => anyhow::bail!("Live temporary layer{kind} is outside11..14"),
        };
        let atlas = server.load(content.archive()).await?;
        Ok(Self::Notification { atlas, content })
    }
    /// Root creates/copies the old Layer base before this ready constructor.
    /// No derived MT or base clock advancement is due for any of these types.
    pub fn construct(
        &self,
        acquired: &mut impl AcquiredBits,
        route_navigation: bool,
    ) -> Result<Owner> {
        let state = match self {
            Self::Character(view) => {
                let mut view = view.duplicate_prepared();
                view.route_navigation = route_navigation;
                State::Character(view)
            }
            Self::Notification { atlas, content } => {
                let (rows, black, header, backup) = match content {
                    Content::Tips { rows, .. } => {
                        let rows = rows
                            .iter()
                            .map(|d| {
                                let old = acquired.acquired(d.id);
                                if !old {
                                    acquired.acquire(d.id)?;
                                }
                                Ok(Row {
                                    id: d.id as i32,
                                    atlas_word: d.atlas_word,
                                    previously_acquired: old,
                                    alpha: HigurashiTweener::new(0.),
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        (rows, 1., 100., 0.)
                    }
                    Content::Otsu { .. } => (vec![], 1., -120., 0.),
                    Content::Kakera { ids } => {
                        let rows = ids
                            .iter()
                            .map(|&id| Row {
                                id,
                                atlas_word: 0,
                                previously_acquired: false,
                                alpha: HigurashiTweener::new(0.),
                            })
                            .collect();
                        (rows, 1., 100., 0.)
                    }
                };
                State::Notification(Arc::new(Mutex::new(Notification {
                    atlas: atlas.clone(),
                    content: content.clone(),
                    rows,
                    black: HigurashiTweener::new(black),
                    header: HigurashiTweener::new(header),
                    backup: HigurashiTweener::new(backup),
                    phase: 0,
                })))
            }
        };
        Ok(Owner {
            state,
            cues: vec![],
            result: None,
        })
    }
}
#[derive(Debug, Clone)]
struct Row {
    id: i32,
    atlas_word: u16,
    previously_acquired: bool,
    alpha: HigurashiTweener,
}
#[derive(Debug, Clone)]
struct Notification {
    atlas: Arc<CharacterTextures>,
    content: Content,
    rows: Vec<Row>,
    black: HigurashiTweener,
    header: HigurashiTweener,
    backup: HigurashiTweener,
    phase: u32,
}
#[derive(Debug)]
enum State {
    Character(CharacterView),
    Notification(Arc<Mutex<Notification>>),
}
impl Clone for State {
    fn clone(&self) -> Self {
        match self {
            Self::Character(v) => Self::Character(v.clone()),
            Self::Notification(n) => {
                let mut copied = n.lock().clone();
                if let Content::Otsu { append, .. } = &mut copied.content {
                    // Original9833c leaves appendbyte1480 unwritten. Its
                    // bounded zero-allocation oracle observes false; use that
                    // deterministic initialized desktop value, not source data.
                    *append = false;
                    // The copy constructor initializes backup14e8 anew and
                    // never copies its source value or pending tween queue.
                    copied.backup = HigurashiTweener::new(0.);
                }
                Self::Notification(Arc::new(Mutex::new(copied)))
            }
        }
    }
}
#[derive(Debug)]
pub(crate) struct Owner {
    state: State,
    cues: Vec<&'static str>,
    result: Option<i32>,
}
impl Clone for Owner {
    fn clone(&self) -> Self {
        let state = self.state.clone();
        if let State::Character(view) = &state {
            // Native scene copies have no queued sound/callback delivery owner.
            let mut model = view.model.lock();
            model.take_cues();
            model.result = None;
        }
        Self {
            state,
            cues: vec![],
            result: None,
        }
    }
}
fn control(t: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    t.control(value, Ticks::from_f32(time), flags)
        .expect("Audited live temporary tween");
}
impl Owner {
    #[cfg(test)]
    pub(crate) fn reference_native_words(&self) -> Vec<u32> {
        let clock = |t: &HigurashiTweener| [t.value().to_bits(), u32::from(t.is_waiting())];
        match &self.state {
            State::Character(v) => {
                let m = v.model.lock();
                let mut w = vec![
                    m.phase,
                    m.selected as u32,
                    m.top as u32,
                    m.rows.len() as u32,
                ];
                for t in &m.clocks {
                    w.extend(clock(t));
                }
                for r in &m.rows {
                    w.extend([r.id as u32, r.original as u32, r.x as u32]);
                    w.extend(clock(&r.alpha));
                    w.extend(clock(&r.flash));
                }
                w
            }
            State::Notification(n) => {
                let n = n.lock();
                match n.content {
                    Content::Tips { alternate, .. } => {
                        let mut w = vec![n.phase, u32::from(alternate)];
                        w.extend(clock(&n.black));
                        w.extend(clock(&n.header));
                        w.push(n.rows.len() as u32);
                        for r in &n.rows {
                            w.extend([
                                r.id as u32,
                                u32::from(r.atlas_word),
                                u32::from(r.previously_acquired),
                            ]);
                            w.extend(clock(&r.alpha));
                        }
                        w
                    }
                    Content::Otsu { argument, append } => {
                        let mut w = vec![n.phase, u32::from(append), argument as u32];
                        w.extend(clock(&n.header));
                        w.extend(clock(&n.backup));
                        w.extend(clock(&n.black));
                        w
                    }
                    Content::Kakera { .. } => {
                        let mut w = vec![n.phase];
                        w.extend(clock(&n.black));
                        w.extend(clock(&n.header));
                        w.push(n.rows.len() as u32);
                        for r in &n.rows {
                            w.push(r.id as u32);
                            w.extend(clock(&r.alpha));
                        }
                        w
                    }
                }
            }
        }
    }
    /// Intrusive native retention shares the live private widget with its
    /// non-drawing wrapper. Scene snapshots instead use native_copy/Clone.
    pub fn retain(&self) -> Self {
        let state = match &self.state {
            State::Character(v) => State::Character(v.retain()),
            State::Notification(n) => State::Notification(n.clone()),
        };
        Self {
            state,
            cues: vec![],
            result: None,
        }
    }
    /// Private widget identity survives intrusive retention; callback/result
    /// ownership stays with its separate controller, not with this pointer.
    pub(crate) fn widget_identity(&self) -> usize {
        match &self.state {
            State::Character(view) => Arc::as_ptr(&view.model) as usize,
            State::Notification(notification) => Arc::as_ptr(notification) as usize,
        }
    }
    pub fn native_copy(&self) -> Self {
        self.clone()
    }
    pub fn native_id(&self) -> i32 {
        match &self.state {
            State::Character(_) => 11,
            State::Notification(n) => n.lock().content.native_id(),
        }
    }
    pub fn character_view(&self) -> Option<&CharacterView> {
        if let State::Character(v) = &self.state {
            Some(v)
        } else {
            None
        }
    }
    pub fn character_view_mut(&mut self) -> Option<&mut CharacterView> {
        if let State::Character(v) = &mut self.state {
            Some(v)
        } else {
            None
        }
    }
    pub fn set_route_navigation(&mut self, value: bool) {
        if let State::Character(v) = &mut self.state {
            v.route_navigation = value;
        }
    }
    pub fn phase(&self) -> u32 {
        match &self.state {
            State::Character(v) => v.model.lock().phase,
            State::Notification(n) => n.lock().phase,
        }
    }
    pub fn advance(&mut self, delta: Ticks) {
        match &mut self.state {
            State::Character(v) => v.advance(delta),
            State::Notification(n) => {
                let mut n = n.lock();
                match n.content {
                    Content::Tips { .. } => {
                        n.black.update(delta);
                        n.header.update(delta);
                        for row in &mut n.rows {
                            row.alpha.update(delta);
                        }
                    }
                    Content::Otsu { .. } => {
                        n.header.update(delta);
                        n.black.update(delta);
                    }
                    Content::Kakera { .. } => {
                        for row in &mut n.rows {
                            row.alpha.update(delta);
                        }
                        n.black.update(delta);
                        n.header.update(delta);
                    }
                }
            }
        }
    }
    pub fn fast_forward(&mut self) {
        match &mut self.state {
            State::Character(v) => v.fast_forward(),
            State::Notification(n) => {
                let mut n = n.lock();
                match n.content {
                    Content::Tips { .. } => {
                        n.black.fast_forward_unless_prohibited();
                        n.header.fast_forward_unless_prohibited();
                        for row in &mut n.rows {
                            row.alpha.fast_forward_unless_prohibited();
                        }
                    }
                    Content::Otsu { .. } => {
                        n.header.fast_forward_unless_prohibited();
                        n.black.fast_forward_unless_prohibited();
                    }
                    Content::Kakera { .. } => {
                        for row in &mut n.rows {
                            row.alpha.fast_forward_unless_prohibited();
                        }
                        n.black.fast_forward_unless_prohibited();
                        n.header.fast_forward_unless_prohibited();
                    }
                }
            }
        }
    }
    /// Exact virtual40; this does not install an input wrapper or opening task.
    pub fn refresh(&mut self) {
        match &mut self.state {
            State::Character(v) => {
                let mut m = v.model.lock();
                for r in &mut m.rows {
                    r.alpha.fast_forward_to(1.);
                }
                for (i, value) in [
                    (0, 0.),
                    (1, 1.),
                    (2, 0.),
                    (3, 1.),
                    (4, 1.),
                    (6, 0.),
                    (7, 1.),
                ] {
                    m.clocks[i].fast_forward_to(value);
                }
            }
            State::Notification(n) => {
                let mut n = n.lock();
                if matches!(n.content, Content::Tips { .. }) {
                    n.black.fast_forward_to(0.);
                    n.header.fast_forward_to(160.);
                    for r in &mut n.rows {
                        r.alpha.fast_forward_to(1.);
                    }
                }
            }
        }
    }
    /// Non-drawing wrapper polls this before the scene advances private clocks.
    pub fn poll_phase(&mut self) -> u32 {
        if let State::Character(v) = &mut self.state {
            v.model.lock().poll_phase();
            return v.model.lock().phase;
        }
        let State::Notification(n) = &mut self.state else {
            unreachable!()
        };
        let mut n = n.lock();
        if matches!(n.content, Content::Tips { .. }) {
            let alternate = matches!(
                n.content,
                Content::Tips {
                    alternate: true,
                    ..
                }
            );
            match n.phase {
                0 => {
                    control(&mut n.black, if alternate { 0.5 } else { 0. }, 60., 0);
                    if alternate {
                        n.header.fast_forward_to(96.);
                        for r in &mut n.rows {
                            r.alpha.fast_forward_to(1.);
                        }
                    } else {
                        control(&mut n.header, 240., 50., 2);
                        for (i, r) in n.rows.iter_mut().enumerate() {
                            control(&mut r.alpha, 0., 50. + 10. * i as f32, 0x80);
                            control(&mut r.alpha, 1., 40., 0);
                        }
                    }
                    self.cues.push("suzu");
                    n.phase = 1;
                }
                2 if alternate => n.phase = 4,
                3 => {
                    if !alternate {
                        control(&mut n.black, 0.5, 30., 0);
                    }
                    n.phase = 4;
                }
                _ => {}
            }
            if n.phase == 1
                && n.black.is_idle()
                && n.header.is_idle()
                && n.rows.iter().all(|r| r.alpha.is_idle())
            {
                n.phase = if alternate { 4 } else { 2 };
            }
            if n.phase == 5
                && n.black.is_idle()
                && n.header.is_idle()
                && n.rows.iter().all(|r| r.alpha.is_idle())
            {
                n.phase = 6;
            }
            if n.phase == 6 {
                self.result = Some(0);
            }
        } else {
            if n.phase == 0 {
                control(&mut n.black, 0., 60., 0);
                let header_target = if matches!(n.content, Content::Otsu { .. }) {0.} else {240.};
                control(
                    &mut n.header,
                    header_target,
                    50.,
                    2,
                );
                for (i, r) in n.rows.iter_mut().enumerate() {
                    control(&mut r.alpha, 0., 50. + 10. * i as f32, 0x80);
                    control(&mut r.alpha, 1., 40., 0);
                }
                self.cues.push("suzu");
                n.phase = 1;
            }
            if matches!(n.phase, 1 | 3)
                && n.header.is_idle()
                && n.black.is_idle()
                && n.rows.iter().all(|r| r.alpha.is_idle())
            {
                n.phase += 1;
            }
            if n.phase == 4 {
                self.result = Some(0);
            }
        }
        n.phase
    }
    pub fn take_cues(&mut self) -> Vec<&'static str> {
        if let State::Character(v) = &self.state {
            v.model.lock().take_cues()
        } else {
            std::mem::take(&mut self.cues)
        }
    }
    pub fn result(&self) -> Option<i32> {
        match &self.state {
            State::Character(v) => v.model.lock().result,
            State::Notification(_) => self.result,
        }
    }
    pub fn input_permissions(&self) -> OverlayInput {
        match &self.state {
            State::Character(v) => v.model.lock().permissions(),
            // Native notification wrappers return false without an attached
            // child; Tips delegates these permissions to its Choice child.
            State::Notification(_) => OverlayInput {
                menus: false,
                hide_window: false,
            },
        }
    }
    pub fn input(&mut self, permission: bool, buttons: Buttons) {
        if let State::Character(v) = &self.state {
            v.model.lock().input(permission, buttons);
            return;
        }
        if permission && buttons.pressed & (1 << 24) != 0 {
            self.confirm();
        }
    }
    pub fn probe(&self, p: Vec2) -> bool {
        match &self.state {
            State::Character(v) => v.model.lock().probe(p),
            State::Notification(n) => {
                let n = n.lock();
                n.phase == 2
                    && !matches!(
                        n.content,
                        Content::Tips {
                            alternate: true,
                            ..
                        }
                    )
            }
        }
    }
    pub fn press(&mut self, p: Vec2) {
        if let State::Character(v) = &self.state {
            v.model.lock().press(p);
        }
    }
    pub fn release(&mut self, p: Vec2) {
        if let State::Character(v) = &self.state {
            v.model.lock().release(p);
        } else {
            self.confirm();
        }
    }
    pub fn cancel(&mut self) {}
    fn confirm(&mut self) {
        let State::Notification(n) = &mut self.state else {
            return;
        };
        let mut n = n.lock();
        if n.phase != 2
            || matches!(
                n.content,
                Content::Tips {
                    alternate: true,
                    ..
                }
            )
        {
            return;
        }
        self.cues.push("decide");
        if !matches!(n.content, Content::Tips { .. }) {
            control(&mut n.black, 1., 60., 0);
        }
        n.phase = 3;
    }
    /// d8a80 creates a separate c0dbc child once at phase4. Its labels are
    /// literal UTF-8 and flags0x8003; the host retains/polls/draws that child.
    pub fn question(&self) -> Option<([&'static str; 2], u32)> {
        if let State::Notification(n) = &self.state {
            let n = n.lock();
            if matches!(n.content, Content::Tips { .. }) && n.phase == 4 && self.result.is_none() {
                return Some((["本編を続ける", "TIPSを見る"], 0x8003));
            }
        }
        None
    }
    /// d97a4: nonzero invokes outer callback1 immediately. Zero queues black1
    /// over60 at phase4 and advances through5/6 before outer callback0.
    pub fn resolve_question(&mut self, result: i32) -> Option<i32> {
        let State::Notification(n) = &mut self.state else {
            return None;
        };
        let mut n = n.lock();
        if !matches!(n.content, Content::Tips { .. }) {
            return None;
        }
        if result != 0 {
            self.result = Some(1);
            return Some(1);
        }
        if n.phase == 4 {
            control(&mut n.black, 1., 60., 0);
            n.phase = 5;
        }
        None
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        properties: &HigurashiProperties,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        if let State::Character(v) = &self.state {
            return v.render(pass, properties, parent, stencil, kind);
        }
        let State::Notification(n) = &self.state else {
            unreachable!()
        };
        let n = n.lock();
        let native = properties
            .composed_transform(parent)
            .compute_native_final_transform();
        let matrix = multiply(
            multiply(native, Mat4::from_translation(vec3(-640., -720., 0.))),
            Mat4::from_scale(vec3(2. / 3., 2. / 3., 0.)),
        );
        let transform = HigurashiTransform::convert_clip_depth(matrix);
        for packet in n.packets(kind) {
            let builder = RenderRequestBuilder::new()
                .depth_stencil_shorthand(stencil, true, false)
                .color_blend_type(if packet.opaque {
                    ColorBlendType::Opaque
                } else {
                    ColorBlendType::Layer1
                });
            if let Some(name) = &packet.texture {
                ensure!(
                    n.atlas.contains(&name),
                    "Live temporary {} has no texture{name}",
                    n.content.archive()
                );
                let vertices = packet.sprites();
                pass.run(builder.build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: n.atlas.get(&name).as_source(),
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            } else {
                let vertices = packet.fills();
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
        Ok(())
    }
}
#[derive(Debug, Clone)]
struct Packet {
    texture: Option<String>,
    rect: Vec4,
    uv: Vec4,
    color: UnormColor,
    opaque: bool,
}
impl Packet {
    fn sprite(name: impl Into<String>, rect: Vec4, uv: Vec4, alpha: f32) -> Self {
        Self {
            texture: Some(name.into()),
            rect,
            uv,
            color: FloatColor4::from_rgba(1., 1., 1., alpha).into_unorm(),
            opaque: false,
        }
    }
    fn fill(alpha: f32, opaque: bool) -> Self {
        Self {
            texture: None,
            rect: vec4(0., 0., 1920., 1080.),
            uv: Vec4::ZERO,
            color: FloatColor4::from_rgba(0., 0., 0., alpha).into_unorm(),
            opaque,
        }
    }
    fn fills(&self) -> [PosColVertex; 4] {
        build_quad_vertices(|c| PosColVertex {
            position: vec3(
                if c.x == 0. { self.rect.x } else { self.rect.z },
                if c.y == 0. { self.rect.y } else { self.rect.w },
                0.,
            ),
            color: self.color,
        })
    }
    fn sprites(&self) -> [PosColTexVertex; 4] {
        build_quad_vertices(|c| PosColTexVertex {
            position: vec3(
                if c.x == 0. { self.rect.x } else { self.rect.z },
                if c.y == 0. { self.rect.y } else { self.rect.w },
                0.,
            ),
            texture_position: vec2(
                if c.x == 0. { self.uv.x } else { self.uv.z },
                if c.y == 0. { self.uv.y } else { self.uv.w },
            ),
            color: self.color,
        })
    }
}
impl Notification {
    fn packets(&self, kind: PassKind) -> Vec<Packet> {
        if kind == PassKind::Opaque {
            return vec![Packet::fill(1., true)];
        }
        let uv = vec4(0., 0., 1., 1.);
        let mut out = vec![];
        match &self.content {
            Content::Otsu { argument, append } => {
                let mut name = if *argument <= 6 {
                    format!("otsuget_{argument:03}")
                } else if *argument <= 31 {
                    format!("snrget_{:03}", argument - 7)
                } else {
                    format!("snrget_{:03}", argument.wrapping_add(18))
                };
                if (27..=31).contains(argument) && *append {
                    name.push('_');
                }
                let y = 384. + self.header.value();
                out.push(Packet::sprite(name, vec4(450., y, 1470., y + 240.), uv, 1.));
            }
            Content::Tips { .. } | Content::Kakera { .. } => {
                let tips = matches!(self.content, Content::Tips { .. });
                let y = self.header.value();
                out.push(Packet::sprite(
                    if tips { "tips_get" } else { "kakera_get" },
                    vec4(624., y, 1296., y + 200.),
                    uv,
                    1.,
                ));
                let mut y = if tips {
                    738. - 48. * self.rows.len() as f32
                } else {
                    (576u64.wrapping_sub((self.rows.len() as u64).wrapping_mul(96)) >> 1) as f32
                        + 450.
                };
                for row in &self.rows {
                    let alpha = row.alpha.value();
                    out.push(Packet::sprite(
                        "plate",
                        vec4(576., y + 3., 1344., (y + 3.) + 96.),
                        uv,
                        alpha,
                    ));
                    let (atlas, band) = if tips {
                        (
                            i32::from(row.atlas_word >> 5),
                            f32::from(row.atlas_word & 31) * f32::from_bits(0x3d000000),
                        )
                    } else {
                        (
                            row.id / 32,
                            (row.id % 32) as f32 * f32::from_bits(0x3d000000),
                        )
                    };
                    let top = y + 18.;
                    out.push(Packet::sprite(
                        format!("title_{atlas:03}"),
                        vec4(696., top, 1224., top + 64.),
                        vec4(0., band, 1., band + f32::from_bits(0x3d000000)),
                        alpha,
                    ));
                    if tips && !row.previously_acquired {
                        out.push(Packet::sprite(
                            "new",
                            vec4(576., y + 30., 684., y + 66.),
                            uv,
                            alpha,
                        ));
                    }
                    y += 96.;
                }
            }
        }
        out.push(Packet::fill(self.black.value(), false));
        out
    }
}
#[cfg(test)]
mod tests;
