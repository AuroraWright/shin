//! Native ADV13af0 full-resolution menu snapshot blur and125a0 crossfade.
//! The host captures the story before acquiring UI-hide/audio-pause ownership.
use crate::render::{PreRenderContext, VIRTUAL_CANVAS_SIZE_VEC, top_left_projection_matrix};
use glam::vec4;
use shin_core::{
    primitives::color::FloatColor4,
    time::{Ticks, Tween, Tweener},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::{QuadVertices, build_quad_vertices},
    render_pass::RenderPass,
    render_texture::RenderTexture,
    shaders::types::{buffer::VertexSource, vertices::PosTexVertex},
};

pub struct ModalBackdrop {
    source: RenderTexture,
    fade: Tweener,
}
impl ModalBackdrop {
    /// Capture is caller-owned: native renders the scene with root children0/1
    /// temporarily removed, before hiding overlays and pausing sound.
    pub fn new(context: &mut PreRenderContext, mut source: RenderTexture) -> Self {
        let mut scratch = context.new_render_texture("Modal blur intermediate".into());
        let (width, height) = source.dimensions();
        // Native14034..1405c and1414c..1416c: offsets .75,2.25,3.75,5.25
        // divided by physical dimensions. Program43 takes step*(.5,1.5,2.5,3.5).
        for step in [
            vec4(1.5 / width as f32, 0.0, 0.0, 0.0),
            vec4(0.0, 1.5 / height as f32, 0.0, 0.0),
        ] {
            let vertices = build_quad_vertices(|uv| PosTexVertex {
                position: uv * VIRTUAL_CANVAS_SIZE_VEC,
                texture_position: uv,
            });
            {
                let mut pass = context.begin_pass(scratch.as_texture_target(), None, "Modal blur");
                pass.run(
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::Opaque)
                        .build(
                            RenderProgramWithArguments::Blur {
                                vertices: VertexSource::VertexData {
                                    vertices: &vertices,
                                },
                                texture: source.as_texture_source(),
                                transform: top_left_projection_matrix(),
                                step,
                            },
                            DrawPrimitive::TrianglesStrip,
                        ),
                );
            }
            std::mem::swap(&mut source, &mut scratch);
        }
        let mut fade = Tweener::new(0.0);
        fade.enqueue(1.0, Tween::linear(Ticks::from_f32(10.0)));
        Self { source, fade }
    }
    pub fn progress(&self) -> f32 {
        self.fade.value()
    }
    /// Native125f4 still draws the live scene below the snapshot while fade<1.
    pub fn needs_scene(&self) -> bool {
        self.progress() < 1.0
    }
    pub fn update(&mut self, delta: Ticks) {
        self.fade.update(delta);
    }
    pub fn begin_close(&mut self) {
        // Native145c0 clears the previous fade, then targets0 at0.1 per tick.
        self.fade
            .enqueue_now(0.0, Tween::linear(Ticks::from_f32(self.progress() / 0.1)));
    }
    pub fn is_closed(&self) -> bool {
        self.fade.is_idle() && self.progress() == 0.0
    }
    pub fn render(&self, pass: &mut RenderPass) {
        let p = self.progress();
        if p <= 0.0 {
            return;
        }
        QuadVertices::new()
            .with_box(0.0, 0.0, 1920.0, 1080.0)
            .with_tex_box(0.0, 0.0, 1.0, 1.0)
            .with_color(FloatColor4::from_rgba(0.5 * p, 0.5 * p, 0.5 * p, p).into_unorm())
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(if p < 1.0 {
                    ColorBlendType::LayerPremultiplied1
                } else {
                    ColorBlendType::Layer1
                }),
                self.source.as_texture_source(),
                top_left_projection_matrix(),
            );
    }
}

#[cfg(test)]
pub(super) fn assert_reference_pixels(context: &mut PreRenderContext, output: &wgpu::Texture) {
    use shin_core::primitives::color::UnormColor;
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let mut source = context.new_render_texture("Modal captured pattern".into());
    let base = [24, 68, 119];
    let red = [203, 49, 71];
    let green = [17, 239, 83];
    let color = |rgb: [u8; 3]| {
        FloatColor4::from_rgba(
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
            1.0,
        )
        .into_unorm()
    };
    {
        let mut pass = context.begin_pass(source.as_texture_target(), None, "Modal source");
        pass.clear(Some(color(base)), None, None);
        for (rect, rgb) in [
            (vec4(301.0, 207.0, 1009.0, 801.0), red),
            (vec4(958.0, 0.0, 962.0, 1080.0), green),
        ] {
            QuadVertices::new()
                .with_box(rect.x, rect.y, rect.z, rect.w)
                .with_color(color(rgb))
                .fill(
                    &mut pass,
                    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                    top_left_projection_matrix(),
                );
        }
    }
    let mut backdrop = ModalBackdrop::new(context, source);
    assert!(backdrop.needs_scene());
    assert!(!backdrop.is_closed());
    let view = output.create_view(&Default::default());
    // Independent native full-resolution separable sampling with an RGBA8
    // intermediate. Four symmetric offsets are literal native pixel distances.
    fn linear(get: &impl Fn(i32, i32) -> f32, x: f32, y: f32) -> f32 {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - ix as f32;
        let fy = y - iy as f32;
        let pixel = |x: i32, y: i32| get(x.clamp(0, 1919), y.clamp(0, 1079));
        (pixel(ix, iy) * (1.0 - fx) + pixel(ix + 1, iy) * fx) * (1.0 - fy)
            + (pixel(ix, iy + 1) * (1.0 - fx) + pixel(ix + 1, iy + 1) * fx) * fy
    }
    let original = |x: i32, y: i32, c: usize| -> f32 {
        let rgb = if (958..962).contains(&x) {
            green
        } else if (301..1009).contains(&x) && (207..801).contains(&y) {
            red
        } else {
            base
        };
        rgb[c] as f32 / 255.0
    };
    let blur = |x: i32, y: i32, c: usize| -> f32 {
        let weights = [0x3eceed6bu32, 0x3ea127be, 0x3e437dbe, 0x3db8afdf];
        let offsets = [0.75, 2.25, 3.75, 5.25];
        let horizontal = |x: i32, y: i32| {
            let mut sum = 0.0;
            for (offset, bits) in offsets.into_iter().zip(weights) {
                for sign in [-1.0, 1.0] {
                    sum += linear(
                        &|x, y| original(x, y, c),
                        x as f32 + sign * offset,
                        y as f32,
                    ) * f32::from_bits(bits)
                        * 0.5;
                }
            }
            (sum * 255.0).round() / 255.0
        };
        let mut sum = 0.0;
        for (offset, bits) in offsets.into_iter().zip(weights) {
            for sign in [-1.0, 1.0] {
                sum += linear(&horizontal, x as f32, y as f32 + sign * offset)
                    * f32::from_bits(bits)
                    * 0.5;
            }
        }
        (sum * 255.0).round() / 255.0
    };
    let mut checked = 0;
    let mut max_error = 0;
    for (ticks, closing, p) in [
        (0.0, false, 0.0),
        (2.5, false, 0.25),
        (2.5, false, 0.5),
        (5.0, false, 1.0),
        (5.0, true, 0.5),
        (5.0, false, 0.0),
    ] {
        if closing {
            backdrop.begin_close();
        }
        backdrop.update(Ticks::from_f32(ticks));
        assert_eq!(backdrop.progress(), p);
        assert_eq!(backdrop.needs_scene(), p < 1.0);
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                None,
                "Modal blurred composition",
            );
            pass.clear(Some(UnormColor::BLACK), None, None);
            QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .with_color(color(base))
                .fill(
                    &mut pass,
                    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                    top_left_projection_matrix(),
                );
            backdrop.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(context, output);
        for y in (0..1080).step_by(17) {
            for x in (0..1920)
                .step_by(17)
                .chain([300, 301, 302, 957, 958, 959, 960, 961, 962, 963, 1008, 1009])
            {
                for c in 0..3 {
                    let expected = (blur(x, y, c) * (p * 0.5 * 255.0).round() / 255.0
                        + base[c] as f32 / 255.0 * (1.0 - (p * 255.0).round() / 255.0))
                        * 255.0;
                    let error = (pixels[(y as usize * 1920 + x as usize) * 4 + c] as i32
                        - expected.round() as i32)
                        .abs();
                    max_error = max_error.max(error);
                    assert!(error <= 2, "modal p{p} pixel({x},{y}) c{c}:error{error}");
                }
                assert_eq!(pixels[(y as usize * 1920 + x as usize) * 4 + 3], 255);
                checked += 1;
            }
        }
    }
    assert!(backdrop.is_closed());
    eprintln!("Modal backdrop: {checked} scalar samples,6 fade cases,max RGB error{max_error}");
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_modal_capture_removes_and_restores_root_children() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x13c54, 0x9102c268),
            (0x13c58, 0xc8dffd09),
            (0x13c5c, 0x9102a26a),
            (0x13c60, 0xf100013f),
            (0x13c64, 0x9a880148),
            (0x13c68, 0xf90007ff),
            (0x13c6c, 0xc8dffd00),
            (0x13c70, 0x910023e8),
            (0x13c74, 0xc85ffd16),
            (0x13c78, 0xc809fd00),
            (0x13c7c, 0x35ffffc9),
            (0x13c80, 0xb4000080),
            (0x13c84, 0xf8410c08),
            (0x13c88, 0xf9400108),
            (0x13c8c, 0xd63f0100),
            (0x13c90, 0xb40000b6),
            (0x13c94, 0xf8410ec8),
            (0x13c98, 0xf9400508),
            (0x13c9c, 0xaa1603e0),
            (0x13ca0, 0xd63f0100),
            (0x13ca4, 0x910023f6),
            (0x13ca8, 0xc8dffec0),
            (0x13cac, 0x9101e3e8),
            (0x13cb0, 0x320003e1),
            (0x13cb4, 0x94019d8b),
            (0x13cb8, 0xc8dffec0),
            (0x13cbc, 0x9101c3e8),
            (0x13cc0, 0x2a1f03e1),
            (0x13cc4, 0x94019d87),
            (0x13cc8, 0x910023f6),
            (0x13ccc, 0xc8dffec0),
            (0x13cd0, 0x320003e1),
            (0x13cd4, 0x2a1f03e2),
            (0x13cd8, 0x94019dee),
            (0x13cdc, 0xc8dffec0),
            (0x13ce0, 0x2a1f03e1),
            (0x13ce4, 0x2a1f03e2),
            (0x13ce8, 0x94019dea),
            (0x13cec, 0x910023e8),
            (0x13cf0, 0xc8dffd00),
            (0x13cf4, 0xf9400008),
            (0x13cf8, 0xf9402108),
            (0x13cfc, 0x910203e1),
            (0x13d00, 0xd63f0100),
            (0x13e04, 0xf9002fff),
            (0x13e08, 0x910183e2),
            (0x13e0c, 0x910163e3),
            (0x13e10, 0xaa1603e0),
            (0x13e14, 0x320003e1),
            (0x13e18, 0x910163f7),
            (0x13e1c, 0x94019a31),
            (0x13eac, 0xf90027ff),
            (0x13eb0, 0x910143e2),
            (0x13eb4, 0x910123e3),
            (0x13eb8, 0xaa1603e0),
            (0x13ebc, 0x2a1f03e1),
            (0x13ec0, 0x910123f7),
            (0x13ec4, 0x94019a07),
            (0x13f04, 0x910023e8),
            (0x13f08, 0xc8dffd00),
            (0x13f0c, 0xf9400008),
            (0x13f10, 0xf9402108),
            (0x13f14, 0x910203e1),
            (0x13f18, 0xd63f0100),
            (0x7b2f4, 0xa9462014),
            (0x7b2f8, 0xeb140109),
            (0x7b2fc, 0x54000160),
            (0x7b300, 0x9344fd29),
            (0x7b304, 0xd341fd2a),
            (0x7b308, 0x8b0a128b),
            (0x7b30c, 0xaa2a03ed),
            (0x7b310, 0x8b0d0129),
            (0x7b314, 0xb841056c),
            (0x7b318, 0x6b01019f),
            (0x7b31c, 0x9a8ab129),
            (0x7b320, 0x9a94b174),
            (0x7b324, 0xb5ffff09),
            (0x7b328, 0xeb08029f),
            (0x7b32c, 0x540004c0),
            (0x7b330, 0xb9400288),
            (0x7b334, 0x6b01011f),
            (0x7b338, 0x54000461),
            (0x7b33c, 0x91002288),
            (0x7b340, 0xc8dffd00),
            (0x7b4f4, 0xb94002c8),
            (0x7b4f8, 0x6b01011f),
            (0x7b4fc, 0x54000ce1),
            (0x7b500, 0x34000622),
            (0x7b504, 0x1e220040),
            (0x7b5c4, 0x910042d7),
            (0x7b5c8, 0xeb1502ff),
            (0x7b5cc, 0x540004c0),
            (0x7b5d0, 0xaa1703e9),
            (0x7b5d4, 0xf840852a),
            (0x7b5d8, 0xaa1603e8),
            (0x7b5dc, 0xf800850a),
            (0x7b5e0, 0xc8dffd20),
            (0x7b5e4, 0xc85ffd14),
            (0x7b5e8, 0xc809fd00),
            (0x7b5ec, 0x35ffffc9),
            (0x7b5f0, 0xb4000080),
            (0x7b5f4, 0xf8410c08),
            (0x7b5f8, 0xf9400108),
            (0x7b5fc, 0xd63f0100),
            (0x7b600, 0xb40000b4),
            (0x7b604, 0xf8410e88),
            (0x7b608, 0xf9400508),
            (0x7b60c, 0xaa1403e0),
            (0x7b610, 0xd63f0100),
            (0x7b614, 0x910042f7),
            (0x7b618, 0x910042d6),
            (0x7b61c, 0xeb1502ff),
            (0x7b620, 0x54fffd81),
            (0x7b624, 0xf9403675),
            (0x7b628, 0x1400000d),
            (0x7b62c, 0x14000001),
            (0x7b630, 0xaa0003f3),
            (0x7b634, 0xc85ffee0),
            (0x7b638, 0xc808feff),
            (0x7b63c, 0x35ffffc8),
            (0x7b640, 0xb4000080),
            (0x7b644, 0xf8410c08),
            (0x7b648, 0xf9400508),
            (0x7b64c, 0xd63f0100),
            (0x7b650, 0xaa1303e0),
            (0x7b654, 0x940284f7),
            (0x7b658, 0x97fe250e),
            (0x7b65c, 0xeb1602bf),
            (0x7b660, 0x54000160),
            (0x7b664, 0xd10022a8),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native instruction at {address:x}"
            );
        }
        //7b2e0 casts a child to LayerGroup::TransitionLayer before returning its
        //underlying layer, confirming these calls address children, not properties.
        assert_eq!(
            &exe[0x1700e1 + 256..0x1700e1 + 256 + 37],
            b"N4shin10LayerGroup15TransitionLayerE\0"
        );
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_modal_full_resolution_blur_and_half_brightness_fade() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x13f7c, 0xd29dad68),
            (0x13f80, 0xf2a7d9c8),
            (0x13f84, 0xd28fb7c9),
            (0x13f88, 0xf2a7c869),
            (0x13f8c, 0xf2c4f7c8),
            (0x13f90, 0xf2e7d428),
            (0x13f94, 0xf2d5fbe9),
            (0x13f98, 0xf2e7b709),
            (0x13f9c, 0xd2e890ea),
            (0x13fa0, 0xa93727a8),
            (0x13fa4, 0xb2491be8),
            (0x13fa8, 0xa90a23ea),
            (0x13fac, 0xd2a89e08),
            (0x13fb0, 0xf2e890e8),
            (0x13fb4, 0xb2091be9),
            (0x13fb8, 0xa90b27e8),
            (0x13fbc, 0x52a89e08),
            (0x13fc0, 0x32091be9),
            (0x13fc4, 0xa9087fff),
            (0x13fc8, 0xa90927e8),
            (0x13ff4, 0x12003ea8),
            (0x13ff8, 0x1e230100),
            (0x13ffc, 0x12003e88),
            (0x14000, 0x1e230105),
            (0x14004, 0x1e2d1001),
            (0x14008, 0x1e205002),
            (0x1400c, 0x1e21d003),
            (0x14010, 0x1e22b004),
            (0x14014, 0x1e20188b),
            (0x14018, 0xd10283a8),
            (0x1401c, 0xb819c3bf),
            (0x14020, 0xb81943bf),
            (0x14024, 0xb818c3bf),
            (0x14028, 0xb81843bf),
            (0x1402c, 0xf9003fff),
            (0x14030, 0x91002108),
            (0x14034, 0x1e201828),
            (0x14038, 0x1e201849),
            (0x1403c, 0x1e20186a),
            (0x14040, 0x1e251863),
            (0x14044, 0xbc1903a3),
            (0x14048, 0x1e251842),
            (0x1404c, 0xbc1883a2),
            (0x14050, 0x1e251821),
            (0x14054, 0xbc1803a1),
            (0x14058, 0x1e251885),
            (0x1405c, 0xbc1983a5),
            (0x14098, 0x9101e3f5),
            (0x1409c, 0xc8dffea0),
            (0x140a0, 0x9402f298),
            (0x140a4, 0xaa1f03e1),
            (0x140a8, 0x9402ec8a),
            (0x140ac, 0x9402ed39),
            (0x140b0, 0x910023e1),
            (0x140b4, 0x2a1f03e0),
            (0x140b8, 0x321a03e2),
            (0x140bc, 0x9402ede1),
            (0x140c0, 0xd10203a1),
            (0x140c4, 0x320003e0),
            (0x140c8, 0x321b03e2),
            (0x140cc, 0x9402eddd),
            (0x140d0, 0xd10243a1),
            (0x140d4, 0x321f03e0),
            (0x140d8, 0x321c03e2),
            (0x140dc, 0x9402edd9),
            (0x140e0, 0xd10283a8),
            (0x140e4, 0xc8dffd00),
            (0x140e8, 0x9402f282),
            (0x140ec, 0xaa0003e1),
            (0x140f0, 0x2a1f03e0),
            (0x140f4, 0xaa1f03e2),
            (0x140f8, 0x9402edb2),
            (0x140fc, 0x52800560),
            (0x14100, 0x9402eda8),
            (0x14104, 0x320003e0),
            (0x14108, 0x9402ed9e),
            (0x1410c, 0x910203e0),
            (0x14110, 0x321a03e1),
            (0x14114, 0x9402ede7),
            (0x14118, 0x320003e0),
            (0x1411c, 0x321e03e1),
            (0x14120, 0x2a1f03e2),
            (0x14124, 0x9402ee1f),
            (0x14128, 0x9402ed46),
            (0x1414c, 0xbc1843a8),
            (0x14150, 0xbc18c3a9),
            (0x14154, 0xd10283a8),
            (0x14158, 0xbc1943aa),
            (0x1415c, 0xbc19c3ab),
            (0x14160, 0xb81983bf),
            (0x14164, 0xb81903bf),
            (0x14168, 0xb81883bf),
            (0x1416c, 0xb81803bf),
            (0x141ac, 0x9101e3e8),
            (0x141b0, 0xc8dffd00),
            (0x141b4, 0x9402f253),
            (0x141b8, 0xaa1f03e1),
            (0x141bc, 0x9402ec45),
            (0x141c0, 0x9402ecf4),
            (0x141c4, 0x910023e1),
            (0x141c8, 0x2a1f03e0),
            (0x141cc, 0x321a03e2),
            (0x141d0, 0x9402ed9c),
            (0x141d4, 0xd10203a1),
            (0x141d8, 0x320003e0),
            (0x141dc, 0x321b03e2),
            (0x141e0, 0x9402ed98),
            (0x141e4, 0xd10243a1),
            (0x141e8, 0x321f03e0),
            (0x141ec, 0x321c03e2),
            (0x141f0, 0x9402ed94),
            (0x141f4, 0xd10283a8),
            (0x141f8, 0x91002108),
            (0x141fc, 0xc8dffd00),
            (0x14200, 0x9402f23c),
            (0x14204, 0xaa0003e1),
            (0x14208, 0x2a1f03e0),
            (0x1420c, 0xaa1f03e2),
            (0x14210, 0x9402ed6c),
            (0x14214, 0x52800560),
            (0x14218, 0x9402ed62),
            (0x1421c, 0x320003e0),
            (0x14220, 0x9402ed58),
            (0x14224, 0x910203e0),
            (0x14228, 0x321a03e1),
            (0x1422c, 0x9402eda1),
            (0x14230, 0x320003e0),
            (0x14234, 0x321e03e1),
            (0x14238, 0x2a1f03e2),
            (0x1423c, 0x9402edd9),
            (0x14240, 0x9402ed00),
            (0x14264, 0xd10283a9),
            (0x14268, 0xc8dffd20),
            (0x1426c, 0x91064268),
            (0x14270, 0xc85ffd13),
            (0x14274, 0xc809fd00),
            (0x14278, 0x35ffffc9),
            (0x1427c, 0xb4000080),
            (0x14280, 0xf9400008),
            (0x14284, 0xf9400108),
            (0x14288, 0xd63f0100),
            (0x125d8, 0x91066260),
            (0x125dc, 0x9402a3b1),
            (0x125e0, 0x91064268),
            (0x125e4, 0xc8dffd08),
            (0x125e8, 0x1e2703e1),
            (0x125ec, 0xf100011f),
            (0x125f0, 0x1e200c28),
            (0x125f4, 0x1e2e1000),
            (0x125f8, 0x1e202100),
            (0x125fc, 0x54000065),
            (0x12600, 0xaa1303e0),
            (0x12604, 0x97fffd3b),
            (0x12654, 0x91064268),
            (0x12658, 0xc8dffd00),
            (0x1265c, 0x9402f925),
            (0x12660, 0xaa0003e1),
            (0x12664, 0x2a1f03e0),
            (0x12668, 0xaa1f03e2),
            (0x1266c, 0x9402f455),
            (0x12670, 0x1e292100),
            (0x12674, 0x528000a8),
            (0x12678, 0x1a9fb500),
            (0x1267c, 0x9402f441),
            (0x12680, 0x1e2703ec),
            (0x12684, 0x4eac1d80),
            (0x12688, 0x4eac1d81),
            (0x1268c, 0x4eaa1d42),
            (0x12690, 0x4eab1d63),
            (0x12694, 0x910283e0),
            (0x12698, 0x9402d186),
            (0x1269c, 0x4eac1d80),
            (0x126a0, 0x4eac1d81),
            (0x126a4, 0x4ea91d22),
            (0x126a8, 0x4ea91d23),
            (0x126ac, 0x910283e0),
            (0x126b0, 0x9402d194),
            (0x126b4, 0x1e2c1000),
            (0x126b8, 0x1e200900),
            (0x126bc, 0x4ea81d01),
            (0x126c0, 0x94011020),
            (0x126c4, 0x910083e0),
            (0x126c8, 0x2d0407e0),
            (0x126cc, 0x2d050fe2),
            (0x126d0, 0x9401105c),
            (0x126d4, 0x2a0003e1),
            (0x126d8, 0x910283e0),
            (0x126dc, 0x9402d195),
            (0x126e0, 0x910283e0),
            (0x126e4, 0x9402d1cb),
            (0x13a84, 0x91066260),
            (0x13a88, 0x94029e66),
            (0x13a8c, 0xd0000a28),
            (0x13a90, 0xbd457101),
            (0x13a94, 0x1e2e1000),
            (0x13a98, 0x321a03e1),
            (0x13a9c, 0x2a1f03e2),
            (0x13aa0, 0x94029fd8),
            (0x145c0, 0x91066260),
            (0x145c4, 0x94029b97),
            (0x145c8, 0xb0000a28),
            (0x145cc, 0xbd457101),
            (0x145d0, 0x1e2703e0),
            (0x145d4, 0xa9417bfd),
            (0x145d8, 0x321a03e1),
            (0x145dc, 0x2a1f03e2),
            (0x145e0, 0xf84207f3),
            (0x145e4, 0x14029d07),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native instruction at {address:x}"
            );
        }
    }
}
