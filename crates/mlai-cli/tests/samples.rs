// Regression coverage for samples/ and the root manifest.toml: these are
// hand-edited content files with no compiler to catch a typo, so a broken
// TOML or a catalog fragment conflict would otherwise only surface the next
// time someone actually ran `mlai install`/`mlai catalog resolve` by hand.
use mlai_core::catalog::{merge_fragments, CatalogFragment, GpuVendor, HardwareProfile, Os};
use mlai_core::manifest::Manifest;
use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/mlai-cli; the workspace root is two
    // levels up. Same convention crates/mlai-gui/src-tauri/src/lib.rs's
    // find_resource() dev-mode fallback already uses.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[test]
fn root_manifest_parses_and_has_two_default_components() {
    let path = workspace_root().join("manifest.toml");
    let content = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("reading {}: {e}", path.display());
    });
    let manifest = Manifest::parse(&content).expect("root manifest.toml must parse");
    assert_eq!(manifest.default_components().len(), 2);
    assert_eq!(
        manifest.gui.app_name.as_deref(),
        Some("MLAppInstaller (Sample)")
    );
}

#[test]
fn sample_catalog_fragments_merge_without_conflict() {
    let root = workspace_root();
    let ta_content =
        fs::read_to_string(root.join("samples/catalog/trusted-autonomy.toml")).unwrap();
    let studio_a_content = fs::read_to_string(root.join("samples/catalog/studio-a.toml")).unwrap();
    let fragments = vec![
        CatalogFragment::parse(&ta_content).unwrap(),
        CatalogFragment::parse(&studio_a_content).unwrap(),
    ];
    let merged = merge_fragments(&fragments).expect("two independent owners must merge cleanly");

    let apple_silicon = HardwareProfile {
        os: Os::Macos,
        gpu_vendor: GpuVendor::Apple,
        vram_gb: 0.0,
        effective_vram_gb: 0.0,
        disk_free_gb: 50.0,
    };
    assert_eq!(
        merged.resolve("voice-transcription", &apple_silicon, 0.0),
        Some("parakeet-mlx")
    );

    let big_nvidia = HardwareProfile {
        os: Os::Linux,
        gpu_vendor: GpuVendor::Nvidia,
        vram_gb: 24.0,
        effective_vram_gb: 24.0,
        disk_free_gb: 50.0,
    };
    assert_eq!(
        merged.resolve("text-structured-json", &big_nvidia, 0.0),
        Some("qwen3:32b")
    );
}
