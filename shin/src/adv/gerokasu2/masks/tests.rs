use super::*;
use shin_core::vm::{
    IntoRuntimeForm, VmCtx,
    command::{CompiletimeCommand, compiletime},
};
fn unload() -> Cmd {
    Cmd::Shared(
        CompiletimeCommand::MASKUNLOAD(compiletime::MASKUNLOAD {})
            .into_runtime_form(&VmCtx::new(0, 42)),
    )
}
struct Services {
    metadata: bool,
    ready: bool,
    previous: bool,
    allowed: bool,
    textures: [bool; 4],
    flags: [i32; 4],
    spawns: u32,
    copies: u32,
    checks: u32,
    done: u32,
    observed_permission: Option<bool>,
    fail_start: bool,
    fail_capture: bool,
    fail_ready: bool,
    fail_publish: bool,
    fail_finish: bool,
    events: Vec<&'static str>,
}
impl Services {
    fn new(metadata: bool, previous: bool, allowed: bool, flags: i32) -> Self {
        Self {
            metadata,
            ready: false,
            previous,
            allowed,
            textures: [true; 4],
            flags: std::array::from_fn(|i| flags.wrapping_add(i as i32)),
            spawns: 0,
            copies: 0,
            checks: 0,
            done: 0,
            observed_permission: None,
            fail_start: false,
            fail_capture: false,
            fail_ready: false,
            fail_publish: false,
            fail_finish: false,
            events: Vec::new(),
        }
    }
}
impl MaskServices for Services {
    fn start_mask_resource(&mut self, _: i32) -> Result<bool> {
        ensure!(!self.fail_start, "injected mask start failure");
        self.events.push("start");
        if self.metadata {
            self.spawns += 1;
        }
        Ok(self.metadata)
    }
    fn mask_resource_ready(&mut self) -> Result<bool> {
        self.events.push("ready");
        self.checks += 1;
        ensure!(!self.fail_ready, "injected mask I/O failure");
        Ok(self.ready)
    }
    fn capture_mask_source(&mut self) -> Result<()> {
        ensure!(!self.fail_capture, "injected source capture failure");
        if !self.previous {
            self.events.push("capture");
            self.previous = true;
            self.copies += 1;
            self.observed_permission = Some(self.allowed);
        }
        Ok(())
    }
    fn publish_mask(&mut self, plane: PlaneId, flags: i32) -> Result<()> {
        ensure!(
            self.ready && !self.fail_publish,
            "injected mask publication failure"
        );
        self.events.push("publish");
        let index = usize::from(plane.raw());
        self.textures[index] = true;
        self.flags[index] = flags;
        Ok(())
    }
    fn clear_mask(&mut self, plane: PlaneId) -> Result<()> {
        self.events.push("clear");
        self.textures[usize::from(plane.raw())] = false;
        Ok(())
    }
    fn set_mask_animations_allowed(&mut self, allowed: bool) {
        self.allowed = allowed;
        self.events.push(if allowed { "allow" } else { "block" });
    }
    fn mask_command_finished(&mut self) -> Result<()> {
        ensure!(!self.fail_finish, "injected retirement failure");
        self.done += 1;
        self.events.push("finish");
        Ok(())
    }
}
fn load(mask_id: i32, flags: i32, permission: i32) -> Cmd {
    Cmd::MaskLoad {
        mask_id,
        flags,
        animation_permission_arg: permission,
    }
}
#[test]
fn ger_masks_capture_plane_and_permission_low_bit_across_pending_publication() {
    for permission in [i32::MIN, -1, 0, 1, 2, 3, i32::MAX] {
        let mut layers = Gerokasu2Layers::default();
        layers.select_plane(2).unwrap();
        let plan = MaskPlan::prepare(&mut layers, &load(7, i32::MIN | 16, permission)).unwrap();
        assert_eq!(layers.plane_mask(PlaneId::new(2)), [7, i32::MIN | 16]);
        layers.select_plane(1).unwrap();
        let mut owner = MaskController::new(plan);
        let mut services = Services::new(true, false, true, 0x40000010);
        assert!(!owner.poll(&mut services).unwrap());
        assert_eq!(services.allowed, permission & 1 != 0);
        assert_eq!(
            &services.events[..],
            if permission & 1 == 0 {
                &["start", "block", "capture", "ready"][..]
            } else {
                &["start", "capture", "ready"][..]
            }
        );
        assert!(!owner.poll(&mut services).unwrap());
        assert_eq!((services.spawns, services.copies), (1, 1));
        services.ready = true;
        assert!(owner.poll(&mut services).unwrap());
        assert_eq!(
            services.flags,
            [0x40000010, 0x40000011, i32::MIN | 16, 0x40000013]
        );
        assert!(services.allowed);
        let events = services.events.len();
        assert!(owner.poll(&mut services).unwrap());
        assert_eq!(services.events.len(), events);
        assert_eq!(services.done, 1);
    }
}
#[test]
fn ger_masks_missing_metadata_and_unload_clear_only_captured_texture_and_preserve_flags() {
    for command in [load(-1, -1, 0), unload()] {
        let mut layers = Gerokasu2Layers::default();
        layers.select_plane(3).unwrap();
        layers.set_mask(91, 0x40000010);
        let plan = MaskPlan::prepare(&mut layers, &command).unwrap();
        layers.select_plane(0).unwrap();
        let unloading = matches!(command, Cmd::Shared(_));
        assert_eq!(
            layers.plane_mask(PlaneId::new(3)),
            if unloading {
                [-1, 0x40000010]
            } else {
                [-1, -1]
            }
        );
        let mut owner = MaskController::new(plan);
        let mut services = Services::new(false, false, false, 0x40000010);
        assert!(owner.poll(&mut services).unwrap());
        assert_eq!(services.textures, [true, true, true, false]);
        assert_eq!(
            services.flags,
            [0x40000010, 0x40000011, 0x40000012, 0x40000013]
        );
        assert_eq!(
            (services.spawns, services.copies, services.checks),
            (0, 1, 0)
        );
        assert_eq!(services.allowed, !unloading);
    }
}
#[test]
fn ger_masks_failed_services_keep_retained_owner_without_repeating_completed_prefixes() {
    let mut layers = Gerokasu2Layers::default();
    let plan = MaskPlan::prepare(&mut layers, &load(7, 19, 0)).unwrap();
    let mut owner = MaskController::new(plan);
    let mut services = Services::new(true, false, true, 27);
    services.fail_start = true;
    assert!(owner.poll(&mut services).is_err());
    assert_eq!(owner.stage(), MaskStage::Start);
    services.fail_start = false;
    services.fail_capture = true;
    assert!(owner.poll(&mut services).is_err());
    assert_eq!((services.spawns, services.copies), (1, 0));
    services.fail_capture = false;
    services.fail_ready = true;
    assert!(owner.poll(&mut services).is_err());
    assert!(services.allowed);
    assert_eq!((services.spawns, services.copies), (1, 1));
    services.fail_ready = false;
    services.ready = true;
    services.fail_publish = true;
    assert!(owner.poll(&mut services).is_err());
    assert_eq!(services.flags[0], 27);
    services.fail_publish = false;
    services.fail_finish = true;
    assert!(owner.poll(&mut services).is_err());
    assert_eq!(services.flags[0], 19);
    let publications = services.events.iter().filter(|e| **e == "publish").count();
    services.fail_finish = false;
    assert!(owner.poll(&mut services).unwrap());
    assert_eq!(
        services.events.iter().filter(|e| **e == "publish").count(),
        publications
    );
    assert_eq!(services.done, 1);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original mask constructors/live callbacks/real plane set-clear and -5 INIT; prepared metadata/jobs/root/RTTI, CPU only"]
fn ger_native_masks_captured_plane_callbacks_raw_permission_and_init_minus5() {
    use std::{
        io::{Read, Write},
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/masks");
    std::fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-masks");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-masks.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut child = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    fn words(output: &mut impl Read) -> [u32; 32] {
        let mut magic = [0; 4];
        output.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"GMK1");
        std::array::from_fn(|_| {
            let mut b = [0; 4];
            output.read_exact(&mut b).unwrap();
            u32::from_le_bytes(b)
        })
    }
    fn check(
        w: [u32; 32],
        layers: &Gerokasu2Layers,
        services: &Services,
        plane: u8,
        raw: u32,
        stage: MaskStage,
        result: bool,
    ) {
        assert_eq!(w[0], u32::from(result));
        assert_eq!(
            w[1],
            match stage {
                MaskStage::Start => 0,
                MaskStage::Publication => 1,
                MaskStage::Complete => u32::MAX,
            }
        );
        assert_eq!(w[2], u32::from(plane));
        assert_eq!(w[3], raw);
        assert_eq!(w[4], u32::from(services.allowed));
        assert_eq!(w[5], u32::from(services.previous));
        assert_eq!(w[6], services.spawns);
        assert_eq!(w[7], services.copies);
        assert_eq!(w[8], services.checks);
        assert_eq!(&w[9..13], &[0, 0, 0, 1]);
        assert_eq!(
            w[13],
            services.observed_permission.map_or(u32::MAX, u32::from)
        );
        for i in 0..4 {
            let [id, flags] = layers.plane_mask(PlaneId::new(i as u8));
            assert_eq!(
                &w[16 + i * 4..20 + i * 4],
                &[
                    id as u32,
                    flags as u32,
                    u32::from(services.textures[i]),
                    services.flags[i] as u32
                ]
            );
        }
    }
    let mut cases = 0;
    let mut snapshots = 0;
    for op in 0u32..3 {
        for plane in 0u8..4 {
            for flags in [0i32, 19, 0x40000010, i32::MIN | 16] {
                for permission in [i32::MIN, -1, 0, 1, 2, 3, i32::MAX] {
                    for metadata in [false, true] {
                        for previous in [false, true] {
                            for allowed in [false, true] {
                                for pending in [0u32, 2] {
                                    if op != 0
                                        && (flags != 0
                                            || permission != 0
                                            || metadata
                                            || pending != 0)
                                    {
                                        continue;
                                    }
                                    let oldflags = 0x40000010;
                                    let raw = [
                                        op,
                                        u32::from(plane),
                                        (u32::from(plane) + 1) % 4,
                                        7,
                                        flags as u32,
                                        permission as u32,
                                        u32::from(metadata),
                                        u32::from(previous),
                                        u32::from(allowed),
                                        oldflags,
                                        pending,
                                    ];
                                    for value in raw {
                                        input.write_all(&value.to_le_bytes()).unwrap();
                                    }
                                    input.flush().unwrap();
                                    let mut layers = Gerokasu2Layers::default();
                                    for i in 0..4 {
                                        layers.select_plane(i).unwrap();
                                        layers.set_mask(91 + i, oldflags as i32 + i);
                                    }
                                    layers.select_plane(i32::from(plane)).unwrap();
                                    let mut services =
                                        Services::new(metadata, previous, allowed, oldflags as i32);
                                    if op == 2 {
                                        let original = layers.reference_adv_bank_bytes();
                                        assert!(layers.initialize(-5).is_err());
                                        assert_eq!(layers.reference_adv_bank_bytes(), original);
                                        let constructor = words(&mut output);
                                        assert_eq!(constructor[10], 0);
                                        if plane == 0 {
                                            // Unsupported native singleton address -5 aliases
                                            // allocator free-list tail47; no live dereference.
                                            assert_eq!(constructor[11], 1);
                                            assert_eq!(constructor[12], 0);
                                            assert_eq!(&constructor[14..16], &[256, 47]);
                                            snapshots += 1;
                                            cases += 1;
                                            continue;
                                        }
                                        assert_eq!(constructor[11], 0);
                                        assert_eq!(constructor[12], 1);
                                        let live = words(&mut output);
                                        assert_eq!(&live[..2], &[1, u32::MAX]);
                                        assert_eq!(live[4], u32::from(allowed));
                                        assert_eq!(
                                            &live[6..13],
                                            &[0, u32::from(!previous), 0, 0, 0, 0, 1]
                                        );
                                        for i in 0..4 {
                                            assert_eq!(
                                                &live[16 + i * 4..20 + i * 4],
                                                &[
                                                    91 + i as u32,
                                                    oldflags + i as u32,
                                                    1,
                                                    oldflags + i as u32
                                                ]
                                            );
                                        }
                                        snapshots += 2;
                                        cases += 1;
                                        continue;
                                    }
                                    let command = if op == 0 {
                                        load(7, flags, permission)
                                    } else {
                                        unload()
                                    };
                                    let plan = MaskPlan::prepare(&mut layers, &command).unwrap();
                                    layers
                                        .select_plane(((u32::from(plane) + 1) % 4) as i32)
                                        .unwrap();
                                    let mut owner = MaskController::new(plan);
                                    if op == 0 {
                                        check(
                                            words(&mut output),
                                            &layers,
                                            &services,
                                            plane,
                                            permission as u32,
                                            MaskStage::Start,
                                            false,
                                        );
                                    } else {
                                        let w = words(&mut output);
                                        assert_eq!(&w[..4], &[0, u32::MAX, u32::from(plane), 0]);
                                        for i in 0..4 {
                                            let [id, flags] =
                                                layers.plane_mask(PlaneId::new(i as u8));
                                            assert_eq!(
                                                &w[16 + i * 4..20 + i * 4],
                                                &[id as u32, flags as u32, 1, oldflags + i as u32]
                                            );
                                        }
                                    }
                                    snapshots += 1;
                                    let n = if op == 0 { pending + 2 } else { 1 };
                                    for step in 0..n {
                                        services.ready = step >= pending;
                                        let complete = owner.poll(&mut services).unwrap();
                                        let w = words(&mut output);
                                        if op == 0 {
                                            check(
                                                w,
                                                &layers,
                                                &services,
                                                plane,
                                                permission as u32,
                                                owner.stage(),
                                                complete,
                                            );
                                        } else {
                                            assert_eq!(
                                                &w[..5],
                                                &[
                                                    1,
                                                    u32::MAX,
                                                    u32::from(plane),
                                                    0,
                                                    u32::from(allowed)
                                                ]
                                            );
                                            assert_eq!(
                                                &w[6..13],
                                                &[0, u32::from(!previous), 0, 0, 0, 0, 1]
                                            );
                                            for i in 0..4 {
                                                let [id, flags] =
                                                    layers.plane_mask(PlaneId::new(i as u8));
                                                assert_eq!(
                                                    &w[16 + i * 4..20 + i * 4],
                                                    &[
                                                        id as u32,
                                                        flags as u32,
                                                        u32::from(i != usize::from(plane)),
                                                        oldflags + i as u32
                                                    ]
                                                );
                                            }
                                        }
                                        snapshots += 1;
                                    }
                                    cases += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    drop(input);
    assert!(child.wait().unwrap().success());
    println!("Ger native masks:{cases} cases/{snapshots} complete snapshots");
}
