//! FirstBoot64cf0: black loading scene, ordered worker65aa0, LoadGame d5310.
//! This build's logo worker659e0 sets phase-1; platform task66490 is a no-op.
use crate::{
    adv::{
        GlobalData, SaveRecovery,
        assets::{AdvAssets, AdvFonts},
        characters::view::CharacterTextures,
        storage_retry_dialog::StorageRetryDialog,
    },
    asset::{effects::EffectsAtlasBorrow, system::AssetServer},
    audio::GlobalAudio,
    layer::message_layer::MessageboxTextures,
};
use shin_core::format::{scenario::Scenario, sysse::SysSe};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
};

/// Native global resources outlive scene retirement. ADV's menu atlas is loaded
/// separately, when its own resource task starts.
pub(super) struct CommonAssets {
    pub scenario: Arc<Scenario>,
    pub fonts: AdvFonts,
    pub messagebox: Arc<MessageboxTextures>,
    pub _system: Arc<CharacterTextures>,
    pub effects: EffectsAtlasBorrow,
    pub _sysse: Arc<SysSe>,
}
impl CommonAssets {
    async fn load(server: &AssetServer, bank: mpsc::Sender<Arc<SysSe>>) -> anyhow::Result<Self> {
        let sysse: Arc<SysSe> = server.load("/sysse.bin").await?;
        let _ = bank.send(sysse.clone());
        let system_font = server.load("/system.fnt").await?;
        let medium_font = server.load("/newrodin-medium.fnt").await?;
        let bold_font = server.load("/newrodin-bold.fnt").await?;
        let system = server.load("/systex.txa").await?;
        let messagebox = server.load("/msgtex.txa").await?;
        let effects = server.initialize_effects_atlas().await?;
        let scenario = server.load("/main.snr").await?;
        Ok(Self {
            scenario,
            fonts: AdvFonts {
                system_font,
                medium_font,
                bold_font,
            },
            messagebox,
            _system: system,
            effects,
            _sysse: sysse,
        })
    }

    pub async fn load_adv(&self, server: &AssetServer) -> anyhow::Result<AdvAssets> {
        Ok(AdvAssets {
            scenario: self.scenario.clone(),
            fonts: self.fonts.clone(),
            messagebox_textures: self.messagebox.clone(),
            menu_textures: server.load("/adv.txa").await?,
            scene_load_mask: server.load("/default.msk").await?,
        })
    }
}

enum Phase {
    Assets(shin_tasks::AsyncTask<anyhow::Result<CommonAssets>>),
    StartLoad,
    Load(shin_tasks::ComputeTask<anyhow::Result<GlobalData>>),
    Complete,
    ReadFailure(StorageRetryDialog),
    RecoveryNotice {
        dialog: StorageRetryDialog,
        recovery: SaveRecovery,
    },
}
pub(super) struct FirstBoot {
    phase: Phase,
    bank: mpsc::Receiver<Arc<SysSe>>,
    save_path: Option<PathBuf>,
    server: Arc<AssetServer>,
    pointer_held: bool,
    pointer_extent: (u16, u16),
}
pub(super) struct BootUpdate {
    pub assets: Option<CommonAssets>,
    pub complete: bool,
}
impl FirstBoot {
    pub fn new(server: Arc<AssetServer>, save_path: Option<PathBuf>) -> Self {
        let (send, bank) = mpsc::channel();
        let worker_server = server.clone();
        Self {
            phase: Phase::Assets(shin_tasks::async_io::spawn(async move {
                CommonAssets::load(&worker_server, send).await
            })),
            bank,
            save_path,
            server,
            pointer_held: false,
            pointer_extent: (1280, 720),
        }
    }
    pub fn update(
        &mut self,
        globals: &mut GlobalData,
        audio: &mut GlobalAudio,
        delta: shin_core::time::Ticks,
    ) -> anyhow::Result<BootUpdate> {
        if let Ok(bank) = self.bank.try_recv() {
            audio.system_player.bank = Some(bank);
        }
        let mut result = BootUpdate {
            assets: None,
            complete: false,
        };
        match &mut self.phase {
            Phase::Assets(task) => {
                if let Some(assets) = task.poll_naive() {
                    let assets = assets?;
                    // Publication can finish between this frame's bank-channel
                    // poll and task poll. Install the same bank before LoadGame.
                    audio.system_player.bank = Some(assets._sysse.clone());
                    result.assets = Some(assets);
                    self.phase = Phase::StartLoad;
                }
            }
            Phase::StartLoad => {
                let path = self.save_path.clone();
                // LoadGame is added after FirstBoot's task-group poll. Start
                // its disk worker on a later frame, keeping file IO off render.
                self.phase = Phase::Load(shin_tasks::compute::spawn(move || {
                    GlobalData::open(path, None)
                }));
            }
            Phase::Load(_) | Phase::Complete => {}
            Phase::ReadFailure(dialog) => {
                if dialog.update(delta) {
                    // The acknowledged retry starts on the next frame, using
                    // the same path. Never attach default persistence on error.
                    self.phase = Phase::StartLoad;
                }
            }
            Phase::RecoveryNotice { dialog, .. } => {
                if dialog.update(delta) {
                    let Phase::RecoveryNotice { recovery, .. } =
                        std::mem::replace(&mut self.phase, Phase::Complete)
                    else {
                        unreachable!()
                    };
                    self.phase = Phase::Load(shin_tasks::compute::spawn(move || {
                        GlobalData::recover_save(recovery)
                    }));
                }
            }
        }
        // Native d46c4 falls through from worker creation into its status poll;
        // don't add a fixed frame of latency when the worker completes quickly.
        if let Phase::Load(task) = &mut self.phase {
            if let Some(loaded) = task.poll_naive() {
                match loaded {
                    Ok(loaded) => {
                        *globals = loaded;
                        self.phase = Phase::Complete;
                        result.complete = true;
                    }
                    Err(error) if GlobalData::is_save_read_error(&error) => {
                        tracing::warn!("Startup save read failed: {error:#}");
                        self.phase = Phase::ReadFailure(StorageRetryDialog::load(
                            &self.server,
                            self.pointer_held,
                            self.pointer_extent,
                        )?);
                    }
                    Err(error) if GlobalData::is_save_recovery_backup_error(&error) => {
                        tracing::warn!("Startup save recovery failed: {error:#}");
                        self.phase = Phase::ReadFailure(StorageRetryDialog::load_backup_error(
                            &self.server,
                            self.pointer_held,
                            self.pointer_extent,
                        )?);
                    }
                    Err(error) => {
                        if let Some(recovery) = GlobalData::save_recovery(&error) {
                            tracing::warn!("Startup save decode failed: {error:#}");
                            let dialog = StorageRetryDialog::load_recovery(
                                &self.server,
                                self.pointer_held,
                                self.pointer_extent,
                                recovery.kind,
                            )?;
                            self.phase = Phase::RecoveryNotice { dialog, recovery };
                        } else {
                            return Err(error);
                        }
                    }
                }
            }
        }
        Ok(result)
    }
    pub fn input(
        &mut self,
        position: glam::Vec2,
        extent: (u16, u16),
        elapsed: f64,
        input: &enum_map::EnumMap<super::AppAction, shin_input::ActionState>,
    ) {
        self.pointer_held = input[super::AppAction::PointerClick].is_held;
        self.pointer_extent = extent;
        if let Phase::ReadFailure(dialog) | Phase::RecoveryNotice { dialog, .. } = &mut self.phase {
            dialog.input(position, self.pointer_held, extent, elapsed, input);
        }
    }
    pub fn prepare(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        audio: &mut GlobalAudio,
    ) {
        if let Phase::ReadFailure(dialog) | Phase::RecoveryNotice { dialog, .. } = &mut self.phase {
            for sound in dialog.take_sounds() {
                audio.system_player.play(
                    &self.server,
                    sound,
                    shin_core::vm::command::types::Volume::default(),
                );
            }
            dialog.prepare(context);
        }
    }
    pub fn render(&self, pass: &mut shin_render::render_pass::RenderPass) {
        if let Phase::ReadFailure(dialog) | Phase::RecoveryNotice { dialog, .. } = &self.phase {
            dialog.render(pass);
        }
    }
    #[cfg(test)]
    pub(super) fn reference_uses_plain_notice(&self) -> bool {
        match &self.phase {
            Phase::ReadFailure(dialog) | Phase::RecoveryNotice { dialog, .. } => {
                dialog.reference_uses_plain_frame()
            }
            _ => false,
        }
    }
    #[cfg(test)]
    pub fn reference_retry_button(&self) -> Option<glam::Vec2> {
        match &self.phase {
            Phase::ReadFailure(dialog) | Phase::RecoveryNotice { dialog, .. } => {
                Some(dialog.reference_button())
            }
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn reference_recovery_kind(&self) -> Option<crate::adv::SaveRecoveryKind> {
        match &self.phase {
            Phase::RecoveryNotice { recovery, .. } => Some(recovery.kind),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_first_boot_order_loadgame_and_frame_clear() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x653cc, 0x94026b39),
            (0x6563c, 0x94026a99),
            (0x65648, 0x36f804a8),
            (0x65658, 0x7100081f),
            (0x6567c, 0x9401bf25),
            (0x656d8, 0xb9007268),
            (0x66590, 0xbd400660),
            (0x665b8, 0xfd000120),
            (0x665c8, 0x9402b94a),
            (0xd5338, 0x97fffc02),
            (0xd46b8, 0xb9404288),
            (0xd46c0, 0xb9004288),
            (0xd46d0, 0x97fd5024),
            (0xd4814, 0xf9401a80),
            (0xd4824, 0x91022281),
            (0xd4828, 0xd63f0100),
            (0x27070, 0x1e2e1000),
            (0x27074, 0x32000be0),
            (0x27078, 0x32081fe1),
            (0x2707c, 0x2a1f03e2),
            (0x27080, 0x9402a4e0),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native {address:x}"
            );
        }
        let name = b"N4shin8LoadGameE";
        assert_eq!(&exe[0x1728e9 + 256..0x1728e9 + 256 + name.len()], name);
        for (address, path) in [
            (0x15e8d7, "/sysse.bin"),
            (0x15bcef, "/system.fnt"),
            (0x15a979, "/newrodin-medium.fnt"),
            (0x161e9c, "/newrodin-bold.fnt"),
            (0x162cfb, "/systex.txa"),
            (0x15ad4b, "/msgtex.txa"),
            (0x161346, "/effects.txa"),
            (0x162122, "/main.snr"),
        ] {
            assert_eq!(
                &exe[address + 256..address + 256 + path.len()],
                path.as_bytes()
            );
        }
    }
}
