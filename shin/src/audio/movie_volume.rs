use shin_core::{
    format::{save::Settings, scenario::info::MovieVolumeSource},
    vm::command::types::Volume,
};

/// Movie settings are sampled at creation and every update, independently of
/// the BGM player's voice-focus gain (native 0xc2744 and 0xc2a84).
#[derive(Debug, Copy, Clone)]
pub struct MovieVolumeSettings {
    bgm: u8,
    se: u8,
    voice: u8,
}

impl From<&Settings> for MovieVolumeSettings {
    fn from(settings: &Settings) -> Self {
        Self {
            bgm: settings.v0_bgmvol,
            se: settings.v1_sfxvol,
            voice: settings.v2_voicevol,
        }
    }
}

impl Default for MovieVolumeSettings {
    fn default() -> Self {
        Self::from(&Settings::default())
    }
}

impl MovieVolumeSettings {
    pub fn volume(self, source: MovieVolumeSource, local: Volume) -> Volume {
        let percent = match source {
            MovieVolumeSource::Ignore => return local,
            MovieVolumeSource::Bgm => self.bgm,
            MovieVolumeSource::Se => self.se,
            MovieVolumeSource::Voice => self.voice,
        };
        Volume((f32::from(percent) * 0.01) * local.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_sources_sample_distinct_settings_and_ignore_voice_focus() {
        let mut settings = Settings::default();
        settings.v0_bgmvol = 70;
        settings.v1_sfxvol = 25;
        settings.v2_voicevol = 90;
        settings.v3_sysvol = 0;
        settings.v4_voicefocus = true;
        let gain = MovieVolumeSettings::from(&settings);
        for (source, expected) in [
            (MovieVolumeSource::Ignore, 0.4),
            (MovieVolumeSource::Bgm, 0.28),
            (MovieVolumeSource::Se, 0.1),
            (MovieVolumeSource::Voice, 0.36),
        ] {
            assert!((gain.volume(source, Volume(0.4)).0 - expected).abs() < 0.000001);
        }
        settings.v0_bgmvol = 0;
        let muted = MovieVolumeSettings::from(&settings);
        assert_eq!(
            muted.volume(MovieVolumeSource::Bgm, Volume(0.4)),
            Volume(0.0)
        );
        assert_eq!(
            muted.volume(MovieVolumeSource::Ignore, Volume(0.4)),
            Volume(0.4)
        );
        settings.v0_bgmvol = 100;
        let restored = MovieVolumeSettings::from(&settings);
        assert_eq!(
            restored.volume(MovieVolumeSource::Bgm, Volume(0.4)),
            Volume(0.4)
        );
    }
}
