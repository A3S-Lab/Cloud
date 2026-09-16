fn main() {
    let input = std::env::args().nth(1).expect("input path");
    let output = std::env::args().nth(2).expect("output path");
    let src = std::fs::read_to_string(&input).expect("read");
    let manifest = a3s_cloud_contracts::AppPlatformParityManifest::parse_acl(&src)
        .unwrap_or_else(|e| panic!("parse: {e}"));
    std::fs::write(&output, manifest.canonical_acl()).expect("write");
    eprintln!("canonicalized {} -> {}", input, output);
}
