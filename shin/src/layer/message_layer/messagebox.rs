use glam::{Mat4, Vec2, vec2, vec3};
use shin_core::{primitives::color::FloatColor4, vm::command::types::MessageboxType};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

use crate::{
    layer::message_layer::{MessageLayer, SlidingOutMessagebox},
    render::normalized_projection_matrix,
};

#[derive(Debug, Copy, Clone)]
pub struct Messagebox {
    pub ty: MessageboxType,
    pub natural_slide: f32,
    pub height: f32,
}

impl From<SlidingOutMessagebox> for Messagebox {
    fn from(value: SlidingOutMessagebox) -> Self {
        Self {
            ty: value.ty,
            natural_slide: value.slide_out.value(),
            height: value.height,
        }
    }
}

const MESSAGEBOX_TEXTURE_SIZE: Vec2 = vec2(1648.0, 288.0);
macro_rules! make_vertices {
    ($color:expr; $([$x:expr, $y:expr, $x_tex:expr, $y_tex:expr]),*) => {
        &[
            $(
                PosColTexVertex {
                    position: vec3($x, $y, 1.0),
                    color: $color,
                    texture_position: vec2($x_tex / MESSAGEBOX_TEXTURE_SIZE.x, $y_tex / MESSAGEBOX_TEXTURE_SIZE.y),
                }
            ),*
        ] as &[_]
    };
}

impl MessageLayer {
    pub(super) fn render_messagebox(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
        messagebox: Messagebox,
    ) {
        if messagebox.natural_slide == 0.0 {
            return;
        }
        let final_slide_progress = messagebox.natural_slide * self.modal_slide.value();

        match messagebox.ty {
            MessageboxType::Neutral | MessageboxType::WitchSpace | MessageboxType::Ushiromiya => {
                // draw a "normal" messagebox at the bottom of the screen
                let texture = match messagebox.ty {
                    MessageboxType::Neutral => &self.messagebox_textures.message_window_1,
                    MessageboxType::WitchSpace => &self.messagebox_textures.message_window_2,
                    MessageboxType::Ushiromiya => &self.messagebox_textures.message_window_3,
                    _ => unreachable!(),
                }
                .as_source();

                // Native0xb2fc0..0xb2fdc reads the saved percentage each draw.
                let msgwinalpha = f32::from(self.settings.v13_msgwinalpha) * 0.01;

                let transform = transform
                    * Mat4::from_translation(vec3(
                        0.0,
                        (1.0 - final_slide_progress) * 96.0 + (1080.0 - messagebox.height) - 32.0,
                        0.0,
                    ));

                // slide progress also affects the messagebox opacity!
                let color =
                    FloatColor4::from_rgba(1.0, 1.0, 1.0, final_slide_progress * msgwinalpha)
                        .into_unorm();

                let char_name_width = self.char_name_width;

                let char_name_vertices = if char_name_width == 0.0 {
                    make_vertices!(color;
                        [130.0, -32.0, 0.0, 144.0],
                        [130.0, 80.0, 0.0, 256.0],
                        [178.0, -32.0, 48.0, 144.0],
                        [178.0, 80.0, 48.0, 256.0],
                        [1742.0, -32.0, 64.0, 144.0],
                        [1742.0, 80.0, 64.0, 256.0],
                        [1790.0, -32.0, 112.0, 144.0],
                        [1790.0, 80.0, 112.0, 256.0]
                    )
                } else {
                    make_vertices!(color;
                        [130.0, -32.0, 0.0, 0.0],
                        [130.0, 80.0, 0.0, 112.0],
                        [178.0, -32.0, 48.0, 0.0],
                        [178.0, 80.0, 48.0, 112.0],
                        [178.0 + char_name_width, -32.0, 64.0, 0.0],
                        [178.0 + char_name_width, 80.0, 64.0, 112.0],
                        [290.0 + char_name_width, -32.0, 160.0, 0.0],
                        [290.0 + char_name_width, 80.0, 160.0, 112.0],
                        [1742.0, -32.0, 176.0, 0.0],
                        [1742.0, 80.0, 176.0, 112.0],
                        [1790.0, -32.0, 224.0, 0.0],
                        [1790.0, 80.0, 224.0, 112.0]
                    )
                };

                let high = self.height.value() + 32.0;
                let mid = high - 256.0;

                let message_vertices = make_vertices!(color;
                    /* 0  */ [130.0, 80.0, 240.0, 16.0],
                    /* 1  */ [178.0, 80.0, 288.0, 16.0],
                    /* 2  */ [446.0, 80.0, 304.0, 16.0],
                    /* 3  */ [1790.0, 80.0, 1648.0, 16.0],
                    /* 4  */ [130.0, mid, 240.0, 32.0],
                    /* 5  */ [178.0, mid, 288.0, 32.0],
                    /* 6  */ [446.0, mid, 304.0, 32.0],
                    /* 7  */ [1790.0, mid, 1648.0, 32.0],
                    /* 8  */ [130.0, high, 240.0, 288.0],
                    /* 9  */ [178.0, high, 288.0, 288.0],
                    /* 10 */ [446.0, high, 304.0, 288.0],
                    /* 11 */ [1790.0, high, 1648.0, 288.0]
                );
                let message_indices = &[0, 4, 1, 5, 2, 6, 3, 7, 11, 6, 10, 5, 9, 4, 8];

                pass.run(builder.color_blend_type(ColorBlendType::Layer1).build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: char_name_vertices,
                        },
                        sprite: texture,
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));

                pass.run(builder.color_blend_type(ColorBlendType::Layer1).build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexAndIndexData {
                            vertices: message_vertices,
                            indices: message_indices,
                        },
                        sprite: texture,
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
            MessageboxType::Novel => {
                // draw a full-screen translucent overlay
                let builder = builder.color_blend_type(ColorBlendType::Layer1);

                let transform = normalized_projection_matrix();
                QuadVertices::new()
                    .with_box(0.0, 0.0, 1.0, 1.0)
                    .with_color(
                        FloatColor4::from_rgba(0.0, 0.0, 0.0, final_slide_progress * 0.7)
                            .into_unorm(),
                    )
                    .fill(pass, builder, transform);
            }
            MessageboxType::Transparent | MessageboxType::NoText => {
                // nothing!
            }
        }
    }
}

#[cfg(test)]
impl MessageLayer {
    pub(crate) fn assert_reference_window_opacity(
        &mut self,
        context: &mut crate::render::PreRenderContext,
    ) {
        use shin_core::{format::save::Settings, primitives::color::UnormColor};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Saved message window opacity"),
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
        self.height.set_value(360.0);
        let mut draw = |this: &mut Self, ty, natural, modal, alpha| {
            this.modal_slide.set_value(modal);
            let mut settings = Settings::default();
            settings.v13_msgwinalpha = alpha;
            this.set_settings(&settings);
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Saved window alpha",
                );
                pass.clear(Some(UnormColor::from_rgba(128, 128, 128, 255)), None, None);
                this.render_messagebox(
                    &mut pass,
                    RenderRequestBuilder::new(),
                    crate::render::top_left_projection_matrix(),
                    Messagebox {
                        ty,
                        natural_slide: natural,
                        height: 360.0,
                    },
                );
            }
            crate::layer::user::readback_test_texture(context, &target)
        };
        let blank = |pixels: &[u8]| pixels.chunks_exact(4).all(|p| p == [128, 128, 128, 255]);
        for ty in [
            MessageboxType::Neutral,
            MessageboxType::WitchSpace,
            MessageboxType::Ushiromiya,
        ] {
            for name_width in [0.0, 300.0] {
                self.char_name_width = name_width;
                let frames = [0, 50, 85, 100].map(|alpha| draw(self, ty, 1.0, 1.0, alpha));
                assert!(
                    blank(&frames[0]),
                    "Zero saved opacity hides the window and name backing"
                );
                let mut changed = 0;
                let mut worst = 0.0f32;
                // Below the name backing, each pixel has one window fragment.
                // Full opacity provides independently measured source coverage;
                // intermediate frames must follow the native percentage blend.
                for y in 900..1000 {
                    for x in 200..1700 {
                        let offset = (y * 1920 + x) * 4;
                        if frames[3][offset..offset + 3] != [128, 128, 128] {
                            changed += 1;
                        }
                        for (index, alpha) in [(1, 127.0 / 255.0), (2, 216.0 / 255.0)] {
                            for channel in 0..3 {
                                let expected = 128.0
                                    + (f32::from(frames[3][offset + channel]) - 128.0) * alpha;
                                worst = worst.max(
                                    (f32::from(frames[index][offset + channel]) - expected).abs(),
                                );
                            }
                        }
                    }
                }
                assert!(changed > 1000, "Supplied window must have visible coverage");
                assert!(worst <= 2.0, "Saved opacity blend error{worst} for{ty:?}");
                println!(
                    "Saved window opacity{ty:?},name{name_width}: {changed} body pixels, worst channel error{worst}"
                );
            }
        }
        self.char_name_width = 300.0;
        let quarter = draw(self, MessageboxType::Neutral, 0.5, 0.5, 100);
        let equivalent = draw(self, MessageboxType::Neutral, 1.0, 0.25, 100);
        assert_eq!(
            quarter, equivalent,
            "Natural and modal slide multiply for position and opacity"
        );
        assert!(blank(&draw(self, MessageboxType::Neutral, 0.5, 0.5, 0)));
        assert!(blank(&draw(self, MessageboxType::Neutral, 0.0, 1.0, 100)));
        assert!(blank(&draw(
            self,
            MessageboxType::Transparent,
            1.0,
            1.0,
            100
        )));
        assert!(blank(&draw(self, MessageboxType::NoText, 1.0, 1.0, 100)));
        let novel = draw(self, MessageboxType::Novel, 1.0, 1.0, 0);
        assert_eq!(novel, draw(self, MessageboxType::Novel, 1.0, 1.0, 100));
        assert!(!blank(&novel), "Novel uses its native fixed70% overlay");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_saved_window_opacity() {
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
            (0xb2e8c, 0xbd4006a0),
            (0xb2e94, 0x54003560),
            (0xb2e9c, 0x7100115f),
            (0xb2ea0, 0x54003508),
            (0xb2ea4, 0xbd403e81),
            (0xb2ec4, 0x1e21080a),
            (0xb2fbc, 0xf9424908),
            (0xb2fc0, 0x3d42f500),
            (0xb2fc8, 0xbd494901),
            (0xb2fd0, 0x7e21d800),
            (0xb2fd4, 0x1e210800),
            (0xb2fdc, 0x1e200943),
            (0xb2fe4, 0x97fe8ddb),
            (0xb2ff4, 0x97fe8e13),
            (0xb34fc, 0xbd45a500),
            (0xb3500, 0x1e200943),
        ] {
            assert_eq!(word(a), expected, "native window opacity{a:x}");
        }
        assert_eq!(word(0x159948), 0.01f32.to_bits());
        assert_eq!(word(0x1595a4), 0.7f32.to_bits());
    }
}
