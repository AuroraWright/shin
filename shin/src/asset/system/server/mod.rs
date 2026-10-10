mod accessor;
mod context;
#[cfg(test)]
mod registry_reference_tests;

use std::{
    borrow::Cow,
    fmt::Debug,
    fs::File,
    future::Future,
    hash::Hash,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::{Arc, RwLock, Weak},
};

use anyhow::{Context, Result, anyhow, bail};
use derive_more::From;
use indexmap::IndexMap;
use shin_core::{
    format::rom::{RomFileReader, RomReader},
    primitives::stateless_reader::StatelessFile,
};
use tracing::debug;

pub use self::{
    accessor::{AssetDataAccessor, AssetDataCursor},
    context::AssetLoadContext,
};

pub trait Asset: Send + Sync + Sized + 'static {
    type Args: Clone + Hash + Eq + Debug + Send + Sync + 'static;

    /// Load an asset from the provided data accessor.
    ///
    /// The future returned by this function will be spawned on the IO task pool.
    /// Blocking is to be avoided, as it will starve the IO task pool.
    /// CPU-intensive work should be offloaded to the compute task pool.
    fn load(
        context: &Arc<AssetLoadContext>,
        args: Self::Args,
        name: &str,
        data: AssetDataAccessor,
    ) -> impl Future<Output = Result<Self>> + Send;
}

struct AssetMap<T: Asset>(IndexMap<(String, T::Args), Weak<T>>);

impl<T: Asset> Deref for AssetMap<T> {
    type Target = IndexMap<(String, T::Args), Weak<T>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T: Asset> DerefMut for AssetMap<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct AssetServer {
    io: AssetIo,
    context: Arc<AssetLoadContext>,
    loaded_assets: RwLock<anymap3::Map<dyn core::any::Any + Send + Sync>>,
    #[cfg(test)]
    requested_paths: std::sync::Mutex<Vec<String>>,
}

impl AssetServer {
    pub fn context(&self) -> &AssetLoadContext {
        &self.context
    }
    pub fn new(io: AssetIo, context: AssetLoadContext) -> Self {
        Self {
            io,
            context: Arc::new(context),
            loaded_assets: RwLock::new(anymap3::Map::new()),
            #[cfg(test)]
            requested_paths: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub async fn load<T: Asset, P: AsRef<str>>(&self, path: P) -> Result<Arc<T>>
    where
        T::Args: Default,
    {
        self.load_with_args(path, T::Args::default()).await
    }

    pub async fn load_with_args<T: Asset, P: AsRef<str>>(
        &self,
        path: P,
        args: T::Args,
    ) -> Result<Arc<T>> {
        let path = path.as_ref();
        #[cfg(test)]
        self.requested_paths.lock().unwrap().push(path.to_string());

        let asset_map_key = (path.to_string(), args.clone());

        if let Some(loaded) = self.loaded_assets.read().unwrap().get::<AssetMap<T>>() {
            if let Some(asset) = loaded.get(&asset_map_key) {
                if let Some(asset) = asset.upgrade() {
                    debug!("Loaded asset from cache: {}", path);
                    return Ok(asset);
                }
            }
        }

        debug!("Loading asset: {}", path);

        // could not find the asset in the cache, load it
        let data = self
            .io
            .read_file(path)
            .with_context(|| format!("Reading asset {:?}", path))?;

        let context = self.context.clone();

        // just await the task
        let asset = T::load(&context, args, &path, data)
            .await
            .with_context(|| format!("Loading asset {:?}", path))?;
        let asset = Arc::new(asset);

        self.loaded_assets
            .write()
            .unwrap()
            .entry::<AssetMap<T>>()
            .or_insert_with(|| AssetMap(IndexMap::default()))
            .insert(asset_map_key, Arc::downgrade(&asset));

        Ok(asset)
    }

    #[cfg(test)]
    pub(crate) fn reference_requested_paths(&self) -> Vec<String> {
        self.requested_paths.lock().unwrap().clone()
    }

    /// Load an asset synchronously. This is useful for assets not requiring much CPU time to load.
    /// Though it might cause lockups if the loading is not blazing fast (tm).
    ///
    /// Ideally I want to get rid of all uses of this function
    pub fn load_sync<T: Asset>(&self, path: impl AsRef<str>) -> Result<Arc<T>>
    where
        T::Args: Default,
    {
        shin_tasks::block_on(self.load(path))
    }
}

#[derive(Debug, From)]
pub enum AssetIo {
    Dir(DirAssetIo),
    RomFile(RomAssetIo),
    Layered(LayeredAssetIo),
}

impl AssetIo {
    pub fn new_dir(dir_path: impl AsRef<Path>) -> Result<Self> {
        let dir_path = dir_path.as_ref();
        let meta = std::fs::metadata(dir_path).with_context(|| {
            format!(
                "Failed to get metadata for {:?}, cannot use as asset directory",
                dir_path
            )
        })?;
        if !meta.is_dir() {
            bail!(
                "{:?} is not a directory, cannot use as asset directory",
                dir_path
            );
        }

        Ok(AssetIo::Dir(DirAssetIo::new(dir_path.to_path_buf())))
    }

    pub fn new_rom(rom_path: impl AsRef<Path>) -> Result<Self> {
        let rom_path = rom_path.as_ref();
        let rom = File::open(rom_path)
            .context("Opening ROM file")
            .and_then(|rom| StatelessFile::new(rom).context("Creating stateless file"))
            .and_then(RomReader::new)
            .with_context(|| format!("Failed to open {:?}, cannot use as asset ROM", rom_path))?;

        Ok(AssetIo::RomFile(RomAssetIo::new(
            rom,
            Some(&rom_path.display().to_string()),
        )))
    }

    fn read_file(&self, path: &str) -> Result<AssetDataAccessor> {
        match self {
            AssetIo::Dir(io) => io.read_file(path),
            AssetIo::RomFile(io) => io.read_file(path),
            AssetIo::Layered(io) => io.read_file(path),
        }
    }
}

#[derive(Debug)]
pub struct DirAssetIo {
    root_path: PathBuf,
}

impl DirAssetIo {
    pub fn new(root_path: PathBuf) -> Self {
        Self { root_path }
    }

    fn read_file(&self, path: &str) -> Result<AssetDataAccessor> {
        // 1. check if the file exists
        let full_path = self.root_path.join(path.trim_start_matches('/'));
        if !full_path.exists() {
            bail!("Asset {:?} not found", path);
        }
        Ok(AssetDataAccessor::from_file(full_path))
    }
}

pub struct RomAssetIo {
    rom: Arc<RomReader<StatelessFile>>,
    label: Option<String>,
}

impl Debug for RomAssetIo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RomAssetIo")
            .field(&self.label.as_deref().unwrap_or("unnamed"))
            .finish()
    }
}

impl RomAssetIo {
    pub fn new(rom: RomReader<StatelessFile>, label: Option<&str>) -> Self {
        Self {
            rom: Arc::new(rom),
            label: label.map(|s| s.to_string()),
        }
    }

    fn read_file(&self, path: &str) -> Result<AssetDataAccessor> {
        // 1. find the file in the ROM
        // Native ROM open (27db0) normalizes separators before raw index lookup.
        let normalized = if path.contains('\\') {
            Cow::Owned(path.replace('\\', "/"))
        } else {
            Cow::Borrowed(path)
        };
        let file = self
            .rom
            .find_file(&normalized)
            .with_context(|| format!("Finding asset {:?}", path))?;
        let file = RomFileReader::new(self.rom.clone(), file);
        Ok(AssetDataAccessor::from_rom_file(file))
    }
}

#[derive(Debug, Default)]
pub struct LayeredAssetIo {
    io: Vec<AssetIo>,
}

impl LayeredAssetIo {
    pub fn new() -> Self {
        Self { io: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.io.is_empty()
    }

    pub fn with(&mut self, io: AssetIo) {
        self.io.push(io);
    }

    pub fn try_with_dir(&mut self, dir_path: impl AsRef<Path>) -> Result<()> {
        self.with(AssetIo::new_dir(dir_path)?);
        Ok(())
    }

    pub fn try_with_rom(&mut self, rom_path: impl AsRef<Path>) -> Result<()> {
        self.with(AssetIo::new_rom(rom_path)?);
        Ok(())
    }

    fn read_file(&self, path: &str) -> Result<AssetDataAccessor> {
        let mut errors = Vec::new();

        for io in &self.io {
            match io.read_file(path) {
                Ok(data) => return Ok(data),
                Err(err) => errors.push(err),
            }
        }

        Err(anyhow!(
            "Failed to read asset {:?} from all layers: {:?}",
            path,
            errors
        ))
    }
}

#[cfg(test)]
mod update_reference_tests {
    #[test]
    #[ignore = "requires supplied base and 1.03 patch ROMs (SHIN_REFERENCE_ROM)"]
    fn reference_update_103_overlay_and_asset_decoding() {
        use super::*;
        use crate::asset::system::locate_assets;
        use shin_core::format::{
            audio::{AudioDecoder, AudioFrameSource, AudioSource, read_audio},
            picture::{SimpleMergedPicture, read_picture},
            rom::{IndexEntry, RomReader},
            scenario::Scenario,
            texture_archive::read_texture_archive,
        };
        use shin_core::primitives::stateless_reader::StatelessFile;
        use std::{io::Read, sync::Arc};
        let base = PathBuf::from(std::env::var_os("SHIN_REFERENCE_ROM").unwrap());
        let directory = base.parent().unwrap();
        let patch = directory.join("patch.rom");
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(patch).unwrap()).unwrap())
                .unwrap();
        let layered = locate_assets(Some(directory)).unwrap();
        let paths: Vec<_> = rom
            .traverse()
            .filter_map(|(path, entry)| matches!(entry, IndexEntry::File(_)).then_some(path))
            .collect();
        assert_eq!(paths.len(), 7);
        for path in paths {
            let mut bytes = Vec::new();
            rom.open_file(rom.find_file(&path).unwrap())
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            assert_eq!(
                layered.read_file(&path).unwrap().read_all_sync(),
                bytes,
                "Patch precedence {path}"
            );
            match &bytes[..4] {
                b"SNR " => {
                    let scenario = Scenario::new(bytes.into()).unwrap();
                    let mut reader = scenario.instruction_reader(scenario.entrypoint_address());
                    for _ in 0..16 {
                        reader.read().unwrap();
                    }
                    println!(
                        "1.03 scenario tables and first16 instructions parse; entry{:?}",
                        scenario.entrypoint_address()
                    );
                }
                b"PIC4" => {
                    let picture = read_picture::<SimpleMergedPicture>(&bytes, ()).unwrap();
                    assert!(picture.image.width() > 0 && picture.image.height() > 0);
                    println!(
                        "1.03 {path}: decoded{}x{}",
                        picture.image.width(),
                        picture.image.height()
                    );
                }
                b"TXA4" => {
                    let archive = read_texture_archive(&bytes).unwrap();
                    assert!(!archive.textures.is_empty());
                    println!("1.03 {path}: decoded{} textures", archive.textures.len());
                }
                b"NXA1" => {
                    let file = Arc::new(read_audio(&bytes).unwrap());
                    let count = file.info().num_samples;
                    let mut source = AudioSource::new(AudioDecoder::new_pcm16(file).unwrap());
                    let mut frames = 0;
                    while source.read_sample().is_some() {
                        frames += 1;
                    }
                    assert!(!source.inner().is_failed());
                    assert_eq!(frames, count);
                    println!("1.03 {path}: decoded{frames} signed16 source frames");
                }
                other => panic!("Unexpected patch format {other:?}"),
            }
        }
        let mut base_rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(base).unwrap()).unwrap())
                .unwrap();
        let fallback = "/bgm/umib_001.nxa";
        let mut bytes = Vec::new();
        base_rom
            .open_file(base_rom.find_file(fallback).unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(layered.read_file(fallback).unwrap().read_all_sync(), bytes);
        println!("1.03 patch precedence and base BGM fallback pass; no audio device opened");
    }
}
