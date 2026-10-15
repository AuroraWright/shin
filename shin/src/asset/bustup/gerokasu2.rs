//! Later-engine UTF-8 expression selection with the shared BUP GPU/cache loader.
use super::*;

/// A separate asset type keeps name decoding explicit and the legacy asset
/// registry unchanged. Both dialects reuse block promises, uploads and owners.
pub struct Gerokasu2Bustup(pub Arc<Bustup>);

impl Asset for Gerokasu2Bustup {
    type Args = BustupArgs;

    async fn load(
        context: &Arc<AssetLoadContext>,
        args: BustupArgs,
        name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        load_bustup(context, args, name, data, TextEncoding::Utf8)
            .await
            .map(|bustup| Self(Arc::new(bustup)))
    }
}
