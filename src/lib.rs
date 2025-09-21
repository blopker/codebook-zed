use std::fs;
use std::path::{Path, PathBuf};
use zed_extension_api::{self as zed, GithubRelease, Result};

// Binary and versioning constants
const EXTENSION_LSP_NAME: &str = "codebook-lsp";
const VERSION_FILE: &str = ".version";
const GITHUB_REPO_OWNER: &str = "blopker";
const GITHUB_REPO_NAME: &str = "codebook";

// Environment variable constants
const ENV_RUST_LOG: &str = "RUST_LOG";
const LOG_LEVEL_DEBUG: &str = "debug";
const LOG_LEVEL_INFO: &str = "info";

struct CodebookExtension {
    binary_cache: Option<PathBuf>,
}

#[derive(Clone)]
struct CodebookBinary {
    path: PathBuf,
    env: Vec<(String, String)>,
}

impl CodebookBinary {
    fn new(path: PathBuf, log_level: &str) -> Self {
        Self {
            path,
            env: vec![(ENV_RUST_LOG.to_string(), log_level.to_string())],
        }
    }
}

impl CodebookExtension {
    fn new() -> Self {
        Self { binary_cache: None }
    }

    fn get_binary(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<CodebookBinary> {
        // Check for development binary
        if let Some(binary) = self.find_development_binary()? {
            return Ok(binary);
        }

        // Check system PATH
        if let Some(binary) = self.find_system_binary(worktree)? {
            return Ok(binary);
        }

        // Check and validate cache
        if let Some(binary) = self.get_cached_binary()? {
            return Ok(binary);
        }

        // Download or update binary
        self.ensure_latest_binary(language_server_id)
    }

    fn find_development_binary(&self) -> Result<Option<CodebookBinary>> {
        let dev_path = PathBuf::from(EXTENSION_LSP_NAME);
        if dev_path.exists() {
            Ok(Some(CodebookBinary::new(dev_path, LOG_LEVEL_DEBUG)))
        } else {
            Ok(None)
        }
    }

    fn find_system_binary(&self, worktree: &zed::Worktree) -> Result<Option<CodebookBinary>> {
        if let Some(path) = worktree.which(EXTENSION_LSP_NAME) {
            Ok(Some(CodebookBinary::new(
                PathBuf::from(path),
                LOG_LEVEL_INFO,
            )))
        } else {
            Ok(None)
        }
    }

    fn get_cached_binary(&self) -> Result<Option<CodebookBinary>> {
        if let Some(path) = &self.binary_cache {
            if path.exists() {
                return Ok(Some(CodebookBinary::new(path.clone(), LOG_LEVEL_INFO)));
            }
        }
        Ok(None)
    }

    fn ensure_latest_binary(
        &mut self,
        language_server_id: &zed::LanguageServerId,
    ) -> Result<CodebookBinary> {
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let result = match self.check_for_update() {
            Ok(Some(release)) => {
                // Update available - download it
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Downloading,
                );
                self.download_and_install_binary(&release, language_server_id)
            }
            Ok(None) => {
                // No update needed - use existing
                self.load_existing_binary()
            }
            Err(_) => {
                // Check failed (likely offline) - try existing
                self.load_existing_binary()
            }
        };

        // Update cache and reset status on success
        if let Ok(ref binary) = result {
            self.binary_cache = Some(binary.path.clone());
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::None,
            );
        }

        result
    }

    fn download_and_install_binary(
        &self,
        release: &GithubRelease,
        language_server_id: &zed::LanguageServerId,
    ) -> Result<CodebookBinary> {
        match self.install_binary(release) {
            Ok(path) => Ok(CodebookBinary::new(path, LOG_LEVEL_INFO)),
            Err(e) => {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Failed(format!(
                        "Failed to install release: {}",
                        e
                    )),
                );
                Err(e)
            }
        }
    }

    fn load_existing_binary(&self) -> Result<CodebookBinary> {
        let path = self.get_cached_binary_path()?;
        Ok(CodebookBinary::new(path, LOG_LEVEL_INFO))
    }

    fn read_version_file(&self) -> Result<String> {
        fs::read_to_string(VERSION_FILE)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("Failed to read version file: {}", e))
    }

    fn get_version_directory_path(&self, version: &str) -> PathBuf {
        PathBuf::from(format!("{}-{}", EXTENSION_LSP_NAME, version))
    }

    fn get_binary_filename(&self) -> PathBuf {
        let (platform, _) = zed::current_platform();
        let mut binary = PathBuf::from(EXTENSION_LSP_NAME);
        if platform == zed::Os::Windows {
            binary.set_extension("exe");
        }
        binary
    }

    fn check_for_update(&self) -> Result<Option<GithubRelease>> {
        let release = zed::latest_github_release(
            &format!("{}/{}", GITHUB_REPO_OWNER, GITHUB_REPO_NAME),
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        // Check if we already have this version
        if let Ok(current_version) = self.read_version_file() {
            if current_version == release.version {
                return Ok(None);
            }
        }

        Ok(Some(release))
    }

    fn get_cached_binary_path(&self) -> Result<PathBuf> {
        let version = self.read_version_file()?;
        let version_dir = self.get_version_directory_path(&version);
        let binary_path = version_dir.join(self.get_binary_filename());

        if !binary_path.exists() {
            return Err(format!(
                "Binary not found at expected path: {}",
                binary_path.display()
            ));
        }

        Ok(binary_path)
    }

    fn install_binary(&self, release: &zed::GithubRelease) -> Result<PathBuf> {
        let asset = self.find_compatible_asset(release)?;
        let version_dir = self.get_version_directory_path(&release.version);
        let binary_path = version_dir.join(self.get_binary_filename());

        if !binary_path.exists() {
            self.download_binary(asset, &version_dir, &binary_path)?;
            self.write_version_file(&release.version)?;
            self.cleanup_old_versions(&version_dir)?;
        }
        Ok(binary_path)
    }

    fn find_compatible_asset<'a>(
        &self,
        release: &'a GithubRelease,
    ) -> Result<&'a zed::GithubReleaseAsset> {
        let (platform, arch) = zed::current_platform();

        let arch_name = match arch {
            zed::Architecture::Aarch64 => "aarch64",
            zed::Architecture::X8664 => "x86_64",
            zed::Architecture::X86 => return Err("x86 architecture is not supported".into()),
        };

        let (os_str, file_ext) = match platform {
            zed::Os::Mac => ("apple-darwin", "tar.gz"),
            zed::Os::Linux => ("unknown-linux-musl", "tar.gz"),
            zed::Os::Windows => ("pc-windows-msvc", "zip"),
        };

        let asset_name = format!(
            "{}-{}-{}.{}",
            EXTENSION_LSP_NAME, arch_name, os_str, file_ext
        );

        release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| format!("No compatible binary found for {}-{}", arch_name, os_str))
    }

    fn download_binary(
        &self,
        asset: &zed::GithubReleaseAsset,
        version_dir: &Path,
        binary_path: &Path,
    ) -> Result<()> {
        let (platform, _) = zed::current_platform();
        let version_dir_str = version_dir
            .to_str()
            .ok_or("Invalid version directory path")?;

        // Download and extract
        zed::download_file(
            &asset.download_url,
            version_dir_str,
            if platform == zed::Os::Windows {
                zed::DownloadedFileType::Zip
            } else {
                zed::DownloadedFileType::GzipTar
            },
        )
        .map_err(|e| format!("Failed to download binary: {}", e))?;

        // Make executable
        let binary_path_str = binary_path.to_str().ok_or("Invalid binary path")?;

        zed::make_file_executable(binary_path_str)
            .map_err(|e| format!("Failed to make binary executable: {}", e))?;

        Ok(())
    }

    fn write_version_file(&self, version: &str) -> Result<()> {
        fs::write(VERSION_FILE, version).map_err(|e| format!("Failed to write version file: {}", e))
    }

    fn cleanup_old_versions(&self, current_version_dir: &Path) -> Result<()> {
        let current_dir_name = current_version_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid version directory name")?;

        let entries = fs::read_dir(".").map_err(|e| format!("Failed to read directory: {}", e))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let entry_name = entry.file_name();

            if let Some(name_str) = entry_name.to_str() {
                // Only remove directories that match our version directory pattern
                if name_str.starts_with(&format!("{}-", EXTENSION_LSP_NAME))
                    && name_str != current_dir_name
                    && entry.path().is_dir()
                {
                    if let Err(e) = fs::remove_dir_all(entry.path()) {
                        // Log but don't fail on cleanup errors
                        eprintln!(
                            "Warning: Failed to remove old version directory {}: {}",
                            name_str, e
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

impl zed::Extension for CodebookExtension {
    fn new() -> Self {
        Self::new()
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = self.get_binary(language_server_id, worktree)?;
        let project_path = worktree.root_path();

        let binary_str = binary
            .path
            .to_str()
            .ok_or("Failed to convert binary path to string")?;

        Ok(zed::Command {
            command: binary_str.to_string(),
            args: vec![format!("--root={}", project_path), "serve".to_string()],
            env: binary.env,
        })
    }
}

zed::register_extension!(CodebookExtension);
