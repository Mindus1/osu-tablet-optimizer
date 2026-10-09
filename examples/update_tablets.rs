use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

fn main() -> Result<()> {
    let checkout = std::env::args()
        .nth(1)
        .context("usage: cargo run --example update_tablets -- <OpenTabletDriver checkout>")?;
    let mut files = Vec::new();
    collect_json(
        &Path::new(&checkout).join("OpenTabletDriver.Configurations/Configurations"),
        &mut files,
    )?;

    let mut rows = Vec::new();
    for path in files {
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let config: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .with_context(|| format!("parsing {}", path.display()))?;
        let digitizer = &config["Specifications"]["Digitizer"];
        if let (Some(name), Some(width), Some(height)) = (
            config["Name"].as_str(),
            digitizer["Width"].as_f64(),
            digitizer["Height"].as_f64(),
        ) {
            rows.push(format!("{name}\t{width}\t{height}"));
        }
    }
    rows.sort();
    rows.dedup();

    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core/tablets.tsv");
    fs::write(&out, rows.join("\n") + "\n")
        .with_context(|| format!("writing {}", out.display()))?;
    println!("{} tablets written to {}", rows.len(), out.display());
    Ok(())
}

fn collect_json(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_json(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "json") {
            out.push(path);
        }
    }
    Ok(())
}
