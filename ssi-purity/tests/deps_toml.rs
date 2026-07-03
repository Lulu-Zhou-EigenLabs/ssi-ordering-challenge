use ssi_purity::{parse_deps_toml, DeclaredDep};

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
