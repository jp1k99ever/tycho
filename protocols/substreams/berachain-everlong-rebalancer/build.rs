use std::{env, fs, path::Path};

use anyhow::{Ok, Result};
use substreams_ethereum::Abigen;

fn main() -> Result<(), anyhow::Error> {
    for (name, artifact_path, out_module) in [
        ("Rebalancer", "abi/Rebalancer.json", "src/abi/rebalancer.rs"),
        ("Swapper", "abi/Swapper.json", "src/abi/swapper.rs"),
        ("Alm", "abi/Alm.json", "src/abi/alm.rs"),
    ] {
        let artifact = fs::read_to_string(artifact_path)?;
        let artifact: serde_json::Value = serde_json::from_str(&artifact)?;
        let abi = artifact.get("abi").unwrap_or(&artifact);
        let abi_path = Path::new(&env::var("OUT_DIR")?).join(format!("{name}.abi.json"));
        fs::write(&abi_path, serde_json::to_vec(abi)?)?;

        let abi_path = abi_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("{name} ABI path is not valid UTF-8"))?
            .to_owned();

        Abigen::new(name, abi_path.as_str())?
            .generate()?
            .write_to_file(out_module)?;
    }
    Ok(())
}
