//! Native initial worker3f1e0 and blocking replacement tasks3e240/3ebb0.
//! Publication records retain the native slot/property/transition requirements;
//! the scene must install them before updating or drawing its layer group.
use super::{bustup_catalog::Background, bustup_menu::Request};
use crate::{
    adv::character_view::CharacterTextures,
    asset::{
        bustup::{Bustup, BustupArgs, CharacterId},
        mask::MaskTexture,
        picture::Picture,
        system::AssetServer,
    },
    layer::{
        DrawableLayer, LayerGroup,
        user::{BustupLayer, NullLayer, PictureLayer, UserLayer},
    },
};
use anyhow::Context;
use shin_core::{
    format::scenario::{
        Scenario,
        info::{BustupId, PictureId},
    },
    time::Ticks,
    vm::command::types::LayerProperty,
};
use std::sync::{Arc, mpsc};

pub(super) struct LayerPublication {
    pub slot: u8,
    pub layer: UserLayer,
    pub transition_ticks: Ticks,
    pub inherit_properties: bool,
}
impl LayerPublication {
    pub fn install(mut self, group: &mut LayerGroup) {
        let slot = shin_core::vm::command::types::LayerbankId::new(self.slot);
        if let Some(previous) = group.get_layer(slot) {
            self.inherit_from(previous);
        }
        let wiper = (self.transition_ticks != Ticks::ZERO).then(|| {
            crate::wiper::AnyWiper::Default(crate::wiper::DefaultWiper::new(self.transition_ticks))
        });
        group.add_layer_with_wiper(slot, self.layer, wiper);
    }

    /// Native old virtual68 -> new virtual60 copies the live property object
    /// at publication, including outstanding animations rather than just values.
    pub fn inherit_from(&mut self, previous: &UserLayer) {
        if self.inherit_properties {
            self.layer.set_properties(previous.properties().clone());
        }
    }
}
pub(super) enum Publication {
    Atlas(Arc<CharacterTextures>),
    Mask(Arc<MaskTexture>),
    Layer(LayerPublication),
    InitialComplete,
}
type Load = shin_tasks::AsyncTask<anyhow::Result<Vec<LayerPublication>>>;
pub(super) struct BustupResources {
    server: Arc<AssetServer>,
    scenario: Arc<Scenario>,
    initial: Option<shin_tasks::AsyncTask<anyhow::Result<()>>>,
    initial_events: mpsc::Receiver<Publication>,
    pending: Vec<Load>,
}
async fn picture(
    server: &AssetServer,
    scenario: &Scenario,
    id: Option<PictureId>,
) -> anyhow::Result<UserLayer> {
    let Some(id) = id else {
        return Ok(UserLayer::Null(NullLayer::new()));
    };
    let index = usize::try_from(id.into_i32()).context("Negative Bustup background ID")?;
    let info = scenario
        .info_tables()
        .picture_info
        .get(index)
        .context("Missing Bustup background metadata")?;
    let path = info.path();
    let asset = server.load::<Picture, _>(&path).await?;
    Ok(UserLayer::Picture(PictureLayer::new(asset, Some(path))))
}
async fn bustup(
    server: &AssetServer,
    scenario: &Scenario,
    id: BustupId,
) -> anyhow::Result<UserLayer> {
    let index = usize::try_from(id.into_i32()).context("Negative gallery Bustup ID")?;
    let info = scenario
        .info_tables()
        .bustup_info
        .get(index)
        .context("Missing gallery Bustup metadata")?;
    let path = info.path();
    let asset = server
        .load_with_args::<Bustup, _>(
            &path,
            BustupArgs {
                expression: info.emotion.to_string(),
                character_id: CharacterId::from_metadata(info.lipsync_character_id),
                disable_animations: true,
            },
        )
        .await?;
    Ok(UserLayer::Bustup(BustupLayer::new(asset, Some(path))))
}
fn initial_layer(slot: u8, mut layer: UserLayer) -> LayerPublication {
    let props = layer.properties_mut();
    let background = slot < 2;
    for (property, value) in [
        (
            LayerProperty::RenderPosition,
            if background {
                1000.0 + f32::from(slot)
            } else {
                1000.0
            },
        ),
        (
            LayerProperty::TranslateZ,
            if background { 2000.0 } else { 1000.0 },
        ),
    ] {
        props.property_tweener_mut(property).fast_forward_to(value);
    }
    if background {
        for property in [LayerProperty::ScaleX, LayerProperty::ScaleY] {
            props.property_tweener_mut(property).fast_forward_to(2000.0);
        }
    }
    LayerPublication {
        slot,
        layer,
        transition_ticks: Ticks::ZERO,
        inherit_properties: false,
    }
}
fn replacement(slot: u8, layer: UserLayer) -> LayerPublication {
    LayerPublication {
        slot,
        layer,
        transition_ticks: Ticks::from_f32(10.0),
        inherit_properties: true,
    }
}
impl BustupResources {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: Arc<Scenario>,
        background: &Background,
        id: BustupId,
    ) -> Self {
        let (send, receive) = mpsc::channel();
        let worker_server = server.clone();
        let worker_scenario = scenario.clone();
        let ids = [Some(background.primary), background.secondary];
        let worker = shin_tasks::async_io::spawn(async move {
            let atlas = worker_server
                .load::<CharacterTextures, _>("/bupmode.txa")
                .await?;
            if send.send(Publication::Atlas(atlas)).is_err() {
                return Ok(());
            }
            let mask = worker_server.load::<MaskTexture, _>("/default.msk").await?;
            if send.send(Publication::Mask(mask)).is_err() {
                return Ok(());
            }
            for (slot, id) in ids.into_iter().enumerate() {
                let layer = picture(&worker_server, &worker_scenario, id).await?;
                if send
                    .send(Publication::Layer(initial_layer(slot as u8, layer)))
                    .is_err()
                {
                    return Ok(());
                }
            }
            let layer = bustup(&worker_server, &worker_scenario, id).await?;
            let _ = send.send(Publication::Layer(initial_layer(10, layer)));
            Ok(())
        });
        Self {
            server,
            scenario,
            initial: Some(worker),
            initial_events: receive,
            pending: Vec::new(),
        }
    }
    pub fn has_blocking_loads(&self) -> bool {
        self.initial.is_some() || !self.pending.is_empty()
    }
    pub fn request(&mut self, request: Request) {
        let server = self.server.clone();
        let scenario = self.scenario.clone();
        let task = match request {
            Request::LoadBustup(id) => shin_tasks::async_io::spawn(async move {
                Ok(vec![replacement(10, bustup(&server, &scenario, id).await?)])
            }),
            Request::LoadBackground { primary, secondary } => {
                shin_tasks::async_io::spawn(async move {
                    // Start both immutable loads together. No slot publishes before
                    // both futures resolve, including the absent secondary Null layer.
                    let (a, b) = futures::try_join!(
                        picture(&server, &scenario, Some(primary)),
                        picture(&server, &scenario, secondary)
                    )?;
                    Ok(vec![replacement(0, a), replacement(1, b)])
                })
            }
            _ => return,
        };
        self.pending.push(task);
    }
    /// Snapshot blocking status before this poll, so the completion frame still
    /// suppresses older input. Completed jobs publish newest first and retire;
    /// their ten-tick layer transitions continue independently in the scene.
    pub fn poll(&mut self) -> anyhow::Result<Vec<Publication>> {
        let finished = self.initial.as_mut().and_then(|task| task.poll_naive());
        let mut events = self.initial_events.try_iter().collect::<Vec<_>>();
        if let Some(result) = finished {
            self.initial = None;
            result?;
            events.push(Publication::InitialComplete);
        }
        for i in (0..self.pending.len()).rev() {
            if let Some(result) = self.pending[i].poll_naive() {
                self.pending.remove(i);
                events.extend(result?.into_iter().map(Publication::Layer));
            }
        }
        Ok(events)
    }
}
#[cfg(test)]
mod tests;
