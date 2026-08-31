use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const ACL_V0_3_SOURCE: &str = "git+https://github.com/A3S-Lab/ACL.git?rev=5317e166222495585909d81f2caffdca90273c99#5317e166222495585909d81f2caffdca90273c99";
const BOOT_SCHEMA_ADMISSION_SOURCE: &str = "git+https://github.com/A3S-Lab/Boot.git?rev=83d489fb2274ab8e0d277ccd87461cc35c1a9b88#83d489fb2274ab8e0d277ccd87461cc35c1a9b88";
const CODE_CORE_SOURCE: &str = "git+https://github.com/A3S-Lab/Code.git?rev=97942a959a6c96b5616daf0f8c09692ec959e013#97942a959a6c96b5616daf0f8c09692ec959e013";
const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";
const ORM_SCHEMA_ADMISSION_SOURCE: &str = "git+https://github.com/A3S-Lab/ORM.git?rev=52944002dc84b07d88a85f2a4a87f913655e62b5#52944002dc84b07d88a85f2a4a87f913655e62b5";

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
fn boot_uses_the_schema_admission_revision() {
    let packages = locked_a3s_packages()
        .into_iter()
        .filter(|package| package.name == "a3s-boot")
        .collect::<Vec<_>>();
    assert_eq!(
        packages.len(),
        1,
        "Cloud must resolve exactly one Boot package"
    );
    assert_eq!(packages[0].version, "0.2.0");
    assert_eq!(packages[0].source, BOOT_SCHEMA_ADMISSION_SOURCE);
}

#[test]
fn code_core_uses_the_exact_agent_release_contract_revision() {
    let packages = locked_a3s_packages()
        .into_iter()
        .filter(|package| package.name == "a3s-code-core")
        .collect::<Vec<_>>();
    assert_eq!(
        packages.len(),
        1,
        "Cloud must resolve exactly one Code Core package"
    );
    assert_eq!(packages[0].version, "8.0.4");
    assert_eq!(packages[0].source, CODE_CORE_SOURCE);
}

#[test]
fn flow_uses_the_published_v1_release() {
    let packages = locked_a3s_packages()
        .into_iter()
        .filter(|package| package.name == "a3s-flow")
        .collect::<Vec<_>>();
    assert_eq!(
        packages.len(),
        1,
        "Cloud must resolve exactly one Flow package"
    );
    assert_eq!(packages[0].version, "1.1.0");
    assert_eq!(packages[0].source, CRATES_IO_SOURCE);
}

#[test]
fn orm_uses_the_schema_admission_revision_required_by_flow_and_boot() {
    let packages = locked_a3s_packages()
        .into_iter()
        .filter(|package| package.name == "a3s-orm")
        .collect::<Vec<_>>();
    assert_eq!(
        packages.len(),
        1,
        "Cloud must resolve exactly one ORM package"
    );
    assert_eq!(packages[0].version, "0.3.1");
    assert_eq!(packages[0].source, ORM_SCHEMA_ADMISSION_SOURCE);
}

#[test]
fn box_provider_gate_uses_the_locked_box_revision() {
    let repository = repository_root();
    let provider_revision =
        fs::read_to_string(repository.join("tools/box-conformance/box-revision"))
            .expect("read Box provider revision");
    let provider_revision = provider_revision.trim();
    assert!(
        provider_revision.len() == 40
            && provider_revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
        "Box provider revision must be a full hexadecimal commit"
    );

    let packages = locked_a3s_packages()
        .into_iter()
        .filter(|package| package.name.starts_with("a3s-box-"))
        .collect::<Vec<_>>();
    assert_eq!(
        packages
            .iter()
            .map(|package| package.name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["a3s-box-core", "a3s-box-netproxy", "a3s-box-runtime"]),
        "the locked Box package closure changed"
    );
    let expected_source_suffix = format!("?rev={provider_revision}#{provider_revision}");
    for package in packages {
        assert!(
            package.source.ends_with(&expected_source_suffix),
            "{} is not locked to the Box provider revision {provider_revision}: {}",
            package.name,
            package.source
        );
    }
}

fn locked_a3s_packages() -> Vec<LockedPackage> {
    let repository = repository_root();
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

fn repository_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn quoted_field(record: &str, field: &str) -> Option<String> {
    let prefix = format!("{field} = \"");
    record
        .lines()
        .find_map(|line| line.strip_prefix(&prefix)?.strip_suffix('"'))
        .map(str::to_owned)
}
