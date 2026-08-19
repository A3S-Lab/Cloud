use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const ACL_V0_3_SOURCE: &str = "git+https://github.com/A3S-Lab/ACL.git?rev=5317e166222495585909d81f2caffdca90273c99#5317e166222495585909d81f2caffdca90273c99";
const FLOW_V1_RC_SOURCE: &str = "git+https://github.com/A3S-Lab/Flow.git?rev=878df66915ca9c1c8c5454b0872043937b60f0e7#878df66915ca9c1c8c5454b0872043937b60f0e7";

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockedPackage {
    name: String,
    version: String,
    source: String,
}

#[test]
fn an_a3s_package_version_has_only_one_source() {
    let mut sources = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for package in locked_a3s_packages() {
        sources
            .entry((package.name, package.version))
            .or_default()
            .insert(package.source);
    }

    let duplicates = sources
        .into_iter()
        .filter(|(_, sources)| sources.len() > 1)
        .collect::<BTreeMap<_, _>>();
    assert!(
        duplicates.is_empty(),
        "one A3S package version resolved from multiple authorities: {duplicates:#?}"
    );
}

#[test]
fn a3s_dependency_version_debt_does_not_expand() {
    let mut versions = BTreeMap::<String, BTreeSet<String>>::new();
    for package in locked_a3s_packages() {
        versions
            .entry(package.name)
            .or_default()
            .insert(package.version);
    }
    let duplicates = versions
        .into_iter()
        .filter(|(_, versions)| versions.len() > 1)
        .collect::<BTreeMap<_, _>>();
    let expected = BTreeMap::from([(
        "a3s-acl".to_owned(),
        BTreeSet::from(["0.2.2".to_owned(), "0.3.0".to_owned()]),
    )]);
    assert_eq!(
        duplicates, expected,
        "the root lock changed its explicit A3S version-debt budget; converge an existing debt or document its owning upstream before changing this guard"
    );
}

#[test]
fn acl_v0_3_uses_the_box_compatible_exact_revision() {
    let sources = locked_a3s_packages()
        .into_iter()
        .filter(|package| package.name == "a3s-acl" && package.version == "0.3.0")
        .map(|package| package.source)
        .collect::<BTreeSet<_>>();
    assert_eq!(sources, BTreeSet::from([ACL_V0_3_SOURCE.to_owned()]));
}

#[test]
fn flow_uses_the_qualified_release_candidate_revision() {
    let packages = locked_a3s_packages()
        .into_iter()
        .filter(|package| package.name == "a3s-flow")
        .collect::<Vec<_>>();
    assert_eq!(
        packages.len(),
        1,
        "Cloud must resolve exactly one Flow package"
    );
    assert_eq!(packages[0].version, "1.0.0-rc.1");
    assert_eq!(packages[0].source, FLOW_V1_RC_SOURCE);
}

fn locked_a3s_packages() -> Vec<LockedPackage> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lock = fs::read_to_string(repository.join("Cargo.lock")).expect("read root Cargo.lock");
    lock.split("[[package]]")
        .skip(1)
        .filter_map(|record| {
            let name = quoted_field(record, "name")?;
            if !name.starts_with("a3s-") {
                return None;
            }
            Some(LockedPackage {
                name,
                version: quoted_field(record, "version").expect("locked package version"),
                source: quoted_field(record, "source").unwrap_or_else(|| "workspace".into()),
            })
        })
        .collect()
}

fn quoted_field(record: &str, field: &str) -> Option<String> {
    let prefix = format!("{field} = \"");
    record
        .lines()
        .find_map(|line| line.strip_prefix(&prefix)?.strip_suffix('"'))
        .map(str::to_owned)
}
