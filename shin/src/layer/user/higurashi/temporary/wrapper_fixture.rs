//! Test-only real notification constructor; no texture/GPU/audio initialization.
use super::*;
use crate::asset::texture_archive::{TextureArchive, TextureArchiveBuilder};
pub(crate) fn notification(kind: i32, alternate: bool, append: bool) -> Owner {
    struct Bits(u32);
    impl AcquiredBits for Bits {
        fn acquired(&self, id: usize) -> bool {
            self.0 & (1 << (2 * id)) != 0
        }
        fn acquire(&mut self, id: usize) -> Result<()> {
            self.0 |= 1 << (2 * id);
            Ok(())
        }
    }
    let content = match kind {
        12 => Content::Tips {
            alternate,
            rows: [(2, 3), (2, 3), (3, 4)]
                .into_iter()
                .map(|(id, atlas_word)| TipDescriptor { id, atlas_word })
                .collect(),
        },
        13 => Content::Otsu {
            argument: 27,
            append,
        },
        14 => Content::Kakera { ids: vec![0, 2, 6] },
        _ => panic!("Checked notification reference kinds12..14"),
    };
    let atlas = Arc::new(
        <<CharacterTextures as TextureArchive>::Builder as TextureArchiveBuilder>::new().build(),
    );
    Prepared::Notification { atlas, content }
        .construct(&mut Bits(0), true)
        .unwrap()
}
