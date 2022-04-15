use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{PathBuf, Path};

use anyhow::{Context, Result};

use ghanima_config::KeyboardConfig;

// Copies the `memory.x` file from the crate root into a directory where
// the linker can always find it at build time.
fn memory(out: &Path) -> Result<()> {
    // Put `memory.x` in our output directory and ensure it's
    // on the linker search path.
    File::create(out.join("memory.x"))
        .and_then(|mut f| f.write_all(include_bytes!("memory.x")))
        .context("Saving memory.x")?;

    // Ensure it's on the linker search path.
    println!("cargo:rustc-link-search={}", out.display());

    // By default, Cargo will re-run a build script whenever
    // any file in the project changes. By specifying `memory.x`
    // here, we ensure the build script is only re-run when
    // `memory.x` is changed.
    println!("cargo:rerun-if-changed=memory.x");

    Ok(())
}

fn json_config(out: &Path) -> Result<()>  {
    // Generate config schema
    KeyboardConfig::schema_to_file(&out.join("schema.json"))
        .context("While generating JSON schema")?;
    KeyboardConfig::schema_to_file(&Path::new("./schema.json"))
        .context("While generating JSON schema")?;

    // Generate config from JSON if enabled
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_JSON_CONFIG");
    if env::var_os("CARGO_FEATURE_JSON_CONFIG").is_some() {
        println!("cargo:rerun-if-changed=ghanima.json");
        let json = Path::new("./ghanima.json");

        let config = KeyboardConfig::from_file(json)
            .context("While reading ghanima.json")?;

        config.to_file(&out.join("config.rs"))
            // .context(format!("With config:\n{:#?}", config))
            .context("While generating config.rs")?;
    }

    Ok(())
}

fn main() -> Result<()>  {
    let out = &PathBuf::from(env::var_os("OUT_DIR").context("Could not get OUT_DIR")?);
    memory(&out)?;
    json_config(&out)?;
    Ok(())
}
