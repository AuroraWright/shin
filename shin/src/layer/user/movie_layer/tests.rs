#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_movie_retained_texture_visibility_before_owner_retirement() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let executable = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0xc3624, 0x9101a008u32),
        (0xc362c, 0xb40013a9),
        (0xc3698, 0xf9404108),
        (0xc36a0, 0xd63f0100),
        (0xc36a4, 0x36000ee0),
        (0xc046c, 0x91342008),
        (0xc0470, 0xc8dffd09),
        (0xc0474, 0xb4000349),
        (0xc0478, 0xf9466809),
        (0xc04cc, 0x320003e0),
        (0xbee54, 0x3931029f), // Stop clears started, not retained texture.
        (0xc3574, 0xc85ffec0),
        (0xc3578, 0xc808fedf),
        (0xc358c, 0xd63f0100),
    ] {
        assert_eq!(
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Native Movie retained draw instruction {address:x}"
        );
    }
    let relocation = |slot: u64| {
        executable[0x11e040 + 256..0x152eb8 + 256]
            .chunks_exact(24)
            .find_map(|entry| {
                (u64::from_le_bytes(entry[..8].try_into().unwrap()) == slot)
                    .then(|| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
            })
            .unwrap()
    };
    assert_eq!(
        relocation(relocation(0x225120) + 16 + 0x80),
        0xc0460,
        "Movie draw getter reads retained texture independently of terminal/started status"
    );
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_movie_platform_focus_suspension() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let executable = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let u64_at = |address: usize| {
        u64::from_le_bytes(executable[address + 256..address + 264].try_into().unwrap())
    };
    let relocs: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|o| (u64_at(o), u64_at(o + 16)))
        .collect();
    for (address, target) in [
        (0x2248d8, 0x1d3bb0), // global audio suspension delegate
        (0x1d3ba0, 0x2c1e0),  // delegate clears mixer run event
        (0x1d3ba8, 0x2c1f0),  // delegate signals mixer run event
        (0x225590, 0x220ef0), // StreamSound vtable
        (0x220f48, 0xf51b0),  // StreamSound global pause interface +10
        (0x220f50, 0xf5290),  // StreamSound global resume interface +18
    ] {
        assert_eq!(
            relocs[&address], target,
            "Native audio suspension relocation at {address:x}"
        );
    }
    for (relocation, got, symbol, name) in [
        (
            0x1534e8,
            0x223cc0,
            76,
            "_ZN2nn2os14WaitLightEventEPNS0_14LightEventTypeE",
        ),
        (
            0x1542b0,
            0x224158,
            133,
            "_ZN2nn2os17TryWaitLightEventEPNS0_14LightEventTypeE",
        ),
        (
            0x154340,
            0x224188,
            217,
            "_ZN2nn2oe22PopNotificationMessageEv",
        ),
        (0x154358, 0x224190, 215, "_ZN2nn2oe20GetCurrentFocusStateEv"),
    ] {
        assert_eq!(u64_at(relocation), got);
        assert_eq!(u64_at(relocation + 8), (symbol << 32) | 0x402);
        let address = 0x154e88 + symbol as usize * 24 + 256;
        let name_offset =
            u32::from_le_bytes(executable[address..address + 4].try_into().unwrap()) as usize;
        let start = 0x1568f8 + name_offset + 256;
        let end = start
            + executable[start..]
                .iter()
                .position(|&byte| byte == 0)
                .unwrap();
        assert_eq!(&executable[start..end], name.as_bytes());
    }
    for (address, instruction) in [
        (0xbcd84, 0x91006260u32), // registers movie's Voice::Delegate at +18
        (0xbcd88, 0x9400fd6a),    // calls global delegate registration fc330
        (0xbec90, 0x91006260),    // releases the same interface when worker retires
        (0xbec94, 0x9400f5ff),    // unregisters through fc490
        (0xfc414, 0x39400509),    // registration samples global suspension byte241001
        (0xfc424, 0xf9400908),    // newly registered delegate pauses when suspended
        (0xfc434, 0xf9400d08),    // otherwise resumes
        (0xf9ce0, 0x39000288),    // main loop sets suspension flag before callbacks
        (0xf9cc0, 0x37001da0),    // signaled run event skips suspension path
        (0xf9e3c, 0xf9400908),    // each still-registered delegate pause virtual+10
        (0xf9e90, 0x94008be8),    // waits for run permission before scene updates
        (0xf9f50, 0xf9400d08),    // each still-registered delegate resume virtual+18
        (0xfa048, 0x3900029f),    // clears suspension flag after resume
        (0xfb440, 0x940088e0),    // notification reader
        (0xfb444, 0x71003c1f),    // only message15 changes focus event
        (0xfb44c, 0x940088e1),    // samples platform focus state
        (0xfb454, 0x7100091f),    // unsigned(state-1)<2 signals run permission
        (0xfb460, 0x54ffff01),    // state3 path clears event
        (0xfb468, 0x9400868a),    // ClearLightEvent
        (0x2aea8, 0x94034522),    // registers global audio delegate
        (0x2c1e8, 0x1403c32a),    // pause tail calls ClearLightEvent
        (0x2c1f8, 0x1403c32a),    // resume tail calls SignalLightEvent
        (0x2af44, 0x91006280),    // mixer run event22a150+18
        (0x2af48, 0x9403c7ba),    // waits before first mixer block
        (0x2b190, 0x91006260),    // same mixer run event at loop boundary
        (0x2b194, 0x9403c727),    // waits before next mixer block
        (0xf3ed0, 0xf8008ee9),    // StreamSound delegate interface at object+8
        (0xf3fc4, 0x940020db),    // registers interface with global focus registry
        (0xf51d4, 0x390a9288),    // pause sets independent platform gate
        (0xf52b0, 0x390a929f),    // resume clears platform gate
        (0xf52bc, 0x394ab909),    // requested-started state remains independent
        (0xf52c4, 0x394ac109),    // additional gate still prevents restarting
        (0xf52cc, 0x394abd08),    // additional gate still prevents restarting
        (0xf52f4, 0xf9400908),    // starts audio only when all gates permit
    ] {
        let actual =
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap());
        assert_eq!(
            actual, instruction,
            "Native focus instruction at {address:x}"
        );
    }
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_movie_shared_frame_guard_and_gain_order() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let executable = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let relocs: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|offset| {
            let address =
                u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap());
            let target =
                u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap());
            (address, target)
        })
        .collect();
    for (address, target) in [
        (0x1db858, 0xcd8c0),  // shared reference release
        (0x1db868, 0xc29b0),  // final shared deleting destructor
        (0x1db4e0, 0xbea70),  // shared destructor closes backend
        (0x225178, 0x1db840), // shared movie vtable
        (0x1db898, 0x6aa40),  // update interface dispatches generic frame guard
        (0x1db8a0, 0xc2b10),  // guarded shared body
        (0x225120, 0x1db4b0), // backend vtable
        (0x1db520, 0xbefb0),  // volume setter
        (0x1db530, 0xbf070),  // playback updater
        (0x1db500, 0xbed60),  // Start
        (0x1db508, 0xbee30),  // Stop
        (0x1db518, 0xbeee0),  // terminal status distinct from started flag
        (0x1db550, 0xc0c40),  // explicit pause
        (0x1db558, 0xc0d40),  // explicit resume
        (0x1db5a8, 0xc0cc0),  // audio Voice::Delegate pause thunk
        (0x1db5b0, 0xc0e10),  // audio Voice::Delegate resume thunk
    ] {
        assert_eq!(relocs[&address], target, "Native relocation at {address:x}");
    }
    for (address, instruction) in [
        (0xc3444, 0x34000048),    // skip preserves repeating owner
        (0xc3454, 0xc809fd1f),    // skip clears only this layer backend reference
        (0xc3464, 0xf9400501),    // skip invokes shared reference release
        (0xc3484, 0xc809fd1f),    // alternate skip clears only this layer reference
        (0xcd8c8, 0x5100050a),    // shared release decrements reference count
        (0xcd8d8, 0x7100051f),    // test previous reference count was one
        (0xcd8dc, 0x54000081),    // other owners prevent destructor
        (0xcd8e4, 0xf9400d01),    // last reference selects deleting destructor
        (0xc28c8, 0xf9401108),    // last shared owner selects backend Close virtual slot
        (0xc28cc, 0xd63f0100),    // close backend at final shared retirement
        (0xc28d4, 0xc808fe9f),    // clear final shared backend ownership
        (0xbea90, 0x080afd3f),    // backend Close clears open gate
        (0xc3548, 0xf9402d08),    // layer queries backend terminal status after shared update
        (0xc354c, 0xd63f0100),    // invoke terminal query virtual slot
        (0xc3570, 0x34000115),    // retain backend while nonterminal
        (0xc3574, 0xc85ffec0),    // exchange layer shared pointer for retirement
        (0xc3578, 0xc808fedf),    // clear layer shared pointer
        (0xc3588, 0xf9400508),    // load reference-release slot
        (0xc358c, 0xd63f0100),    // release layer backend ownership
        (0xc3590, 0x9101c288),    // still-picture owner is separate
        (0xc35a4, 0xf9400908),    // still picture continues its independent update
        (0xbdce8, 0xf9453669),    // video decode loads cumulative cycle offset
        (0xbdcec, 0x8b080128),    // video presentation timestamp includes cycle offset
        (0xbe400, 0xf9453669),    // audio decode loads cumulative cycle offset
        (0xbe404, 0x8b080128),    // audio presentation timestamp includes cycle offset
        (0xbe7b0, 0xf9453669),    // second audio path loads cumulative cycle offset
        (0xbe7b4, 0x8b080128),    // second audio path includes cycle offset
        (0xbf0ac, 0x9400037d),    // refill current audio voice
        (0xbf0c0, 0x14000424),    // then check repeat decoder EOF
        (0xc016c, 0x36000c88),    // require video decoder EOF
        (0xc017c, 0x36000c08),    // require audio decoder EOF
        (0xc0184, 0x34000bc8),    // require repeat flag
        (0xc01ac, 0xf9401908),    // video decoder finish/reset slot
        (0xc01bc, 0xf9401d08),    // video decoder rewind slot
        (0xc01d4, 0x089ffd1f),    // clear video EOF for next cycle
        (0xc01e4, 0xf9401908),    // audio decoder finish/reset slot
        (0xc01f4, 0xf9401d08),    // audio decoder rewind slot
        (0xc020c, 0x089ffd1f),    // clear audio EOF for next cycle
        (0xc0210, 0xf9453288),    // load one cycle duration
        (0xc0218, 0x8b080128),    // add duration to cumulative timestamp offset
        (0xc0220, 0xf9053688),    // retain cumulative offset
        (0xc02c8, 0x9401063e),    // dispatch next decode job without replacing audio voice
        (0xc2654, 0x97fea0e3u32), // constructor initializes zero frame marker
        (0xc2674, 0x91012129),    // shared update interface at vtable +48
        (0xc34ec, 0x4ea81d00),    // forwards delta to shared updater
        (0xc34f0, 0x2a1503e1),    // forwards animation permission
        (0xc34f4, 0xd63f0100),    // invokes guarded update
        (0xc2b2c, 0xf9403908),    // shared body loads playback update +70
        (0xc2b30, 0xd63f0100),    // advances backend before reading gain
        (0xc2b34, 0xb9400e68),    // reads volume source after playback
        (0xc2bac, 0xf9403101),    // loads volume setter +60
        (0xc2bb4, 0xd61f0020),    // final tail call applies gain
        (0xbefd8, 0xbd0d2688),    // setter saves gain for future backend restarts
        (0xbefec, 0xd63f0100),    // setter forwards gain to existing audio
        (0xbf090, 0xb94c4675),    // backend reads actual clock-playing status
        (0xbf09c, 0x34000155),    // paused backend skips clock/frame work
        (0xbf0a4, 0x9400000f),    // active backend advances playback
        (0xbed80, 0x39310288),    // Start sets requested-started flag
        (0xbee54, 0x3931029f),    // Stop clears requested-started flag
        (0xbee68, 0xf9400d08),    // Stop invokes audio pause virtual +18
        (0xbee74, 0x94011ebb),    // Stop pauses independent clock
        (0xbeefc, 0x39710e88),    // terminal status uses failure flag +c43
        (0xbc6e8, 0x39310adb),    // initializes preparation gate +c42
        (0xbdec8, 0x39310a7f),    // worker clears preparation gate
        (0xbd7ec, 0x39310e68),    // worker failure sets terminal gate
        (0xc0c60, 0x39310688),    // explicit pause sets +c41
        (0xc0d60, 0x3931069f),    // explicit resume clears +c41
    ] {
        let actual =
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap());
        assert_eq!(actual, instruction, "Native instruction at {address:x}");
    }
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_movie_padding_and_repeat_handlers() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let path = std::path::Path::new(&rom)
        .parent()
        .unwrap()
        .join("uncompressed_executable");
    let executable = std::fs::read(path).unwrap();
    for (address, instruction) in [
        (0xbf500, 0x51001d0au32), // subtract7 from inclusive crop height
        (0xbf504, 0x53017d4a),    // divide visible color height by2
        (0xbfccc, 0x11002108),    // add8 to mask start before normalization
        (0xc04c4, 0x1e202800),    // double transparent movie height for display
        (0xc0180, 0x39408a88),    // repeat byte gates decoder restart
        (0xbef28, 0x35000268),    // repeat suppresses terminal status
        (0xc2768, 0x3d42c100),    // initial BGM settings byte
        (0xc2778, 0x3d42c500),    // initial SE settings byte
        (0xc2788, 0x3d42c900),    // initial voice settings byte
        (0xc2aa8, 0x3d42c100),    // per-frame BGM settings byte
        (0xc2ab8, 0x3d42c900),    // per-frame voice settings byte
        (0xc2ac8, 0x3d42c500),    // per-frame SE settings byte
        (0xc2ae0, 0x1e2e1000),    // ignore settings uses unit gain
        (0xc2af4, 0x1e210800),    // multiply category and script gain
    ] {
        let actual = u32::from_le_bytes(
            executable[address + 0x100..address + 0x104]
                .try_into()
                .unwrap(),
        );
        assert_eq!(actual, instruction, "Native instruction at {address:x}");
    }
}
