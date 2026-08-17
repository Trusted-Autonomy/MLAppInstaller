// Translates this crate's own DistributionProfile into cargo-packager's
// actual JSON config shape. Field names and casing (camelCase) verified
// directly against cargo-packager 0.11.8, installed and run locally --
// not guessed from documentation. See this plan's Global Constraints for
// what was specifically confirmed (macOS signingIdentity, Windows
// certificateThumbprint, no PFX/password fields exist).

use crate::profile::{DistributionProfile, PackageFormat, Target};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackagerBinary {
    path: String,
    main: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackagerMacosConfig {
    signing_identity: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackagerWindowsConfig {
    certificate_thumbprint: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackagerConfig {
    product_name: String,
    identifier: String,
    formats: Vec<String>,
    binaries: Vec<PackagerBinary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    macos: Option<PackagerMacosConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    windows: Option<PackagerWindowsConfig>,
}

/// Maps this crate's `PackageFormat` to the exact format string
/// `cargo-packager`'s `-f`/`formats` option expects (confirmed via
/// `cargo packager --help`; `msi` is generated through the WiX toolset,
/// whose format string is `wix`, not `msi`).
pub fn packager_format_str(format: &PackageFormat) -> &'static str {
    match format {
        PackageFormat::Dmg => "dmg",
        PackageFormat::App => "app",
        PackageFormat::Msi => "wix",
        PackageFormat::Nsis => "nsis",
        PackageFormat::Deb => "deb",
        PackageFormat::Appimage => "appimage",
    }
}

/// Builds the JSON string to pass as `cargo packager -c <this>` — verified
/// directly that `-c` accepts a raw JSON string, not only a file path, so
/// this never needs to write a config file or touch the adopter's own
/// `Cargo.toml`.
pub fn build_packager_config(
    profile: &DistributionProfile,
    target: &Target,
    binary_path: &str,
) -> String {
    let config = PackagerConfig {
        product_name: profile.distribution.name.clone(),
        identifier: format!("com.mlappinstaller.{}", profile.distribution.name),
        formats: vec![packager_format_str(&target.format).to_string()],
        binaries: vec![PackagerBinary {
            path: binary_path.to_string(),
            main: true,
        }],
        macos: target
            .signing_identity
            .clone()
            .map(|signing_identity| PackagerMacosConfig { signing_identity }),
        windows: target
            .certificate_thumbprint
            .clone()
            .map(|certificate_thumbprint| PackagerWindowsConfig {
                certificate_thumbprint,
            }),
    };
    serde_json::to_string(&config).expect("PackagerConfig always serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{DistributionProfile, PackageFormat, Platform};

    fn sample_profile() -> DistributionProfile {
        DistributionProfile::parse(
            r#"
[distribution]
name = "hello-app"
manifest = "manifest.toml"

[[targets]]
platform = "macos"
format = "dmg"
signing_identity = "keychain:Developer ID Application: Example, Inc."
"#,
        )
        .unwrap()
    }

    #[test]
    fn includes_product_name_identifier_and_binary() {
        let profile = sample_profile();
        let target = &profile.targets[0];
        let json = build_packager_config(&profile, target, "target/release/hello-app");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["productName"], "hello-app");
        assert_eq!(value["identifier"], "com.mlappinstaller.hello-app");
        assert_eq!(value["formats"], serde_json::json!(["dmg"]));
        assert_eq!(value["binaries"][0]["path"], "target/release/hello-app");
        assert_eq!(value["binaries"][0]["main"], true);
    }

    #[test]
    fn includes_macos_signing_identity_when_present() {
        let profile = sample_profile();
        let target = &profile.targets[0];
        let json = build_packager_config(&profile, target, "bin/hello-app");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(
            value["macos"]["signingIdentity"],
            "keychain:Developer ID Application: Example, Inc."
        );
    }

    #[test]
    fn omits_macos_and_windows_blocks_when_no_signing_configured() {
        let mut profile = sample_profile();
        profile.targets[0].signing_identity = None;
        let target = &profile.targets[0];
        let json = build_packager_config(&profile, target, "bin/hello-app");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(value.get("macos").is_none());
        assert!(value.get("windows").is_none());
    }

    #[test]
    fn includes_windows_certificate_thumbprint_when_present() {
        let mut profile = sample_profile();
        profile.targets[0].platform = Platform::Windows;
        profile.targets[0].format = PackageFormat::Msi;
        profile.targets[0].signing_identity = None;
        profile.targets[0].certificate_thumbprint = Some("AB12CD34".to_string());
        let target = &profile.targets[0];
        let json = build_packager_config(&profile, target, "bin/hello-app.exe");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["windows"]["certificateThumbprint"], "AB12CD34");
    }

    #[test]
    fn format_enum_maps_to_cargo_packager_format_strings() {
        let profile = sample_profile();
        let cases = [
            (PackageFormat::Dmg, "dmg"),
            (PackageFormat::App, "app"),
            (PackageFormat::Msi, "wix"),
            (PackageFormat::Nsis, "nsis"),
            (PackageFormat::Deb, "deb"),
            (PackageFormat::Appimage, "appimage"),
        ];
        for (format, expected) in cases {
            let mut target = profile.targets[0].clone();
            target.format = format;
            assert_eq!(
                packager_format_str(&target.format),
                expected,
                "format {format:?}"
            );
        }
    }
}
