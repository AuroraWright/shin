//! Ger BD060 hierarchy and3F7B0 remembered selection; retained across parent switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    Background { primary: u16, secondary_offset: u16 },
    Bustup(u16),
    EndExpressionGroup,
    EndPoseGroup,
    EndDefinition,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Background {
    pub primary: u16,
    pub secondary: Option<u16>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pose {
    pub expressions: Vec<u16>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outfit {
    pub poses: Vec<Pose>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Character {
    pub outfits: Vec<Outfit>,
}
#[derive(Clone, Debug)]
pub struct Catalog {
    pub backgrounds: Vec<Background>,
    pub characters: Vec<Character>,
}
impl Catalog {
    pub fn from_segments(info: impl IntoIterator<Item = Segment>) -> Result<Self, &'static str> {
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
                Segment::Background {
                    primary: primary_picture_id,
                    secondary_offset: secondary_picture_id_offset,
                } => {
                    if !background_phase {
                        return Err("Background after character definitions");
                    }
                    this.backgrounds.push(Background {
                        primary: primary_picture_id,
                        secondary: (secondary_picture_id_offset != 0)
                            .then(|| primary_picture_id.wrapping_add(secondary_picture_id_offset)),
                    });
                }
                Segment::Bustup(bustup_id) => {
                    if background_phase {
                        return Err("Bustup before background terminator");
                    }
                    expressions.push(bustup_id);
                }
                Segment::EndDefinition if background_phase => {
                    background_phase = false;
                }
                Segment::EndExpressionGroup | Segment::EndPoseGroup | Segment::EndDefinition => {
                    if background_phase || expressions.is_empty() {
                        return Err("Empty expression group");
                    }
                    poses.push(Pose {
                        expressions: std::mem::take(&mut expressions),
                    });
                    if !matches!(segment, Segment::EndExpressionGroup) {
                        outfits.push(Outfit {
                            poses: std::mem::take(&mut poses),
                        });
                    }
                    if matches!(segment, Segment::EndDefinition) {
                        this.characters.push(Character {
                            outfits: std::mem::take(&mut outfits),
                        });
                    }
                }
            }
        }
        // The sized section ends with the last expression list, without a
        // 0x22 marker. EOF closes that definition.
        if background_phase {
            return Err("Missing background terminator");
        }
        if !expressions.is_empty() {
            poses.push(Pose { expressions });
        }
        if !poses.is_empty() {
            outfits.push(Outfit { poses });
        }
        if !outfits.is_empty() {
            this.characters.push(Character { outfits });
        }
        if this.backgrounds.is_empty() || this.characters.is_empty() {
            return Err("Empty character-box metadata");
        }
        Ok(this)
    }
}

#[derive(Clone, Debug)]
struct OutfitSelection {
    pose: usize,
    expressions: Vec<usize>,
}
#[derive(Clone, Debug)]
struct CharacterSelection {
    outfit: usize,
    outfits: Vec<OutfitSelection>,
}
/// Native constructor37f38..38100 allocates a parallel hierarchy of zeroed
/// selections. 3ad70 reads character258, outfit u16, pose u16, then expression
/// u16 indexed by pose; switching parent groups preserves their prior choices.
#[derive(Clone, Debug)]
pub struct Selection {
    character: usize,
    background: usize,
    characters: Vec<CharacterSelection>,
}
impl Selection {
    pub fn background_index(&self) -> usize {
        self.background
    }
    pub fn new(catalog: &Catalog) -> Self {
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
    pub fn bustup(&self, catalog: &Catalog) -> u16 {
        let (c, o, p, e) = self.indices();
        catalog.characters[c].outfits[o].poses[p].expressions[e]
    }
    pub fn background<'a>(&self, catalog: &'a Catalog) -> &'a Background {
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
    pub fn select_expression(&mut self, catalog: &Catalog, index: usize) -> bool {
        let (c, o, p, _) = self.indices();
        if index >= catalog.characters[c].outfits[o].poses[p].expressions.len() {
            return false;
        }
        self.characters[c].outfits[o].expressions[p] = index;
        true
    }
    pub fn select_background(&mut self, catalog: &Catalog, index: usize) -> bool {
        if index >= catalog.backgrounds.len() {
            return false;
        }
        self.background = index;
        true
    }
}
