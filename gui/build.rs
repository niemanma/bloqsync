fn main() {
    gen_json_index("animations", "animations_index.rs");
    gen_json_index("models", "models_index.rs");
    tauri_build::build()
}

/// Generate a Rust file exposing every `*.json` in `dir` as an embedded string
/// constant, so each animation/model can live in its own file and be added by
/// simply dropping a new file into the folder.
fn gen_json_index(dir: &str, out_name: &str) {
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let entries = files
        .iter()
        .map(|p| {
            let abs = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            format!("include_str!(\"{}\")", abs.display())
        })
        .collect::<Vec<_>>()
        .join(",\n    ");
    let body = format!("&[\n    {entries}\n]\n");
    std::fs::write(std::path::Path::new(&out).join(out_name), body).expect("write index");
    println!("cargo:rerun-if-changed={dir}");
}
