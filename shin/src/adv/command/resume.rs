use super::prelude::*;

/// Native 7b40 queues a load once and never releases this command's token.
/// The replacement scene takes ownership of script execution after loading.
#[derive(Debug)]
pub struct RESUME {
    requested: bool,
}

impl StartableCommand for command::runtime::RESUME {
    type StateInfo = ();

    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        Yield(RESUME { requested: false }.into())
    }
}

impl UpdatableCommand for RESUME {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if !self.requested {
            adv_state.pending_resume = true;
            self.requested = true;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_resume_bookmark_one_shot_load_and_browser_stop() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (a, word) in [
            (0x1aab0, 0x71000d1fu32),
            (0x1aab4, 0x540000e1),
            (0x1aab8, 0x1400005a),
            (0x1aac8, 0x71000d1f),
            (0x1aacc, 0x54000aa0),
            (0x1aaf8, 0x9401265a),
            (0x1ab04, 0x94020e8f),
            (0x1ab8c, 0xf900015f),
            (0x1abe0, 0x91262100),
            (0x1abf8, 0x94028382),
            (0x1ac28, 0x2a1f03e1),
            (0x1ac2c, 0x9401209d),
            (0x62ebc, 0x1e210c41),
            (0x62ec4, 0x1e200c40),
            (0x62f20, 0x1a9f57e0),
            (0x1130c, 0x3951c668),
            (0x11310, 0x35001328),
            (0x11574, 0xb9447668),
            (0x11578, 0x35000488),
            (0x11584, 0x3951ca61),
            (0x11594, 0xd63f0100),
            (0x116c8, 0x940422fa),
            (0x116d4, 0x94038503),
            (0x116e0, 0x94029c78),
            (0x116ec, 0x94014615),
            (0x11708, 0x94014b6a),
            (0x11718, 0x9402339a),
            (0x1190c, 0x9402a851),
            (0x11918, 0x9402a84e),
            (0x1a180, 0x9100e288),
            (0x1a18c, 0x7100041f),
            (0x1a190, 0x540057c1),
            (0x11034, 0x940002af),
            (0x11088, 0x9401473e),
            (0x110b0, 0xb9446263),
            (0x110c0, 0x94002270),
            (0x1387c, 0x320003e8),
            (0x13880, 0xb9046268),
            (0x19a14, 0xb904611f),
            (0x1ad6c, 0x321e03e1),
            (0x62f7c, 0x2d408400),
            (0x62f84, 0x540000a5),
            (0x62f90, 0xb4000068),
            (0x631b4, 0x9401b173),
            (0x63208, 0x2d408660),
            (0x6320c, 0x1e211800),
            (0x63210, 0x1e203900),
            (0x63214, 0x97ffcd47),
            (0x63258, 0x321d03e0),
            (0x5174, 0xb96ddc48),
            (0x517c, 0x37f801c8),
            (0x5180, 0xb96db848),
            (0x5184, 0xb92db448),
            (0x51c4, 0x11000529),
            (0x51d0, 0xb96db909),
            (0x51d4, 0xb92db509),
            (0x51dc, 0xb96db109),
            (0x51e0, 0xb92db909),
            (0x51f4, 0xb96dcd09),
            (0x51f8, 0x11000529),
            (0x51fc, 0xb92dcd09),
            (0x2860, 0xb96db909),
            (0x2864, 0xb92db509),
            (0x2924, 0xb96db909),
            (0x2928, 0xb92db509),
            (0x1381c, 0xb96dcc08),
            (0x13820, 0x7100051f),
            (0x13824, 0x54000369),
            (0x1382c, 0xf8686808),
            (0x13834, 0x1a9f17e8),
            (0x13838, 0x8b284808),
            (0x1383c, 0xb96db501),
            (0x12138, 0x39404fe8),
            (0x1213c, 0x37000168),
            (0x1216c, 0x940005a1),
            (0xfbac, 0xaa1b03f6),
            (0xfbb0, 0xf8098edf),
            (0x1acdc, 0xf94002c8),
            (0x1ace4, 0x9103a115),
            (0x1ace8, 0x91026108),
            (0x1ad28, 0xbd4af100),
            (0x1ad34, 0x321f0fe1),
            (0x1ad38, 0x2a1f03e3),
            (0x1ad40, 0x9401202c),
            (0x62e3c, 0xc8dffc40),
            (0x62e44, 0xc85ffd15),
            (0x62e78, 0xbd002288),
            (0x62e7c, 0xb9002693),
            (0x62e80, 0x39000288),
            (0x62f88, 0x91006008),
            (0x62f8c, 0xc8dffd08),
            (0x62f94, 0x14000003),
            (0x62f9c, 0x14000079),
            (0x117a0, 0x91262276),
            (0xe6a0, 0xa901fc1fu32),
            (0x2af0, 0x94003344u32),
            (0x2af8, 0xb96db109),
            (0x2b00, 0xb92db109),
            (0x2b0c, 0xb92dd509),
            (0x7b4c, 0x39406408),
            (0x7b50, 0x35000248),
            (0x7b5c, 0xb96dd401),
            (0x7b60, 0x94001dac),
            (0x7b70, 0x320007e2),
            (0x7b74, 0x94002f67),
            (0x7b7c, 0x39006668),
            (0x7b9c, 0x2a1f03e0),
            (0xf210, 0x2a0103e2),
            (0xf214, 0x14000003),
            (0xabcb0, 0xb9408c08),
            (0xabcb4, 0x11000508),
            (0xabcb8, 0xb9008c08),
            (0xabcc0, 0x320003e8),
            (0xabcc4, 0x39022008),
            (0xabcc8, 0xd65f03c0),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                word,
                "native RESUME {a:x}"
            );
        }
    }
}
