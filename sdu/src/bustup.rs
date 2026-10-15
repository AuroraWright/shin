use std::{fmt::Write, path::PathBuf};

use image::RgbaImage;
use itertools::Itertools;
use shin_core::format::text::TextEncoding;
use shin_core::format::{bustup::default_builder::DefaultBustupBuilder, picture::PicBlock};

#[derive(clap::Subcommand, Debug)]
pub enum BustupCommand {
    /// Convert a BUP file into a bunch of PNG files (one for base image, one per expression, one per mouth position and one per eye position).
    ///
    /// The PNG files correspond to different images encoded in the BUP file and have to be composited to look like something you can find in-game.
    /// This export is suitable for conversion to other engines
    Decode {
        /// Decode Higurashi BUP4 v3, preserving its fixed animation slots.
        #[clap(long)]
        higurashi: bool,
        /// Path to the BUP file
        bustup_path: PathBuf,
        /// Path to the output directory
        output_path: PathBuf,
        /// Decode UTF-8 expression names used by Gerokasu2.
        #[clap(long, conflicts_with = "higurashi")]
        utf8_names: bool,
    },
    /// Convert a BUP file into pre-baked PNG files for every possible sprite state.
    ///
    /// The PNG files will look close to what they look in-game, but will use up a lot of disk space, because there will be redundancy.
    Bake {
        /// Decode Higurashi BUP4 v3 before compositing sprite states.
        #[clap(long)]
        higurashi: bool,
        /// Path to the BUP file
        bustup_path: PathBuf,
        /// Path to the output directory
        output_path: PathBuf,
        /// Decode UTF-8 expression names used by Gerokasu2.
        #[clap(long, conflicts_with = "higurashi")]
        utf8_names: bool,
    },
}

pub fn bustup_command(command: BustupCommand) -> anyhow::Result<()> {
    match command {
        BustupCommand::Decode {
            higurashi,
            bustup_path,
            output_path,
            utf8_names,
        } => {
            let bustup = std::fs::read(bustup_path)?;
            let bustup = read_bustup(&bustup, higurashi, utf8_names)?;

            std::fs::create_dir_all(&output_path)?;

            let mut metadata = String::new();
            writeln!(metadata, "expressions:")?;
            for (expression_name, expression) in bustup.expressions.iter().sorted_by_key(|v| v.0) {
                writeln!(metadata, "  \"{}\":", expression_name.replace('\"', "\\\""))?;
                if let Some(face1) = &expression.face1 {
                    writeln!(
                        metadata,
                        "    face1_pos: {:?}",
                        (face1.offset_x, face1.offset_y)
                    )?;
                }
                if let Some(face2) = &expression.face2 {
                    writeln!(
                        metadata,
                        "    face2_pos: {:?}",
                        (face2.offset_x, face2.offset_y)
                    )?;
                }

                writeln!(metadata, "    mouths:")?;
                for (i, mouth) in expression.mouths.iter().enumerate() {
                    writeln!(
                        metadata,
                        "      {}: {:?}",
                        i,
                        (mouth.offset_x, mouth.offset_y)
                    )?;
                }
                writeln!(metadata, "    eyes:")?;
                for (i, eye) in expression.eyes.iter().enumerate() {
                    writeln!(metadata, "      {}: {:?}", i, (eye.offset_x, eye.offset_y))?;
                }
            }
            std::fs::write(output_path.join("metadata.txt"), metadata)?;

            bustup.base_image.save(output_path.join("base.png"))?;

            for (expression_name, expression) in bustup.expressions.iter() {
                if let Some(face1) = &expression.face1 {
                    face1
                        .data
                        .save(output_path.join(format!("{}_face1.png", expression_name)))?;
                }
                if let Some(face2) = &expression.face2 {
                    face2
                        .data
                        .save(output_path.join(format!("{}_face2.png", expression_name)))?;
                }

                for (i, mouth) in expression.mouths.iter().enumerate() {
                    if !mouth.is_empty() {
                        mouth.data.save(
                            output_path.join(format!("{}_mouth_{}.png", expression_name, i)),
                        )?;
                    }
                }
                for (i, eye) in expression.eyes.iter().enumerate() {
                    if !eye.is_empty() {
                        eye.data
                            .save(output_path.join(format!("{}_eye_{}.png", expression_name, i)))?;
                    }
                }
            }

            Ok(())
        }
        BustupCommand::Bake {
            higurashi,
            bustup_path,
            output_path,
            utf8_names,
        } => {
            let bustup = std::fs::read(bustup_path)?;
            let bustup = read_bustup(&bustup, higurashi, utf8_names)?;

            std::fs::create_dir_all(&output_path)?;

            let base = bustup.base_image;

            for (expression_name, expression) in bustup.expressions {
                let mut image = base.clone();

                fn overlay(image: &mut RgbaImage, block: &PicBlock) {
                    image::imageops::overlay(
                        image,
                        &block.data,
                        block.offset_x as i64,
                        block.offset_y as i64,
                    );
                }

                if let Some(face1) = expression.face1 {
                    overlay(&mut image, &face1);
                }
                if let Some(face2) = expression.face2 {
                    overlay(&mut image, &face2);
                }

                let mouths = if expression.mouths.is_empty()
                    || (higurashi && expression.mouths.iter().all(PicBlock::is_empty))
                {
                    vec![PicBlock::empty()]
                } else {
                    expression.mouths
                };

                let eyes = if expression.eyes.is_empty()
                    || (higurashi && expression.eyes.iter().all(PicBlock::is_empty))
                {
                    vec![PicBlock::empty()]
                } else {
                    expression.eyes
                };

                for (mouth_index, mouth) in (0..).zip(&mouths) {
                    for (eye_index, eye) in (0..).zip(&eyes) {
                        let name = format!("{expression_name}_m{mouth_index}_e{eye_index}.png");
                        let mut image = image.clone();
                        overlay(&mut image, mouth);
                        overlay(&mut image, eye);

                        image.save(output_path.join(name))?;
                    }
                }
            }

            Ok(())
        }
    }
}

fn read_bustup(
    data: &[u8],
    higurashi: bool,
    utf8_names: bool,
) -> anyhow::Result<shin_core::format::bustup::default_builder::Bustup> {
    if higurashi {
        shin_core::format::bustup::higurashi::read_higurashi_bustup(data)
    } else {
        shin_core::format::bustup::read_bustup_with_encoding::<DefaultBustupBuilder>(
            data,
            (),
            if utf8_names {
                TextEncoding::Utf8
            } else {
                TextEncoding::ShiftJis
            },
        )
    }
}
