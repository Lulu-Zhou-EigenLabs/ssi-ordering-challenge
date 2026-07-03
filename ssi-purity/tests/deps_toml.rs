use ssi_purity::{parse_deps_toml, DeclaredDep};
use std::fs;
use ssi_purity::filter_declared_deps;

#[test]
fn missing_deps_toml_yields_empty() {
    let dir = std::env::temp_dir().join("ssi-purity-test-nodeps");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::remove_file(dir.join("deps.toml"));
    assert!(filter_declared_deps(&dir).unwrap().is_empty());
}

#[test]
fn present_deps_toml_is_parsed() {
    let dir = std::env::temp_dir().join("ssi-purity-test-withdeps");
    let _ = fs::create_dir_all(&dir);
    fs::write(dir.join("deps.toml"), "[dependencies]\nrand = \"0.8.5\"\n").unwrap();
    let got = filter_declared_deps(&dir).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].name, "rand");
}

#[test]
fn parses_simple_version_entries() {
    let src = "[dependencies]\nrand = \"0.8.5\"\npetgraph = \"0.6.4\"\n";
    let got = parse_deps_toml(src).expect("valid");
    assert_eq!(
        got,
        vec![
            DeclaredDep { name: "rand".into(), version: "0.8.5".into() },
            DeclaredDep { name: "petgraph".into(), version: "0.6.4".into() },
        ]
    );
}

#[test]
fn empty_file_is_ok_and_empty() {
    assert!(parse_deps_toml("").unwrap().is_empty());
    assert!(parse_deps_toml("[dependencies]\n").unwrap().is_empty());
}

#[test]
fn inline_table_is_rejected() {
    // The form that could carry git/path/features — must be impossible.
    let src = "[dependencies]\nevil = { git = \"https://x/y\" }\n";
    assert!(parse_deps_toml(src).is_err());
}

#[test]
fn unknown_section_is_rejected() {
    let src = "[build-dependencies]\ncc = \"1\"\n";
    assert!(parse_deps_toml(src).is_err());
}

#[test]
fn non_semverish_version_is_rejected() {
    // A version string must look like digits/dots (no "*", no ranges, no git refs).
    let src = "[dependencies]\nrand = \"*\"\n";
    assert!(parse_deps_toml(src).is_err());
}
