#[path = "../chapter.rs"]
mod chapter;
use chapter::{
    Controller,
    artwork::{self, Marker, Quad},
};
use std::io::{self, BufRead};
fn words(values: impl IntoIterator<Item = u32>) {
    for value in values {
        print!(" {value}");
    }
}
fn packet(p: Quad) {
    let textured = p.texture.is_some();
    print!(
        "P {} {} {}",
        if textured { 2 } else { 1 },
        p.blend,
        if textured { 0 } else { u32::MAX }
    );
    words(p.stencil);
    words(p.matrix.map(f32::to_bits));
    print!(" {}", if textured { 24 } else { 16 });
    for i in 0..4 {
        words([
            p.rect[if i & 1 == 0 { 0 } else { 2 }].to_bits(),
            p.rect[if i & 2 == 0 { 1 } else { 3 }].to_bits(),
            0,
            p.colors[i],
        ]);
        if textured {
            words([
                if i & 1 == 0 { 0 } else { 1f32.to_bits() },
                if i & 2 == 0 { 0 } else { 1f32.to_bits() },
            ]);
        }
    }
    println!();
}
fn main() {
    for (index, line) in io::stdin().lock().lines().enumerate() {
        let line = line.unwrap();
        let a: Vec<_> = line.split_whitespace().collect();
        let u = |i: usize| a[i].parse::<u32>().unwrap();
        let b = |i: usize| f32::from_bits(u32::from_str_radix(a[i], 16).unwrap());
        let markers: Vec<_> = (0..16).filter(|i| u(4) >> i & 1 != 0).collect();
        let controller = Controller::new(16, &markers, u(2) as usize, u(3) as usize).unwrap();
        let frame = artwork::frame(
            b(0),
            b(1),
            controller.entries(),
            controller.selected(),
            u(5) as usize,
            u(6) as usize,
            |row| match u(7) {
                1 => Marker::Chapter(if row & 1 == 0 { "Title" } else { "" }),
                2 => Marker::Choice {
                    title: if row & 1 == 0 { "Title" } else { "" },
                    text: "Choice",
                },
                3 => Marker::Quiz,
                _ => Marker::Other,
            },
        );
        println!("D {index}");
        for row in &frame.rows {
            print!("L {} ", row.index);
            for byte in row.text.as_bytes() {
                print!("{byte:02x}");
            }
            words([
                artwork::TEXT_WIDTH.to_bits(),
                artwork::FONT_SIZE.to_bits(),
                0,
                1,
                0,
                0,
                0,
                0,
            ]);
            println!();
        }
        for p in frame.panel {
            packet(p);
        }
        println!("K 4 0");
        packet(frame.clip);
        for row in frame.rows {
            print!("G {}", row.index);
            words(row.position.map(f32::to_bits));
            words(row.anchor.map(f32::to_bits));
            words(row.scale.map(f32::to_bits));
            words(row.color.map(f32::to_bits));
            words(row.color.map(f32::to_bits));
            words(frame.text_matrix.map(f32::to_bits));
            println!();
        }
        packet(frame.header);
        for p in frame.borders {
            packet(p);
        }
        if let Some(packets) = frame.scrollbar {
            for p in packets {
                packet(p);
            }
        }
        println!("E");
    }
    println!("T 0 chapter");
}
