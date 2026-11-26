//! Build script: embed the built Web UI (`apps/web/dist`) into the `hub` binary.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One embedded asset: the absolute canonical path plus its HTTP content type.
struct Asset {
    path: PathBuf,
    content_type: &'static str,
}

fn main() {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );
    let dist = manifest_dir.join("../../apps/web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    println!(
        "cargo:rerun-if-changed={}",
        dist.join("index.html").display()
    );

    let mut assets = BTreeMap::new();
    collect(&dist, &dist, &mut assets);

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    std::fs::write(out_dir.join("web_assets.rs"), render(&assets))
        .expect("failed to write web_assets.rs");
}

/// Walk `dist` deterministically, keyed by the URL path served to the browser.
fn collect(root: &Path, dir: &Path, assets: &mut BTreeMap<String, Asset>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            collect(root, &path, assets);
        } else if path.is_file() {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let url = format!("/{}", relative.to_string_lossy().replace('\\', "/"));
            let content_type = content_type(&path);
            assets.insert(
                url,
                Asset {
                    path: std::fs::canonicalize(&path).unwrap_or(path),
                    content_type,
                },
            );
        }
    }
}

fn render(assets: &BTreeMap<String, Asset>) -> String {
    if assets.is_empty() {
        return "pub const ASSETS: &[(&str, &str, &[u8])] = &[];\n".to_string();
    }
    let mut source = String::from("pub const ASSETS: &[(&str, &str, &[u8])] = &[\n");
    for (url, asset) in assets {
        source.push_str(&format!(
            "    ({url:?}, {:?}, include_bytes!({:?})),\n",
            asset.content_type, asset.path
        ));
    }
    source.push_str("];\n");
    source
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
