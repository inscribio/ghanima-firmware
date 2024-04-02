use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{PathBuf, Path};

use anyhow::{Context, Result};

use ghanima_config::KeyboardConfig;

/// Generate build metadata file that is then included in code
fn build_metadata() -> Result<()> {
    built::write_built_file()?;
    Ok(())
}

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
    KeyboardConfig::schema_to_file(Path::new("./schema.json"))
        .context("While generating JSON schema")?;

    // Generate config from JSON if enabled
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_JSON_CONFIG");
    println!("cargo:rerun-if-env-changed=GHANIMA_JSON_CONFIG");
    if env::var_os("CARGO_FEATURE_JSON_CONFIG").is_some() {
        // Get path from env variable or use default
        let default_path = String::from("ghanima.json");
        let path = env::var_os("GHANIMA_JSON_CONFIG")
            .map(|s| s.into_string())
            .transpose()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "CARGO_FEATURE_JSON_CONFIG is not utf-8"))?
            .unwrap_or(default_path);

        println!("cargo:rerun-if-changed={}", path);
        let json = Path::new(&path);

        let config = KeyboardConfig::from_file(json)
            .context(format!("While reading {}", path))?;

        config.to_file(&out.join("config.rs"))
            // .context(format!("With config:\n{:#?}", config))
            .context("While generating config.rs")?;
    } else if env::var_os("GHANIMA_JSON_CONFIG").is_some() {
        println!("cargo:warning=GHANIMA_JSON_CONFIG defined but ignored because feature \"json-config\" is not enabled");
    }

    Ok(())
}

fn main() -> Result<()>  {
    build_metadata()?;
    let out = &PathBuf::from(env::var_os("OUT_DIR").context("Could not get OUT_DIR")?);
    memory(out)?;
    json_config(out)?;
    Ok(())
}
