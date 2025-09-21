use std::fs;
use std::path::PathBuf;
use zed_extension_api::{self as zed, GithubRelease, Result};

const EXTENSION_LSP_NAME: &str = "codebook-lsp";
const VERSION_FILE: &str = ".version";

struct CodebookExtension {
    binary_cache: Option<PathBuf>,
}

#[derive(Clone)]
struct CodebookBinary {
    path: PathBuf,
    env: Option<Vec<(String, String)>>,
}

impl CodebookBinary {
    fn new(path: PathBuf, env: Option<Vec<(String, String)>>) -> Self {
        let env = match env {
            Some(env) => env,
            None => vec![("RUST_LOG".to_string(), "info".to_string())],
        };
        Self {
            path,
            env: Some(env),
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
        let dev_path = PathBuf::from(EXTENSION_LSP_NAME);
        if dev_path.exists() {
            return Ok(CodebookBinary {
                path: dev_path,
                env: Some(vec![("RUST_LOG".to_string(), "debug".to_string())]),
            });
        }

        if let Some(path) = worktree.which(EXTENSION_LSP_NAME) {
            return Ok(CodebookBinary {
                path: PathBuf::from(path),
                env: Some(vec![("RUST_LOG".to_string(), "info".to_string())]),
            });
        }

        if let Some(path) = &self.binary_cache {
            if path.exists() {
                return Ok(CodebookBinary {
                    path: path.clone(),
                    env: Some(vec![("RUST_LOG".to_string(), "info".to_string())]),
                });
            }
        }

        // check for update
        // on fail, try cached version
        // on that fail, return error
        // on success, if update available, download
        // on success, if no update, use cached version
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let bin = match self.check_for_update() {
            Ok(Some(release)) => {
                // Update available
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Downloading,
                );
                let path = self.install_binary(&release);
                match path {
                    Err(e) => {
                        zed::set_language_server_installation_status(
                            language_server_id,
                            &zed::LanguageServerInstallationStatus::Failed(format!(
                                "Failed to get latest release: {}",
                                e.clone()
                            )),
                        );
                        Err(e)
                    }
                    Ok(path) => Ok(CodebookBinary::new(path, None)),
                }
            }
            Ok(None) => {
                // No update
                let path = self.file_binary()?;
                Ok(CodebookBinary::new(path, None))
            }
            Err(_) => {
                // Check failed, likely no internet. Fallback to existing binary
                let path = self.file_binary()?;
                Ok(CodebookBinary::new(path, None))
            }
        };
        // All good? let's cache this bad boy
        if let Ok(bin_ok) = bin.clone() {
            self.binary_cache = Some(bin_ok.path);

            // Reset status if no issues
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::None,
            );
        }
        bin
    }

    fn version_from_version_file(&self) -> Result<String> {
        fs::read_to_string(VERSION_FILE)
            .map(|s| s.trim().to_string())
            .map_err(|e| e.to_string())
    }

    fn versioned_folder(&self, version: &str) -> Result<PathBuf> {
        let folder_path = format!("{EXTENSION_LSP_NAME}-{}", version);
        let folder_path = PathBuf::from(folder_path);
        Ok(folder_path)
    }

    fn get_filename(&self) -> PathBuf {
        let (platform, _) = zed::current_platform();
        let mut binary = PathBuf::from(EXTENSION_LSP_NAME);
        if platform == zed::Os::Windows {
            binary.set_extension("exe");
        }
        binary
    }

    fn check_for_update(&self) -> Result<Option<GithubRelease>> {
        let release = zed::latest_github_release(
            "blopker/codebook",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;
        if let Ok(version) = self.version_from_version_file() {
            if version == release.version {
                // No update available
                return Ok(None);
            }
        };
        Ok(Some(release))
    }

    fn file_binary(&mut self) -> Result<PathBuf> {
        // Get version from VERSION_FILE, try and get binary path from that
        let version = self.version_from_version_file()?;
        let folder = self.versioned_folder(&version)?;
        let binary_path = folder.join(self.get_filename());
        Ok(binary_path)
    }

    fn install_binary(&mut self, release: &zed::GithubRelease) -> Result<PathBuf> {
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

        let asset_name = format!("{EXTENSION_LSP_NAME}-{arch_name}-{os_str}.{file_ext}");
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| {
                format!("No compatible Codebook binary found for {arch_name}-{os_str}")
            })?;

        let version_dir = self.versioned_folder(&release.version)?;
        let binary_path = PathBuf::from(&version_dir).join(self.get_filename());
        let version_dir_str = version_dir.to_string_lossy();

        if !binary_path.exists() {
            let download_result = (|| -> Result<()> {
                zed::download_file(
                    &asset.download_url,
                    &version_dir_str,
                    if platform == zed::Os::Windows {
                        zed::DownloadedFileType::Zip
                    } else {
                        zed::DownloadedFileType::GzipTar
                    },
                )
                .map_err(|e| format!("Failed to download Codebook binary: {}", e))?;

                zed::make_file_executable(binary_path.to_str().ok_or("Invalid binary path")?)
                    .map_err(|e| format!("Failed to make binary executable: {}", e))?;

                Ok(())
            })();

            if let Err(e) = download_result {
                fs::remove_dir_all(&version_dir).ok();
                return Err(e);
            }

            // place version file
            fs::write(VERSION_FILE, &release.version)
                .map_err(|e| format!("Failed to write version file: {}", e))?;

            if let Ok(entries) = fs::read_dir(".") {
                for entry in entries.flatten() {
                    if let Ok(name) = entry.file_name().into_string() {
                        if name != version_dir_str {
                            fs::remove_dir_all(entry.path()).ok();
                        }
                    }
                }
            }
        }

        self.binary_cache = Some(binary_path.clone());
        Ok(binary_path)
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
        Ok(zed::Command {
            command: binary
                .path
                .to_str()
                .ok_or("Failed to convert binary path to string")?
                .to_string(),
            args: vec![format!("--root={project_path}"), "serve".to_string()],
            env: binary.env.unwrap_or_default(),
        })
    }
}

zed::register_extension!(CodebookExtension);
