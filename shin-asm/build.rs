fn main() {
    generate_fixture_tests();
    // Enumerate files in sub-folder "res/*", being relevant for the test-generation
    // If function returns with error, exit with error message.
    build_deps::rerun_if_changed_paths("test_data/*/*").unwrap();

    // Adding the parent directory "res" to the watch-list will capture new-files being added
    build_deps::rerun_if_changed_paths("test_data/*").unwrap();
}

// Generate resource tests using manifest-relative paths, independently of the
// directory from which Cargo was launched. Every .sal fixture gets its own test.
fn generate_fixture_tests() {
    use std::{env, fs, path::PathBuf};
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut parser = String::new();
    let mut hir = String::new();
    let fixtures = [
        ("test_data/lexer/ok", "lex_ok", true),
        ("test_data/lexer/err", "lex_err", true),
        ("test_data/parser/ok", "parse_ok", true),
        ("test_data/parser/err", "parse_err", true),
        ("test_data/hir/ok", "lower_ok", false),
    ];
    for (directory, function, is_parser) in fixtures {
        let directory = manifest.join(directory);
        println!("cargo:rerun-if-changed={}", directory.display());
        let mut paths: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "sal"))
            .collect();
        paths.sort();
        assert!(
            !paths.is_empty(),
            "Missing fixtures in {}",
            directory.display()
        );
        for (index, path) in paths.iter().enumerate() {
            let source = format!(
                "#[test] fn {function}_resource_{index}() {{ {function}({:?}); }}\n",
                path.to_str().unwrap()
            );
            if is_parser {
                parser.push_str(&source);
            } else {
                hir.push_str(&source);
            }
        }
    }
    fs::write(out.join("parser_resource_tests.rs"), parser).unwrap();
    fs::write(out.join("hir_resource_tests.rs"), hir).unwrap();
}
