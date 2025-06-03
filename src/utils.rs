use crate::cli::Cli;
use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use git2::Cred;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

fn ssh_to_https_url(repo_url: &str) -> Option<String> {
    if repo_url.starts_with("git@github.com:") {
        let repo_path = repo_url.trim_start_matches("git@github.com:");
        Some(format!("https://github.com/{}", repo_path))
    } else {
        None
    }
}

fn validate_ssh_key(key_path: &Path) -> Result<(), git2::Error> {
    if !key_path.exists() {
        log(
            LogLevel::Error,
            &format!("SSH key file not found: {}", key_path.display()),
        );
        return Err(git2::Error::from_str("SSH key file not found"));
    }
    if key_path
        .extension()
        .map(|ext| ext == "pub")
        .unwrap_or(false)
    {
        log(
            LogLevel::Error,
            &format!(
                "Invalid SSH key: {} is a public key (.pub)",
                key_path.display()
            ),
        );
        return Err(git2::Error::from_str(
            "Public key provided instead of private key",
        ));
    }
    Ok(())
}

pub fn clone_repository(args: &Cli, dest_path: &Path) -> FeludaResult<()> {
    let token = &args.token;
    let ssh_key = &args.ssh_key;
    let ssh_passphrase = &args.ssh_passphrase;
    let repo_url = &args.repo.as_deref().unwrap();

    log(
        LogLevel::Info,
        &format!(
            "Initializing clone of {} to {}",
            repo_url,
            dest_path.display()
        ),
    );

    let auth_attempts = AtomicUsize::new(0);
    const MAX_AUTH_ATTEMPTS: usize = 5;

    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(|url, username_from_url, allowed_types| {
        let attempts = auth_attempts.fetch_add(1, Ordering::SeqCst);
        if attempts >= MAX_AUTH_ATTEMPTS {
            log(LogLevel::Error, "Max authentication attempts reached");
            return Err(git2::Error::from_str("Too many authentication attempts"));
        }

        log(
            LogLevel::Info,
            &format!(
                "Credentials callback for URL: {}, username: {:?}",
                url, username_from_url
            ),
        );
        if allowed_types.is_ssh_key() {
            log(LogLevel::Info, "Attempting SSH authentication");

            if let Some(key_path) = ssh_key {
                let key_path = Path::new(&key_path);
                validate_ssh_key(key_path)?;
                log(
                    LogLevel::Info,
                    &format!("Using custom SSH key at: {}", key_path.display()),
                );
                Cred::ssh_key(
                    username_from_url.unwrap_or("git"),
                    None,
                    key_path,
                    ssh_passphrase.as_deref(),
                )
            } else {
                log(LogLevel::Info, "Trying SSH agent");
                match Cred::ssh_key_from_agent(username_from_url.unwrap_or("git")) {
                    Ok(cred) => {
                        log(LogLevel::Info, "Using SSH agent credentials");
                        Ok(cred)
                    }
                    Err(e) => {
                        log(
                            LogLevel::Warn,
                            &format!("SSH agent failed: {}, trying default key", e),
                        );
                        Err(e)
                    }
                }
            }
        } else if allowed_types.is_user_pass_plaintext() && token.is_some() {
            log(LogLevel::Info, "Using HTTPS token authentication");
            Cred::userpass_plaintext("x-access-token", token.as_deref().unwrap())
        } else {
            log(LogLevel::Info, "Using default credentials for HTTPS");
            Cred::default()
        }
    });

    let mut fetch_options = git2::FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_options);

    log(
        LogLevel::Info,
        &format!("Cloning {} into {}", repo_url, dest_path.display()),
    );
    match builder.clone(repo_url, dest_path) {
        Ok(_) => {
            log(LogLevel::Info, "Clone successful");
            Ok(())
        }
        Err(e) => {
            if repo_url.starts_with("git@") {
                if let Some(https_url) = ssh_to_https_url(repo_url) {
                    log(
                        LogLevel::Warn,
                        &format!("SSH clone failed: {}, trying HTTPS: {}", e, https_url),
                    );
                    let mut https_callbacks = git2::RemoteCallbacks::new();
                    https_callbacks.credentials(|_url, _username, allowed_types| {
                        if allowed_types.is_user_pass_plaintext() && token.is_some() {
                            log(LogLevel::Info, "Using HTTPS token authentication");
                            Cred::userpass_plaintext("x-access-token", token.as_deref().unwrap())
                        } else {
                            log(LogLevel::Info, "Using default credentials for HTTPS");
                            Cred::default()
                        }
                    });
                    let mut https_fetch_options = git2::FetchOptions::new();
                    https_fetch_options.remote_callbacks(https_callbacks);
                    let mut https_builder = git2::build::RepoBuilder::new();
                    https_builder.fetch_options(https_fetch_options);

                    log(
                        LogLevel::Info,
                        &format!("Cloning {} into {}", https_url, dest_path.display()),
                    );
                    return match https_builder.clone(&https_url, dest_path) {
                        Ok(_) => {
                            log(LogLevel::Info, "HTTPS clone successful");
                            Ok(())
                        }
                        Err(e) => {
                            log(LogLevel::Error, &format!("HTTPS clone failed: {}", e));
                            Err(FeludaError::Unknown(format!(
                                "Failed to clone repository: {}",
                                e
                            )))
                        }
                    };
                }
            }
            log(
                LogLevel::Error,
                &format!("Failed to clone repository: {}", e),
            );
            Err(FeludaError::Unknown(format!(
                "Failed to clone repository: {}",
                e
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    #[test]
    fn test_ssh_to_https_url_github_ssh() {
        let url = "git@github.com:anistark/feluda.git";
        let result = ssh_to_https_url(url);
        assert_eq!(
            result,
            Some("https://github.com/anistark/feluda.git".to_string())
        );
    }

    #[test]
    fn test_ssh_to_https_url_non_github() {
        let url = "git@gitlab.com:user/repo.git";
        let result = ssh_to_https_url(url);
        assert_eq!(result, None);
    }

    #[test]
    fn test_ssh_to_https_url_non_ssh() {
        let url = "https://github.com/anistark/feluda.git";
        let result = ssh_to_https_url(url);
        assert_eq!(result, None);
    }

    #[test]
    fn test_validate_ssh_key_exists() {
        let temp_dir = TempDir::new().unwrap();
        let key_path = temp_dir.path().join("id_rsa");
        File::create(&key_path).unwrap();

        let result = validate_ssh_key(&key_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_ssh_key_not_exists() {
        let temp_dir = TempDir::new().unwrap();
        let key_path = temp_dir.path().join("id_rsa");

        let result = validate_ssh_key(&key_path);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "SSH key file not found");
    }

    #[test]
    fn test_validate_ssh_key_public_key() {
        let temp_dir = TempDir::new().unwrap();
        let key_path = temp_dir.path().join("id_rsa.pub");
        File::create(&key_path).unwrap();

        let result = validate_ssh_key(&key_path);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Public key provided instead of private key"
        );
    }

    #[test]
    fn test_clone_repository_ok() {
        let temp_dir = TempDir::new().unwrap();
        let repo_url = "git@github.com:anistark/feluda.git";
        let dest_path = temp_dir.path().join("repo");
        let key_dir = TempDir::new().unwrap();
        let ssh_key_path = key_dir.path().join("id_rsa");
        File::create(&ssh_key_path).unwrap();
        let ssh_key = ssh_key_path.to_str().unwrap();
        let passphrase = "test-passphrase";

        let cli = Cli {
            path: "./".to_string(),
            repo: Some(repo_url.to_string()),
            token: None,
            ssh_key: Some(ssh_key.to_string()),
            ssh_passphrase: Some(passphrase.to_string()),
            json: false,
            yaml: false,
            verbose: false,
            strict: false,
            gui: false,
            debug: false,
            language: None,
            ci_format: None,
            output_file: None,
            fail_on_restrictive: false,
            incompatible: false,
            fail_on_incompatible: false,
            project_license: None,
        };

        let result = clone_repository(&cli, dest_path.as_path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_clone_repository_ssh_key_with_passphrase() {
        let temp_dir = TempDir::new().unwrap();
        let repo_url = "git@github.com:anistark/nonexistent.git";
        let dest_path = temp_dir.path().join("repo");
        let key_dir = TempDir::new().unwrap();
        let ssh_key_path = key_dir.path().join("id_rsa");
        File::create(&ssh_key_path).unwrap();
        let ssh_key = ssh_key_path.to_str().unwrap();
        let passphrase = "test-passphrase";

        let cli = Cli {
            path: "./".to_string(),
            repo: Some(repo_url.to_string()),
            token: None,
            ssh_key: Some(ssh_key.to_string()),
            ssh_passphrase: Some(passphrase.to_string()),
            json: false,
            yaml: false,
            verbose: false,
            strict: false,
            gui: false,
            debug: false,
            language: None,
            ci_format: None,
            output_file: None,
            fail_on_restrictive: false,
            incompatible: false,
            fail_on_incompatible: false,
            project_license: None,
        };

        let result = clone_repository(&cli, dest_path.as_path());
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("Failed to clone repository")
                || error.contains("authentication required but no callback set"),
            "Unexpected error: {}",
            error
        );
        assert!(!dest_path.exists() || std::fs::read_dir(&dest_path).unwrap().next().is_none());
    }

    #[test]
    fn test_clone_repository_https_with_token() {
        let temp_dir = TempDir::new().unwrap();
        let repo_url = "https://github.com/anistark/nonexistent.git";
        let dest_path = temp_dir.path().join("repo");
        let token = "ghp_testtoken";

        let cli = Cli {
            path: "./".to_string(),
            repo: Some(repo_url.to_string()),
            token: Some(token.to_string()),
            ssh_key: None,
            ssh_passphrase: None,
            json: false,
            yaml: false,
            verbose: false,
            strict: false,
            gui: false,
            debug: false,
            language: None,
            ci_format: None,
            output_file: None,
            fail_on_restrictive: false,
            incompatible: false,
            fail_on_incompatible: false,
            project_license: None,
        };

        let result = clone_repository(&cli, dest_path.as_path());

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("Failed to clone repository: Too many authentication attempts"),
            "Unexpected error: {}",
            error
        );
        assert!(!dest_path.exists() || std::fs::read_dir(&dest_path).unwrap().next().is_none());
    }

    #[test]
    fn test_clone_repository_max_auth_attempts() {
        let temp_dir = TempDir::new().unwrap();
        let repo_url = "git@github.com:anistark/nonexistent.git";
        let dest_path = temp_dir.path().join("repo");
        let token = "ghp_testtoken";

        let cli = Cli {
            path: "./".to_string(),
            repo: Some(repo_url.to_string()),
            token: Some(token.to_string()),
            ssh_key: None,
            ssh_passphrase: None,
            json: false,
            yaml: false,
            verbose: false,
            strict: false,
            gui: false,
            debug: false,
            language: None,
            ci_format: None,
            output_file: None,
            fail_on_restrictive: false,
            incompatible: false,
            fail_on_incompatible: false,
            project_license: None,
        };

        let result = clone_repository(&cli, dest_path.as_path());

        assert!(result.is_err());
        assert!(!dest_path.exists() || std::fs::read_dir(&dest_path).unwrap().next().is_none());
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("Failed to clone repository")
                || error.contains("Too many authentication attempts")
                || error.contains("invalid privatekey")
                || error.contains("authentication required but no callback set"),
            "Unexpected error: {}",
            error
        );
        assert!(!dest_path.exists() || std::fs::read_dir(&dest_path).unwrap().next().is_none());
    }
}
