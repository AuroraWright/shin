use std::{path::Path, process::Command};

#[test]
#[ignore = "requires supplied original/1.03 executables beside SHIN_REFERENCE_ROM; CPU-only native factory/hydration"]
fn reference_native_invalid_drawable_metadata_fallback() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let files = Path::new(&rom).parent().unwrap();
    let out = std::env::var_os("SHIN_NATIVE_METADATA_OUTPUT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("shin-native-metadata-{}", std::process::id()))
        });
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("metadata-fallback");
    assert!(
        Command::new("clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-drawable-metadata-fallback.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    let mut previous = None;
    for (version, executable) in [
        ("original", "uncompressed_executable"),
        ("1.03", "uncompressed_executable_update"),
    ] {
        let result = Command::new(&helper)
            .arg(files.join(executable))
            .output()
            .unwrap();
        std::fs::write(out.join(format!("{version}.trace")), &result.stdout).unwrap();
        std::fs::write(out.join(format!("{version}.log")), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let trace = String::from_utf8(result.stdout.clone()).unwrap();
        let mut rows = trace.lines();
        let mut case = 0;
        let mut published = 0;
        for count in [0, 1, 17] {
            for banks in [3, 48] {
                for seed in [0, 1] {
                    for id in [-1, i32::MIN, i32::MAX, count] {
                        assert_eq!(
                            rows.next().unwrap(),
                            format!("F {case} {count} {banks} {seed} {id} 1 {banks}")
                        );
                        assert_eq!(rows.next().unwrap(), format!("D {case}"));
                        published += banks;
                        case += 1;
                    }
                }
            }
        }
        assert_eq!((case, published), (48, 1224));
        assert!(rows.next().is_none());
        if let Some(previous) = &previous {
            assert_eq!(
                &result.stdout, previous,
                "Original/update fallback behavior differs"
            );
        }
        for mutation in [
            "picture_null",
            "bustup_null",
            "movie_null",
            "null_getter",
            "worker",
            "cleanup",
        ] {
            let mutant = Command::new(&helper)
                .arg(files.join(executable))
                .env("SHIN_NATIVE_METADATA_FALLBACK_MUTANT", mutation)
                .output()
                .unwrap();
            std::fs::write(
                out.join(format!("{version}-{mutation}.log")),
                &mutant.stderr,
            )
            .unwrap();
            assert!(
                !mutant.status.success() || mutant.stdout != result.stdout,
                "Undetected native mutation {mutation}"
            );
        }
        previous = Some(result.stdout);
        eprintln!(
            "{version}:48 native invalid metadata factory/hydration cases,1224 exact Null owners and balanced cleanup,6 body mutations detected"
        );
    }
    eprintln!("Native metadata fallback evidence: {}", out.display());
}
