//! GitHub Releases update checker, SHA-256 verifier, and Windows in-place self-updater.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver, Sender};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use reqwest::StatusCode;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::Builder as TempDirBuilder;
use thiserror::Error;
use zip::ZipArchive;

/// Package version baked in at compile time from `Cargo.toml`.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// GitHub Releases latest endpoint for GloWled.
pub const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/ghostside-net/GloWled/releases/latest";

/// Network timeout for GitHub API metadata checks.
pub const API_TIMEOUT: Duration = Duration::from_secs(8);

/// Network timeout for binary/archive downloads.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

const MAX_RELEASE_NOTES_CHARS: usize = 1500;
const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;

/// Returns the HTTP `User-Agent` header required by the GitHub REST API.
pub fn user_agent() -> String {
    format!("glowled-updater/{CURRENT_VERSION}")
}

/// Returns the parsed current crate version.
pub fn current_version() -> std::result::Result<Version, UpdateError> {
    parse_release_version(CURRENT_VERSION)
}

/// Typed errors produced by update checking, downloading, verification, and installation.
#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("Ungültige Versionsangabe '{tag}': {reason}")]
    InvalidVersion { tag: String, reason: String },

    #[error("HTTP-Client konnte nicht initialisiert werden: {0}")]
    HttpClientBuild(String),

    #[error("Netzwerkfehler bei Update-Anfrage: {0}")]
    Network(String),

    #[error("GitHub API Rate-Limit erreicht{}. Bitte später erneut versuchen.", reset_at.as_deref().map(|r| format!(" (Reset: {r})")).unwrap_or_default())]
    RateLimited { reset_at: Option<String> },

    #[error("GitHub API antwortete mit HTTP {status}: {message}")]
    HttpStatus { status: u16, message: String },

    #[error("Ungültige GitHub Release-Antwort: {0}")]
    InvalidPayload(String),

    #[error("Kein kompatibles Windows-Release-Artefakt für {tag} gefunden")]
    NoCompatibleAsset { tag: String },

    #[error("SHA-256-Prüfsumme stimmt nicht überein (erwartet {expected}, erhalten {actual})")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Prüfsummendatei hat ein ungültiges Format: {0}")]
    ChecksumFormat(String),

    #[error("Heruntergeladene Datei ist keine gültige Windows-EXE")]
    InvalidBinary,

    #[error("ZIP-Archiv konnte nicht entpackt werden: {0}")]
    ZipExtract(String),

    #[error("Unsicheres Download-URL-Schema (HTTPS erforderlich): {0}")]
    UnsupportedUrlScheme(String),

    #[error("Dateisystemfehler ({context}): {source}")]
    Io {
        context: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("Update fehlgeschlagen ({replace_err}) und Rollback fehlgeschlagen ({rollback_err})")]
    RollbackFailed {
        replace_err: String,
        rollback_err: String,
    },
}

/// GitHub Release JSON representation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<GitHubAsset>,
}

/// GitHub Release asset metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub content_type: String,
}

/// Metadata describing an available stable update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current_version: Version,
    pub new_version: Version,
    pub tag_name: String,
    pub release_name: String,
    pub release_notes: String,
    pub html_url: String,
    pub asset: GitHubAsset,
    pub checksum_asset: Option<GitHubAsset>,
}

/// Outcome of checking the latest GitHub release against the installed version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateCheckOutcome {
    UpToDate { current: Version, latest: Version },
    UpdateAvailable(Box<UpdateInfo>),
}

/// Parses a Git tag such as `"v2.1.0"`, `"V2.1.0"`, or `"2.1.0"` into a `semver::Version`.
pub fn parse_release_version(tag: &str) -> std::result::Result<Version, UpdateError> {
    let trimmed = tag.trim();
    if trimmed.is_empty() {
        return Err(UpdateError::InvalidVersion {
            tag: tag.to_string(),
            reason: "leerer Versionstag".to_string(),
        });
    }
    let normalized = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);

    Version::parse(normalized).map_err(|e| UpdateError::InvalidVersion {
        tag: tag.to_string(),
        reason: e.to_string(),
    })
}

/// Returns `true` only if `candidate` is a stable release strictly newer than `current`.
pub fn is_newer_stable_version(current: &Version, candidate: &Version) -> bool {
    candidate.pre.is_empty() && candidate > current
}

/// Normalizes and truncates release notes for concise display in the update dialog.
pub fn summarize_release_notes(body: Option<&str>) -> String {
    let raw = body.unwrap_or("").replace("\r\n", "\n");
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "Keine Release-Notes verfügbar.".to_string();
    }
    let char_count = trimmed.chars().count();
    if char_count <= MAX_RELEASE_NOTES_CHARS {
        return trimmed.to_string();
    }
    let mut end_byte = trimmed.len();
    for (idx, (byte_idx, _)) in trimmed.char_indices().enumerate() {
        if idx == MAX_RELEASE_NOTES_CHARS {
            end_byte = byte_idx;
            break;
        }
    }
    format!("{}...", trimmed[..end_byte].trim_end())
}

fn is_non_x64_windows_arch(lower_name: &str) -> bool {
    lower_name.contains("arm64")
        || lower_name.contains("aarch64")
        || lower_name.contains("i686")
        || (lower_name.contains("x86") && !lower_name.contains("x86_64"))
        || lower_name.contains("linux")
        || lower_name.contains("macos")
        || lower_name.contains("darwin")
}

/// Selects the best Windows x86_64 `.exe` or `.zip` asset from a GitHub release.
pub fn select_windows_asset(assets: &[GitHubAsset]) -> Option<GitHubAsset> {
    let valid: Vec<&GitHubAsset> = assets
        .iter()
        .filter(|a| !a.name.trim().is_empty() && !a.browser_download_url.trim().is_empty())
        .collect();

    let score = |asset: &GitHubAsset| -> Option<u8> {
        let lower = asset.name.to_ascii_lowercase();
        if is_non_x64_windows_arch(&lower) {
            return None;
        }
        let is_exe = lower.ends_with(".exe");
        let is_zip = lower.ends_with(".zip");
        if !is_exe && !is_zip {
            return None;
        }
        let has_glowled = lower.contains("glowled");
        let has_win = lower.contains("win");
        let has_x64 = lower.contains("x86_64") || lower.contains("x64") || lower.contains("amd64");

        if is_exe && has_glowled && has_win && has_x64 {
            Some(5)
        } else if is_exe && has_glowled {
            Some(4)
        } else if is_zip && has_win && has_x64 {
            Some(3)
        } else if is_zip && has_glowled {
            Some(2)
        } else if is_exe {
            Some(1)
        } else {
            None
        }
    };

    valid
        .into_iter()
        .filter_map(|a| score(a).map(|s| (s, a)))
        .max_by_key(|(s, _)| *s)
        .map(|(_, a)| a.clone())
}

/// Finds an accompanying `.sha256` / `SHA256SUMS` asset if provided by the release.
pub fn select_checksum_asset(
    assets: &[GitHubAsset],
    binary_asset_name: &str,
) -> Option<GitHubAsset> {
    let target_lower = binary_asset_name.to_ascii_lowercase();
    let exact_names = [
        format!("{target_lower}.sha256"),
        format!("{target_lower}.sha256sum"),
    ];

    for asset in assets {
        if asset.browser_download_url.trim().is_empty() {
            continue;
        }
        let lower = asset.name.to_ascii_lowercase();
        if exact_names.contains(&lower) {
            return Some(asset.clone());
        }
    }

    for asset in assets {
        if asset.browser_download_url.trim().is_empty() {
            continue;
        }
        let lower = asset.name.to_ascii_lowercase();
        if lower.contains("sha256sums")
            || lower.contains("checksums")
            || lower.ends_with(".sha256")
            || lower.ends_with(".sha256sum")
        {
            return Some(asset.clone());
        }
    }

    None
}

fn is_valid_sha256_hex(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parses a SHA-256 hex digest for `asset_name` from single-hash or multi-line `sha256sum` files.
pub fn parse_sha256_for_asset(
    checksum_text: &str,
    asset_name: &str,
) -> std::result::Result<String, UpdateError> {
    let target_base = Path::new(asset_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(asset_name)
        .to_ascii_lowercase();

    let non_empty_lines: Vec<&str> = checksum_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    if non_empty_lines.is_empty() {
        return Err(UpdateError::ChecksumFormat(
            "Leere Prüfsummendatei".to_string(),
        ));
    }

    let mut single_line_hash: Option<String> = None;

    for line in &non_empty_lines {
        let mut parts = line.split_whitespace();
        let Some(first) = parts.next() else {
            continue;
        };
        if !is_valid_sha256_hex(first) {
            continue;
        }
        if let Some(file_part) = parts.next() {
            let cleaned = file_part.trim_start_matches('*').trim_start_matches("./");
            let file_base = Path::new(cleaned)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(cleaned)
                .to_ascii_lowercase();
            if file_base == target_base {
                return Ok(first.to_ascii_lowercase());
            }
        } else if non_empty_lines.len() == 1 {
            single_line_hash = Some(first.to_ascii_lowercase());
        }
    }

    if let Some(hash) = single_line_hash {
        return Ok(hash);
    }

    // Also allow "<64hex>  <something>" when there is only a single entry in a per-asset file
    if non_empty_lines.len() == 1 {
        if let Some(first) = non_empty_lines[0].split_whitespace().next() {
            if is_valid_sha256_hex(first) {
                return Ok(first.to_ascii_lowercase());
            }
        }
    }

    Err(UpdateError::ChecksumFormat(format!(
        "Kein gültiger SHA-256-Eintrag für '{asset_name}' gefunden"
    )))
}

/// Evaluates a parsed `GitHubRelease` against `current_version`.
pub fn evaluate_release(
    release: &GitHubRelease,
    current_version: &Version,
) -> std::result::Result<UpdateCheckOutcome, UpdateError> {
    if release.draft || release.prerelease {
        return Ok(UpdateCheckOutcome::UpToDate {
            current: current_version.clone(),
            latest: current_version.clone(),
        });
    }

    let remote_version = parse_release_version(&release.tag_name)?;
    if !is_newer_stable_version(current_version, &remote_version) {
        return Ok(UpdateCheckOutcome::UpToDate {
            current: current_version.clone(),
            latest: remote_version,
        });
    }

    let asset =
        select_windows_asset(&release.assets).ok_or_else(|| UpdateError::NoCompatibleAsset {
            tag: release.tag_name.clone(),
        })?;
    let checksum_asset = select_checksum_asset(&release.assets, &asset.name);
    let release_name = release
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&release.tag_name)
        .to_string();

    Ok(UpdateCheckOutcome::UpdateAvailable(Box::new(UpdateInfo {
        current_version: current_version.clone(),
        new_version: remote_version,
        tag_name: release.tag_name.clone(),
        release_name,
        release_notes: summarize_release_notes(release.body.as_deref()),
        html_url: release.html_url.clone(),
        asset,
        checksum_asset,
    })))
}

/// Blocking HTTP client wrapper for GitHub Releases API checks and asset downloads.
#[derive(Clone, Debug)]
pub struct UpdaterClient {
    api_url: String,
    api_client: Client,
    download_client: Client,
    allow_insecure_http: bool,
}

impl UpdaterClient {
    /// Creates an updater client targeting the official GloWled GitHub repository over HTTPS.
    pub fn new() -> std::result::Result<Self, UpdateError> {
        Self::with_endpoint(GITHUB_LATEST_RELEASE_URL, API_TIMEOUT, false)
    }

    /// Creates an updater client targeting a custom API URL (intended for tests).
    pub fn with_endpoint(
        api_url: impl Into<String>,
        api_timeout: Duration,
        allow_insecure_http: bool,
    ) -> std::result::Result<Self, UpdateError> {
        let mut headers = HeaderMap::new();
        let ua = HeaderValue::from_str(&user_agent())
            .map_err(|e| UpdateError::HttpClientBuild(format!("Ungültiger User-Agent: {e}")))?;
        headers.insert(USER_AGENT, ua);
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );

        let api_client = Client::builder()
            .default_headers(headers.clone())
            .timeout(api_timeout)
            .build()
            .map_err(|e| UpdateError::HttpClientBuild(e.to_string()))?;

        let download_client = Client::builder()
            .default_headers(headers)
            .timeout(DOWNLOAD_TIMEOUT)
            .build()
            .map_err(|e| UpdateError::HttpClientBuild(e.to_string()))?;

        Ok(Self {
            api_url: api_url.into(),
            api_client,
            download_client,
            allow_insecure_http,
        })
    }

    fn validate_url(&self, url: &str) -> std::result::Result<(), UpdateError> {
        if url.starts_with("https://") {
            return Ok(());
        }
        if self.allow_insecure_http && url.starts_with("http://") {
            return Ok(());
        }
        Err(UpdateError::UnsupportedUrlScheme(url.to_string()))
    }

    /// Queries the GitHub Releases endpoint and compares against `current_version`.
    pub fn check_for_update(
        &self,
        current_version: &Version,
    ) -> std::result::Result<UpdateCheckOutcome, UpdateError> {
        self.validate_url(&self.api_url)?;

        let response = self
            .api_client
            .get(&self.api_url)
            .send()
            .map_err(|e| UpdateError::Network(e.to_string()))?;

        let status = response.status();
        let rate_remaining = response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let rate_reset = response
            .headers()
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(UpdateError::RateLimited {
                reset_at: rate_reset,
            });
        }

        let body_text = response
            .text()
            .map_err(|e| UpdateError::Network(e.to_string()))?;

        if status == StatusCode::FORBIDDEN {
            let is_rate_limited = rate_remaining.as_deref() == Some("0")
                || body_text.to_ascii_lowercase().contains("rate limit");
            if is_rate_limited {
                return Err(UpdateError::RateLimited {
                    reset_at: rate_reset,
                });
            }
            return Err(UpdateError::HttpStatus {
                status: status.as_u16(),
                message: extract_github_error_message(&body_text, "Zugriff verweigert (403)"),
            });
        }

        if !status.is_success() {
            return Err(UpdateError::HttpStatus {
                status: status.as_u16(),
                message: extract_github_error_message(&body_text, status.as_str()),
            });
        }

        let release: GitHubRelease = serde_json::from_str(&body_text)
            .map_err(|e| UpdateError::InvalidPayload(e.to_string()))?;

        evaluate_release(&release, current_version)
    }

    fn download_file_and_sha256(
        &self,
        url: &str,
        dest_path: &Path,
    ) -> std::result::Result<String, UpdateError> {
        self.validate_url(url)?;

        let mut response = self
            .download_client
            .get(url)
            .send()
            .map_err(|e| UpdateError::Network(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            return Err(UpdateError::HttpStatus {
                status: status.as_u16(),
                message: format!("Download fehlgeschlagen ({status})"),
            });
        }

        let mut file = File::create(dest_path).map_err(|source| UpdateError::Io {
            context: "Temporäre Update-Datei anlegen",
            source,
        })?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        let mut total_bytes = 0u64;

        loop {
            let read = response
                .read(&mut buf)
                .map_err(|e| UpdateError::Network(e.to_string()))?;
            if read == 0 {
                break;
            }
            total_bytes = total_bytes.saturating_add(read as u64);
            if total_bytes > MAX_DOWNLOAD_BYTES {
                return Err(UpdateError::InvalidPayload(
                    "Download überschreitet zulässige Maximalgröße".to_string(),
                ));
            }
            hasher.update(&buf[..read]);
            file.write_all(&buf[..read])
                .map_err(|source| UpdateError::Io {
                    context: "Temporäre Update-Datei schreiben",
                    source,
                })?;
        }
        file.flush().map_err(|source| UpdateError::Io {
            context: "Temporäre Update-Datei flushen",
            source,
        })?;

        Ok(format!("{:x}", hasher.finalize()))
    }

    fn fetch_text(&self, url: &str) -> std::result::Result<String, UpdateError> {
        self.validate_url(url)?;

        let response = self
            .api_client
            .get(url)
            .send()
            .map_err(|e| UpdateError::Network(e.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(UpdateError::HttpStatus {
                status: status.as_u16(),
                message: format!("Prüfsummen-Download fehlgeschlagen ({status})"),
            });
        }
        response
            .text()
            .map_err(|e| UpdateError::Network(e.to_string()))
    }

    /// Downloads, verifies (if `.sha256` exists), optionally extracts `.zip`, and validates the new binary.
    pub fn download_and_stage_update<F>(
        &self,
        info: &UpdateInfo,
        staging_dir: &Path,
        expect_pe_header: bool,
        mut on_stage: F,
    ) -> std::result::Result<PathBuf, UpdateError>
    where
        F: FnMut(&str),
    {
        on_stage(&format!("Lade {} herunter...", info.asset.name));

        let safe_name = Path::new(&info.asset.name)
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("glowled-update.bin");
        let download_path = staging_dir.join(safe_name);

        let actual_sha256 =
            self.download_file_and_sha256(&info.asset.browser_download_url, &download_path)?;

        if let Some(checksum_asset) = &info.checksum_asset {
            on_stage("Prüfe SHA-256-Prüfsumme...");
            let checksum_text = self.fetch_text(&checksum_asset.browser_download_url)?;
            let expected_sha256 = parse_sha256_for_asset(&checksum_text, &info.asset.name)?;
            if actual_sha256 != expected_sha256 {
                return Err(UpdateError::ChecksumMismatch {
                    expected: expected_sha256,
                    actual: actual_sha256,
                });
            }
        }

        let staged_exe = if safe_name.to_ascii_lowercase().ends_with(".zip") {
            on_stage("Entpacke Update-Archiv...");
            let extracted_path = staging_dir.join("glowled-new.exe");
            extract_exe_from_zip(&download_path, &extracted_path)?;
            extracted_path
        } else {
            download_path
        };

        if expect_pe_header {
            verify_pe_header(&staged_exe)?;
        }

        Ok(staged_exe)
    }

    /// Downloads, verifies, and installs an update onto `target_exe` with automatic `.old` rollback.
    pub fn download_and_install_update<F>(
        &self,
        info: &UpdateInfo,
        target_exe: &Path,
        expect_pe_header: bool,
        mut on_stage: F,
    ) -> std::result::Result<PathBuf, UpdateError>
    where
        F: FnMut(&str),
    {
        let parent_dir = target_exe.parent().filter(|p| !p.as_os_str().is_empty());
        let temp_dir = match parent_dir {
            Some(dir) => TempDirBuilder::new()
                .prefix(".glowled-update-")
                .tempdir_in(dir),
            None => TempDirBuilder::new().prefix(".glowled-update-").tempdir(),
        }
        .or_else(|_| TempDirBuilder::new().prefix(".glowled-update-").tempdir())
        .map_err(|source| UpdateError::Io {
            context: "Temporäres Update-Verzeichnis erstellen",
            source,
        })?;

        let staged_exe =
            self.download_and_stage_update(info, temp_dir.path(), expect_pe_header, &mut on_stage)?;

        on_stage("Installiere neue Version...");
        replace_executable_with_rollback(target_exe, &staged_exe)
    }
}

fn extract_github_error_message(body: &str, fallback: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = val.get("message").and_then(|m| m.as_str()) {
            let trimmed = msg.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    fallback.to_string()
}

fn extract_exe_from_zip(zip_path: &Path, dest_exe: &Path) -> std::result::Result<(), UpdateError> {
    let file = File::open(zip_path).map_err(|source| UpdateError::Io {
        context: "ZIP-Archiv öffnen",
        source,
    })?;
    let mut archive = ZipArchive::new(file).map_err(|e| UpdateError::ZipExtract(e.to_string()))?;

    let mut candidate_idx: Option<usize> = None;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| UpdateError::ZipExtract(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let Some(safe_rel) = entry.enclosed_name() else {
            continue;
        };
        let is_exe = safe_rel
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        if !is_exe {
            continue;
        }
        let is_glowled = safe_rel
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("glowled.exe"));
        candidate_idx = Some(i);
        if is_glowled {
            break;
        }
    }

    let idx = candidate_idx.ok_or_else(|| {
        UpdateError::ZipExtract("Keine .exe-Datei im Release-Archiv gefunden".to_string())
    })?;
    let mut entry = archive
        .by_index(idx)
        .map_err(|e| UpdateError::ZipExtract(e.to_string()))?;
    let mut out_file = File::create(dest_exe).map_err(|source| UpdateError::Io {
        context: "Entpackte EXE schreiben",
        source,
    })?;
    std::io::copy(&mut entry, &mut out_file).map_err(|source| UpdateError::Io {
        context: "ZIP-Inhalt kopieren",
        source,
    })?;
    out_file.flush().map_err(|source| UpdateError::Io {
        context: "Entpackte EXE flushen",
        source,
    })?;
    Ok(())
}

fn verify_pe_header(path: &Path) -> std::result::Result<(), UpdateError> {
    let mut file = File::open(path).map_err(|source| UpdateError::Io {
        context: "Heruntergeladene EXE prüfen",
        source,
    })?;
    let mut magic = [0u8; 2];
    if file.read_exact(&mut magic).is_err() || &magic != b"MZ" {
        return Err(UpdateError::InvalidBinary);
    }
    Ok(())
}

/// Returns the `.old` backup path for a target executable (e.g. `glowled.exe` -> `glowled.exe.old`).
pub fn backup_path_for(target_exe: &Path) -> PathBuf {
    let mut os = target_exe.as_os_str().to_os_string();
    os.push(".old");
    PathBuf::from(os)
}

/// Best-effort cleanup of a leftover `.old` backup binary from a previous update.
pub fn cleanup_old_binary() {
    if let Ok(exe) = std::env::current_exe() {
        let old = backup_path_for(&exe);
        if old.exists() {
            let _ = fs::remove_file(old);
        }
    }
}

fn move_file_into_place(src: &Path, dst: &Path) -> std::result::Result<(), std::io::Error> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(src, dst)?;
            let _ = fs::remove_file(src);
            Ok(())
        }
    }
}

/// Replaces `target_exe` with `new_exe` via `.old` rename, rolling back cleanly if placement fails.
pub fn replace_executable_with_rollback(
    target_exe: &Path,
    new_exe: &Path,
) -> std::result::Result<PathBuf, UpdateError> {
    let backup_path = backup_path_for(target_exe);
    if backup_path.exists() {
        let _ = fs::remove_file(&backup_path);
    }

    let had_existing = target_exe.exists();
    if had_existing {
        fs::rename(target_exe, &backup_path).map_err(|source| UpdateError::Io {
            context: "Laufende EXE in .old umbenennen",
            source,
        })?;
    }

    if let Err(replace_err) = move_file_into_place(new_exe, target_exe) {
        if had_existing {
            let _ = fs::remove_file(target_exe);
            if let Err(rollback_err) = fs::rename(&backup_path, target_exe) {
                return Err(UpdateError::RollbackFailed {
                    replace_err: replace_err.to_string(),
                    rollback_err: rollback_err.to_string(),
                });
            }
        }
        return Err(UpdateError::Io {
            context: "Neue EXE an Zielpfad verschieben (Rollback durchgeführt)",
            source: replace_err,
        });
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(target_exe) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(target_exe, perms);
        }
    }

    Ok(backup_path)
}

/// Spawns the newly installed executable cleanly so the current process can exit.
pub fn spawn_restarted_executable(exe_path: &Path) -> std::result::Result<(), UpdateError> {
    let mut cmd = Command::new(exe_path);
    cmd.arg("--post-update").arg("--show");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    cmd.spawn().map_err(|source| UpdateError::Io {
        context: "Aktualisierte Anwendung neu starten",
        source,
    })?;
    Ok(())
}

/// Events emitted asynchronously by `UpdateController` to the Slint UI thread.
#[derive(Clone, Debug)]
pub enum UpdateEvent {
    UpToDate {
        current_version: String,
        latest_version: String,
        manual: bool,
    },
    UpdateAvailable(Box<UpdateInfo>),
    CheckFailed {
        error: String,
        manual: bool,
    },
    InstallProgress(String),
    InstallCompleted {
        new_version: String,
        exe_path: PathBuf,
    },
    InstallFailed(String),
}

/// Non-blocking background coordinator for update checks and installation.
pub struct UpdateController {
    tx: Sender<UpdateEvent>,
    rx: Receiver<UpdateEvent>,
    busy: Arc<AtomicBool>,
}

impl Default for UpdateController {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateController {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        Self {
            tx,
            rx,
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    pub fn try_recv_event(&self) -> Option<UpdateEvent> {
        self.rx.try_recv().ok()
    }

    /// Starts a non-blocking GitHub Releases check on a background thread.
    pub fn spawn_check(&self, manual: bool) -> bool {
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }

        let tx = self.tx.clone();
        let busy = self.busy.clone();
        let spawn_res = thread::Builder::new()
            .name("glowled-update-check".to_string())
            .spawn(move || {
                let result = (|| -> std::result::Result<UpdateCheckOutcome, UpdateError> {
                    let current = current_version()?;
                    let client = UpdaterClient::new()?;
                    client.check_for_update(&current)
                })();

                match result {
                    Ok(UpdateCheckOutcome::UpToDate { current, latest }) => {
                        let _ = tx.send(UpdateEvent::UpToDate {
                            current_version: current.to_string(),
                            latest_version: latest.to_string(),
                            manual,
                        });
                    }
                    Ok(UpdateCheckOutcome::UpdateAvailable(info)) => {
                        let _ = tx.send(UpdateEvent::UpdateAvailable(info));
                    }
                    Err(err) => {
                        let _ = tx.send(UpdateEvent::CheckFailed {
                            error: err.to_string(),
                            manual,
                        });
                    }
                }
                busy.store(false, Ordering::Release);
            });

        if let Err(err) = spawn_res {
            self.busy.store(false, Ordering::Release);
            let _ = self.tx.send(UpdateEvent::CheckFailed {
                error: format!("Update-Thread konnte nicht gestartet werden: {err}"),
                manual,
            });
            return false;
        }
        true
    }

    /// Starts a non-blocking download, verification, and in-place replacement on a background thread.
    pub fn spawn_install(&self, info: UpdateInfo) -> bool {
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }

        let tx = self.tx.clone();
        let busy = self.busy.clone();
        let spawn_res = thread::Builder::new()
            .name("glowled-update-install".to_string())
            .spawn(move || {
                let res = (|| -> std::result::Result<PathBuf, UpdateError> {
                    let target_exe = std::env::current_exe().map_err(|source| UpdateError::Io {
                        context: "Aktuellen EXE-Pfad ermitteln",
                        source,
                    })?;
                    let client = UpdaterClient::new()?;
                    let expect_pe = cfg!(windows);
                    client.download_and_install_update(&info, &target_exe, expect_pe, |stage| {
                        let _ = tx.send(UpdateEvent::InstallProgress(stage.to_string()));
                    })?;
                    Ok(target_exe)
                })();

                match res {
                    Ok(exe_path) => {
                        let _ = tx.send(UpdateEvent::InstallCompleted {
                            new_version: info.new_version.to_string(),
                            exe_path,
                        });
                    }
                    Err(err) => {
                        let _ = tx.send(UpdateEvent::InstallFailed(err.to_string()));
                    }
                }
                busy.store(false, Ordering::Release);
            });

        if let Err(err) = spawn_res {
            self.busy.store(false, Ordering::Release);
            let _ = self.tx.send(UpdateEvent::InstallFailed(format!(
                "Update-Thread konnte nicht gestartet werden: {err}"
            )));
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_and_rejects_invalid_git_tags() {
        assert_eq!(
            parse_release_version("v1.2.3").unwrap(),
            Version::new(1, 2, 3)
        );
        assert_eq!(
            parse_release_version("1.2.3").unwrap(),
            Version::new(1, 2, 3)
        );
        assert_eq!(
            parse_release_version(" V2.4.1 ").unwrap(),
            Version::new(2, 4, 1)
        );

        let beta = parse_release_version("v1.2.3-beta.1").unwrap();
        assert_eq!(beta, Version::parse("1.2.3-beta.1").unwrap());
        assert!(!beta.pre.is_empty());

        let with_build = parse_release_version("v2.0.1+20261123").unwrap();
        assert_eq!(with_build.major, 2);
        assert_eq!(with_build.minor, 0);
        assert_eq!(with_build.patch, 1);

        assert!(matches!(
            parse_release_version(""),
            Err(UpdateError::InvalidVersion { .. })
        ));
        assert!(matches!(
            parse_release_version("latest"),
            Err(UpdateError::InvalidVersion { .. })
        ));
        assert!(matches!(
            parse_release_version("vv1.2.3"),
            Err(UpdateError::InvalidVersion { .. })
        ));
    }

    #[test]
    fn compares_versions_and_ignores_prereleases_as_stable_updates() {
        let current = Version::new(2, 0, 0);
        assert!(is_newer_stable_version(&current, &Version::new(2, 0, 1)));
        assert!(is_newer_stable_version(&current, &Version::new(2, 1, 0)));
        assert!(is_newer_stable_version(&current, &Version::new(3, 0, 0)));

        assert!(!is_newer_stable_version(&current, &Version::new(2, 0, 0)));
        assert!(!is_newer_stable_version(&current, &Version::new(1, 9, 9)));
        assert!(!is_newer_stable_version(
            &current,
            &Version::parse("2.1.0-beta.1").unwrap()
        ));
        assert!(!is_newer_stable_version(
            &current,
            &Version::parse("2.0.1-rc.2").unwrap()
        ));
    }

    #[test]
    fn selects_windows_x64_asset_and_checksum_by_priority() {
        let assets = vec![
            GitHubAsset {
                name: "glowled-linux-x86_64.tar.gz".into(),
                browser_download_url: "https://example.com/linux.tar.gz".into(),
                size: 100,
                content_type: "application/gzip".into(),
            },
            GitHubAsset {
                name: "glowled-windows-arm64.exe".into(),
                browser_download_url: "https://example.com/arm64.exe".into(),
                size: 100,
                content_type: "application/octet-stream".into(),
            },
            GitHubAsset {
                name: "glowled-windows-x86_64.zip".into(),
                browser_download_url: "https://example.com/win.zip".into(),
                size: 100,
                content_type: "application/zip".into(),
            },
            GitHubAsset {
                name: "glowled-windows-x86_64.exe".into(),
                browser_download_url: "https://example.com/win.exe".into(),
                size: 100,
                content_type: "application/octet-stream".into(),
            },
            GitHubAsset {
                name: "glowled-windows-x86_64.exe.sha256".into(),
                browser_download_url: "https://example.com/win.exe.sha256".into(),
                size: 64,
                content_type: "text/plain".into(),
            },
        ];

        let selected = select_windows_asset(&assets).unwrap();
        assert_eq!(selected.name, "glowled-windows-x86_64.exe");

        let checksum = select_checksum_asset(&assets, &selected.name).unwrap();
        assert_eq!(checksum.name, "glowled-windows-x86_64.exe.sha256");
    }

    #[test]
    fn parses_single_and_multiline_sha256_files() {
        let hash_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let hash_b = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

        assert_eq!(
            parse_sha256_for_asset(hash_a, "glowled-windows-x86_64.exe").unwrap(),
            hash_a
        );

        let multi = format!(
            "# release checksums\n{hash_a}  glowled-linux-x86_64\n{hash_b} *dist/glowled-windows-x86_64.exe\n"
        );
        assert_eq!(
            parse_sha256_for_asset(&multi, "glowled-windows-x86_64.exe").unwrap(),
            hash_b.to_ascii_lowercase()
        );

        assert!(matches!(
            parse_sha256_for_asset("not-a-hash", "glowled.exe"),
            Err(UpdateError::ChecksumFormat(_))
        ));
    }

    #[test]
    fn evaluate_release_ignores_drafts_and_prereleases() {
        let current = Version::new(2, 0, 0);
        let asset = GitHubAsset {
            name: "glowled-windows-x86_64.exe".into(),
            browser_download_url: "https://example.com/glowled.exe".into(),
            size: 100,
            content_type: "application/octet-stream".into(),
        };

        let draft = GitHubRelease {
            tag_name: "v2.5.0".into(),
            name: Some("Draft".into()),
            body: Some("notes".into()),
            draft: true,
            prerelease: false,
            html_url: String::new(),
            assets: vec![asset.clone()],
        };
        assert!(matches!(
            evaluate_release(&draft, &current).unwrap(),
            UpdateCheckOutcome::UpToDate { .. }
        ));

        let pre = GitHubRelease {
            draft: false,
            prerelease: true,
            ..draft
        };
        assert!(matches!(
            evaluate_release(&pre, &current).unwrap(),
            UpdateCheckOutcome::UpToDate { .. }
        ));
    }
}
