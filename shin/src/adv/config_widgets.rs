//! ConfigMenu's authored rows and native ButtonMenuItem/SliderMenuItem controls.
//! Coordinates are local to the content panel; the screen owns tabs and clipping.
use shin_core::{
    format::save::Settings,
    time::{Ticks, Tween, Tweener},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSetting {
    SystemVolume,
    BgmVolume,
    SoundVolume,
    VoiceVolume,
    VoiceFocus,
    VoicePanning,
    MessageSpeed,
    SkipUnread,
    WindowOpacity,
    AutoDelay,
    MusicTitle,
    SceneTitle,
    TouchEffects,
    SuppressAutoSleep,
}
impl ConfigSetting {
    pub fn value(self, s: &Settings) -> u8 {
        match self {
            Self::SystemVolume => s.v3_sysvol,
            Self::BgmVolume => s.v0_bgmvol,
            Self::SoundVolume => s.v1_sfxvol,
            Self::VoiceVolume => s.v2_voicevol,
            Self::VoiceFocus => s.v4_voicefocus.into(),
            Self::VoicePanning => s.v5_voicepanapot.into(),
            Self::MessageSpeed => s.v9_msgspeed,
            Self::SkipUnread => s.v11_disallowskipunread.into(),
            Self::WindowOpacity => s.v13_msgwinalpha,
            Self::AutoDelay => s.v10_skipspeed,
            Self::MusicTitle => s.v15.into(),
            Self::SceneTitle => s.v14_showroutenavi.into(),
            Self::TouchEffects => s.v17_showscenetitle.into(),
            Self::SuppressAutoSleep => s.v18_showsongtitle.into(),
        }
    }
    fn set(self, s: &mut Settings, value: u8) {
        match self {
            Self::SystemVolume => s.v3_sysvol = value,
            Self::BgmVolume => s.v0_bgmvol = value,
            Self::SoundVolume => s.v1_sfxvol = value,
            Self::VoiceVolume => s.v2_voicevol = value,
            Self::VoiceFocus => s.v4_voicefocus = value != 0,
            Self::VoicePanning => s.v5_voicepanapot = value != 0,
            Self::MessageSpeed => s.v9_msgspeed = value,
            Self::SkipUnread => s.v11_disallowskipunread = value != 0,
            Self::WindowOpacity => s.v13_msgwinalpha = value,
            Self::AutoDelay => s.v10_skipspeed = value,
            Self::MusicTitle => s.v15 = value != 0,
            Self::SceneTitle => s.v14_showroutenavi = value != 0,
            Self::TouchEffects => s.v17_showscenetitle = value != 0,
            Self::SuppressAutoSleep => s.v18_showsongtitle = value != 0,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigControlKind {
    /// Native subtype0 volume,1 opacity,2 text/auto speed.
    Slider(u8),
    /// Label bands in the authored buttons atlas, in value order.
    Buttons([u8; 2]),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigRowSpec {
    pub setting: ConfigSetting,
    pub y: u16,
    pub title_band: u8,
    pub kind: ConfigControlKind,
}
use ConfigControlKind::{Buttons, Slider};
use ConfigSetting::*;
pub const CONFIG_ROWS: [&[ConfigRowSpec]; 3] = [
    &[
        ConfigRowSpec {
            setting: SystemVolume,
            y: 0,
            title_band: 0,
            kind: Slider(0),
        },
        ConfigRowSpec {
            setting: BgmVolume,
            y: 102,
            title_band: 1,
            kind: Slider(0),
        },
        ConfigRowSpec {
            setting: SoundVolume,
            y: 204,
            title_band: 2,
            kind: Slider(0),
        },
        ConfigRowSpec {
            setting: VoiceVolume,
            y: 306,
            title_band: 3,
            kind: Slider(0),
        },
        ConfigRowSpec {
            setting: VoiceFocus,
            y: 408,
            title_band: 4,
            kind: Buttons([1, 0]),
        },
        ConfigRowSpec {
            setting: VoicePanning,
            y: 510,
            title_band: 18,
            kind: Buttons([1, 0]),
        },
    ],
    &[
        ConfigRowSpec {
            setting: MessageSpeed,
            y: 0,
            title_band: 8,
            kind: Slider(2),
        },
        ConfigRowSpec {
            setting: SkipUnread,
            y: 102,
            title_band: 11,
            kind: Buttons([1, 0]),
        },
        ConfigRowSpec {
            setting: WindowOpacity,
            y: 204,
            title_band: 15,
            kind: Slider(1),
        },
    ],
    &[
        ConfigRowSpec {
            setting: AutoDelay,
            y: 0,
            title_band: 10,
            kind: Slider(2),
        },
        ConfigRowSpec {
            setting: MusicTitle,
            y: 102,
            title_band: 16,
            kind: Buttons([5, 4]),
        },
        ConfigRowSpec {
            setting: SceneTitle,
            y: 204,
            title_band: 19,
            kind: Buttons([5, 4]),
        },
        ConfigRowSpec {
            setting: TouchEffects,
            y: 306,
            title_band: 17,
            kind: Buttons([5, 4]),
        },
        ConfigRowSpec {
            setting: SuppressAutoSleep,
            y: 408,
            title_band: 12,
            kind: Buttons([1, 0]),
        },
    ],
];
pub const CONFIG_CONTENT_HEIGHT: f32 = 666.0;
pub const AUTO_SLEEP_NOTICE: &str =
    "オートモード中とMusicBoxの楽曲再生中は、@r本体の自動スリープ機能が動作しないようになります。";

/// Side effects beyond writing the live Settings. Audio cues belong to the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigEdit {
    pub cue: Option<&'static str>,
    pub show_auto_sleep_notice: bool,
    pub touch_effects_enabled: Option<bool>,
}
#[derive(Debug)]
pub struct ConfigWidget {
    pub spec: ConfigRowSpec,
    value: u8,
    focused: bool,
    selection: [Tweener; 2],
    flash: [Tweener; 2],
    capture: Option<u8>,
    drag_origin: u8,
    drag_distance: f32,
}
fn retarget(t: &mut Tweener, value: f32, speed: f32) {
    t.enqueue_now(
        value,
        Tween::linear(Ticks::from_f32((value - t.value()).abs() / speed)),
    );
}
impl ConfigWidget {
    pub fn new(spec: ConfigRowSpec, settings: &Settings) -> Self {
        Self {
            spec,
            value: spec.setting.value(settings),
            focused: false,
            selection: std::array::from_fn(|_| Tweener::new(0.0)),
            flash: std::array::from_fn(|_| Tweener::new(0.0)),
            capture: None,
            drag_origin: 0,
            drag_distance: 0.0,
        }
    }
    pub fn value(&self) -> u8 {
        self.value
    }
    pub fn selection(&self, choice: usize) -> f32 {
        self.selection[choice].value()
    }
    pub fn flash(&self, choice: usize) -> f32 {
        self.flash[choice].value()
    }
    fn selected_slot(&self) -> usize {
        match self.spec.kind {
            Slider(_) => 0,
            Buttons(_) => usize::from(self.value),
        }
    }
    pub fn focus(&mut self, focused: bool, animate: bool) {
        let slot = self.selected_slot();
        let target = if focused { 1.0 } else { 0.0 };
        if animate {
            retarget(
                &mut self.selection[slot],
                target,
                if focused { 0.25 } else { 0.2 },
            );
        } else {
            self.selection[slot] = Tweener::new(target);
        }
        self.focused = focused;
    }
    pub fn focused(&self) -> bool {
        self.focused
    }
    pub fn update(&mut self, delta: Ticks) {
        for t in self.selection.iter_mut().chain(self.flash.iter_mut()) {
            t.update(delta);
        }
    }
    fn apply(
        &mut self,
        value: u8,
        settings: &mut Settings,
        cue: Option<&'static str>,
    ) -> ConfigEdit {
        self.value = value;
        self.spec.setting.set(settings, value);
        ConfigEdit {
            cue,
            show_auto_sleep_notice: self.spec.setting == SuppressAutoSleep && value == 1,
            touch_effects_enabled: (self.spec.setting == TouchEffects).then_some(value != 0),
        }
    }
    fn move_choice(&mut self, value: u8) {
        retarget(&mut self.selection[usize::from(self.value)], 0.0, 0.2);
        retarget(&mut self.selection[usize::from(value)], 1.0, 0.25);
    }
    /// Left has native priority even when both directions are held at a boundary.
    pub fn controller(
        &mut self,
        left: bool,
        right: bool,
        settings: &mut Settings,
    ) -> Option<ConfigEdit> {
        let (step, max) = match self.spec.kind {
            Slider(_) => (5, 100),
            Buttons(_) => (1, 1),
        };
        let value = if left {
            self.value.saturating_sub(step)
        } else if right {
            self.value.saturating_add(step).min(max)
        } else {
            return None;
        };
        if value == self.value {
            return None;
        }
        if matches!(self.spec.kind, Buttons(_)) {
            self.move_choice(value);
        }
        Some(self.apply(value, settings, Some("move")))
    }
    /// Native Button57070 probes choices; Slider58160 clears its drag latch.
    pub fn probe(&mut self, x: f32, y: f32) {
        match self.spec.kind {
            Slider(_) => self.capture = None,
            Buttons(_) => {
                self.capture = None;
                let top = f32::from(self.spec.y);
                if y >= top && y < top + 88.0 {
                    for choice in (0..2u8).rev() {
                        let left = 1132.0 + f32::from(choice) * 252.0;
                        if x >= left && x < left + 240.0 {
                            self.capture = Some(choice);
                            break;
                        }
                    }
                }
            }
        }
    }
    pub fn press_captured(&mut self, x: f32, y: f32) -> bool {
        match self.spec.kind {
            Buttons(_) => {
                if let Some(choice) = self.capture {
                    retarget(&mut self.flash[usize::from(choice)], 0.7, 0.2);
                    return true;
                }
            }
            Slider(_) => {
                if self.knob_hit(x, y) {
                    self.capture = Some(0);
                    self.drag_origin = self.value;
                    self.drag_distance = 0.0;
                    return true;
                }
            }
        }
        false
    }
    fn knob_hit(&self, x: f32, y: f32) -> bool {
        let top = f32::from(self.spec.y);
        let knob = 1000.0 + f32::from(self.value) * 6.0;
        y >= top && y < top + 88.0 && x >= knob - 48.0 && x < knob + 48.0
    }
    /// Convenience for direct callers; InputArea keeps probe and press separate.
    pub fn press(&mut self, x: f32, y: f32) -> bool {
        self.probe(x, y);
        self.press_captured(x, y)
    }
    pub fn cancel(&mut self) {
        // Native Slider58170 is empty; its latch resets at the next probe.
        if matches!(self.spec.kind, Buttons(_)) {
            if let Some(choice) = self.capture.take() {
                retarget(&mut self.flash[usize::from(choice)], 0.0, 0.2);
            }
        }
    }
    /// Slider58200 accepts a captured drag, or a horizontal first drag starting
    /// inside the knob. Button572e0 cancels feedback and falls back to page drag.
    pub fn pointer_drag(
        &mut self,
        previous: glam::Vec2,
        current: glam::Vec2,
        settings: &mut Settings,
    ) -> (bool, Option<ConfigEdit>) {
        if matches!(self.spec.kind, Buttons(_)) {
            self.cancel();
            return (false, None);
        }
        if self.capture.is_none() {
            if !self.knob_hit(previous.x, previous.y) {
                return (false, None);
            }
            let delta = current - previous;
            let angle = (delta.y.atan2(delta.x) / 3.141592_f32 * 180.0).abs();
            if angle >= 40.0 && angle <= 140.0 {
                return (false, None);
            }
            self.capture = Some(0);
            self.drag_origin = self.value;
            self.drag_distance = 0.0;
        }
        (true, self.drag(current.x - previous.x, settings))
    }
    /// Native drag uses accumulated physical delta/6 truncated toward zero,
    /// measured against the press value. It does not emit per-change audio cues.
    pub fn drag(&mut self, delta_x: f32, settings: &mut Settings) -> Option<ConfigEdit> {
        if self.capture.is_none() || !matches!(self.spec.kind, Slider(_)) {
            return None;
        }
        self.drag_distance += delta_x;
        let value = (i32::from(self.drag_origin) + (self.drag_distance * (1.0 / 6.0)) as i32)
            .clamp(0, 100) as u8;
        (value != self.value).then(|| self.apply(value, settings, None))
    }
    /// The shared InputArea retains the original press target on release.
    pub fn release(&mut self, settings: &mut Settings) -> Option<ConfigEdit> {
        if !matches!(self.spec.kind, Buttons(_)) {
            return None; // Native Slider581f0 leaves the drag latch untouched.
        }
        let choice = self.capture.take()?;
        if choice != self.value {
            self.move_choice(choice);
        }
        let t = &mut self.flash[usize::from(choice)];
        retarget(t, 0.7, 0.2);
        t.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
        Some(self.apply(choice, settings, Some("decide")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_control_priorities_captured_release_and_live_settings() {
        let mut settings = Settings::default();
        settings.v3_sysvol = 2;
        let mut volume = ConfigWidget::new(CONFIG_ROWS[0][0], &settings);
        assert_eq!(
            volume.controller(true, true, &mut settings).unwrap().cue,
            Some("move")
        );
        assert_eq!(settings.v3_sysvol, 0);
        assert!(volume.controller(true, true, &mut settings).is_none());
        assert_eq!(settings.v3_sysvol, 0);
        volume.controller(false, true, &mut settings);
        assert_eq!(settings.v3_sysvol, 5);
        assert!(!volume.press(982.0, 88.0));
        assert!(volume.press(982.0, 0.0));
        assert!(volume.drag(5.9, &mut settings).is_none());
        assert_eq!(volume.drag(0.2, &mut settings).unwrap().cue, None);
        assert_eq!(settings.v3_sysvol, 6);
        volume.drag(-12.2, &mut settings);
        assert_eq!(settings.v3_sysvol, 4);
        volume.drag(6000.0, &mut settings);
        assert_eq!(settings.v3_sysvol, 100);
        volume.release(&mut settings);
        volume.probe(0.0, 0.0); // Next native probe clears the retained slider latch.
        assert!(volume.drag(-6000.0, &mut settings).is_none());

        let mut focus = ConfigWidget::new(CONFIG_ROWS[0][4], &settings);
        focus.focus(true, false);
        assert_eq!(focus.selection(0), 1.0);
        assert!(!focus.press(1372.0, 408.0));
        assert!(focus.press(1384.0, 408.0));
        assert!(!settings.v4_voicefocus); // Press feedback has no settings callback.
        focus.update(Ticks::from_f32(2.0));
        assert!((focus.flash(1) - 0.4).abs() < 0.0001);
        assert_eq!(focus.release(&mut settings).unwrap().cue, Some("decide"));
        assert!(settings.v4_voicefocus);
        focus.update(Ticks::from_f32(4.0));
        assert_eq!(focus.selection(1), 1.0);
        assert!(focus.flash(1) > 0.0);
        focus.update(Ticks::from_f32(4.0));
        assert_eq!(focus.flash(1), 0.0);
        assert!(focus.press(1132.0, 408.0));
        focus.cancel();
        assert!(focus.release(&mut settings).is_none());
        assert!(settings.v4_voicefocus);
    }
    #[test]
    fn authored_schema_and_non_setting_fields_survive_callbacks() {
        assert_eq!(CONFIG_ROWS.map(|rows| rows.len()), [6, 3, 5]);
        let mut settings = Settings::default();
        settings.v7 = 2;
        settings.v8 = 1;
        settings.v19 = 0x01234567;
        for spec in CONFIG_ROWS.into_iter().flatten() {
            let mut widget = ConfigWidget::new(*spec, &settings);
            let effect = widget.controller(true, false, &mut settings);
            if let Some(effect) = effect {
                assert_eq!(spec.setting.value(&settings), widget.value());
                assert_eq!(effect.cue, Some("move"));
            }
        }
        assert_eq!((settings.v7, settings.v8, settings.v19), (2, 1, 0x01234567));
        let mut sleep = ConfigWidget::new(CONFIG_ROWS[2][4], &settings);
        assert!(
            sleep
                .controller(false, true, &mut settings)
                .unwrap()
                .show_auto_sleep_notice
        );
        assert!(settings.v18_showsongtitle);
        assert!(
            !sleep
                .controller(true, false, &mut settings)
                .unwrap()
                .show_auto_sleep_notice
        );
        let mut touch = ConfigWidget::new(CONFIG_ROWS[2][3], &settings);
        assert_eq!(
            touch
                .controller(false, true, &mut settings)
                .unwrap()
                .touch_effects_enabled,
            Some(true)
        );
    }
    #[test]
    fn native_slider_first_drag_angle_gate_and_toggle_probe_capture() {
        use glam::Vec2;
        for (angle, handled) in [
            (39.9_f32, true),
            (40.1, false),
            (139.9, false),
            (140.1, true),
        ] {
            let mut settings = Settings::default();
            let mut slider = ConfigWidget::new(CONFIG_ROWS[0][0], &settings);
            let previous = Vec2::new(1330.0, 30.0);
            let radians = angle / 180.0 * 3.141592_f32;
            let current = previous + Vec2::new(radians.cos(), radians.sin()) * 30.0;
            slider.probe(previous.x, previous.y);
            assert_eq!(
                slider.pointer_drag(previous, current, &mut settings).0,
                handled,
                "angle{angle}"
            );
        }
        let mut settings = Settings::default();
        let mut slider = ConfigWidget::new(CONFIG_ROWS[0][0], &settings);
        slider.probe(1330.0, 30.0);
        assert!(slider.press_captured(1330.0, 30.0));
        assert!(
            slider
                .pointer_drag(
                    Vec2::new(1330.0, 30.0),
                    Vec2::new(1330.0, 300.0),
                    &mut settings
                )
                .0
        );
        slider.cancel();
        assert!(
            slider
                .pointer_drag(Vec2::ZERO, Vec2::new(6.0, 0.0), &mut settings)
                .0
        );
        assert_eq!(settings.v3_sysvol, 56);
        slider.probe(0.0, 0.0);
        assert!(
            !slider
                .pointer_drag(Vec2::ZERO, Vec2::new(6.0, 0.0), &mut settings)
                .0
        );
        let mut button = ConfigWidget::new(CONFIG_ROWS[0][4], &settings);
        button.probe(1400.0, 420.0);
        button.press_captured(1200.0, 420.0);
        button.release(&mut settings);
        assert!(settings.v4_voicefocus);
        button.probe(1200.0, 420.0);
        assert!(
            !button
                .pointer_drag(Vec2::ZERO, Vec2::new(0.0, 10.0), &mut settings)
                .0
        );
        assert!(button.release(&mut settings).is_none());
        assert!(settings.v4_voicefocus);
    }
    #[test]
    #[ignore = "requires supplied ROM/executable (SHIN_REFERENCE_ROM)"]
    fn reference_native_config_authored_rows_callbacks_and_controls() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        // Fixed words independently retain constructor arguments, callback stores,
        // controller priority/steps, accumulated drag and captured toggle release.
        for (address, expected) in [
            (0x59098, 0x3942cd05),
            (0x590c0, 0x2a1f03e2),
            (0x590c4, 0x2a1f03e3),
            (0x590c8, 0x2a1f03e4),
            (0x590cc, 0x97fffb11),
            (0x5927c, 0x3942c105),
            (0x5929c, 0x52800cc2),
            (0x592a8, 0x320003e3),
            (0x5945c, 0x3942c505),
            (0x5947c, 0x52801982),
            (0x59488, 0x321f03e3),
            (0x5963c, 0x3942c905),
            (0x5965c, 0x52802642),
            (0x59668, 0x320007e3),
            (0x5981c, 0x3942d105),
            (0x5983c, 0x52803302),
            (0x5984c, 0x321e03e3),
            (0x59a1c, 0x52800243),
            (0x59a2c, 0x321f1fe2),
            (0x59be4, 0x3942e505),
            (0x59c0c, 0x2a1f03e2),
            (0x59c10, 0x321d03e3),
            (0x59c14, 0x321f03e4),
            (0x59ddc, 0x3942ed05),
            (0x59dfc, 0x52800cc2),
            (0x59e00, 0x52800163),
            (0x59fd0, 0x3942f505),
            (0x59ff0, 0x52801982),
            (0x59ffc, 0x32000fe3),
            (0x5a000, 0x320003e4),
            (0x5a1cc, 0x3942e905),
            (0x5a1ec, 0x52800143),
            (0x5a1f8, 0x2a1f03e2),
            (0x5a1fc, 0x321f03e4),
            (0x5a3c4, 0x3942fd05),
            (0x5a3e4, 0x52800cc2),
            (0x5a3f4, 0x321c03e3),
            (0x5a5b8, 0x3942f905),
            (0x5a5d8, 0x52801982),
            (0x5a5dc, 0x52800263),
            (0x5a7a4, 0x39430505),
            (0x5a7c4, 0x52802642),
            (0x5a7c8, 0x52800223),
            (0x5a998, 0x39430925),
            (0x5a9b8, 0x52803302),
            (0x5a9c8, 0x321e07e3),
            (0x5abb0, 0x52805348),
            (0x5abb4, 0xb9040ea8),
            (0x580c4, 0x37d000c9),
            (0x580c8, 0x37d801a9),
            (0x580dc, 0x51001509),
            (0x580fc, 0x11001509),
            (0x58100, 0x7101913f),
            (0x58240, 0x1e210800),
            (0x58244, 0x1e38000a),
            (0x570c8, 0x1e351002),
            (0x570e0, 0x1e222884),
            (0x571c4, 0xb9407414),
            (0x571d4, 0x6b14011f),
            (0x57280, 0xb9407268),
            (0x5f48c, 0x3902cd28),
            (0x5f54c, 0x3902c128),
            (0x5f60c, 0x3902c528),
            (0x5f6cc, 0x3902c928),
            (0x5f78c, 0x3902d128),
            (0x5f85c, 0x3902d528),
            (0x5f91c, 0x3902e528),
            (0x5f9dc, 0x3902ed28),
            (0x5fa9c, 0x3902f528),
            (0x5fb5c, 0x3902e928),
            (0x5fc1c, 0x3902fd28),
            (0x5fcdc, 0x3902f928),
            (0x5fda4, 0x39030528),
            (0x5fe84, 0x7100051f),
            (0x5fefc, 0x3903091f),
            (0x5ff58, 0x39030909),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [
            (0x159940, 88.0_f32),
            (0x1598cc, 666.0),
            (0x159578, 1000.0),
            (0x1599c4, -48.0),
            (0x159584, 48.0),
            (0x159540, 1.0 / 6.0),
            (0x1593f4, 1624.0),
            (0x1597f0, -240.0),
            (0x159660, 240.0),
            (0x1595a4, 0.7),
            (0x159974, 0.2),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
        assert_eq!(&exe[0x1609b4 + 256..0x1609b4 + 268], b"/config.txa\0");
        let notice =
            std::str::from_utf8(exe[0x160cce + 256..].split(|&b| b == 0).next().unwrap()).unwrap();
        assert_eq!(notice, AUTO_SLEEP_NOTICE);
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap())
                .unwrap();
        let mut bytes = Vec::new();
        let file = rom.find_file("/config.txa").unwrap();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let atlas = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        assert_eq!(atlas.name_to_index.len(), 17);
        for (name, dimensions) in [
            ("bg", (960, 540)),
            ("obi", (1920, 320)),
            ("window", (1824, 834)),
            ("caption", (960, 128)),
            ("close", (84, 84)),
            ("default", (306, 84)),
            ("tab_on", (320, 216)),
            ("tab_off", (320, 216)),
            ("left", (752, 1440)),
            ("right", (480, 640)),
            ("button_on", (240, 88)),
            ("button_off", (240, 88)),
            ("slider", (648, 192)),
            ("focus", (160, 32)),
            ("line", (32, 14)),
            ("help_config", (320, 240)),
            ("key", (384, 96)),
        ] {
            assert_eq!(
                atlas.textures[atlas.name_to_index[name]].dimensions(),
                dimensions,
                "Config {name}"
            );
        }
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        let mov_immediate = |a: usize, register: u32| {
            let w = word(a);
            assert_eq!(w & 31, register);
            if w == 0x2a1f03e0 | register {
                0
            } else if w & 0xff8003e0 == 0x320003e0 {
                // ORR Wd,WZR,#logical-immediate is another native MOV alias.
                let imms = (w >> 10) & 63;
                let immr = (w >> 16) & 63;
                let len = 31 - ((!imms) & 63).leading_zeros();
                let size = 1u32 << len;
                let levels = size - 1;
                let ones = (imms & levels) + 1;
                let rotation = immr & levels;
                assert!(size <= 32 && ones < size);
                let mut value = 0u32;
                for bit in 0..32 {
                    if (bit + rotation) % size < ones {
                        value |= 1 << bit;
                    }
                }
                value
            } else {
                assert_eq!(w & 0xffe00000, 0x52800000, "MOV native{a:x}");
                (w >> 5) & 0xffff
            }
        };
        // Bind the Rust schema to recovered constructor arguments, not merely
        // to the independent instruction anchors above.
        for (spec, (load, y, title, offset)) in CONFIG_ROWS.into_iter().flatten().zip([
            (0x59098, 0x590c0, 0x590c4, 0xb3),
            (0x5927c, 0x5929c, 0x592a8, 0xb0),
            (0x5945c, 0x5947c, 0x59488, 0xb1),
            (0x5963c, 0x5965c, 0x59668, 0xb2),
            (0x5981c, 0x5983c, 0x5984c, 0xb4),
            (0x599fc, 0x59a2c, 0x59a1c, 0xb5),
            (0x59be4, 0x59c0c, 0x59c10, 0xb9),
            (0x59ddc, 0x59dfc, 0x59e00, 0xbb),
            (0x59fd0, 0x59ff0, 0x59ffc, 0xbd),
            (0x5a1cc, 0x5a1f8, 0x5a1ec, 0xba),
            (0x5a3c4, 0x5a3e4, 0x5a3f4, 0xbf),
            (0x5a5b8, 0x5a5d8, 0x5a5dc, 0xbe),
            (0x5a7a4, 0x5a7c4, 0x5a7c8, 0xc1),
            (0x5a998, 0x5a9b8, 0x5a9c8, 0xc2),
        ]) {
            assert_eq!((word(load) >> 10) & 0xfff, offset);
            assert_eq!(mov_immediate(y, 2), u32::from(spec.y));
            assert_eq!(mov_immediate(title, 3), u32::from(spec.title_band));
            let mut sentinel = Settings::default();
            sentinel.v4_voicefocus = false;
            sentinel.v5_voicepanapot = false;
            sentinel.v11_disallowskipunread = false;
            sentinel.v15 = false;
            sentinel.v14_showroutenavi = false;
            sentinel.v17_showscenetitle = false;
            sentinel.v18_showsongtitle = false;
            spec.setting.set(&mut sentinel, 1);
            assert_eq!(
                match offset {
                    0xb3 => sentinel.v3_sysvol,
                    0xb0 => sentinel.v0_bgmvol,
                    0xb1 => sentinel.v1_sfxvol,
                    0xb2 => sentinel.v2_voicevol,
                    0xb4 => sentinel.v4_voicefocus.into(),
                    0xb5 => sentinel.v5_voicepanapot.into(),
                    0xb9 => sentinel.v9_msgspeed,
                    0xbb => sentinel.v11_disallowskipunread.into(),
                    0xbd => sentinel.v13_msgwinalpha,
                    0xba => sentinel.v10_skipspeed,
                    0xbf => sentinel.v15.into(),
                    0xbe => sentinel.v14_showroutenavi.into(),
                    0xc1 => sentinel.v17_showscenetitle.into(),
                    0xc2 => sentinel.v18_showsongtitle.into(),
                    _ => unreachable!(),
                },
                1
            );
        }
    }
}
