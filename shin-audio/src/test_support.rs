//! Deterministic sample capture through Kira's actual renderer.
//! Its built-in mock discards PCM; this backend exposes it to reference checks.
use kira::{
    Frame,
    backend::{Backend, Renderer},
};

pub use kira::backend::mock::MockBackendSettings;

pub struct SampleBackend {
    sample_rate: u32,
    renderer: Option<Renderer>,
    samples: Vec<f32>,
}
impl Backend for SampleBackend {
    type Settings = MockBackendSettings;
    type Error = ();
    fn setup(settings: Self::Settings, internal_buffer_size: usize) -> Result<(Self, u32), ()> {
        Ok((
            Self {
                sample_rate: settings.sample_rate,
                renderer: None,
                samples: vec![0.0; internal_buffer_size * 2],
            },
            settings.sample_rate,
        ))
    }
    fn start(&mut self, renderer: Renderer) -> Result<(), ()> {
        self.renderer = Some(renderer);
        Ok(())
    }
}
impl SampleBackend {
    pub fn on_start_processing(&mut self) {
        self.renderer
            .as_mut()
            .expect("Renderer initialized")
            .on_start_processing();
    }
    pub fn process(&mut self) -> Frame {
        self.renderer
            .as_mut()
            .expect("Renderer initialized")
            .process(&mut self.samples, 2);
        Frame::new(self.samples[0], self.samples[1])
    }
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate;
        self.renderer
            .as_mut()
            .expect("Renderer initialized")
            .on_change_sample_rate(sample_rate);
    }
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }
}
