//! Fixture harness for tests/adapt-sizes.test.mjs. Always opens an isolated copy.
use desegnos_core::{adapt_size, direct_export_bundle};
use desegnos_storage::{ProjectDatabase, write_export_into};
use std::{path::Path, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let output = Path::new(&args[2]);
    std::fs::create_dir_all(output).unwrap();
    let copy = output.join("fixture-copy.desegnos");
    std::fs::copy(&args[1], &copy).unwrap();
    let db = ProjectDatabase::open(&copy).unwrap();
    let mut project = db
        .autosave_work()
        .unwrap()
        .map(|a| a.project)
        .unwrap_or_else(|| db.load().unwrap().project);
    let ids: Vec<_> = project.pages.iter().map(|p| p.id).collect();
    fn export(project: &desegnos_core::Project, ids: &[uuid::Uuid], output: &Path) {
        let bundle = direct_export_bundle(project, ids).unwrap();
        let export = desegnos_core::WebsiteExport {
            page_count: bundle.page_count(),
            files: bundle.files,
            warnings: bundle.warnings,
        };
        std::fs::create_dir_all(output).unwrap();
        write_export_into(output, &export).unwrap();
    }
    export(&project, &ids, &output.join("before"));
    let before: Vec<_> = project
        .pages
        .iter()
        .map(|p| desegnos_core::resolve_for_size(&project, p, p.base_size_id()))
        .collect();
    let start = Instant::now();
    for page in &mut project.pages {
        let sizes: Vec<_> = page
            .sizes
            .iter()
            .filter(|s| s.width < page.base_size().width)
            .map(|s| s.id)
            .collect();
        for id in sizes {
            adapt_size(page, id).unwrap();
        }
    }
    let elapsed = start.elapsed();
    println!("Adapted {} pages in {:?}", project.pages.len(), elapsed);
    // Debug builds include font discovery and unoptimised layout; release should be much faster.
    let budget_ms = if cfg!(debug_assertions) { 1000 } else { 100 };
    assert!(
        elapsed.as_millis() < budget_ms,
        "adaptation exceeded {budget_ms} ms budget"
    );
    for (page, base) in project.pages.iter().zip(before) {
        let mut resolved = desegnos_core::resolve_for_size(&project, page, page.base_size_id());
        fn strip(elements: &mut [desegnos_core::Element]) {
            for e in elements {
                e.overrides.clear();
                strip(&mut e.children);
            }
        }
        let mut base = base;
        strip(&mut base);
        strip(&mut resolved);
        assert_eq!(base, resolved);
    }
    export(&project, &ids, &output.join("after"));
    std::fs::write(
        output.join("adapted.json"),
        serde_json::to_vec_pretty(&project).unwrap(),
    )
    .unwrap();
}
