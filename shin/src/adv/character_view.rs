//! Native character-screen artwork, panel geometry and transition timing.
use std::collections::HashMap;

use glam::{Mat4, Vec4, vec2};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosTexVertex},
    },
};

use crate::asset::texture_archive::{TextureArchive, TextureArchiveBuilder};

pub(crate) fn character_page_palette(page: u8) -> [f32; 3] {
    [[166u8, 0, 0], [146, 92, 0], [135, 135, 135], [0, 0, 0]][page as usize]
        .map(|c| c as f32 / 255.0)
}

/// `chars.txa` contains both UI artwork and scenario-addressed portrait names.
/// Full character archives use the same dynamic names as scenario metadata.
#[derive(Debug)]
pub struct CharacterTextures {
    textures: HashMap<String, GpuTexture>,
    first_texture_name: Option<String>,
}

/// Native opening0x55d10 and closing0x56170 advance linearly by0.1 per tick.
/// The screen starts closing after its close-button flash, not at the click itself.
#[derive(Debug, Clone)]
pub struct CharacterScreenTransition {
    progress: f32,
    direction: i8,
}
impl Default for CharacterScreenTransition {
    fn default() -> Self {
        Self {
            progress: 0.0,
            direction: 1,
        }
    }
}
impl CharacterScreenTransition {
    pub fn progress(&self) -> f32 {
        self.progress
    }
    pub fn is_open(&self) -> bool {
        self.direction == 0 && self.progress == 1.0
    }
    pub fn is_closed(&self) -> bool {
        self.direction == 0 && self.progress == 0.0
    }
    pub fn begin_close(&mut self) {
        self.direction = -1;
    }
    pub fn update(&mut self, delta: shin_core::time::Ticks) {
        if self.direction == 0 {
            return;
        }
        self.progress += delta.as_f32() * (0.1 * self.direction as f32);
        if self.direction > 0 && self.progress >= 1.0 {
            self.progress = 1.0;
            self.direction = 0;
        }
        if self.direction < 0 && self.progress <= 0.0 {
            self.progress = 0.0;
            self.direction = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn character_screen_transition_matches_native_linear_updates() {
        use super::CharacterScreenTransition;
        use shin_core::time::Ticks;
        let mut animation = CharacterScreenTransition::default();
        assert!(!animation.is_closed());
        animation.update(Ticks::ZERO);
        assert!(!animation.is_closed());
        animation.update(Ticks::from_f32(2.5));
        assert_eq!(animation.progress(), 0.25);
        animation.update(Ticks::from_f32(2.5));
        assert_eq!(animation.progress(), 0.5);
        animation.update(Ticks::from_f32(20.0));
        assert!(animation.is_open());
        animation.update(Ticks::from_f32(40.0));
        assert_eq!(animation.progress(), 1.0);
        animation.begin_close();
        animation.update(Ticks::from_f32(7.5));
        assert_eq!(animation.progress(), 0.25);
        animation.update(Ticks::from_f32(2.5));
        assert!(animation.is_closed());
        animation.update(Ticks::from_f32(100.0));
        assert_eq!(animation.progress(), 0.0);
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_character_footer_and_new_marker_flags() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, expected) in [
            (0xfb9f0, 0x2a1f03e0),
            (0xfb9f4, 0xd65f03c0),
            (0x4f78c, 0x320003fa),
            (0x4fd30, 0x320003f7),
            (0x4f8ec, 0x381fa15a),
            (0x4fa2c, 0x381fa1ba),
            (0x4fb68, 0x381fa1ba),
            (0x4fd70, 0x381fa157),
            (0x51930, 0x7100019f),
            (0x5193c, 0x1a9f07f6),
            (0x51bcc, 0x2a1602e8),
            (0x51bd0, 0x37000568),
            (0x52250, 0x1e269001),
            (0x52268, 0x320003e2),
            (0x52290, 0x320007e2),
            (0x522a4, 0x2a1f03e2),
            (0x522ac, 0xb9439668),
            (0x522b4, 0x540000cb),
            (0x522c4, 0x321f07e2),
            (0x522fc, 0x321f03e2),
            (0x52308, 0x71001d1f),
            (0x5230c, 0x540002cc),
            (0x5232c, 0x7100051f),
            (0x5235c, 0x321e03e2),
            (0x52aa8, 0x913b42f7),
            (0x52ab0, 0xbd400e80),
            (0x52ac8, 0x1e2c392a),
            (0x52b24, 0x1e201982),
            (0x52b30, 0x1e231821),
            (0x52b58, 0x1e201000),
            (0x52b64, 0x1e20294a),
            (0x52bb0, 0x12000a88),
            (0x52bb8, 0x53037e88),
            (0x52c70, 0x1e361000),
        ] {
            assert_eq!(word(a), expected, "Native footer/new flag{a:x}");
        }
        for (i, row) in [
            [0i32, -1, 0, 206],
            [4, -1, 48, 72],
            [3, -1, 96, 272],
            [9, -1, 144, 176],
            [5, -1, 192, 140],
            [6, -1, 240, 140],
            [7, -1, 288, 238],
        ]
        .into_iter()
        .enumerate()
        {
            for (j, value) in row.into_iter().enumerate() {
                assert_eq!(word(0x16ded0 + i * 16 + j * 4), value as u32);
            }
        }
        for (a, value) in [
            (0x159a24, -88.0f32),
            (0x1599d0, 1872.0),
            (0x159584, 48.0),
            (0x159444, 320.0),
            (0x159a84, 336.0),
            (0x1596cc, 1.0 / 7.0),
            (0x1599c4, -48.0),
            (0x159a70, -730.0),
        ] {
            assert_eq!(word(a), value.to_bits());
        }
        assert_eq!(&exe[0x15c0c3 + 256..0x15c0c7 + 256], b"new\0");
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_character_grid_layout() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |address: usize| {
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap())
        };
        for (address, value) in [
            (0x159484, 112.0f32),
            (0x159758, 1920.0),
            (0x159614, 1080.0),
            (0x15937c, 160.0),
            (0x159af8, -160.0),
            (0x1597b8, 128.0),
            (0x1597ec, 480.0),
            (0x1599d4, 1440.0),
            (0x1597ac, 84.0),
            (0x159480, 1764.0),
            (0x15971c, 1848.0),
            (0x15959c, 1380.0),
            (0x159a60, 100.0),
            (0x159990, 2082.0),
            (0x159354, 1072.0),
            (0x159570, 0.1),
            (0x15999c, -0.1),
            (0x159a68, 80.0),
            (0x159994, 1526.0),
            (0x1596f8, 1606.0),
            (0x159550, 898.0),
            (0x159390, 80.0 / 1602.0),
            (0x15976c, 1526.0 / 1602.0),
            (0x159628, 1606.0 / 1602.0),
            (0x159490, 510.0),
            (0x159afc, 203.0),
            (0x159830, 528.0),
            (0x1595ec, 903.0),
            (0x1599e0, 90.0),
            (0x1597b4, 198.0),
            (0x1596c8, 400.0),
            (0x159918, 526.0),
            (0x1598c4, 380.0),
            (0x159450, 440.0),
            (0x159694, 32.0),
            (0x159824, 36.0),
            (0x159a7c, 144.0),
        ] {
            assert_eq!(word(address), value.to_bits(), "native layout{address:x}");
        }
        for (index, value) in [0.0f32, 80.0, 810.0, 898.0].into_iter().enumerate() {
            assert_eq!(word(0x16ddc0 + index * 4), value.to_bits());
        }
        // Page palette constructor, connector color constructor, stack/counter geometry.
        for (address, opcode) in [
            (0x564cc, 0x52bff4c0u32),
            (0x55d30, 0x1e210800),
            (0x55d38, 0x1e212800),
            (0x55d58, 0xb900e688),
            (0x5618c, 0x1e210800),
            (0x56194, 0x1e202820),
            (0x561a8, 0xb900e67f),
            (0x564dc, 0x528b8000),
            (0x564e0, 0x72bff240),
            (0x564f8, 0x5290f0e0),
            (0x564fc, 0x72bff0e0),
            (0x523d0, 0x52828780),
            (0x523d4, 0x72bff800),
            (0x523ec, 0x5282dc00),
            (0x523f0, 0x72bfe780),
            (0x52408, 0x529a0280),
            (0x5240c, 0x72bffe00),
            (0x51a70, 0x9344fd08),
            (0x51a74, 0xf100091f),
            (0x528c0, 0x5100194a),
            (0x51b30, 0xd344fd03),
        ] {
            assert_eq!(word(address), opcode, "native layout{address:x}");
        }
        for column in 0..11 {
            for (field, value) in [column * 16, 0, 16, 16].into_iter().enumerate() {
                assert_eq!(word(0x16de20 + column as usize * 16 + field * 4), value);
            }
        }
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_character_button_program() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, v) in [
            (0x15950c, 184.0f32),
            (0x159544, 56.0),
            (0x15982c, 216.0),
            (0x1597a4, 248.0),
            (0x159900, 64.0),
            (0x1593ec, 2.0 / 3.0),
            (0x159a18, 120.0),
            (0x159558, 264.0),
            (0x15962c, 124.0),
            (0x15949c, 1.0 / 3.0),
            (0x159498, 724.0),
            (0x159ab8, f32::EPSILON),
            (0x159770, f32::MAX),
        ] {
            assert_eq!(word(a), v.to_bits(), "native button constant{a:x}");
        }
        // Native program5: two RGB flashes followed by vertex color multiplication;
        // alpha only multiplies sampled and vertex alpha.
        let base = 0x1e1c30 + 0xb40;
        for (offset, opcode) in [
            (0x30, 0xd832008020170000u64),
            (0x68, 0x4c59100c00070007),
            (0x70, 0x4c59100c0017010a),
            (0x78, 0x4c59100c00270209),
            (0x88, 0x5c68100000870303),
            (0x90, 0x49a0000c00370707),
            (0x98, 0x49a0008c00370a0a),
            (0xa8, 0x49a0010c00370909),
            (0xb0, 0x4c59101000070700),
            (0xb8, 0x4c59101000170a01),
            (0xc8, 0x4c5910100027090b),
            (0xd0, 0x49a0039000370000),
            (0xd8, 0x49a0051000370102),
            (0xe8, 0x49a0049000370b07),
            (0xf0, 0x5c68100000670000),
            (0xf8, 0x5c68100000570201),
            (0x108, 0x5c68100000470702),
        ] {
            let a = base + offset + 256;
            assert_eq!(
                u64::from_le_bytes(exe[a..a + 8].try_into().unwrap()),
                opcode,
                "native button shader{offset:x}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_portrait_programs() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |address: usize| {
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap())
        };
        // Native mode->program table and shader constant buffers.
        for (mode, program) in [48, 49, 50, 51].into_iter().enumerate() {
            assert_eq!(word(0x16de00 + mode * 4), program);
        }
        for (address, value) in [
            (0x1e4370 + 0xcc0, 0.1f32),
            (0x1e5130 + 0xcc0, 0.587),
            (0x1e5130 + 0xcc4, 0.114),
        ] {
            assert_eq!(word(address), value.to_bits());
        }
        // Alive two-texture samples/composition; missing luma coefficient;
        // half-dead strict diagonal test and conditional green/blue scaling.
        for (address, opcode) in [
            (0x1e4238, 0xd83200a02097080au64),
            (0x1e4240, 0xd832008040970806),
            (0x1e4248, 0x59a1050000570a00),
            (0x1e4268, 0x59a0000000570600),
            (0x1e4f48, 0x49a0028400070401),
            (0x1e4f58, 0x49a0010400070b02),
            (0x1e5cc8, 0x1e23e99168770000),
            (0x1e6a78, 0x3859103f80070109),
            (0x1e6a80, 0x5bb1838000870907),
            (0x1e6a88, 0x1e23dcccccd70709),
            (0x1e6a98, 0x1e23dcccccd7040c),
            (0x1e6ac0, 0x5c98078000900007),
            (0x1e6ac8, 0x5c98078000c00004),
        ] {
            assert_eq!(
                u64::from_le_bytes(exe[address + 256..address + 264].try_into().unwrap()),
                opcode,
                "native portrait{address:x}"
            );
        }
    }
}
pub struct CharacterTextureBuilder {
    textures: HashMap<String, GpuTexture>,
    first_texture_name: Option<String>,
}
impl TextureArchiveBuilder for CharacterTextureBuilder {
    type Output = CharacterTextures;
    fn new() -> Self {
        Self {
            textures: HashMap::new(),
            first_texture_name: None,
        }
    }
    fn add_texture(&mut self, name: &str, texture: GpuTexture) {
        assert!(
            self.textures
                .insert(name.to_ascii_lowercase(), texture)
                .is_none(),
            "Duplicate character texture{name}"
        );
    }
    fn build(self) -> Self::Output {
        CharacterTextures {
            textures: self.textures,
            first_texture_name: self.first_texture_name,
        }
    }
    fn add_texture_with_index(&mut self, index: usize, name: &str, texture: GpuTexture) {
        if index == 0 {
            self.first_texture_name = Some(name.to_ascii_lowercase());
        }
        self.add_texture(name, texture);
    }
}
impl TextureArchive for CharacterTextures {
    type Builder = CharacterTextureBuilder;
}
impl CharacterTextures {
    pub(crate) fn require_names(&self, path: &str, names: &[&str]) -> anyhow::Result<()> {
        for name in names {
            anyhow::ensure!(self.contains(name), "Missing texture {name} in {path}");
        }
        Ok(())
    }
    pub(crate) fn require_first_texture(&self, path: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.first_texture_name
                .as_deref()
                .is_some_and(|name| self.contains(name)),
            "Missing texture at index0 in {path}"
        );
        Ok(())
    }
    pub(crate) fn validate_character_screen(
        &self,
        model: &super::characters::CharacterModel,
    ) -> anyhow::Result<()> {
        self.require_names(
            "/chars.txa",
            &[
                "bg",
                "obi",
                "window",
                "separator",
                "hana",
                "caption",
                "close",
                "help_chars",
                "key",
            ],
        )?;
        if !model.connectors.is_empty() {
            self.require_names("/chars.txa", &["line"])?;
        }
        if model.page_mask.count_ones() > 1 {
            self.require_names("/chars.txa", &["lr"])?;
        }
        if model.editable {
            self.require_names("/chars.txa", &["button"])?;
        }
        if !model.cells.is_empty() {
            self.require_names("/chars.txa", &["non"])?;
        }
        if model.cells.iter().any(|cell| cell.definitions.len() > 1) {
            self.require_names("/chars.txa", &["numbase", "num"])?;
        }
        for cell in &model.cells {
            for definition in &cell.definitions {
                if definition.default_state == 0 || definition.states.is_empty() {
                    self.require_names("/chars.txa", &["null"])?;
                    continue;
                }
                self.require_names("/chars.txa", &["cur", "bar"])?;
                for state in &definition.states {
                    if state.variants.is_empty() {
                        self.require_names("/chars.txa", &["null"])?;
                    }
                    for variant in &state.variants {
                        self.require_names("/chars.txa", &[&variant.portrait])?;
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn first_texture(&self) -> &GpuTexture {
        self.get(
            self.first_texture_name
                .as_deref()
                .expect("Archive has no texture at index0"),
        )
    }
    pub fn contains(&self, name: &str) -> bool {
        self.textures.contains_key(&name.to_ascii_lowercase())
    }
    pub fn get(&self, name: &str) -> &GpuTexture {
        self.textures
            .get(&name.to_ascii_lowercase())
            .unwrap_or_else(|| panic!("Missing character texture{name}"))
    }

    /// Native0x51990..0x51a60 samples the selected portrait with `non` as frame.
    /// Empty cells instead select `null`, also composited with `non`.
    pub fn render_portrait(
        &self,
        pass: &mut RenderPass,
        name: &str,
        mode: u8,
        rect: Vec4,
        transform: Mat4,
    ) {
        self.portrait(
            pass,
            name,
            mode,
            rect,
            transform,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
        );
    }

    fn portrait(
        &self,
        pass: &mut RenderPass,
        name: &str,
        mode: u8,
        rect: Vec4,
        transform: Mat4,
        builder: RenderRequestBuilder,
    ) {
        let vertices = shin_render::quad_vertices::build_quad_vertices(|uv| PosTexVertex {
            position: vec2(
                rect.x + (rect.z - rect.x) * uv.x,
                rect.y + (rect.w - rect.y) * uv.y,
            ),
            texture_position: uv,
        });
        pass.run(builder.build(
            RenderProgramWithArguments::CharacterPortrait {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                portrait: self.get(name).as_source(),
                frame: self.get("non").as_source(),
                transform,
                mode,
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }

    /// Native0x510b0..0x51260: the backdrop fades independently of the sliding panel.
    pub fn render_background(&self, pass: &mut RenderPass, progress: f32, transform: Mat4) {
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        self.sprite(
            pass,
            "bg",
            glam::vec4(0.0, 0.0, 1920.0, 1080.0),
            glam::vec4(0.0, 0.0, 1.0, 1.0),
            FloatColor4::from_rgba(1.0, 1.0, 1.0, progress).into_unorm(),
            transform,
            builder,
        );
        let builder = builder.color_blend_type(ColorBlendType::Layer2);
        let tint = FloatColor4::from_rgba(1.0, 1.0, 1.0, 0.25).into_unorm();
        let top = 160.0 * progress - 160.0;
        self.sprite(
            pass,
            "obi",
            glam::vec4(0.0, top, 1920.0, top + 160.0),
            glam::vec4(0.0, 0.0, 1.0, 0.5),
            tint,
            transform,
            builder,
        );
        let bottom = 1080.0 - 160.0 * progress;
        self.sprite(
            pass,
            "obi",
            glam::vec4(0.0, bottom, 1920.0, bottom + 160.0),
            glam::vec4(0.0, 0.5, 1.0, 1.0),
            tint,
            transform,
            builder,
        );
    }

    /// Native0x51264: window, grid, details and full sprite share this translation.
    pub fn body_transform(progress: f32, transform: Mat4) -> Mat4 {
        transform * Mat4::from_translation(glam::vec3(1920.0 - 1920.0 * progress, 0.0, 0.0))
    }

    /// Native0x520d0..0x52204: caption and close button use screen coordinates.
    /// The close-button flash is composed separately by the screen's input animation.
    pub fn render_header(&self, pass: &mut RenderPass, progress: f32, transform: Mat4) {
        self.render_header_with_flash(pass, progress, 0.0, transform);
    }

    pub fn render_header_with_flash(
        &self,
        pass: &mut RenderPass,
        progress: f32,
        flash: f32,
        transform: Mat4,
    ) {
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        let top = 160.0 * progress - 160.0;
        self.sprite(
            pass,
            "caption",
            glam::vec4(480.0, top, 1440.0, top + 128.0),
            glam::vec4(0.0, 0.0, 1.0, 1.0),
            UnormColor::WHITE,
            transform,
            builder,
        );
        self.render_button(
            pass,
            "close",
            glam::vec4(1764.0, top + 32.0, 1848.0, top + 116.0),
            glam::vec4(0.0, 0.0, 1.0, 1.0),
            UnormColor::WHITE,
            Vec4::ZERO,
            glam::vec4(1.0, 1.0, 1.0, flash),
            transform,
        );
    }

    /// Native0x51f38..0x51fd8: first full-character texture, without portrait effects.
    pub fn render_full_sprite(&self, pass: &mut RenderPass, transform: Mat4) {
        self.sprite(
            pass,
            self.first_texture_name
                .as_deref()
                .expect("Character archive has no texture at index0"),
            glam::vec4(1380.0, 100.0, 2082.0, 1072.0),
            glam::vec4(0.0, 0.0, 1.0, 1.0),
            UnormColor::WHITE,
            transform,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
        );
    }

    pub fn render_controls(
        &self,
        pass: &mut RenderPass,
        model: &super::characters::CharacterModel,
        flashes: [f32; 5],
        transform: Mat4,
    ) {
        use super::characters::{CharacterButton, CharacterButtonState};
        let states = model.button_states();
        for button in CharacterButton::ALL {
            let state = states[button as usize];
            if state == CharacterButtonState::Hidden {
                continue;
            }
            let color = if state == CharacterButtonState::Disabled {
                UnormColor::from_rgba(128, 128, 128, 255)
            } else {
                UnormColor::WHITE
            };
            self.render_button(
                pass,
                button.texture(),
                button.rectangle(),
                button.uv(),
                color,
                Vec4::ZERO,
                glam::vec4(1.0, 1.0, 1.0, flashes[button as usize]),
                transform,
            );
        }
    }

    /// Native program5 interpolates sampled RGB through both color uniforms,
    /// then multiplies by vertex RGB. Alpha is sampled alpha times vertex alpha.
    pub fn render_button(
        &self,
        pass: &mut RenderPass,
        name: &str,
        rect: Vec4,
        uv: Vec4,
        color: UnormColor,
        flash1: Vec4,
        flash2: Vec4,
        transform: Mat4,
    ) {
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
            position: glam::vec3(
                rect.x + (rect.z - rect.x) * corner.x,
                rect.y + (rect.w - rect.y) * corner.y,
                0.0,
            ),
            texture_position: vec2(
                uv.x + (uv.z - uv.x) * corner.x,
                uv.y + (uv.w - uv.y) * corner.y,
            ),
            color,
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Button {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        texture: self.get(name).as_source(),
                        transform,
                        flash1,
                        flash2,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }

    /// Native0x51ea0 divider is opaque and shares the text scroll/stencil mask.
    pub(crate) fn render_text_divider(
        &self,
        pass: &mut RenderPass,
        name_height: f32,
        scroll: f32,
        transform: Mat4,
        builder: RenderRequestBuilder,
    ) {
        let top = 188.0 + name_height - 20.0 - scroll;
        self.sprite(
            pass,
            "bar",
            glam::vec4(544.0, top, 1520.0, top + 16.0),
            glam::vec4(0.0, 0.0, 1.0, 1.0),
            UnormColor::WHITE,
            transform,
            builder.color_blend_type(ColorBlendType::Opaque),
        );
    }

    pub fn render_window(&self, pass: &mut RenderPass, transform: Mat4) {
        // Native0x52450: fixed three vertical bands, horizontally split80/1446/80.
        // The right UV border includes the native four-pixel overrun and clamp.
        let ys = [0.0, 80.0, 810.0, 898.0];
        let xs = [0.0, 80.0, 1526.0, 1606.0];
        let us = [0.0, 80.0 / 1602.0, 1526.0 / 1602.0, 1606.0 / 1602.0];
        for row in 0..3 {
            for column in 0..3 {
                self.sprite(
                    pass,
                    "window",
                    glam::vec4(
                        xs[column],
                        ys[row] + 112.0,
                        xs[column + 1],
                        ys[row + 1] + 112.0,
                    ),
                    glam::vec4(
                        us[column],
                        ys[row] / 898.0,
                        us[column + 1],
                        ys[row + 1] / 898.0,
                    ),
                    UnormColor::WHITE,
                    transform,
                    RenderRequestBuilder::new().color_blend_type(if row == 1 && column == 1 {
                        ColorBlendType::Opaque
                    } else {
                        ColorBlendType::Layer1
                    }),
                );
            }
        }
        self.sprite(
            pass,
            "separator",
            glam::vec4(510.0, 203.0, 528.0, 903.0),
            glam::vec4(0.0, 0.0, 1.0, 1.0),
            UnormColor::WHITE,
            transform,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
        );
    }

    pub fn render_grid(
        &self,
        pass: &mut RenderPass,
        model: &super::characters::CharacterModel,
        scroll: f32,
        hovered: Option<usize>,
        transform: Mat4,
    ) {
        let selected: Vec<_> = model
            .cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if model.selection_on_page(c.page) == Some(i) {
                    1.0
                } else {
                    0.0
                }
            })
            .collect();
        let clicked: Vec<_> = model
            .cells
            .iter()
            .enumerate()
            .map(|(i, _)| if hovered == Some(i) { 1.0 } else { 0.0 })
            .collect();
        self.render_grid_with_effects(
            pass,
            model,
            scroll,
            character_page_palette(model.page),
            &selected,
            &clicked,
            transform,
        );
    }

    pub fn render_grid_with_effects(
        &self,
        pass: &mut RenderPass,
        model: &super::characters::CharacterModel,
        scroll: f32,
        palette: [f32; 3],
        selected: &[f32],
        clicked: &[f32],
        transform: Mat4,
    ) {
        assert_eq!(selected.len(), model.cells.len());
        assert_eq!(clicked.len(), model.cells.len());
        use crate::layer::render_params::{DrawableClipMode, DrawableClipParams, TransformParams};
        use shin_render::quad_vertices::QuadVertices;
        use shin_render::{
            DepthStencilState, StencilFunction, StencilMask, StencilPipelineState, StencilState,
        };
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        QuadVertices::new()
            .with_box(90.0, 198.0, 490.0, 724.0)
            .with_color(FloatColor4::from_rgba(0.0, 0.0, 0.0, 0.75).into_unorm())
            .fill(pass, builder, transform);
        // Native0x564b0 initializes three page palettes; the fourth remains zero.
        self.sprite(
            pass,
            "hana",
            glam::vec4(90.0, 198.0, 470.0, 638.0),
            glam::vec4(1.0, 1.0, 0.0, 0.0),
            FloatColor4::from_rgba(palette[0], palette[1], palette[2], 0.5).into_unorm(),
            transform,
            builder,
        );
        let clip = DrawableClipParams {
            mode: DrawableClipMode::Clip,
            area: glam::vec4(90.0, 198.0, 400.0, 526.0),
        };
        clip.begin(
            pass,
            &TransformParams {
                transform: crate::render::centered_projection_matrix().inverse() * transform,
                ..Default::default()
            },
        );
        let builder = builder.depth_stencil(DepthStencilState {
            stencil: StencilState {
                stencil_reference: 0,
                pipeline: StencilPipelineState {
                    function: StencilFunction::Equal,
                    stencil_read_mask: StencilMask::ClipOnly,
                    ..Default::default()
                },
            },
            ..Default::default()
        });
        let page_slot =
            |page: u8| (model.page_mask & ((1u8 << page) - 1)).count_ones() as f32 * 400.0;
        for connector in &model.connectors {
            if connector.color == 0 {
                continue;
            }
            let x = 110.0 + page_slot(connector.page) - scroll + f32::from(connector.x) * 24.0;
            let y = 214.0 + f32::from(connector.y) * 26.0;
            let u = (4.0 + f32::from(connector.shape & 3) * 32.0) / 128.0;
            let v = (5.0 + f32::from(connector.shape >> 2) * 36.0) / 144.0;
            let rgb =
                [[0, 0, 0], [192, 20, 60], [60, 22, 224], [240, 208, 20]][connector.color as usize];
            self.sprite(
                pass,
                "line",
                glam::vec4(x, y, x + 24.0, y + 26.0),
                glam::vec4(u, v, u + 24.0 / 128.0, v + 26.0 / 144.0),
                UnormColor::from_rgba(rgb[0], rgb[1], rgb[2], 255),
                transform,
                builder,
            );
        }
        for (index, cell) in model.cells.iter().enumerate() {
            let x = 84.0 + page_slot(cell.page) - scroll + f32::from(cell.x) * 24.0;
            let y = 184.0 + f32::from(cell.y) * 26.0;
            let rect = glam::vec4(x, y, x + 76.0, y + 86.0);
            let variant = cell
                .is_selectable()
                .then(|| cell.definition().state().and_then(|s| s.variant()))
                .flatten();
            let (name, mode) = variant
                .map(|v| (v.portrait.as_str(), v.display_mode))
                .unwrap_or(("null", 0));
            self.portrait(pass, name, mode, rect, transform, builder);
            if cell.definitions.len() > 1 {
                self.sprite(
                    pass,
                    "numbase",
                    glam::vec4(x + 40.0, y + 66.0, x + 80.0, y + 86.0),
                    glam::vec4(0.0, 0.0, 1.0, 1.0),
                    UnormColor::WHITE,
                    transform,
                    builder,
                );
                self.counter(
                    pass,
                    &format!("{}/{}", cell.definition_index() + 1, cell.definitions.len()),
                    x + 72.0,
                    y + 68.0,
                    transform,
                    builder,
                );
            }
            if selected[index] > 0.0 {
                self.sprite(
                    pass,
                    "cur",
                    rect,
                    glam::vec4(0.0, 0.0, 1.0, 1.0),
                    FloatColor4::from_rgba(1.0, 1.0, 1.0, selected[index] * 0.5).into_unorm(),
                    transform,
                    builder,
                );
            }
            if clicked[index] > 0.0 {
                self.sprite(
                    pass,
                    "cur",
                    rect,
                    glam::vec4(0.0, 0.0, 1.0, 1.0),
                    FloatColor4::from_rgba(1.0, 1.0, 1.0, clicked[index]).into_unorm(),
                    transform,
                    builder,
                );
            }
        }
        clip.end(pass);
    }

    /// Native0x52238..0x52360 and0x52a60: right-to-left controller help.
    pub fn render_footer(
        &self,
        pass: &mut RenderPass,
        model: &super::characters::CharacterModel,
        can_scroll: bool,
        progress: f32,
        transform: Mat4,
    ) {
        let cell = model.selected_cell();
        let definition = cell.map(|c| c.definition());
        let hints = [
            (1, true, true),
            (3, true, can_scroll),
            (0, true, true),
            (6, model.page_mask.count_ones() >= 2, true),
            (2, true, cell.is_some_and(|c| c.definitions.len() > 1)),
            (
                5,
                model.editable,
                definition.is_some_and(|d| d.states.len() > 1),
            ),
            (
                4,
                model.editable,
                definition
                    .and_then(|d| d.state())
                    .is_some_and(|s| s.variants.len() > 1),
            ),
        ];
        let rows = [
            (0, 206.0),
            (4, 72.0),
            (3, 272.0),
            (9, 176.0),
            (5, 140.0),
            (6, 140.0),
            (7, 238.0),
        ];
        let mut right = 1872.0;
        let top = 1100.0 - 88.0 * progress;
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        for (row, visible, enabled) in hints {
            if !visible {
                continue;
            }
            let (key, width) = rows[row];
            let left = right - width;
            let tint = if enabled {
                UnormColor::WHITE
            } else {
                UnormColor::from_rgba(128, 128, 128, 255)
            };
            self.sprite(
                pass,
                "help_chars",
                glam::vec4(left, top, right, top + 48.0),
                glam::vec4(0.0, row as f32 / 7.0, width / 320.0, (row + 1) as f32 / 7.0),
                tint,
                transform,
                builder,
            );
            let icon_left = left + 2.0 - 48.0;
            let u = (key & 7) as f32 / 8.0;
            let v = (key >> 3) as f32 / 2.0;
            self.sprite(
                pass,
                "key",
                glam::vec4(icon_left, top, icon_left + 48.0, top + 48.0),
                glam::vec4(u, v, u + 0.125, v + 0.5),
                tint,
                transform,
                builder,
            );
            right = icon_left - 16.0;
        }
    }

    fn sprite(
        &self,
        pass: &mut RenderPass,
        name: &str,
        rect: Vec4,
        uv: Vec4,
        color: UnormColor,
        transform: Mat4,
        builder: RenderRequestBuilder,
    ) {
        shin_render::quad_vertices::QuadVertices::new()
            .with_box(rect.x, rect.y, rect.z, rect.w)
            .with_tex_box(uv.x, uv.y, uv.z, uv.w)
            .with_color(color)
            .render_sprite(pass, builder, self.get(name).as_source(), transform);
    }

    fn counter(
        &self,
        pass: &mut RenderPass,
        text: &str,
        right: f32,
        y: f32,
        transform: Mat4,
        builder: RenderRequestBuilder,
    ) {
        // Native0x52850:16-pixel glyphs, three-pixel bearing, ten-pixel advances.
        let mut x = right - text.len() as f32 * 10.0 - 3.0;
        for byte in text.bytes() {
            let column = if byte.is_ascii_digit() {
                byte - b'0'
            } else {
                10
            };
            let u = f32::from(column) * 16.0 / 176.0;
            self.sprite(
                pass,
                "num",
                glam::vec4(x, y, x + 16.0, y + 16.0),
                glam::vec4(u, 0.0, u + 16.0 / 176.0, 1.0),
                UnormColor::WHITE,
                transform,
                builder,
            );
            x += 10.0;
        }
    }
}
