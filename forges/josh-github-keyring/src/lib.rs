use std::any::Any;
use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use keyring_core::api::CredentialApi;
use serde::{Deserialize, Serialize};

use josh_github_auth::device_flow::AccessTokenResponse;

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredToken {
    access_token: String,
    token_type: String,
    scope: String,
    refresh_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

impl From<AccessTokenResponse> for StoredToken {
    fn from(resp: AccessTokenResponse) -> Self {
        let expires_at = resp
            .expires_in
            .and_then(|secs| Duration::try_seconds(secs as i64))
            .map(|d| Utc::now() + d);

        Self {
            access_token: resp.access_token,
            token_type: resp.token_type,
            scope: resp.scope,
            refresh_token: resp.refresh_token,
            expires_at,
        }
    }
}

impl From<StoredToken> for AccessTokenResponse {
    fn from(stored: StoredToken) -> Self {
        let expires_in = stored.expires_at.map(|at| {
            let remaining = at - Utc::now();
            remaining.num_seconds().max(0) as u64
        });

        Self {
            access_token: stored.access_token,
            token_type: stored.token_type,
            scope: stored.scope,
            refresh_token: stored.refresh_token,
            expires_in,
        }
    }
}

/// File-based credential store implementing [`CredentialApi`].
///
/// Stores a JSON blob at `~/.config/josh-cli/credentials.json` with `0600`
/// permissions on Unix. Used as the default backend when the macOS Keychain
/// is not available.
#[derive(Debug)]
pub struct FileCredentialStore {
    path: PathBuf,
}

impl FileCredentialStore {
    pub fn new() -> Option<Self> {
        let path = dirs::config_dir()?
            .join("josh-cli")
            .join("credentials.json");

        Some(Self { path })
    }
}

impl CredentialApi for FileCredentialStore {
    fn set_secret(&self, secret: &[u8]) -> keyring_core::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| keyring_core::Error::PlatformFailure(Box::new(e)))?;
        }

        std::fs::write(&self.path, secret)
            .map_err(|e| keyring_core::Error::PlatformFailure(Box::new(e)))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| keyring_core::Error::PlatformFailure(Box::new(e)))?;
        }

        Ok(())
    }

    fn get_secret(&self) -> keyring_core::Result<Vec<u8>> {
        std::fs::read(&self.path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                keyring_core::Error::NoEntry
            } else {
                keyring_core::Error::PlatformFailure(Box::new(e))
            }
        })
    }

    fn delete_credential(&self) -> keyring_core::Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(keyring_core::Error::NoEntry),
            Err(e) => Err(keyring_core::Error::PlatformFailure(Box::new(e))),
        }
    }

    fn get_credential(
        &self,
    ) -> keyring_core::Result<Option<std::sync::Arc<keyring_core::Credential>>> {
        if self.path.exists() {
            Ok(None)
        } else {
            Err(keyring_core::Error::NoEntry)
        }
    }

    fn get_specifiers(&self) -> Option<(String, String)> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Return the default credential store for the current build configuration.
pub fn default_store() -> anyhow::Result<keyring_core::Entry> {
    #[cfg(all(feature = "codesign", target_os = "macos"))]
    let entry = {
        use keyring_core::api::CredentialStoreApi;

        const KEYRING_SERVICE: &str = "josh-cli";
        const KEYRING_KEY: &str = "github:credentials";

        apple_native_keyring_store::keychain::Store::new()?.build(
            KEYRING_SERVICE,
            KEYRING_KEY,
            None,
        )?
    };

    #[cfg(not(all(feature = "codesign", target_os = "macos")))]
    let entry = {
        use anyhow::Context;

        keyring_core::Entry::new_with_credential(std::sync::Arc::new(
            FileCredentialStore::new()
                .context("could not determine config directory for credential storage")?,
        ))
    };

    Ok(entry)
}

/// Load a stored device-flow token from the default credential store.
pub fn load_stored_token() -> Option<AccessTokenResponse> {
    let keyring = default_store().ok()?;
    let json = keyring.get_password().ok()?;
    let stored: StoredToken = serde_json::from_str(&json).ok()?;
    Some(AccessTokenResponse::from(stored))
}
