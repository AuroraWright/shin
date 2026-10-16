//!8133C padded glyph quad and81A7C outline/fill thresholds.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub origin: [i32; 2],
    pub dimensions: [u32; 2],
    pub uv_scale: [f32; 2],
    pub font_metrics: [i32; 2],
    pub extra_metric: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: [f32; 4],
    pub color: f32,
}
pub struct Draw {
    pub vertices: [Vertex; 4],
    pub outline: [f32; 2],
    pub fill: [f32; 2],
}
impl Draw {
    pub fn new(
        g: &Geometry,
        position: [f32; 2],
        scale: [f32; 2],
        is_ruby: bool,
    ) -> Result<Self, &'static str> {
        if !position.into_iter().all(f32::is_finite)
            || !scale.into_iter().all(|v| v.is_finite() && v > 0.)
            || !g.uv_scale.into_iter().all(f32::is_finite)
            || !g
                .dimensions
                .into_iter()
                .all(|v| v > 0 && v <= i32::MAX as u32)
            || !g.extra_metric.is_finite()
            || g.extra_metric <= 0.
            || g.font_metrics[0]
                .checked_add(g.font_metrics[1])
                .is_none_or(|v| v <= 0)
        {
            return Err("Ger backlog glyph geometry/position/scale is invalid");
        }
        let pixels = [
            g.dimensions[0] as f32 * scale[0],
            g.dimensions[1] as f32 * scale[1],
        ];
        let near = [
            (g.origin[0] as f32).mul_add(scale[0], 220.) + -2.,
            (g.origin[1] as f32).mul_add(scale[1], -2.),
        ];
        let far = [(pixels[0] + near[0]) + 4., (pixels[1] + near[1]) + 4.];
        let ratio = [
            (((far[0] - near[0]) / pixels[0]) + -1.) * 0.5,
            (((far[1] - near[1]) / pixels[1]) + -1.) * 0.5,
        ];
        let uv_near = [-ratio[0] * g.uv_scale[0], -ratio[1] * g.uv_scale[1]];
        let uv_far = [
            (ratio[0] + 1.) * g.uv_scale[0],
            (ratio[1] + 1.) * g.uv_scale[1],
        ];
        let inverse = 1. / (g.font_metrics[0] + g.font_metrics[1]) as f32;
        let effective = (g.origin[1] + g.font_metrics[0]) as f32;
        let colors = if is_ruby {
            [0.; 2]
        } else {
            [
                inverse * effective,
                inverse * (effective + g.dimensions[1] as f32),
            ]
        };
        let vertices = std::array::from_fn(|i| Vertex {
            position: [
                position[0] + if i & 1 == 0 { near[0] } else { far[0] },
                position[1] + if i & 2 == 0 { near[1] } else { far[1] },
                if i & 1 == 0 { uv_near[0] } else { uv_far[0] },
                if i & 2 == 0 { uv_near[1] } else { uv_far[1] },
            ],
            color: colors[usize::from(i & 2 != 0)],
        });
        let center = (-4. / g.extra_metric) / scale[1] + 0.;
        let radius = (1.5 / g.extra_metric) / scale[1];
        Ok(Self {
            vertices,
            outline: [(center - radius).max(-1.), (radius + center).max(-1.)],
            fill: [(-radius).max(-1.), radius.max(-1.)],
        })
    }
}
