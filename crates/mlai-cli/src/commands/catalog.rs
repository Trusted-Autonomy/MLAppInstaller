use anyhow::{bail, Context, Result};
use mlai_core::catalog::{merge_fragments, CatalogFragment, GpuVendor, HardwareProfile, Os};
use std::fs;
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn resolve(
    purpose: &str,
    catalog_paths: &[PathBuf],
    os: Os,
    gpu_vendor: GpuVendor,
    vram_gb: f64,
    effective_vram_gb: f64,
    disk_free_gb: f64,
    reserve_vram_gb: f64,
) -> Result<()> {
    let mut fragments = Vec::new();
    for path in catalog_paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("reading catalog fragment at {}", path.display()))?;
        let fragment = CatalogFragment::parse(&content)
            .with_context(|| format!("parsing catalog fragment at {}", path.display()))?;
        fragments.push(fragment);
    }

    let merged = merge_fragments(&fragments).map_err(|e| anyhow::anyhow!("{e}"))?;
    let profile = HardwareProfile {
        os,
        gpu_vendor,
        vram_gb,
        effective_vram_gb,
        disk_free_gb,
    };

    match merged.resolve(purpose, &profile, reserve_vram_gb) {
        Some(model) => {
            println!("{model}");
            Ok(())
        }
        None => bail!(
            "no model in '{purpose}' fits this hardware profile (effective {effective_vram_gb}GB VRAM, \
             {reserve_vram_gb}GB reserved, vendor {gpu_vendor:?}, os {os:?}) — check the catalog's tiers \
             for '{purpose}' and whether any qualify"
        ),
    }
}
