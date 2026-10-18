use super::*;
use shin_core::vm::gerokasu2::Gerokasu2RuntimeCommand as Cmd;
struct Status {
    value: u32,
    queries: usize,
}
impl MovieServices for Status {
    fn movie_status(&mut self, plane: PlaneId, bank: LayerbankId) -> Result<u32> {
        assert_eq!(plane.raw(), 2);
        self.queries += 1;
        Ok(if bank.raw() == 0 { self.value } else { 0 })
    }
}
#[test]
#[ignore = "original MOVIEWAIT poll/status callbacks;CPU only"]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn ger_native_movie_wait_two_status_bits_and_captured_bank_lists() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/movie");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-movie");
    let c = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-movie.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    let mut request = Vec::new();
    let mut expected = Vec::new();
    for playing in [0, 1, 2, 3] {
        for terminal in [0, 1, 2, 3] {
            for backend in [false, true] {
                for still in [false, true] {
                    for count in [0, 1, 3] {
                        for mask in [0u32, 1, 2, 3, 4, 255, u32::MAX] {
                            request.extend(
                                [0, 0, 0, playing, terminal, backend as u32, still as u32, 0]
                                    .into_iter()
                                    .flat_map(u32::to_le_bytes),
                            );
                            expected.push(true);
                            let mut layers = Gerokasu2Layers::default();
                            layers.select_plane(2).unwrap();
                            for logical in 0..count {
                                layers.load(logical, 6, 0, [0; 8]).unwrap();
                            }
                            layers.select(0, 2);
                            let wait = MovieWait::new(
                                &mut layers,
                                &Cmd::MovieWait {
                                    layer_id: -6,
                                    statuses: mask as i32,
                                },
                            )
                            .unwrap();
                            let value = if backend {
                                playing & 1 | if terminal == 0 { 2 } else { 0 }
                            } else {
                                0
                            };
                            let mut services = Status { value, queries: 0 };
                            let done = wait.poll(&mut services).unwrap();
                            layers.select_plane(0).unwrap();
                            assert_eq!(wait.poll(&mut services).unwrap(), done); // Captured plane survives subsequent state changes.
                            request.extend(
                                [5, mask, count as u32, 0, 0, 0, 0, 0]
                                    .into_iter()
                                    .flat_map(u32::to_le_bytes),
                            );
                            expected.push(done);
                        }
                    }
                }
            }
        }
    }
    let input = folder.join("wait-request.bin");
    fs::write(&input, request).unwrap();
    let run = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::from(fs::File::open(input).unwrap()))
        .output()
        .unwrap();
    fs::write(folder.join("wait-native.trace"), &run.stdout).unwrap();
    fs::write(folder.join("wait-native.stderr"), &run.stderr).unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout.len(), expected.len() * 120);
    for (i, (row, done)) in run
        .stdout
        .chunks_exact(120)
        .zip(expected.iter())
        .enumerate()
    {
        assert_eq!(&row[..4], b"GMOV");
        assert_eq!(
            u32::from_le_bytes(row[4..8].try_into().unwrap()),
            *done as u32,
            "MOVIEWAIT snapshot{i}"
        );
    }
    println!(
        "GEROKASU2_MOVIE_WAIT_NATIVE snapshots={} compared_results={} masks_0_1_2_3_4_255_MAX_sdk_predicates_captured_plane_banks=verified",
        expected.len(),
        expected.len()
    );
}
