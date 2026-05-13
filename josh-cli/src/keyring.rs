use anyhow::Context;

#[cfg(not(all(feature = "codesign", target_os = "macos")))]
pub(crate) mod file_store {
    use keyring::credential::CredentialApi;
    use std::any::Any;
    use std::path::PathBuf;

    /// File-based credential store implementing [`CredentialApi`].
    ///
    /// Stores a JSON blob at `~/.config/josh/credentials.json` with `0600`
    /// permissions on Unix. Used as the default backend when the `codesign`
    /// feature is not enabled.
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
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| keyring::Error::PlatformFailure(Box::new(e)))?;
            }

            std::fs::write(&self.path, secret)
                .map_err(|e| keyring::Error::PlatformFailure(Box::new(e)))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))
                    .map_err(|e| keyring::Error::PlatformFailure(Box::new(e)))?;
            }

            Ok(())
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            std::fs::read(&self.path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    keyring::Error::NoEntry
                } else {
                    keyring::Error::PlatformFailure(Box::new(e))
                }
            })
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(keyring::Error::NoEntry),
                Err(e) => Err(keyring::Error::PlatformFailure(Box::new(e))),
            }
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }
}

/// Return the default credential store for the current build configuration.
pub fn default_store() -> anyhow::Result<keyring::Entry> {
    #[cfg(all(feature = "codesign", target_os = "macos"))]
    let credential = {
        const KEYRING_SERVICE: &str = "josh-cli";
        const KEYRING_KEY: &str = "github:credentials";

        Box::new(keyring::macos::MacCredential::new_with_target(
            None,
            KEYRING_SERVICE,
            KEYRING_KEY,
        )?)
    };

    #[cfg(not(all(feature = "codesign", target_os = "macos")))]
    let credential = {
        Box::new(
            file_store::FileCredentialStore::new()
                .context("could not determine config directory for credential storage")?,
        )
    };

    Ok(keyring::Entry::new_with_credential(credential))
}
