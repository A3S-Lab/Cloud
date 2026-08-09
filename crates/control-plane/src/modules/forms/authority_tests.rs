use std::path::{Path, PathBuf};

#[test]
fn forms_domain_reuses_form_core_without_workflow_or_persistence_authority() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules/forms/domain");
    let mut violations = Vec::new();
    visit_rust_sources(&root, &mut |path, source| {
        for forbidden in [
            "a3s_flow::",
            "a3s_orm::",
            "sqlx::",
            "tokio::",
            "reqwest::",
            "crate::modules::workflow",
            "struct FormReleaseRef",
            "enum FormReleaseMode",
        ] {
            if source.contains(forbidden) {
                violations.push(format!("{} contains {forbidden}", path.display()));
            }
        }
    });
    assert!(
        violations.is_empty(),
        "Forms domain crossed an authority boundary: {}",
        violations.join(", ")
    );
}

fn visit_rust_sources(root: &Path, visit: &mut impl FnMut(&Path, &str)) {
    let mut pending = vec![PathBuf::from(root)];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            let entries = std::fs::read_dir(&path).expect("read Forms source directory");
            pending.extend(entries.map(|entry| entry.expect("read Forms source entry").path()));
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            let source = std::fs::read_to_string(&path).expect("read Forms source");
            visit(&path, &source);
        }
    }
}
