use super::prelude::*;

impl StartableCommand for command::runtime::AUTOSAVE {
    type StateInfo = ();

    fn apply_state(&self, state: &mut VmState) {
        state.commit_persistent();
        state.record_chapter_boundary();
        state.snapshot_scene_title();
        state.capture_autosave();
    }

    fn start(
        self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        adv_state
            .route_label
            .show(context.asset_server, &vm_state.scene_title);
        self.token.finish().into()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_autosave_scene_title_and_menu_route_snapshot() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x2784, 0xf952da68),
            (0x2788, 0x52922109),
            (0x278c, 0xf8696909),
            (0x2790, 0x5292220a),
            (0x2794, 0xf82a6909),
            (0x1c1dc, 0x5292230a),
            (0x1c1e0, 0xb9008269),
            (0x1c1e4, 0xf9423509),
            (0x1c1e8, 0xb86a6929),
            (0x1c1ec, 0xb9008669),
            (0x1c1f0, 0xf9423509),
            (0x1c1f4, 0x5292238a),
            (0x1c1f8, 0xb86a6929),
            (0x1c1fc, 0xb9008a69),
            (0x1c200, 0xf9423508),
            (0x1c204, 0x52922409),
            (0x1c208, 0x8b090109),
            (0x1c20c, 0xeb0902bf),
            (0x1c210, 0x540000c0),
            (0x1c214, 0xf9400121),
            (0x1c218, 0x52922509),
            (0x1c21c, 0xf8696902),
            (0x1c220, 0xaa1503e0),
            (0x1c224, 0x97ffe93b),
            (0x1cb90, 0xf9400a68),
            (0x1cb94, 0xf9423508),
            (0x1cb98, 0x52922209),
            (0x1cb9c, 0xf8696919),
            (0x1cba0, 0x39400328),
            (0x1cba4, 0x34000928),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native instruction at {address:x}"
            );
        }
    }
}
