//! Original a08e4 worker order: quiz atlas, mask, eleven BUPs, ten question TXAs.
#[cfg(test)]
use super::Model;
use crate::{
    adv::character_view::CharacterTextures,
    asset::{mask::MaskTexture, system::AssetServer},
    layer::user::higurashi::HigurashiUserLayer,
};
use anyhow::{Result, ensure};
use shin_core::{format::scenario::higurashi::HigurashiScenario, vm::command::types::LayerType};
use std::sync::{Arc, mpsc};

pub(super) const BUSTUP_IDS: [[i32; 11]; 9] = [
    [0, 1, 1, 2, 2, 2, 3, 5, 6, 4, 5],
    [7, 8, 8, 8, 8, 8, 9, 10, 10, 11, 10],
    [13, 12, 12, 14, 14, 14, 15, 16, 17, 18, 19],
    [20, 21, 21, 21, 21, 21, 22, 22, 23, 22, 22],
    [24, 25, 25, 25, 25, 26, 27, 25, 26, 25, 25],
    [28, 29, 30, 30, 30, 31, 29, 29, 32, 31, 32],
    [34, 33, 33, 35, 35, 35, 36, 37, 38, 39, 40],
    [42, 41, 47, 47, 44, 47, 43, 46, 47, 43, 45],
    [48, 54, 48, 54, 50, 54, 48, 52, 53, 51, 49],
];
#[derive(Debug, Clone)]
pub(in crate::app::higurashi) struct Plan {
    pub bustups: [i32; 11],
    pub questions: [String; 10],
}
impl Plan {
    pub fn bustups(scenario: &HigurashiScenario, context: i32) -> Result<[i32; 11]> {
        let context = usize::try_from(context)?;
        let bustups = *BUSTUP_IDS
            .get(context)
            .ok_or_else(|| anyhow::anyhow!("Quiz character context outside0..9"))?;
        for id in bustups {
            ensure!(
                (id as usize) < scenario.bustup_info.len(),
                "Quiz BUP ID outside scenario"
            );
        }
        Ok(bustups)
    }
    #[cfg(test)]
    pub fn new(model: &Model, scenario: &HigurashiScenario) -> Result<Self> {
        let bustups = Self::bustups(scenario, model.args[1])?;
        Ok(Self {
            bustups,
            questions: model
                .questions
                .map(|q| format!("/quiz/{}.txa", q.descriptor().texture)),
        })
    }
}
pub(super) struct Prepared {
    pub mask: Option<Arc<MaskTexture>>,
    pub bustups: [HigurashiUserLayer; 11],
    pub questions: [Arc<CharacterTextures>; 10],
}
struct Job {
    task: Option<shin_tasks::AsyncTask<Result<Prepared>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            self.retirement.clone().retire(());
        }
    }
}
pub(super) struct Loader {
    pub atlas: Option<Arc<CharacterTextures>>,
    pub prepared: Option<Prepared>,
    pub published: bool,
    job: Option<Job>,
    received: mpsc::Receiver<Arc<CharacterTextures>>,
    fault: Option<String>,
}
impl Loader {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: Arc<HigurashiScenario>,
        plan: Plan,
        mut seed: impl FnMut() -> u32 + Send + 'static,
    ) -> Self {
        let (sender, received) = mpsc::channel();
        let task = shin_tasks::async_io::spawn_loader(async move {
            let atlas = server.load::<CharacterTextures, _>("/quiz.txa").await?;
            let _ = sender.send(atlas);
            let mask = server.load::<MaskTexture, _>("/default.msk").await?;
            let mut bustups = Vec::with_capacity(11);
            for id in plan.bustups {
                // Native bca14 receives setting0 here. Each owner has four
                // fresh entropy seeds even when its cached BUP is shared.
                let seeds = std::array::from_fn(|_| seed());
                bustups.push(
                    HigurashiUserLayer::load_with_setting(
                        &server,
                        &scenario,
                        LayerType::Bustup,
                        (id, 0, 0, 0, 0, 0, 0, 0),
                        seeds,
                        0,
                    )
                    .await?,
                );
            }
            let mut questions = Vec::with_capacity(10);
            for path in plan.questions {
                questions.push(server.load::<CharacterTextures, _>(path).await?);
            }
            Ok(Prepared {
                mask: Some(mask),
                bustups: bustups.try_into().ok().unwrap(),
                questions: questions.try_into().ok().unwrap(),
            })
        });
        let retirement = task.retirement_handle().unwrap();
        Self {
            atlas: None,
            prepared: None,
            published: false,
            job: Some(Job {
                task: Some(task),
                retirement,
            }),
            received,
            fault: None,
        }
    }
    pub fn poll(&mut self) -> Result<u32> {
        if let Some(fault) = &self.fault {
            anyhow::bail!("Quiz preparation failed:{fault}");
        }
        while let Ok(atlas) = self.received.try_recv() {
            self.atlas = Some(atlas);
        }
        if let Some(job) = &mut self.job {
            if let Some(task) = &mut job.task {
                if let Some(result) = task.poll_naive() {
                    job.task = None;
                    match result {
                        Ok(prepared) => self.prepared = Some(prepared),
                        Err(error) => {
                            self.fault = Some(format!("{error:#}"));
                            return Err(error);
                        }
                    }
                    while let Ok(atlas) = self.received.try_recv() {
                        self.atlas = Some(atlas);
                    }
                    ensure!(
                        self.atlas.is_some(),
                        "Quiz worker completed without parent atlas"
                    );
                }
            }
        }
        Ok(if self.prepared.is_some() { 2 } else { 1 })
    }
    pub fn publish(&mut self) -> Result<()> {
        ensure!(!self.published, "Quiz resources already published");
        let prepared = self
            .prepared
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Quiz resources are not ready"))?;
        for bustup in &mut prepared.bustups {
            bustup
                .properties
                .tweener_mut(0)
                .unwrap()
                .fast_forward_to(-500.0);
        }
        self.published = true;
        Ok(())
    }
    pub fn retire_first(&mut self) {
        self.job.take();
        if let Some(prepared) = &mut self.prepared {
            prepared.mask.take();
        }
    }
    #[cfg(test)]
    pub(super) fn retirement(&self) -> Option<shin_tasks::LoaderRetirement> {
        self.job.as_ref().map(|job| job.retirement.clone())
    }
}

#[cfg(test)]
mod tests;
