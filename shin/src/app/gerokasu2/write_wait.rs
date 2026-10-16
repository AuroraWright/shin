//! Desktop services for the original SaveGame/zero-answer Question owner.
//! Memory staging precedes this task; its open-state worker starts file IO.
use super::saves::Store;
use crate::{
    adv::gerokasu2::{
        config::dialog::GerDialog,
        save_write::{Controller, Event, PROMPT},
    },
    asset::system::AssetServer,
    render::PreRenderContext,
};
use anyhow::Result;
use shin_core::format::save::gerokasu2::SaveData;
use shin_core::time::Ticks;
use shin_render::render_pass::RenderPass;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Purpose {
    Manual,
    Config,
    Exit,
}
pub(super) struct Host {
    pub purpose: Purpose,
    controller: Controller,
    dialog: GerDialog,
    started: bool,
    result: Option<bool>,
    snapshot: Option<SaveData>,
}
impl Host {
    pub fn load(assets: &AssetServer, purpose: Purpose, data: &SaveData) -> Result<Self> {
        Ok(Self {
            purpose,
            controller: Controller::new(u32::from(purpose != Purpose::Manual)),
            dialog: GerDialog::load(assets, PROMPT, &[], 0, false, false)?,
            started: false,
            result: None,
            snapshot: (purpose != Purpose::Manual).then(|| data.clone()),
        })
    }
    pub fn update(&mut self, store: &mut Store, delta: Ticks) -> Option<bool> {
        if self.started {
            if self.result.is_none() {
                let result = match self.purpose {
                    Purpose::Manual => store.poll_manual_write(),
                    Purpose::Config | Purpose::Exit => match store.poll_flush() {
                        Err(error) => Some(Err(error)),
                        Ok(()) if !store.is_flush_pending() => Some(Ok(())),
                        Ok(()) => None,
                    },
                };
                if let Some(result) = result {
                    if let Err(error) = &result {
                        tracing::error!(%error, "Ger desktop write recovery; native SDK would abort");
                    }
                    self.result = Some(result.is_ok());
                }
            }
            match self.result {
                Some(true) => self.controller.io_finished(),
                Some(false) => self.controller.desktop_io_failed(),
                None => {}
            }
        }
        self.controller.update();
        for event in self.controller.take_events() {
            if let Event::Completed { success } = event {
                return Some(success);
            }
        }
        for response in self
            .dialog
            .update_notification(delta, || self.controller.poll_write())
        {
            self.controller.question_response(response.final_flag);
        }
        for event in self.controller.take_events() {
            if event == Event::RequestWrite {
                self.started = true;
                let result = match self.purpose {
                    Purpose::Manual => store.start_manual_write(Duration::from_secs(1)),
                    Purpose::Config | Purpose::Exit => store.request_frozen_flush(
                        self.snapshot.take().unwrap(),
                        Duration::from_secs(1),
                    ),
                };
                if let Err(error) = result {
                    tracing::error!(%error,"Ger write worker could not start");
                    self.result = Some(false);
                }
            }
        }
        None
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        self.dialog.prepare(context)
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        self.dialog.render(pass)
    }
}
