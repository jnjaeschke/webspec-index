use std::path::{Path, PathBuf};

use zed_extension_api as zed;
use zed_extension_api::settings::LspSettings;

const LANGUAGE_SERVER_ID: &str = "webspec-index";
const GITHUB_REPO: &str = "jnjaeschke/webspec-index";
const BINARY_STEM: &str = "webspec-index";

struct WebspecLensExtension;

impl WebspecLensExtension {
    fn binary_name(os: zed::Os) -> &'static str {
        match os {
            zed::Os::Windows => "webspec-index.exe",
            _ => "webspec-index",
        }
    }

    fn target_triple(os: zed::Os, arch: zed::Architecture) -> Option<&'static str> {
        match (os, arch) {
            (zed::Os::Linux, zed::Architecture::X8664) => Some("x86_64-unknown-linux-gnu"),
            (zed::Os::Linux, zed::Architecture::Aarch64) => Some("aarch64-unknown-linux-gnu"),
            (zed::Os::Mac, zed::Architecture::X8664) => Some("x86_64-apple-darwin"),
            (zed::Os::Mac, zed::Architecture::Aarch64) => Some("aarch64-apple-darwin"),
            (zed::Os::Windows, zed::Architecture::X8664) => Some("x86_64-pc-windows-msvc"),
            _ => None,
        }
    }

    fn archive_format(os: zed::Os) -> (&'static str, zed::DownloadedFileType) {
        match os {
            zed::Os::Windows => ("zip", zed::DownloadedFileType::Zip),
            _ => ("tar.gz", zed::DownloadedFileType::GzipTar),
        }
    }

    fn release_tag() -> String {
        format!("v{}", env!("CARGO_PKG_VERSION"))
    }

    fn managed_binary_path(os: zed::Os) -> PathBuf {
        Path::new(BINARY_STEM)
            .join(Self::release_tag())
            .join(Self::binary_name(os))
    }

    fn build_command(
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    ) -> zed::Command {
        zed::Command { command, args, env }
    }

    fn ensure_installed(
        &self,
        language_server_id: &zed::LanguageServerId,
        os: zed::Os,
        arch: zed::Architecture,
    ) -> zed::Result<String> {
        let managed_binary = Self::managed_binary_path(os);
        if managed_binary.exists() {
            return Ok(managed_binary.to_string_lossy().into_owned());
        }

        let target = Self::target_triple(os, arch)
            .ok_or_else(|| format!("unsupported platform for auto-install: {:?}-{:?}", os, arch))?;
        let (archive_ext, downloaded_file_type) = Self::archive_format(os);
        let asset_name = format!("{BINARY_STEM}-{target}.{archive_ext}");

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::github_release_by_tag_name(GITHUB_REPO, &Self::release_tag())?;

        let asset = release
            .assets
            .iter()
            .find(|candidate| candidate.name == asset_name)
            .ok_or_else(|| format!("release {} missing asset {}", release.version, asset_name))?;

        let install_dir = managed_binary
            .parent()
            .ok_or_else(|| "invalid install path".to_string())?;
        let install_dir_str = install_dir.to_string_lossy();

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::Downloading,
        );

        if let Err(err) =
            zed::download_file(&asset.download_url, &install_dir_str, downloaded_file_type)
        {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Failed(format!(
                    "failed to download {}: {err}",
                    asset.name
                )),
            );
            return Err(err);
        }

        if os != zed::Os::Windows {
            zed::make_file_executable(&managed_binary.to_string_lossy())
                .map_err(|e| format!("failed to mark binary executable: {e}"))?;
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::None,
        );
        Ok(managed_binary.to_string_lossy().into_owned())
    }
}

impl zed::Extension for WebspecLensExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        if language_server_id.as_ref() != LANGUAGE_SERVER_ID {
            return Err(format!(
                "unsupported language server id: {}",
                language_server_id.as_ref()
            ));
        }

        let lsp_settings = LspSettings::for_worktree(LANGUAGE_SERVER_ID, worktree)?;
        if let Some(binary_settings) = lsp_settings.binary {
            if let Some(path) = binary_settings.path {
                let args = binary_settings
                    .arguments
                    .unwrap_or_else(|| vec!["lsp".to_string()]);
                let env = binary_settings
                    .env
                    .map(|pairs| pairs.into_iter().collect::<Vec<_>>())
                    .unwrap_or_default();
                return Ok(Self::build_command(path, args, env));
            }
        }

        if let Some(path) = worktree.which(BINARY_STEM) {
            return Ok(Self::build_command(path, vec!["lsp".to_string()], vec![]));
        }

        let (os, arch) = zed::current_platform();
        let path = self.ensure_installed(language_server_id, os, arch)?;
        Ok(Self::build_command(path, vec!["lsp".to_string()], vec![]))
    }

    fn language_server_initialization_options(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        if language_server_id.as_ref() != LANGUAGE_SERVER_ID {
            return Ok(None);
        }

        let lsp_settings = LspSettings::for_worktree(LANGUAGE_SERVER_ID, worktree)?;
        Ok(lsp_settings.initialization_options)
    }

    fn language_server_workspace_configuration(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        if language_server_id.as_ref() != LANGUAGE_SERVER_ID {
            return Ok(None);
        }

        let lsp_settings = LspSettings::for_worktree(LANGUAGE_SERVER_ID, worktree)?;
        Ok(lsp_settings.settings)
    }
}

zed::register_extension!(WebspecLensExtension);

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::WebspecLensExtension as Ext;
    use zed_extension_api as zed;

    #[test]
    fn maps_supported_targets() {
        assert_eq!(
            Ext::target_triple(zed::Os::Linux, zed::Architecture::X8664),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(
            Ext::target_triple(zed::Os::Linux, zed::Architecture::Aarch64),
            Some("aarch64-unknown-linux-gnu")
        );
        assert_eq!(
            Ext::target_triple(zed::Os::Mac, zed::Architecture::X8664),
            Some("x86_64-apple-darwin")
        );
        assert_eq!(
            Ext::target_triple(zed::Os::Mac, zed::Architecture::Aarch64),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(
            Ext::target_triple(zed::Os::Windows, zed::Architecture::X8664),
            Some("x86_64-pc-windows-msvc")
        );
    }

    #[test]
    fn rejects_unsupported_targets() {
        assert_eq!(
            Ext::target_triple(zed::Os::Windows, zed::Architecture::Aarch64),
            None
        );
        assert_eq!(
            Ext::target_triple(zed::Os::Linux, zed::Architecture::X86),
            None
        );
    }

    #[test]
    fn chooses_archive_format_by_os() {
        let (ext, _) = Ext::archive_format(zed::Os::Windows);
        assert_eq!(ext, "zip");

        let (ext, _) = Ext::archive_format(zed::Os::Linux);
        assert_eq!(ext, "tar.gz");
    }

    #[test]
    fn chooses_binary_name_by_os() {
        assert_eq!(Ext::binary_name(zed::Os::Windows), "webspec-index.exe");
        assert_eq!(Ext::binary_name(zed::Os::Mac), "webspec-index");
    }

    #[test]
    fn builds_expected_managed_paths() {
        let path = Ext::managed_binary_path(zed::Os::Linux);
        assert!(path.ends_with(
            Path::new("webspec-index")
                .join("v0.5.0")
                .join("webspec-index")
        ));

        let path = Ext::managed_binary_path(zed::Os::Windows);
        assert!(path.ends_with(
            Path::new("webspec-index")
                .join("v0.5.0")
                .join("webspec-index.exe")
        ));
    }
}
