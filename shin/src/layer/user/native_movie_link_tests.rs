use std::{path::Path, process::Command};

#[test]
#[ignore = "requires supplied original/1.03 executables beside SHIN_REFERENCE_ROM; CPU-only native Movie factory/request/worker"]
fn reference_native_movie_rejected_linked_picture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let files = Path::new(&rom).parent().unwrap();
    let out = std::env::var_os("SHIN_NATIVE_MOVIE_LINK_OUTPUT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("shin-native-movie-link-{}", std::process::id()))
        });
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("movie-link");
    assert!(
        Command::new("clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-movie-linked-picture.c"))
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
        assert!(result.stderr.is_empty());
        let trace = String::from_utf8(result.stdout.clone()).unwrap();
        let mut rows = trace.lines();
        let mut case = 0;
        let mut pictures = 0;
        for count in [0, 1, 17, 32768, 65536] {
            for name in [0, 1] {
                for settings in 0..4 {
                    for id in [i16::MIN, -1, 0, 1, 16, 17, i16::MAX] {
                        let wanted = usize::from(id >= 0 && i32::from(id) < count);
                        assert_eq!(
                            rows.next().unwrap(),
                            format!(
                                "L {case} {count} {name} {settings} {id} {wanted} {wanted} {wanted} 1"
                            )
                        );
                        assert_eq!(rows.next().unwrap(), format!("D {case}"));
                        pictures += wanted;
                        case += 1;
                    }
                }
            }
        }
        assert_eq!((case, pictures), (280, 112));
        assert!(rows.next().is_none());
        if let Some(previous) = &previous {
            assert_eq!(&result.stdout, previous);
        }
        for mutation in [
            "lookup",
            "signed",
            "worker",
            "picture_gate",
            "request_cleanup",
        ] {
            let mutant = Command::new(&helper)
                .arg(files.join(executable))
                .env("SHIN_NATIVE_LINKED_PICTURE_MUTANT", mutation)
                .output()
                .unwrap();
            std::fs::write(
                out.join(format!("{version}-{mutation}.log")),
                &mutant.stderr,
            )
            .unwrap();
            assert!(
                !mutant.status.success() || mutant.stdout != result.stdout,
                "Undetected mutation {mutation}"
            );
        }
        previous = Some(result.stdout);
        eprintln!(
            "{version}:280 native Movie linked-picture cases,112 requested/168 omitted stills, balanced request/resource cleanup,5 mutations detected"
        );
    }
    eprintln!("Native Movie linked-picture evidence: {}", out.display());
}
