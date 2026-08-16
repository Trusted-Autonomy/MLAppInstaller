use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Macos,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Apple,
    Intel,
    None,
}

/// Hardware capability, as an input to `MergedCatalog::resolve` — this
/// project does not detect any of these values itself; see this plan's
/// "Out of scope."
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HardwareProfile {
    pub os: Os,
    pub gpu_vendor: GpuVendor,
    /// Raw detected VRAM.
    pub vram_gb: f64,
    /// VRAM after platform-specific derating (e.g. Apple unified-memory
    /// derating) — this is what `resolve` actually compares tiers against.
    pub effective_vram_gb: f64,
    pub disk_free_gb: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ModelTier {
    pub min_vram_gb: f64,
    pub model: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub requires_vendor: Vec<GpuVendor>,
    #[serde(default)]
    pub requires_os: Vec<Os>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Purpose {
    pub owner: String,
    #[serde(default)]
    pub tiers: Vec<ModelTier>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct CatalogFragment {
    #[serde(default)]
    pub purposes: BTreeMap<String, Purpose>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("failed to parse catalog TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error(
        "purpose '{purpose}' is defined with conflicting tiers by both '{owner_a}' and '{owner_b}' — \
         only the owning fragment may define a purpose's tiers; a non-owner must reference it \
         (declare the purpose with no tiers), not redefine it"
    )]
    Conflict {
        purpose: String,
        owner_a: String,
        owner_b: String,
    },
}

impl CatalogFragment {
    pub fn parse(toml_str: &str) -> Result<CatalogFragment, CatalogError> {
        toml::from_str(toml_str).map_err(CatalogError::from)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MergedCatalog {
    purposes: BTreeMap<String, Purpose>,
}

/// Merges catalog fragments from multiple independently-developed
/// sub-projects. A purpose declared identically (same owner, same tiers —
/// or one side merely referencing it with no tiers) by more than one
/// fragment is fine. A purpose declared with *different* tiers, or a
/// *different* owner, by more than one fragment is a hard error — this is
/// the mechanism that prevents the exact fragmentation bug this design
/// exists to stop (two products independently inventing different tier
/// tables for what should be one shared decision).
pub fn merge_fragments(fragments: &[CatalogFragment]) -> Result<MergedCatalog, CatalogError> {
    let mut merged: BTreeMap<String, Purpose> = BTreeMap::new();
    for fragment in fragments {
        for (name, purpose) in &fragment.purposes {
            match merged.get(name) {
                None => {
                    merged.insert(name.clone(), purpose.clone());
                }
                Some(existing) => {
                    if existing.owner != purpose.owner {
                        return Err(CatalogError::Conflict {
                            purpose: name.clone(),
                            owner_a: existing.owner.clone(),
                            owner_b: purpose.owner.clone(),
                        });
                    }
                    match (existing.tiers.is_empty(), purpose.tiers.is_empty()) {
                        (true, false) => {
                            merged.insert(name.clone(), purpose.clone());
                        }
                        (false, false) if existing.tiers != purpose.tiers => {
                            return Err(CatalogError::Conflict {
                                purpose: name.clone(),
                                owner_a: existing.owner.clone(),
                                owner_b: purpose.owner.clone(),
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(MergedCatalog { purposes: merged })
}

impl MergedCatalog {
    /// Resolves the best-fit model for `purpose` given `profile`, after
    /// subtracting `reserve_vram_gb` (headroom for a co-resident heavy GPU
    /// consumer, e.g. Unreal Engine) from `profile.effective_vram_gb`.
    /// Tiers are checked from highest `min_vram_gb` down; a tier is skipped
    /// if the profile doesn't meet its `min_vram_gb` after reservation, or
    /// if it declares `requires_vendor`/`requires_os` constraints the
    /// profile doesn't satisfy.
    pub fn resolve(
        &self,
        purpose: &str,
        profile: &HardwareProfile,
        reserve_vram_gb: f64,
    ) -> Option<&str> {
        let purpose = self.purposes.get(purpose)?;
        let usable_vram = (profile.effective_vram_gb - reserve_vram_gb).max(0.0);

        let mut sorted_tiers: Vec<&ModelTier> = purpose.tiers.iter().collect();
        sorted_tiers.sort_by(|a, b| b.min_vram_gb.partial_cmp(&a.min_vram_gb).unwrap());

        for tier in sorted_tiers {
            if tier.min_vram_gb > usable_vram {
                continue;
            }
            if !tier.requires_vendor.is_empty()
                && !tier.requires_vendor.contains(&profile.gpu_vendor)
            {
                continue;
            }
            if !tier.requires_os.is_empty() && !tier.requires_os.contains(&profile.os) {
                continue;
            }
            return Some(&tier.model);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[purposes.text-structured-json]
owner = "cinepipe-stories"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 24
model = "qwen3:32b"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 8
model = "qwen3:8b"
notes = "recommended baseline"

[purposes.voice-transcription]
owner = "trusted-autonomy"

[[purposes.voice-transcription.tiers]]
min_vram_gb = 0
model = "parakeet-mlx"
requires_vendor = ["apple"]
requires_os = ["macos"]
"#;

    #[test]
    fn parses_purposes_with_tiers_and_constraints() {
        let fragment = CatalogFragment::parse(SAMPLE).unwrap();
        assert_eq!(fragment.purposes.len(), 2);

        let text_json = &fragment.purposes["text-structured-json"];
        assert_eq!(text_json.owner, "cinepipe-stories");
        assert_eq!(text_json.tiers.len(), 2);
        assert_eq!(text_json.tiers[0].min_vram_gb, 24.0);
        assert_eq!(text_json.tiers[0].model, "qwen3:32b");
        assert_eq!(text_json.tiers[1].notes, "recommended baseline");

        let voice = &fragment.purposes["voice-transcription"];
        assert_eq!(voice.tiers[0].requires_vendor, vec![GpuVendor::Apple]);
        assert_eq!(voice.tiers[0].requires_os, vec![Os::Macos]);
    }

    #[test]
    fn tiers_default_to_no_constraints_and_empty_notes() {
        let toml = r#"
[purposes.simple]
owner = "example"

[[purposes.simple.tiers]]
min_vram_gb = 4
model = "small-model"
"#;
        let fragment = CatalogFragment::parse(toml).unwrap();
        let tier = &fragment.purposes["simple"].tiers[0];
        assert_eq!(tier.notes, "");
        assert!(tier.requires_vendor.is_empty());
        assert!(tier.requires_os.is_empty());
    }

    #[test]
    fn a_purpose_with_no_tiers_is_a_valid_reference() {
        let toml = r#"
[purposes.text-structured-json]
owner = "cinepipe-stories"
"#;
        let fragment = CatalogFragment::parse(toml).unwrap();
        assert!(fragment.purposes["text-structured-json"].tiers.is_empty());
    }

    #[test]
    fn rejects_invalid_toml() {
        let err = CatalogFragment::parse("not valid toml [[[").unwrap_err();
        assert!(matches!(err, CatalogError::Parse(_)));
    }

    fn purpose(owner: &str, tiers: Vec<ModelTier>) -> Purpose {
        Purpose {
            owner: owner.to_string(),
            tiers,
        }
    }

    fn tier(min_vram_gb: f64, model: &str) -> ModelTier {
        ModelTier {
            min_vram_gb,
            model: model.to_string(),
            notes: String::new(),
            requires_vendor: vec![],
            requires_os: vec![],
        }
    }

    fn profile(effective_vram_gb: f64) -> HardwareProfile {
        HardwareProfile {
            os: Os::Linux,
            gpu_vendor: GpuVendor::Nvidia,
            vram_gb: effective_vram_gb,
            effective_vram_gb,
            disk_free_gb: 100.0,
        }
    }

    #[test]
    fn merges_purposes_from_multiple_fragments_that_dont_overlap() {
        let mut a = CatalogFragment::default();
        a.purposes.insert(
            "text-structured-json".into(),
            purpose("cinepipe-stories", vec![tier(8.0, "qwen3:8b")]),
        );
        let mut b = CatalogFragment::default();
        b.purposes.insert(
            "voice-transcription".into(),
            purpose("trusted-autonomy", vec![tier(0.0, "parakeet-mlx")]),
        );

        let merged = merge_fragments(&[a, b]).unwrap();
        assert!(merged
            .resolve("text-structured-json", &profile(8.0), 0.0)
            .is_some());
        assert!(merged
            .resolve("voice-transcription", &profile(0.0), 0.0)
            .is_some());
    }

    #[test]
    fn a_reference_fragment_with_no_tiers_does_not_conflict_with_the_owner() {
        let mut owner_fragment = CatalogFragment::default();
        owner_fragment.purposes.insert(
            "text-structured-json".into(),
            purpose("cinepipe-stories", vec![tier(8.0, "qwen3:8b")]),
        );
        let mut reference_fragment = CatalogFragment::default();
        reference_fragment.purposes.insert(
            "text-structured-json".into(),
            purpose("cinepipe-stories", vec![]),
        );

        let merged = merge_fragments(&[owner_fragment, reference_fragment]).unwrap();
        assert_eq!(
            merged.resolve("text-structured-json", &profile(8.0), 0.0),
            Some("qwen3:8b")
        );
    }

    #[test]
    fn two_fragments_defining_the_same_purpose_identically_is_not_a_conflict() {
        let mut a = CatalogFragment::default();
        a.purposes.insert(
            "text-structured-json".into(),
            purpose("cinepipe-stories", vec![tier(8.0, "qwen3:8b")]),
        );
        let b = a.clone();

        let merged = merge_fragments(&[a, b]).unwrap();
        assert_eq!(
            merged.resolve("text-structured-json", &profile(8.0), 0.0),
            Some("qwen3:8b")
        );
    }

    #[test]
    fn two_fragments_disagreeing_on_the_same_purpose_is_a_hard_error() {
        let mut a = CatalogFragment::default();
        a.purposes.insert(
            "text-structured-json".into(),
            purpose("cinepipe-stories", vec![tier(8.0, "qwen3:8b")]),
        );
        let mut b = CatalogFragment::default();
        b.purposes.insert(
            "text-structured-json".into(),
            purpose("cinepipe-director", vec![tier(8.0, "llama3:8b")]),
        );

        let err = merge_fragments(&[a, b]).unwrap_err();
        match err {
            CatalogError::Conflict {
                purpose,
                owner_a,
                owner_b,
            } => {
                assert_eq!(purpose, "text-structured-json");
                assert_eq!(owner_a, "cinepipe-stories");
                assert_eq!(owner_b, "cinepipe-director");
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
    }

    #[test]
    fn different_owners_for_the_same_purpose_is_a_hard_error_even_with_no_tiers() {
        let mut a = CatalogFragment::default();
        a.purposes.insert(
            "text-structured-json".into(),
            purpose("cinepipe-stories", vec![]),
        );
        let mut b = CatalogFragment::default();
        b.purposes.insert(
            "text-structured-json".into(),
            purpose("cinepipe-director", vec![]),
        );

        let err = merge_fragments(&[a, b]).unwrap_err();
        assert!(matches!(err, CatalogError::Conflict { .. }));
    }

    #[test]
    fn resolve_picks_the_highest_tier_the_profile_qualifies_for() {
        let mut a = CatalogFragment::default();
        a.purposes.insert(
            "text-structured-json".into(),
            purpose(
                "cinepipe-stories",
                vec![
                    tier(24.0, "qwen3:32b"),
                    tier(8.0, "qwen3:8b"),
                    tier(0.0, "qwen3:4b"),
                ],
            ),
        );
        let merged = merge_fragments(&[a]).unwrap();

        assert_eq!(
            merged.resolve("text-structured-json", &profile(30.0), 0.0),
            Some("qwen3:32b")
        );
        assert_eq!(
            merged.resolve("text-structured-json", &profile(10.0), 0.0),
            Some("qwen3:8b")
        );
        assert_eq!(
            merged.resolve("text-structured-json", &profile(2.0), 0.0),
            Some("qwen3:4b")
        );
    }

    #[test]
    fn resolve_returns_none_for_an_unknown_purpose() {
        let merged = merge_fragments(&[]).unwrap();
        assert_eq!(merged.resolve("nonexistent", &profile(100.0), 0.0), None);
    }

    #[test]
    fn resolve_subtracts_the_reservation_before_matching_tiers() {
        let mut a = CatalogFragment::default();
        a.purposes.insert(
            "text-structured-json".into(),
            purpose(
                "cinepipe-stories",
                vec![tier(24.0, "qwen3:32b"), tier(8.0, "qwen3:8b")],
            ),
        );
        let merged = merge_fragments(&[a]).unwrap();

        // 30GB effective, but 24GB reserved for a co-resident heavy consumer
        // (e.g. Unreal Engine) -- only 6GB usable, below even the 8GB tier.
        assert_eq!(
            merged.resolve("text-structured-json", &profile(30.0), 24.0),
            None
        );
    }

    #[test]
    fn resolve_skips_a_tier_whose_vendor_constraint_is_not_met() {
        let mut a = CatalogFragment::default();
        let mut mlx_tier = tier(0.0, "parakeet-mlx");
        mlx_tier.requires_vendor = vec![GpuVendor::Apple];
        mlx_tier.requires_os = vec![Os::Macos];
        a.purposes.insert(
            "voice-transcription".into(),
            purpose("trusted-autonomy", vec![mlx_tier]),
        );
        let merged = merge_fragments(&[a]).unwrap();

        let nvidia_linux = HardwareProfile {
            os: Os::Linux,
            gpu_vendor: GpuVendor::Nvidia,
            vram_gb: 24.0,
            effective_vram_gb: 24.0,
            disk_free_gb: 100.0,
        };
        assert_eq!(
            merged.resolve("voice-transcription", &nvidia_linux, 0.0),
            None
        );

        let apple_macos = HardwareProfile {
            os: Os::Macos,
            gpu_vendor: GpuVendor::Apple,
            vram_gb: 16.0,
            effective_vram_gb: 12.0,
            disk_free_gb: 100.0,
        };
        assert_eq!(
            merged.resolve("voice-transcription", &apple_macos, 0.0),
            Some("parakeet-mlx")
        );
    }
}
