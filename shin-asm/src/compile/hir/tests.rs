use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use expect_test::expect_file;
use crate::compile::{
    File,
    db::Database,
    diagnostics::{HirDiagnosticAccumulator, SourceDiagnosticAccumulator},
    hir,
};

fn lower_block(code: &str) -> Arc<hir::HirBlockBody> {
    let db = Database::default();
    let db = &db;
    let file = File::new(db, "test.sal".to_string(), code.to_string());
    let bodies = hir::collect_file_bodies(db, file);

    let hir_errors = hir::collect_file_bodies::accumulated::<HirDiagnosticAccumulator>(db, file)
        .into_iter()
        .map(|diagnostic| diagnostic.0.clone())
        .collect::<Vec<_>>();
    let source_errors =
        hir::collect_file_bodies::accumulated::<SourceDiagnosticAccumulator>(db, file)
            .into_iter()
            .map(|diagnostic| diagnostic.0.clone())
            .collect::<Vec<_>>();
    if !source_errors.is_empty() || !hir_errors.is_empty() {
        panic!(
            "lowering produced errors:\n\
                source-level: {source_errors:?}\n\
                hir-level: {hir_errors:?}"
        );
    }

    let block_ids = bodies.get_block_ids(db);
    assert_eq!(block_ids.len(), 1, "expected exactly one block");
    bodies.get_block(db, block_ids[0]).unwrap().clone()
}

fn lower_ok(sal: &str) {
    let case = TestCase::from_sal_path(sal);
    let block_body = lower_block(&case.text);
    let block_body = block_body.debug_dump();
    expect_file![case.hir].assert_eq(&block_body);
}

// // fn parse_err(sal: &str) {
//     let case = TestCase::from_sal_path(sal);
//     let (actual, errors) = parse(&case.text);
//     assert!(
//         errors,
//         "no errors in an ERR file {}:\n{actual}",
//         case.sal.display()
//     );
//     expect_file![case.sast].assert_eq(&actual)
// }

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct TestCase {
    sal: PathBuf,
    hir: PathBuf,
    text: String,
}

impl TestCase {
    fn from_sal_path(path: &str) -> TestCase {
        let crate_root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let sal = crate_root_dir.join(path);
        let hir = sal.with_extension("hir");
        let text = fs::read_to_string(&sal).unwrap();
        TestCase { sal, hir, text }
    }
}

include!(concat!(env!("OUT_DIR"), "/hir_resource_tests.rs"));
