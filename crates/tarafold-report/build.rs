//! Embeds the single-file web UI (`web/dist-single/index.html`) when it has been built, and a
//! small placeholder page otherwise, so `cargo build` works without Node (design §11).

use std::path::PathBuf;

const PLACEHOLDER: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="UTF-8"><title>Tarafold report</title>
<style>body{font-family:system-ui,sans-serif;margin:2rem;max-width:60rem}pre{white-space:pre-wrap;background:#f4f5f7;padding:1rem}</style>
</head><body>
<h1>Tarafold report</h1>
<p>This binary was built without the web UI. Build it with <code>pnpm --dir web install; pnpm --dir web build</code>
and rebuild Tarafold to get the interactive report. The raw report follows.</p>
<pre id="raw"></pre>
<!--TARAFOLD_DATA-->
<script>
var el = document.getElementById("tarafold-report");
document.getElementById("raw").textContent = el ? JSON.stringify(JSON.parse(el.textContent), null, 2) : "No report embedded.";
</script>
</body></html>
"#;

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let ui = manifest.join("../../web/dist-single/index.html");
    println!("cargo::rerun-if-changed={}", ui.display());
    println!("cargo::rerun-if-env-changed=TARAFOLD_UI_HTML");
    let src = std::env::var_os("TARAFOLD_UI_HTML")
        .map(PathBuf::from)
        .unwrap_or(ui);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("ui.html");
    let (html, built) = match std::fs::read_to_string(&src) {
        Ok(s) if s.contains("<!--TARAFOLD_DATA-->") => (s, true),
        _ => (PLACEHOLDER.to_string(), false),
    };
    if let Err(e) = std::fs::write(&out, html) {
        println!("cargo::warning=could not write {}: {e}", out.display());
    }
    println!(
        "cargo::rustc-env=TARAFOLD_UI_BUILT={}",
        if built { "1" } else { "0" }
    );
}
