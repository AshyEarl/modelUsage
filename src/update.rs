use crate::cache::{load_update_state, save_update_state};
use crate::cli::Cli;
use crate::model::UpdateState;
use anyhow::{Context, Result, anyhow, bail};
use chrono::{Duration, Utc};
use std::cmp::Ordering;
use std::env;
use std::fs::{self, File};
use std::io::{self, IsTerminal, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const RELEASE_API_URL: &str = "https://api.github.com/repos/AshyEarl/modelUsage/releases/latest";
const AUTO_CHECK_INTERVAL_HOURS: i64 = 24;
const RELEASE_NOTES_MAX_LINES: usize = 5;
const RELEASE_NOTES_MAX_CHARS: usize = 280;
const DOWNLOAD_PROGRESS_STEP_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone)]
struct ReleaseInfo {
    version: String,
    asset_name: String,
    asset_url: String,
    release_notes_summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlatformAsset {
    archive_name: &'static str,
}

pub fn maybe_check_for_updates(cli: &Cli) -> Result<()> {
    if !should_auto_check(cli) {
        return Ok(());
    }

    let now = Utc::now();
    let mut state = load_update_state().unwrap_or_default();
    let mut release =
        release_from_state(&state).filter(|release| is_newer_than_current(&release.version));

    if should_check_now(&state, now) {
        print_checking_message();
        if let Ok(fresh_release) = fetch_latest_release() {
            state.version = 1;
            state.last_checked_at = Some(now);
            state.latest_version = Some(fresh_release.version.clone());
            state.asset_name = Some(fresh_release.asset_name.clone());
            state.asset_url = Some(fresh_release.asset_url.clone());
            state.release_notes_summary = Some(fresh_release.release_notes_summary.clone());
            let _ = save_update_state(&state);

            release = if is_newer_than_current(&fresh_release.version) {
                Some(fresh_release)
            } else {
                None
            };
        }
    }

    let Some(release) = release else {
        return Ok(());
    };

    print_update_notice(&release);
    if confirm_update_now()? {
        install_release(&release)?;
        println!("Updated to {}. Please rerun `modelUsage`.", release.version);
    } else {
        println!("Run `modelUsage --update` to update later.");
    }

    Ok(())
}

pub fn run_manual_update() -> Result<()> {
    print_checking_message();
    let release = match fetch_latest_release() {
        Ok(release) => release,
        Err(fetch_err) => {
            let state = load_update_state()?;
            release_from_state(&state).ok_or(fetch_err)?
        }
    };

    if !is_newer_than_current(&release.version) {
        println!(
            "modelUsage {} is already up to date.",
            current_version_tag()
        );
        return Ok(());
    }

    install_release(&release)?;

    let mut state = load_update_state()?;
    state.version = 1;
    state.last_checked_at = Some(Utc::now());
    state.latest_version = Some(release.version.clone());
    state.asset_name = Some(release.asset_name.clone());
    state.asset_url = Some(release.asset_url.clone());
    state.release_notes_summary = Some(release.release_notes_summary.clone());
    save_update_state(&state)?;

    println!("Updated to {}. Please rerun `modelUsage`.", release.version);
    Ok(())
}

fn should_auto_check(cli: &Cli) -> bool {
    !cli.json
        && io::stdout().is_terminal()
        && io::stderr().is_terminal()
        && io::stdin().is_terminal()
}

fn should_check_now(state: &UpdateState, now: chrono::DateTime<Utc>) -> bool {
    let Some(last_checked_at) = state.last_checked_at else {
        return true;
    };
    now.signed_duration_since(last_checked_at) >= Duration::hours(AUTO_CHECK_INTERVAL_HOURS)
}

fn fetch_latest_release() -> Result<ReleaseInfo> {
    let platform = current_platform_asset()?;
    let response = agent_for_url(RELEASE_API_URL)?
        .get(RELEASE_API_URL)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "modelUsage-self-update")
        .call()
        .context("failed to fetch release metadata")?;
    let release: GithubRelease = response
        .into_json()
        .context("failed to parse GitHub release metadata")?;
    let version = normalize_tag(&release.tag_name)?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == platform.archive_name)
        .ok_or_else(|| anyhow!("release asset {} not found", platform.archive_name))?;

    Ok(ReleaseInfo {
        version,
        asset_name: asset.name.clone(),
        asset_url: asset.browser_download_url.clone(),
        release_notes_summary: summarize_release_notes(release.body.as_deref().unwrap_or("")),
    })
}

fn agent_for_url(url: &str) -> Result<ureq::Agent> {
    let mut builder = ureq::AgentBuilder::new();
    if let Some(proxy_url) = proxy_url_for(url) {
        builder = builder.proxy(
            ureq::Proxy::new(&proxy_url)
                .with_context(|| format!("invalid proxy URL: {proxy_url}"))?,
        );
    }
    Ok(builder.build())
}

fn proxy_url_for(url: &str) -> Option<String> {
    let lower_url = url.to_ascii_lowercase();
    let is_https = lower_url.starts_with("https://");
    let is_http = lower_url.starts_with("http://");

    if is_https {
        env_value(["https_proxy", "HTTPS_PROXY"])
            .or_else(|| env_value(["http_proxy", "HTTP_PROXY"]))
            .or_else(|| env_value(["all_proxy", "ALL_PROXY"]))
    } else if is_http {
        env_value(["http_proxy", "HTTP_PROXY"]).or_else(|| env_value(["all_proxy", "ALL_PROXY"]))
    } else {
        env_value(["all_proxy", "ALL_PROXY"])
    }
}

fn env_value<const N: usize>(keys: [&str; N]) -> Option<String> {
    keys.into_iter().find_map(|key| {
        let value = env::var(key).ok()?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn release_from_state(state: &UpdateState) -> Option<ReleaseInfo> {
    let platform = current_platform_asset().ok()?;
    let asset_name = state.asset_name.clone()?;
    if asset_name != platform.archive_name {
        return None;
    }

    Some(ReleaseInfo {
        version: state.latest_version.clone()?,
        asset_name,
        asset_url: state.asset_url.clone()?,
        release_notes_summary: state
            .release_notes_summary
            .clone()
            .unwrap_or_else(|| "No release notes provided.".to_string()),
    })
}

fn print_update_notice(release: &ReleaseInfo) {
    println!();
    println!(
        "Update available: {} -> {}",
        current_version_tag(),
        release.version
    );
    println!("Run `modelUsage --update` to update later.");
    if !release.release_notes_summary.is_empty() {
        println!();
        println!("Release notes:");
        println!("{}", release.release_notes_summary);
    }
}

fn confirm_update_now() -> Result<bool> {
    print!("\nUpdate now? [y/N] ");
    io::stdout()
        .flush()
        .context("failed to flush update prompt")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read update confirmation")?;
    Ok(matches!(input.trim(), "y" | "Y"))
}

fn install_release(release: &ReleaseInfo) -> Result<()> {
    let current_exe = env::current_exe().context("failed to resolve current executable path")?;
    let target_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve executable directory"))?;

    let temp_root = env::temp_dir().join(format!(
        "modelUsage-update-{}-{}",
        std::process::id(),
        Utc::now().timestamp_millis()
    ));
    fs::create_dir_all(&temp_root)
        .with_context(|| format!("failed to create {}", temp_root.display()))?;

    let archive_path = temp_root.join(&release.asset_name);
    let extracted_path = temp_root.join("modelUsage");
    print_status_line(&format!("Downloading {}...", release.asset_name));
    download_release_archive(&release.asset_url, &archive_path)?;
    print_status_line("Extracting archive...");
    extract_release_binary(&archive_path, &temp_root)?;

    let staging_path = target_dir.join(format!(
        ".modelUsage.tmp-{}-{}",
        std::process::id(),
        Utc::now().timestamp_millis()
    ));

    print_status_line("Replacing binary...");
    copy_binary_to_staging(&extracted_path, &staging_path)?;
    fs::rename(&staging_path, &current_exe).with_context(|| {
        format!(
            "failed to replace {} with {}",
            current_exe.display(),
            staging_path.display()
        )
    })?;
    let _ = sync_directory(target_dir);
    let _ = fs::remove_dir_all(&temp_root);
    finish_status_line("Update installed.");
    Ok(())
}

fn download_release_archive(url: &str, archive_path: &Path) -> Result<()> {
    let response = agent_for_url(url)?
        .get(url)
        .set("User-Agent", "modelUsage-self-update")
        .call()
        .with_context(|| format!("failed to download release archive from {url}"))?;
    let total_bytes = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok());
    let mut reader = response.into_reader();
    let mut file = File::create(archive_path)
        .with_context(|| format!("failed to create {}", archive_path.display()))?;
    copy_with_progress(&mut reader, &mut file, total_bytes, archive_path)?;
    file.sync_all()
        .with_context(|| format!("failed to fsync {}", archive_path.display()))?;
    Ok(())
}

fn extract_release_binary(archive_path: &Path, output_dir: &Path) -> Result<()> {
    let output = Command::new("tar")
        .args(["-xzf"])
        .arg(archive_path)
        .args(["-C"])
        .arg(output_dir)
        .output()
        .context("failed to execute tar for release archive")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("failed to extract release archive");
        }
        bail!("failed to extract release archive: {stderr}");
    }
    Ok(())
}

fn copy_binary_to_staging(source: &Path, target: &Path) -> Result<()> {
    let bytes = fs::copy(source, target).with_context(|| {
        format!(
            "failed to copy {} to {}",
            source.display(),
            target.display()
        )
    })?;
    if bytes == 0 {
        bail!("downloaded binary is empty");
    }
    fs::set_permissions(target, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("failed to chmod {}", target.display()))?;
    File::open(target)
        .with_context(|| format!("failed to reopen {}", target.display()))?
        .sync_all()
        .with_context(|| format!("failed to fsync {}", target.display()))?;
    Ok(())
}

fn sync_directory(dir: &Path) -> Result<()> {
    File::open(dir)
        .with_context(|| format!("failed to open {}", dir.display()))?
        .sync_all()
        .with_context(|| format!("failed to fsync {}", dir.display()))?;
    Ok(())
}

fn current_version_tag() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

fn is_newer_than_current(version: &str) -> bool {
    compare_versions(version, &current_version_tag()).is_gt()
}

fn current_platform_asset() -> Result<PlatformAsset> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok(PlatformAsset {
            archive_name: "modelUsage-linux-x86_64.tar.gz",
        }),
        ("linux", "aarch64") => Ok(PlatformAsset {
            archive_name: "modelUsage-linux-aarch64.tar.gz",
        }),
        ("macos", "aarch64") => Ok(PlatformAsset {
            archive_name: "modelUsage-macos-aarch64.tar.gz",
        }),
        (os, arch) => bail!("self-update is not supported on {os}/{arch}"),
    }
}

fn normalize_tag(tag: &str) -> Result<String> {
    if parse_version_triplet(tag).is_none() {
        bail!("unsupported release tag: {tag}");
    }
    Ok(tag.strip_prefix('v').unwrap_or(tag).to_string())
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    parse_version_triplet(left).cmp(&parse_version_triplet(right))
}

fn parse_version_triplet(input: &str) -> Option<(u64, u64, u64)> {
    let trimmed = input.trim().strip_prefix('v').unwrap_or(input.trim());
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn summarize_release_notes(body: &str) -> String {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for raw_line in body.lines() {
        let line = raw_line.trim();
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block || line.is_empty() {
            continue;
        }
        lines.push(line.to_string());
        if lines.len() >= RELEASE_NOTES_MAX_LINES {
            break;
        }
    }

    if lines.is_empty() {
        return "No release notes provided.".to_string();
    }

    let mut summary = lines.join("\n");
    if summary.chars().count() > RELEASE_NOTES_MAX_CHARS {
        summary = summary
            .chars()
            .take(RELEASE_NOTES_MAX_CHARS.saturating_sub(1))
            .collect();
        summary.push('…');
    }
    summary
}

fn print_checking_message() {
    println!();
    println!("Checking for updates...");
}

fn copy_with_progress(
    reader: &mut impl Read,
    writer: &mut impl Write,
    total_bytes: Option<u64>,
    archive_path: &Path,
) -> Result<()> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut copied = 0_u64;
    let mut next_progress = DOWNLOAD_PROGRESS_STEP_BYTES;

    loop {
        let read = reader.read(&mut buffer).with_context(|| {
            format!(
                "failed to read download stream for {}",
                archive_path.display()
            )
        })?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .with_context(|| format!("failed to write {}", archive_path.display()))?;
        copied += read as u64;

        let should_print = match total_bytes {
            Some(total) => copied >= next_progress || copied == total,
            None => copied >= next_progress,
        };
        if should_print {
            print_download_progress(copied, total_bytes);
            next_progress = copied.saturating_add(DOWNLOAD_PROGRESS_STEP_BYTES);
        }
    }

    print_download_progress(copied, total_bytes);
    finish_status_line("Download complete.");
    Ok(())
}

fn print_download_progress(copied: u64, total_bytes: Option<u64>) {
    if io::stderr().is_terminal() {
        match total_bytes {
            Some(total) if total > 0 => {
                let percent = (copied as f64 / total as f64) * 100.0;
                eprint!(
                    "\rDownloading... {:>5.1}% ({}/{})",
                    percent.min(100.0),
                    format_bytes(copied),
                    format_bytes(total)
                );
            }
            _ => {
                eprint!("\rDownloading... {}", format_bytes(copied));
            }
        }
        let _ = io::stderr().flush();
    } else {
        match total_bytes {
            Some(total) if total > 0 => eprintln!(
                "Downloading... {:>5.1}% ({}/{})",
                (copied as f64 / total as f64 * 100.0).min(100.0),
                format_bytes(copied),
                format_bytes(total)
            ),
            _ => eprintln!("Downloading... {}", format_bytes(copied)),
        }
    }
}

fn print_status_line(message: &str) {
    if io::stderr().is_terminal() {
        eprintln!("{message}");
    } else {
        eprintln!("{message}");
    }
}

fn finish_status_line(message: &str) {
    if io::stderr().is_terminal() {
        eprint!("\r\x1b[2K{message}\n");
        let _ = io::stderr().flush();
    } else {
        eprintln!("{message}");
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_idx = 0_usize;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{bytes} {}", UNITS[unit_idx])
    } else {
        format!("{value:.1} {}", UNITS[unit_idx])
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PlatformAsset, compare_versions, current_platform_asset, format_bytes,
        parse_version_triplet, proxy_url_for, should_check_now, summarize_release_notes,
    };
    use crate::model::UpdateState;
    use chrono::{Duration, Utc};
    use std::cmp::Ordering;
    use std::env;

    #[test]
    fn parses_release_tags() {
        assert_eq!(parse_version_triplet("v0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version_triplet("0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version_triplet("0.1"), None);
    }

    #[test]
    fn enforces_check_interval() {
        let now = Utc::now();
        let state = UpdateState {
            last_checked_at: Some(now - Duration::hours(23)),
            ..UpdateState::default()
        };
        assert!(!should_check_now(&state, now));

        let state = UpdateState {
            last_checked_at: Some(now - Duration::hours(24)),
            ..UpdateState::default()
        };
        assert!(should_check_now(&state, now));
    }

    #[test]
    fn summarizes_release_notes_without_code_blocks() {
        let body = "# v0.1.2\n\n- line one\n- line two\n\n```bash\ncargo run\n```\n\n- line three";
        assert_eq!(
            summarize_release_notes(body),
            "# v0.1.2\n- line one\n- line two\n- line three"
        );
    }

    #[test]
    fn compares_versions_semantically() {
        assert_eq!(compare_versions("0.1.10", "0.1.2"), Ordering::Greater);
        assert_eq!(compare_versions("0.2.0", "0.10.0"), Ordering::Less);
    }

    #[test]
    fn maps_supported_platform_to_asset() {
        let expected = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => Some(PlatformAsset {
                archive_name: "modelUsage-linux-x86_64.tar.gz",
            }),
            ("linux", "aarch64") => Some(PlatformAsset {
                archive_name: "modelUsage-linux-aarch64.tar.gz",
            }),
            ("macos", "aarch64") => Some(PlatformAsset {
                archive_name: "modelUsage-macos-aarch64.tar.gz",
            }),
            _ => None,
        };

        match expected {
            Some(expected) => assert_eq!(current_platform_asset().unwrap(), expected),
            None => assert!(current_platform_asset().is_err()),
        }
    }

    #[test]
    fn prefers_https_proxy_for_https_urls() {
        unsafe {
            env::set_var("HTTPS_PROXY", "http://secure-proxy:8443");
            env::set_var("HTTP_PROXY", "http://plain-proxy:8080");
        }
        assert_eq!(
            proxy_url_for("https://api.github.com/repos/AshyEarl/modelUsage/releases/latest"),
            Some("http://secure-proxy:8443".to_string())
        );
        unsafe {
            env::remove_var("HTTPS_PROXY");
            env::remove_var("HTTP_PROXY");
        }
    }

    #[test]
    fn falls_back_to_http_proxy_for_http_urls() {
        unsafe {
            env::set_var("HTTP_PROXY", "http://plain-proxy:8080");
        }
        assert_eq!(
            proxy_url_for("http://example.com/archive"),
            Some("http://plain-proxy:8080".to_string())
        );
        unsafe {
            env::remove_var("HTTP_PROXY");
        }
    }

    #[test]
    fn formats_progress_sizes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1_536), "1.5 KB");
        assert_eq!(format_bytes(2_621_440), "2.5 MB");
    }
}
