use crate::cli::Cli;
use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use git2::Cred;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

fn ssh_to_https_url(repo_url: &str) -> Option<String> {
    if repo_url.is_empty() || repo_url.len() < "git@github.com:a/b".len() {
        return None;
    }
    if !repo_url.starts_with("git@github.com:") {
        return None;
    }
    let repo_path = &repo_url["git@github.com:".len()..];
    if !is_valid_github_repo_path(repo_path) {
        return None;
    }
    Some(format!("https://github.com/{repo_path}"))
}

fn is_valid_github_repo_path(repo_path: &str) -> bool {
    if repo_path.is_empty() {
        return false;
    }
    let parts: Vec<&str> = repo_path.split('/').collect();
    if parts.len() != 2 {
        return false;
    }

    let (user_or_org, repo_name) = (parts[0], parts[1]);
    if user_or_org.is_empty() || repo_name.is_empty() {
        return false;
    }
    if !is_valid_github_username(user_or_org) {
        return false;
    }
    let repo_name_clean = repo_name.strip_suffix(".git").unwrap_or(repo_name);
    if !is_valid_github_repo_name(repo_name_clean) {
        return false;
    }

    true
}

fn is_valid_github_username(username: &str) -> bool {
    if username.is_empty() || username.len() > 39 {
        return false;
    }
    if username.starts_with('-') || username.ends_with('-') {
        return false;
    }
    username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_valid_github_repo_name(repo_name: &str) -> bool {
    if repo_name.is_empty() || repo_name.len() > 100 {
        return false;
    }

    if repo_name == "." || repo_name == ".." {
        return false;
    }

    repo_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
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
            &format!("Credentials callback for URL: {url}, username: {username_from_url:?}"),
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
                            &format!("SSH agent failed: {e}, trying default key"),
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
                        &format!("SSH clone failed: {e}, trying HTTPS: {https_url}"),
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
                            log(LogLevel::Error, &format!("HTTPS clone failed: {e}"));
                            Err(FeludaError::RepositoryClone(format!(
                                "Failed to clone repository: {e}"
                            )))
                        }
                    };
                }
            }
            log(LogLevel::Error, &format!("Failed to clone repository: {e}"));
            Err(FeludaError::RepositoryClone(format!(
                "Failed to clone repository: {e}"
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
    fn test_ssh_to_https_url_various_formats() {
        // Test standard GitHub SSH
        assert_eq!(
            ssh_to_https_url("git@github.com:user/repo.git"),
            Some("https://github.com/user/repo.git".to_string())
        );

        // Test without .git extension
        assert_eq!(
            ssh_to_https_url("git@github.com:user/repo"),
            Some("https://github.com/user/repo".to_string())
        );

        // Test with organization
        assert_eq!(
            ssh_to_https_url("git@github.com:organization/project.git"),
            Some("https://github.com/organization/project.git".to_string())
        );

        // Test HTTPS URL
        assert_eq!(ssh_to_https_url("https://github.com/user/repo.git"), None);

        // Test GitLab SSH
        assert_eq!(ssh_to_https_url("git@gitlab.com:user/repo.git"), None);

        // Test other SSH formats that aren't GitHub
        assert_eq!(ssh_to_https_url("git@bitbucket.org:user/repo.git"), None);
        assert_eq!(ssh_to_https_url("git@codeberg.org:user/repo.git"), None);

        // Test malformed SSH URL
        assert_eq!(ssh_to_https_url("invalid-ssh-format"), None);
        assert_eq!(ssh_to_https_url("git@github.com"), None);

        // These now return None with improved validation
        assert_eq!(ssh_to_https_url("git@github.com:"), None);

        // Test empty string
        assert_eq!(ssh_to_https_url(""), None);

        // Test with special characters in repo name
        assert_eq!(
            ssh_to_https_url("git@github.com:user/repo-with-dashes.git"),
            Some("https://github.com/user/repo-with-dashes.git".to_string())
        );

        assert_eq!(
            ssh_to_https_url("git@github.com:user/repo_name.git"),
            Some("https://github.com/user/repo_name.git".to_string())
        );

        // Test case sensitivity
        assert_eq!(ssh_to_https_url("git@GitHub.com:user/repo.git"), None);
        assert_eq!(ssh_to_https_url("git@GITHUB.COM:user/repo.git"), None);
    }

    #[test]
    fn test_validate_ssh_key_scenarios() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        // Test valid private key
        let private_key_path = temp_dir.path().join("id_rsa");
        std::fs::File::create(&private_key_path).unwrap();
        assert!(validate_ssh_key(&private_key_path).is_ok());

        // Test public key rejection
        let public_key_path = temp_dir.path().join("id_rsa.pub");
        std::fs::File::create(&public_key_path).unwrap();
        let result = validate_ssh_key(&public_key_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message()
            .contains("Public key provided"));

        // Test non-existent key
        let missing_key_path = temp_dir.path().join("missing_key");
        let result = validate_ssh_key(&missing_key_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message()
            .contains("SSH key file not found"));

        // Test different private key types
        let ed25519_key_path = temp_dir.path().join("id_ed25519");
        std::fs::File::create(&ed25519_key_path).unwrap();
        assert!(validate_ssh_key(&ed25519_key_path).is_ok());

        let ecdsa_key_path = temp_dir.path().join("id_ecdsa");
        std::fs::File::create(&ecdsa_key_path).unwrap();
        assert!(validate_ssh_key(&ecdsa_key_path).is_ok());

        // Test key with no extension
        let no_ext_key_path = temp_dir.path().join("ssh_key");
        std::fs::File::create(&no_ext_key_path).unwrap();
        assert!(validate_ssh_key(&no_ext_key_path).is_ok());

        // Test various public key extensions
        let pub_variations = [
            "key.pub",
            "id_rsa.pub",
            "id_ed25519.pub",
            "id_ecdsa.pub",
            "custom.pub",
        ];

        for pub_key_name in &pub_variations {
            let pub_key_path = temp_dir.path().join(pub_key_name);
            std::fs::File::create(&pub_key_path).unwrap();
            let result = validate_ssh_key(&pub_key_path);
            assert!(result.is_err());
            assert!(result
                .unwrap_err()
                .message()
                .contains("Public key provided"));
        }
    }

    #[test]
    fn test_clone_repository_error_handling() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        // Create CLI args with invalid repository
        let args = Cli {
            debug: false,
            command: None,
            path: "./".to_string(),
            repo: Some("invalid-repo-url".to_string()),
            token: None,
            ssh_key: None,
            ssh_passphrase: None,
            json: false,
            yaml: false,
            verbose: false,
            restrictive: false,
            gui: false,
            language: None,
            ci_format: None,
            output_file: None,
            fail_on_restrictive: false,
            incompatible: false,
            fail_on_incompatible: false,
            project_license: None,
            gist: false,
            osi: None,
            strict: false,
            no_local: false,
        };

        let result = clone_repository(&args, temp_dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_ssh_key_permissions() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        // Create a key file
        let key_path = temp_dir.path().join("test_key");
        std::fs::write(&key_path, "fake key content").unwrap();

        // Test that the file exists and validation passes
        assert!(validate_ssh_key(&key_path).is_ok());

        // Test validation after the file is deleted
        std::fs::remove_file(&key_path).unwrap();
        let result = validate_ssh_key(&key_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message()
            .contains("SSH key file not found"));
    }

    #[test]
    fn test_clone_repository_debug_mode() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let args = Cli {
            debug: true,
            command: None,
            path: "./".to_string(),
            repo: Some("https://github.com/nonexistent/repo.git".to_string()),
            token: None,
            ssh_key: None,
            ssh_passphrase: None,
            json: false,
            yaml: false,
            verbose: false,
            restrictive: false,
            gui: false,
            language: None,
            ci_format: None,
            output_file: None,
            fail_on_restrictive: false,
            incompatible: false,
            fail_on_incompatible: false,
            project_license: None,
            gist: false,
            osi: None,
            strict: false,
            no_local: false,
        };

        // Enable debug mode for this test
        crate::debug::set_debug_mode(true);

        let result = clone_repository(&args, temp_dir.path());
        assert!(result.is_err());

        // Reset debug mode
        crate::debug::set_debug_mode(false);
    }

    #[test]
    fn test_ssh_to_https_url_case_sensitivity() {
        // Test that the function handles case correctly
        assert_eq!(ssh_to_https_url("git@GitHub.com:user/repo.git"), None);

        assert_eq!(ssh_to_https_url("git@GITHUB.COM:user/repo.git"), None);

        // Test correct case
        assert_eq!(
            ssh_to_https_url("git@github.com:user/repo.git"),
            Some("https://github.com/user/repo.git".to_string())
        );
    }

    #[test]
    fn test_clone_repository_empty_repo_url() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let args = Cli {
            debug: false,
            command: None,
            path: "./".to_string(),
            repo: Some("".to_string()),
            token: None,
            ssh_key: None,
            ssh_passphrase: None,
            json: false,
            yaml: false,
            verbose: false,
            restrictive: false,
            gui: false,
            language: None,
            ci_format: None,
            output_file: None,
            fail_on_restrictive: false,
            incompatible: false,
            fail_on_incompatible: false,
            project_license: None,
            gist: false,
            osi: None,
            strict: false,
            no_local: false,
        };

        let result = clone_repository(&args, temp_dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_ssh_to_https_url_validation() {
        // Test valid cases
        assert_eq!(
            ssh_to_https_url("git@github.com:microsoft/vscode.git"),
            Some("https://github.com/microsoft/vscode.git".to_string())
        );

        assert_eq!(
            ssh_to_https_url("git@github.com:user-name/repo-name.git"),
            Some("https://github.com/user-name/repo-name.git".to_string())
        );

        assert_eq!(
            ssh_to_https_url("git@github.com:user123/repo_name.config.git"),
            Some("https://github.com/user123/repo_name.config.git".to_string())
        );

        // Test invalid usernames (start/end with hyphen)
        assert_eq!(ssh_to_https_url("git@github.com:-user/repo.git"), None);
        assert_eq!(ssh_to_https_url("git@github.com:user-/repo.git"), None);

        // Test invalid repository names
        assert_eq!(ssh_to_https_url("git@github.com:user/.git"), None);
        assert_eq!(ssh_to_https_url("git@github.com:user/..git"), None);

        // Test too many path components
        assert_eq!(ssh_to_https_url("git@github.com:user/repo/extra"), None);

        // Test very long usernames/repos
        let long_username = "a".repeat(40);
        assert_eq!(
            ssh_to_https_url(&format!("git@github.com:{long_username}/repo.git")),
            None
        );

        let long_repo = "a".repeat(101);
        assert_eq!(
            ssh_to_https_url(&format!("git@github.com:user/{long_repo}.git")),
            None
        );

        // Test empty components
        assert_eq!(ssh_to_https_url("git@github.com:/repo.git"), None);
        assert_eq!(ssh_to_https_url("git@github.com:user/"), None);
    }

    #[test]
    fn test_ssh_to_https_url_special_characters() {
        // Valid special characters in repository names
        assert_eq!(
            ssh_to_https_url("git@github.com:user/my-project.js.git"),
            Some("https://github.com/user/my-project.js.git".to_string())
        );

        assert_eq!(
            ssh_to_https_url("git@github.com:user/config_file.json"),
            Some("https://github.com/user/config_file.json".to_string())
        );

        // Invalid characters in usernames
        assert_eq!(
            ssh_to_https_url("git@github.com:user-name/repo.git"),
            Some("https://github.com/user-name/repo.git".to_string())
        );

        // underscores in usernames should be invalid
        assert_eq!(ssh_to_https_url("git@github.com:user_name/repo.git"), None);

        // Invalid characters
        assert_eq!(ssh_to_https_url("git@github.com:user@name/repo.git"), None);
        assert_eq!(ssh_to_https_url("git@github.com:user/repo@name.git"), None);
        assert_eq!(ssh_to_https_url("git@github.com:user name/repo.git"), None);
    }
}
