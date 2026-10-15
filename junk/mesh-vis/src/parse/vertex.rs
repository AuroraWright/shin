use iced::font;
use iced_core::{Length, alignment::Horizontal, color};
use iced_widget::{Column, Space, Text, scrollable, text};

use crate::{VertexView, descriptors::VertexDescriptor};

pub enum VertexParseResult {
    Valid,
    PartiallyValid,
}

impl VertexParseResult {
    pub fn view(&self) -> Text<'static> {
        match self {
            VertexParseResult::Valid => text("Valid vertices").color(color!(0x00c000)),
            VertexParseResult::PartiallyValid => text("Invalid vertices!").color(color!(0xc00000)),
        }
    }
}

pub struct VertexParseState {
    pub descriptor: VertexDescriptor,
    pub vertices: Vec<VertexView>,
    pub result: VertexParseResult,
}

impl VertexParseState {
    pub fn parse(raw_floats: &[[u8; 4]], descriptor: VertexDescriptor) -> Self {
        let vertex_len = descriptor.float_count_per_vertex() as usize;

        let mut valid_prefix_len = raw_floats.len();
        valid_prefix_len -= valid_prefix_len % vertex_len;
        let valid_prefix = &raw_floats[..valid_prefix_len];

        let vertices = valid_prefix
            .chunks_exact(vertex_len)
            .map(|v| VertexView::new(v, descriptor.fields))
            .collect();

        let result = if valid_prefix_len == raw_floats.len() {
            VertexParseResult::Valid
        } else {
            VertexParseResult::PartiallyValid
        };

        Self {
            descriptor,
            vertices,
            result,
        }
    }

    pub fn view<Message: 'static>(&self) -> Column<'static, Message> {
        Column::with_children([
            // ---
            Space::new().height(10).into(),
            // ---
            self.result.view().into(),
            text(format!("{0} (0x{0:x}) vertices", self.vertices.len())).into(),
            scrollable(
                iced_widget::table::table(
                        self.descriptor
                            .column_names()
                            .into_iter()
                            .enumerate()
                        .map(|(index, name)| {
                            iced_widget::table::column(
                                text(name).font(
                                    iced::Font {
                                        weight: font::Weight::Bold,
                                        ..iced::Font::MONOSPACE
                                    }),
                                move |values: Vec<crate::descriptors::VertexPrimitiveValue>| {
                                    text(values[index].to_string()).font(iced::Font::MONOSPACE)
                                },
                            )
                            .align_x(Horizontal::Right)
                            }),
                    self.vertices.iter()
                            .map(|vertex| vertex.primitive_values()
                    .collect::<Vec<_>>()),
                )
                .padding_x(10).width(Length::Fill),
            )
            .width(Length::Fill)
            .into(),
        ])
    }
}
