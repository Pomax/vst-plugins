//! Deflate the explainer document into the build directory, so the plugin
//! carries it compressed and inflates it when it is loaded.

use std::io::Write;
use std::path::Path;

fn main() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../default-content.md");
    println!("cargo:rerun-if-changed={}", source.display());
    let text = std::fs::read(&source).expect("reading the explainer document");

    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&text).expect("deflating the explainer document");
    let deflated = encoder.finish().expect("deflating the explainer document");

    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("default-content.deflate");
    std::fs::write(&out, deflated).expect("writing the deflated explainer document");
}
