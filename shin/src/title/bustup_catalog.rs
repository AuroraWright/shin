//! Character-box hierarchy returned by ea3f0 and indexed by BustupMode3ad70.
use shin_core::format::scenario::{
    info::{BustupId, CharacterBoxInfo, CharacterBoxSegment, PictureId},
    instruction_elements::FromNumber,
};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Background {
    pub primary: PictureId,
    pub secondary: Option<PictureId>,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Pose {
    pub expressions: Vec<BustupId>,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Outfit {
    pub poses: Vec<Pose>,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Character {
    pub outfits: Vec<Outfit>,
}
#[derive(Debug)]
pub(super) struct BustupCatalog {
    pub backgrounds: Vec<Background>,
    pub characters: Vec<Character>,
}
impl BustupCatalog {
    pub fn new(info: &CharacterBoxInfo) -> anyhow::Result<Self> {
        let mut this = Self {
            backgrounds: Vec::new(),
            characters: Vec::new(),
        };
        let mut background_phase = true;
        let mut expressions = Vec::new();
        let mut poses = Vec::new();
        let mut outfits = Vec::new();
        for segment in info {
            match segment {
                CharacterBoxSegment::Background {
                    primary_picture_id,
                    secondary_picture_id_offset,
                } => {
                    anyhow::ensure!(background_phase, "Background after character definitions");
                    this.backgrounds.push(Background {
                        primary: *primary_picture_id,
                        secondary: (*secondary_picture_id_offset != 0).then(|| {
                            PictureId::from_number(
                                primary_picture_id.into_i32()
                                    + i32::from(*secondary_picture_id_offset),
                            )
                        }),
                    });
                }
                CharacterBoxSegment::Bustup { bustup_id } => {
                    anyhow::ensure!(!background_phase, "Bustup before background terminator");
                    expressions.push(*bustup_id);
                }
                CharacterBoxSegment::EndDefinition if background_phase => {
                    background_phase = false;
                }
                CharacterBoxSegment::EndExpressionGroup
                | CharacterBoxSegment::EndPoseGroup
                | CharacterBoxSegment::EndDefinition => {
                    anyhow::ensure!(
                        !background_phase && !expressions.is_empty(),
                        "Empty expression group"
                    );
                    poses.push(Pose {
                        expressions: std::mem::take(&mut expressions),
                    });
                    if !matches!(segment, CharacterBoxSegment::EndExpressionGroup) {
                        outfits.push(Outfit {
                            poses: std::mem::take(&mut poses),
                        });
                    }
                    if matches!(segment, CharacterBoxSegment::EndDefinition) {
                        this.characters.push(Character {
                            outfits: std::mem::take(&mut outfits),
                        });
                    }
                }
            }
        }
        // The sized section ends with the last expression list, without a
        // 0x22 marker (original final IDs10903..10908). EOF closes that definition.
        anyhow::ensure!(!background_phase, "Missing background terminator");
        if !expressions.is_empty() {
            poses.push(Pose { expressions });
        }
        if !poses.is_empty() {
            outfits.push(Outfit { poses });
        }
        if !outfits.is_empty() {
            this.characters.push(Character { outfits });
        }
        anyhow::ensure!(
            !this.backgrounds.is_empty() && !this.characters.is_empty(),
            "Empty character-box metadata"
        );
        Ok(this)
    }
}

struct OutfitSelection {
    pose: usize,
    expressions: Vec<usize>,
}
struct CharacterSelection {
    outfit: usize,
    outfits: Vec<OutfitSelection>,
}
/// Native constructor37f38..38100 allocates a parallel hierarchy of zeroed
/// selections. 3ad70 reads character258, outfit u16, pose u16, then expression
/// u16 indexed by pose; switching parent groups preserves their prior choices.
pub(super) struct BustupSelection {
    character: usize,
    background: usize,
    characters: Vec<CharacterSelection>,
}
impl BustupSelection {
    pub fn background_index(&self) -> usize {
        self.background
    }
    pub fn new(catalog: &BustupCatalog) -> Self {
        Self {
            character: 0,
            background: 0,
            characters: catalog
                .characters
                .iter()
                .map(|c| CharacterSelection {
                    outfit: 0,
                    outfits: c
                        .outfits
                        .iter()
                        .map(|o| OutfitSelection {
                            pose: 0,
                            expressions: vec![0; o.poses.len()],
                        })
                        .collect(),
                })
                .collect(),
        }
    }
    pub fn indices(&self) -> (usize, usize, usize, usize) {
        let c = &self.characters[self.character];
        let o = &c.outfits[c.outfit];
        (self.character, c.outfit, o.pose, o.expressions[o.pose])
    }
    pub fn bustup(&self, catalog: &BustupCatalog) -> BustupId {
        let (c, o, p, e) = self.indices();
        catalog.characters[c].outfits[o].poses[p].expressions[e]
    }
    pub fn background<'a>(&self, catalog: &'a BustupCatalog) -> &'a Background {
        &catalog.backgrounds[self.background]
    }
    pub fn select_character(&mut self, index: usize) -> bool {
        if index >= self.characters.len() {
            return false;
        }
        self.character = index;
        true
    }
    pub fn select_outfit(&mut self, index: usize) -> bool {
        let c = &mut self.characters[self.character];
        if index >= c.outfits.len() {
            return false;
        }
        c.outfit = index;
        true
    }
    pub fn select_pose(&mut self, index: usize) -> bool {
        let c = &mut self.characters[self.character];
        let o = &mut c.outfits[c.outfit];
        if index >= o.expressions.len() {
            return false;
        }
        o.pose = index;
        true
    }
    pub fn select_expression(&mut self, catalog: &BustupCatalog, index: usize) -> bool {
        let (c, o, p, _) = self.indices();
        if index >= catalog.characters[c].outfits[o].poses[p].expressions.len() {
            return false;
        }
        self.characters[c].outfits[o].expressions[p] = index;
        true
    }
    pub fn select_background(&mut self, catalog: &BustupCatalog, index: usize) -> bool {
        if index >= catalog.backgrounds.len() {
            return false;
        }
        self.background = index;
        true
    }
}
#[cfg(test)]
mod tests;
