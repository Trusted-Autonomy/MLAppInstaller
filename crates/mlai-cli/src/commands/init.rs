use anyhow::{Context, Result};
use mlai_package::profile::{
    DeployConfig, Distribution, DistributionProfile, PackageFormat, Platform, Target,
};
use std::io::{BufRead, Write};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("unrecognized platform '{0}' — expected one of: macos, windows, linux")]
    UnknownPlatform(String),
    #[error("unrecognized package format '{0}'")]
    UnknownFormat(String),
    #[error("failed to read input: {0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected end of input while waiting for an answer to '{0}'")]
    UnexpectedEof(String),
}

fn prompt(writer: &mut impl Write, text: &str) -> std::io::Result<()> {
    write!(writer, "{text}")?;
    writer.flush()
}

/// Reads a line and reports how many bytes were read, so callers can tell
/// a blank line (the user just pressed Enter; `read_line` reports at least
/// 1 byte for the newline) apart from true EOF (`read_line` reports `0`
/// bytes because stdin has closed or run out of input).
fn read_line_checked(reader: &mut impl BufRead) -> Result<(String, usize), InitError> {
    let mut line = String::new();
    let bytes_read = reader.read_line(&mut line)?;
    Ok((line.trim().to_string(), bytes_read))
}

fn read_answer(reader: &mut impl BufRead) -> Result<String, InitError> {
    let (answer, _bytes_read) = read_line_checked(reader)?;
    Ok(answer)
}

/// Like `read_answer`, but re-prompts with the same prompt text until a
/// non-empty answer is given. Used for answers that are semantically
/// required — the wizard is the only point in the flow with a human
/// present to answer, so blank input must not silently pass through.
///
/// If stdin hits true EOF while waiting (closed pipe, exhausted redirect,
/// truncated scripted input), this returns `InitError::UnexpectedEof`
/// immediately instead of re-prompting forever: EOF reads as `""` just
/// like a blank line, so without this check the loop would spin on empty
/// reads with no way out.
fn read_required(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    prompt_text: &str,
) -> Result<String, InitError> {
    loop {
        let (answer, bytes_read) = read_line_checked(reader)?;
        if bytes_read == 0 {
            return Err(InitError::UnexpectedEof(prompt_text.to_string()));
        }
        if !answer.is_empty() {
            return Ok(answer);
        }
        prompt(writer, prompt_text)?;
    }
}

fn parse_platform(s: &str) -> Result<Platform, InitError> {
    match s.to_lowercase().as_str() {
        "macos" => Ok(Platform::Macos),
        "windows" => Ok(Platform::Windows),
        "linux" => Ok(Platform::Linux),
        other => Err(InitError::UnknownPlatform(other.to_string())),
    }
}

fn default_format_for(platform: Platform) -> PackageFormat {
    match platform {
        Platform::Macos => PackageFormat::Dmg,
        Platform::Windows => PackageFormat::Msi,
        Platform::Linux => PackageFormat::Deb,
    }
}

fn parse_format(s: &str) -> Result<PackageFormat, InitError> {
    match s.to_lowercase().as_str() {
        "dmg" => Ok(PackageFormat::Dmg),
        "app" => Ok(PackageFormat::App),
        "msi" => Ok(PackageFormat::Msi),
        "nsis" => Ok(PackageFormat::Nsis),
        "deb" => Ok(PackageFormat::Deb),
        "appimage" => Ok(PackageFormat::Appimage),
        other => Err(InitError::UnknownFormat(other.to_string())),
    }
}

/// The wizard's testable core: reads answers from `reader`, writes prompts
/// to `writer`, returns the constructed profile. Decoupled from real
/// stdin/stdout so tests can drive it with an in-memory buffer.
pub fn run_wizard(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<DistributionProfile, InitError> {
    let name_prompt = "Distribution name: ";
    prompt(writer, name_prompt)?;
    let name = read_required(reader, writer, name_prompt)?;

    prompt(writer, "Manifest path [manifest.toml]: ")?;
    let manifest_answer = read_answer(reader)?;
    let manifest = if manifest_answer.is_empty() {
        "manifest.toml".to_string()
    } else {
        manifest_answer
    };

    prompt(
        writer,
        "Components (comma-separated, blank = all from manifest): ",
    )?;
    let components_answer = read_answer(reader)?;
    let components: Vec<String> = if components_answer.is_empty() {
        vec![]
    } else {
        components_answer
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    prompt(writer, "Target platform (macos/windows/linux): ")?;
    let platform = parse_platform(&read_answer(reader)?)?;
    let default_format = default_format_for(platform);

    prompt(writer, &format!("Package format [{default_format:?}]: "))?;
    let format_answer = read_answer(reader)?;
    let format = if format_answer.is_empty() {
        default_format
    } else {
        parse_format(&format_answer)?
    };

    prompt(
        writer,
        "Signing identity (macOS keychain name, blank = none): ",
    )?;
    let signing_identity_answer = read_answer(reader)?;
    let signing_identity = if signing_identity_answer.is_empty() {
        None
    } else {
        Some(signing_identity_answer)
    };

    prompt(writer, "Certificate thumbprint (Windows, blank = none): ")?;
    let thumbprint_answer = read_answer(reader)?;
    let certificate_thumbprint = if thumbprint_answer.is_empty() {
        None
    } else {
        Some(thumbprint_answer)
    };

    prompt(writer, "Configure a deploy target? [y/N]: ")?;
    let wants_deploy = read_answer(reader)?.to_lowercase().starts_with('y');
    let deploy = if wants_deploy {
        let repo_prompt = "GitHub repo (owner/name): ";
        prompt(writer, repo_prompt)?;
        let repo = read_required(reader, writer, repo_prompt)?;
        Some(DeployConfig {
            adapter: "github-releases".to_string(),
            repo: Some(repo),
        })
    } else {
        None
    };

    Ok(DistributionProfile {
        distribution: Distribution {
            name,
            manifest,
            components,
        },
        targets: vec![Target {
            platform,
            format,
            signing_identity,
            certificate_thumbprint,
            notarize: false,
        }],
        deploy,
    })
}

pub fn run(output: &Path) -> Result<()> {
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let profile = run_wizard(&mut reader, &mut writer).context("running the init wizard")?;
    let toml_str =
        toml::to_string_pretty(&profile).context("serializing the distribution profile")?;
    std::fs::write(output, toml_str)
        .with_context(|| format!("writing distribution profile to {}", output.display()))?;
    println!("Wrote distribution profile to {}", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run_with(input: &str) -> Result<DistributionProfile, InitError> {
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer: Vec<u8> = Vec::new();
        run_wizard(&mut reader, &mut writer)
    }

    #[test]
    fn blank_distribution_name_reprompts_and_uses_second_answer() {
        let input = "\
\n\
my-app\n\
\n\
\n\
linux\n\
\n\
\n\
\n\
n\n\
";
        let profile = run_with(input).expect("wizard should succeed");
        assert_eq!(profile.distribution.name, "my-app");
    }

    #[test]
    fn components_with_empty_entries_are_filtered_out() {
        let input = "\
my-app\n\
\n\
comp-a,,comp-b,\n\
linux\n\
\n\
\n\
\n\
n\n\
";
        let profile = run_with(input).expect("wizard should succeed");
        assert_eq!(
            profile.distribution.components,
            vec!["comp-a".to_string(), "comp-b".to_string()]
        );
    }

    #[test]
    fn blank_deploy_repo_reprompts_and_uses_second_answer() {
        let input = "\
my-app\n\
\n\
\n\
linux\n\
\n\
\n\
\n\
y\n\
\n\
example/my-app\n\
";
        let profile = run_with(input).expect("wizard should succeed");
        let deploy = profile.deploy.expect("deploy config should be present");
        assert_eq!(deploy.repo, Some("example/my-app".to_string()));
    }

    #[test]
    fn invalid_platform_answer_returns_unknown_platform_error() {
        let input = "\
my-app\n\
\n\
\n\
bogus\n\
";
        let err = run_with(input).expect_err("invalid platform should error");
        assert!(matches!(err, InitError::UnknownPlatform(p) if p == "bogus"));
    }

    #[test]
    fn invalid_format_answer_returns_unknown_format_error() {
        let input = "\
my-app\n\
\n\
\n\
linux\n\
rpm\n\
";
        let err = run_with(input).expect_err("invalid format should error");
        assert!(matches!(err, InitError::UnknownFormat(f) if f == "rpm"));
    }

    #[test]
    fn eof_after_blank_name_answer_returns_error_instead_of_hanging() {
        // A single blank line answers the name prompt with "" (re-prompt
        // territory), then input runs out entirely. Before the fix,
        // `read_required` couldn't tell EOF's "" apart from a blank line's
        // "" and would loop on `read_line` forever. It must now return
        // `UnexpectedEof` naming the unanswered prompt instead of hanging.
        let input = "\n";
        let err = run_with(input)
            .expect_err("EOF while waiting for a required answer should error, not hang");
        assert!(matches!(err, InitError::UnexpectedEof(p) if p == "Distribution name: "));
    }
}
