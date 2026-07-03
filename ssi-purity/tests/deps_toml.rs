use ssi_purity::{parse_deps_toml, DeclaredDep};
use std::fs;
use ssi_purity::filter_declared_deps;
use ssi_purity::scan_vendored_tree;

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

fn vwrite(dir: &std::path::Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn clean_vendor_tree_passes() {
    let root = std::env::temp_dir().join("ssi-vendor-clean");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(&root, "rand-0.8.5/Cargo.toml", "[package]\nname=\"rand\"\n");
    vwrite(&root, "rand-0.8.5/src/lib.rs", "pub fn f() -> u32 { 1 }\n");
    assert!(scan_vendored_tree(&root).is_ok());
}

#[test]
fn cc_build_dependency_is_rejected() {
    // A `cc`/`cmake`/`bindgen`/... build-dependency compiles native code from
    // build.rs — a sound, false-positive-free native signal we reject.
    let root = std::env::temp_dir().join("ssi-vendor-cc");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(
        &root,
        "flate2-1.0/Cargo.toml",
        "[package]\nname=\"flate2\"\n[build-dependencies]\ncc = \"1.0\"\n",
    );
    vwrite(&root, "flate2-1.0/src/lib.rs", "pub fn f() {}\n");
    assert!(scan_vendored_tree(&root).is_err());
}

#[test]
fn cc_build_dependency_table_form_is_rejected() {
    // The `[build-dependencies.cc]` table form must be caught too.
    let root = std::env::temp_dir().join("ssi-vendor-cc-table");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(
        &root,
        "foo-1.0/Cargo.toml",
        "[package]\nname=\"foo\"\n[build-dependencies.cc]\nversion = \"1.0\"\n",
    );
    vwrite(&root, "foo-1.0/src/lib.rs", "pub fn f() {}\n");
    assert!(scan_vendored_tree(&root).is_err());
}

#[test]
fn target_conditional_cc_build_dependency_is_rejected() {
    // A C-toolchain build-dep under a target-conditional section
    // (`[target.'cfg(...)'.build-dependencies]`) must be caught too — otherwise
    // a crate that compiles C only on some target would pass the scan on a host
    // where that target's section is inert.
    let root = std::env::temp_dir().join("ssi-vendor-cc-target");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(
        &root,
        "foo-1.0/Cargo.toml",
        "[package]\nname=\"foo\"\n[target.'cfg(unix)'.build-dependencies]\ncc = \"1.0\"\n",
    );
    vwrite(&root, "foo-1.0/src/lib.rs", "pub fn f() {}\n");
    assert!(scan_vendored_tree(&root).is_err());
}

#[test]
fn workspace_inherited_cc_build_dependency_is_rejected() {
    // The dotted `cc.workspace = true` / `cc.version = "1"` key forms must be
    // recognized as the `cc` build-dep, not read as a key named `cc.workspace`.
    let root = std::env::temp_dir().join("ssi-vendor-cc-workspace");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(
        &root,
        "foo-1.0/Cargo.toml",
        "[package]\nname=\"foo\"\n[build-dependencies]\ncc.workspace = true\n",
    );
    vwrite(&root, "foo-1.0/src/lib.rs", "pub fn f() {}\n");
    assert!(scan_vendored_tree(&root).is_err());
}

#[test]
fn prebuilt_native_artifact_is_rejected() {
    // A committed linkable blob bypasses building from source.
    let root = std::env::temp_dir().join("ssi-vendor-blob");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(&root, "foo-1.0/Cargo.toml", "[package]\nname=\"foo\"\n");
    vwrite(&root, "foo-1.0/src/lib.rs", "pub fn f() {}\n");
    vwrite(&root, "foo-1.0/vendor/libfoo.a", "\x7fELF not really\n");
    assert!(scan_vendored_tree(&root).is_err());
}

#[test]
fn bare_links_version_guard_is_allowed() {
    // `links` alone links NOTHING in the common single-version-guard idiom
    // (e.g. rayon-core, which feral's tree pulls in). Rejecting it would bar a
    // pure-Rust crate; the no-C-compiler build backstops any real native link.
    let root = std::env::temp_dir().join("ssi-vendor-links-guard");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(
        &root,
        "rayon-core-1.13.0/Cargo.toml",
        "[package]\nname=\"rayon-core\"\nlinks=\"rayon-core\"\n",
    );
    vwrite(&root, "rayon-core-1.13.0/src/lib.rs", "pub fn f() {}\n");
    assert!(scan_vendored_tree(&root).is_ok());
}

#[test]
fn c_abi_export_and_proc_macro_in_dep_are_allowed() {
    // A dependency may legitimately EXPORT a C ABI (`#[no_mangle] extern "C" fn`,
    // a definition — no foreign code runs) or use a proc-macro. These are NOT
    // scanned for in the dependency tree: source purity is enforced only on the
    // submission (src/ordering/), and the no-cc build guarantees no foreign code
    // executes. (This is exactly what feral's capi.rs / serde's proc-macros do.)
    let root = std::env::temp_dir().join("ssi-vendor-cabi-export");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(&root, "feralish-0.1.0/Cargo.toml", "[package]\nname=\"feralish\"\n");
    vwrite(
        &root,
        "feralish-0.1.0/src/lib.rs",
        "#[no_mangle]\npub extern \"C\" fn exported() -> u32 { 1 }\nuse proc_macro::TokenStream;\n",
    );
    assert!(scan_vendored_tree(&root).is_ok());
}

#[test]
fn sys_suffix_crate_is_rejected() {
    let root = std::env::temp_dir().join("ssi-vendor-sys");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(&root, "openssl-sys-0.9/Cargo.toml", "[package]\nname=\"openssl-sys\"\n");
    vwrite(&root, "openssl-sys-0.9/src/lib.rs", "pub fn f() {}\n");
    assert!(scan_vendored_tree(&root).is_err());
}

#[test]
fn sys_suffix_crate_with_hyphenated_prerelease_version_is_rejected() {
    // A hyphenated pre-release version (`1.0.0-alpha.1`) must not let a `*-sys`
    // crate slip past the name check: a naive last-hyphen split would leave
    // `foo-sys-1.0.0`, which does not end in `-sys`. The version boundary is the
    // last `-` before a digit, so the name is correctly `foo-sys`.
    let root = std::env::temp_dir().join("ssi-vendor-sys-prerelease");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(&root, "foo-sys-1.0.0-alpha.1/Cargo.toml", "[package]\nname=\"foo-sys\"\n");
    vwrite(&root, "foo-sys-1.0.0-alpha.1/src/lib.rs", "pub fn f() {}\n");
    assert!(scan_vendored_tree(&root).is_err());
}

#[test]
fn non_sys_crate_with_hyphenated_name_passes() {
    // An innocent hyphenated crate name whose last component isn't `sys` must not
    // be falsely rejected (guards against over-eager name stripping).
    let root = std::env::temp_dir().join("ssi-vendor-hyphen-ok");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(&root, "system-deps-1.2.3/Cargo.toml", "[package]\nname=\"system-deps\"\n");
    vwrite(&root, "system-deps-1.2.3/src/lib.rs", "pub fn f() -> u32 { 1 }\n");
    assert!(scan_vendored_tree(&root).is_ok());
}
