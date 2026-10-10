use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

use shin_core::{
    format::{mask::read_mask, rom::RomReader},
    primitives::stateless_reader::StatelessFile,
};

#[test]
#[ignore = "requires supplied original/1.03 executables and ROMs; native mask IO/decode/geometry, no hardware"]
fn reference_native_complete_mask_loading() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let data = Path::new(&std::env::var("SHIN_REFERENCE_ROM").unwrap()).to_path_buf();
    let files = data.parent().unwrap();
    let mut base =
        RomReader::new(StatelessFile::new(std::fs::File::open(&data).unwrap()).unwrap()).unwrap();
    let masks = base
        .traverse()
        .map(|(path, _)| path)
        .filter(|path| path.ends_with(".msk"))
        .collect::<Vec<_>>();
    assert_eq!(masks.len(), 30, "supplied original MSK inventory changed");
    // 1.03 provides no MSK replacements; the layered loader uses the base ROM.
    let patch = RomReader::new(
        StatelessFile::new(std::fs::File::open(files.join("patch.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    assert!(
        !patch.traverse().any(|(path, _)| path.ends_with(".msk")),
        "updated MSK overlay needs its own native ROM reads"
    );
    let out = std::env::temp_dir().join(format!("shin-native-mask-load-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("load");
    assert!(
        Command::new("clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-mask-load.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    let mut pixel_count = 0usize;
    let mut rectangle_count = 0usize;
    for (version, executable) in [
        ("original", "uncompressed_executable"),
        ("1.03", "uncompressed_executable_update"),
    ] {
        let dir = out.join(version);
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new(&helper)
            .arg(files.join(executable))
            .arg(&data)
            .arg(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all((masks.join("\n") + "\n").as_bytes())
            .unwrap();
        let native = child.wait_with_output().unwrap();
        std::fs::write(dir.join("trace.txt"), &native.stdout).unwrap();
        std::fs::write(dir.join("stderr.txt"), &native.stderr).unwrap();
        assert!(
            native.status.success(),
            "{version}: {}",
            String::from_utf8_lossy(&native.stderr)
        );
        let text = String::from_utf8(native.stdout).unwrap();
        let mut lines = text.lines();
        for (index, path) in masks.iter().enumerate() {
            let entry = base.find_file(path).unwrap();
            let mut bytes = Vec::new();
            base.open_file(entry)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let mask = read_mask(&bytes).unwrap();
            let (vertices, indices, offsets) = super::vertex_data(&mask.regions);
            let mut vertex_bytes = Vec::with_capacity(vertices.len() * 16);
            for vertex in &vertices {
                let position = vertex.position;
                for value in position.to_array() {
                    vertex_bytes.extend(value.to_le_bytes());
                }
                vertex_bytes.extend(vertex.color.0.to_le_bytes());
            }
            let index_bytes = indices
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>();
            let label = format!("{version}: {path}");
            assert_eq!(
                std::fs::read(dir.join(format!("{index}-vertices.bin"))).unwrap(),
                vertex_bytes,
                "{label}: vertices"
            );
            assert_eq!(
                std::fs::read(dir.join(format!("{index}-indices.bin"))).unwrap(),
                index_bytes,
                "{label}: indices"
            );
            assert_eq!(
                std::fs::read(dir.join(format!("{index}-pixels.bin"))).unwrap(),
                *mask.texels.as_raw(),
                "{label}: texels"
            );
            let row = lines.next().unwrap().split_whitespace().collect::<Vec<_>>();
            assert_eq!(&row[..2], ["M", path.as_str()], "{label}");
            let values = row[2..11]
                .iter()
                .map(|value| value.parse::<u32>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                values,
                vec![
                    mask.texels.width(),
                    mask.texels.height(),
                    vertices.len() as u32,
                    indices.len() as u32,
                    mask.regions.black_regions.region_area,
                    offsets.black_count as u32 * 6,
                    mask.regions.white_regions.region_area,
                    offsets.white_count as u32 * 6,
                    mask.regions.transparent_regions.region_area
                ],
                "{label}: dimensions/region groups"
            );
            assert_eq!(offsets.black_offset, 0);
            assert_eq!(offsets.white_offset, offsets.black_count);
            assert_eq!(
                offsets.transparent_offset,
                offsets.black_count + offsets.white_count
            );
            assert_eq!(
                vertices.len(),
                (offsets.black_count + offsets.white_count + offsets.transparent_count) * 4
            );
            assert_eq!(
                lines.next().unwrap(),
                format!("D {path}"),
                "{label}: native cleanup"
            );
            pixel_count += mask.texels.as_raw().len();
            rectangle_count += mask.regions.rects.len();
        }
        assert!(lines.next().is_none());
        eprintln!(
            "{version}: 30 complete native mask constructors/destructors; actual native ROM lookup/host file reads, byte-exact geometry and decoded texels; GPU services use guarded CPU storage"
        );
    }
    eprintln!(
        "Compared {pixel_count} texels and {rectangle_count} rectangles across both executables. Evidence: {}",
        out.display()
    );
}
