use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let lockfile = manifest_dir.join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lockfile.display());
    let contents = fs::read_to_string(&lockfile).expect("workspace Cargo.lock should be readable");
    for (package, variable) in [
        ("docx-rs", "DOCX_RS_VERSION"),
        ("pdf-extract", "PDF_EXTRACT_VERSION"),
    ] {
        let version = locked_version(&contents, package)
            .unwrap_or_else(|| panic!("{package} must be pinned in Cargo.lock"));
        println!("cargo:rustc-env={variable}={version}");
    }
}

fn locked_version<'a>(lockfile: &'a str, package: &str) -> Option<&'a str> {
    lockfile.split("[[package]]").find_map(|entry| {
        let mut name = None;
        let mut version = None;
        for line in entry.lines().map(str::trim) {
            if let Some(value) = line.strip_prefix("name = ") {
                name = Some(value.trim_matches('"'));
            } else if let Some(value) = line.strip_prefix("version = ") {
                version = Some(value.trim_matches('"'));
            }
        }
        (name == Some(package)).then_some(version).flatten()
    })
}
