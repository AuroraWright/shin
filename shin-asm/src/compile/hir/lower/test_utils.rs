use std::sync::Arc;

use itertools::Itertools;

use crate::compile::{
    BlockId, Db, File, HirBlockBody,
    diagnostics::{
        AriadneDbCache, Diagnostic, HirDiagnosticAccumulator, HirLocation,
        SourceDiagnosticAccumulator, Span,
    },
    hir,
    hir::{HirBlockBodies, lower::HirDiagnosticCollector},
};

pub fn diagnostics_to_str(
    db: &dyn Db,
    hir_diags: Vec<Diagnostic<HirLocation>>,
    source_diags: Vec<Diagnostic<Span>>,
) -> String {
    let mut cache = AriadneDbCache::new(db);

    source_diags
        .into_iter()
        .map(|diag| diag.into_ariadne(db))
        .chain(hir_diags.into_iter().map(|diag| diag.into_ariadne(db)))
        .map(|diag| {
            let mut out = Vec::new();
            diag.write(&mut cache, &mut out).unwrap();
            String::from_utf8(strip_ansi_escapes::strip(out)).unwrap()
        })
        .join("\n\n")
}

pub fn diagnostic_collector_to_str(db: &dyn Db, collector: HirDiagnosticCollector) -> String {
    diagnostics_to_str(db, collector.into_diagnostics(), vec![])
}

pub fn lower_hir_ok<'db>(db: &'db dyn Db, source: &str) -> (File, HirBlockBodies<'db>) {
    let file = File::new(db, "test.sal".to_string(), source.to_string());
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
    let diags = diagnostics_to_str(db, hir_errors, source_errors);

    if !diags.is_empty() {
        panic!("lowering produced errors:\n{diags}");
    }

    (file, bodies)
}

pub fn lower_hir_block_ok(db: &dyn Db, source: &str) -> (File, BlockId, Arc<HirBlockBody>) {
    let (file, bodies) = lower_hir_ok(db, source);
    let block_ids = bodies.get_block_ids(db);
    assert_eq!(block_ids.len(), 1, "expected exactly one block");
    let block_id = block_ids[0];

    (
        file,
        block_id,
        bodies.get_block(db, block_id).unwrap().clone(),
    )
}
