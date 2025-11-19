use crate::cli::with_spinner;
use crate::debug::{log, log_debug, LogLevel};
use crate::licenses::{
    detect_project_license, is_license_compatible, LicenseCompatibility, LicenseInfo,
};
use crate::parser::parse_root;
use colored::*;
use reqwest::blocking::Client;
use std::fs;
use std::io::{self, Write};
use std::io::{stdin, Read};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::time::Duration;

/// Key input handling for cross-platform compatibility
#[derive(Debug, PartialEq)]
enum KeyInput {
    Up,
    Down,
    Enter,
    Escape,
    Char(char),
    Unknown,
}

/// Enable raw terminal mode (For Unix)
#[cfg(unix)]
fn enable_raw_mode() -> std::io::Result<()> {
    let fd = stdin().as_raw_fd();
    let mut termios = unsafe { std::mem::zeroed() };

    unsafe {
        if libc::tcgetattr(fd, &mut termios) != 0 {
            return Err(std::io::Error::last_os_error());
        }

        termios.c_lflag &= !(libc::ICANON | libc::ECHO);
        termios.c_cc[libc::VMIN] = 1;
        termios.c_cc[libc::VTIME] = 0;

        if libc::tcsetattr(fd, libc::TCSANOW, &termios) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }

    Ok(())
}

/// Disable raw terminal mode (For Unix)
#[cfg(unix)]
fn disable_raw_mode() -> std::io::Result<()> {
    let fd = stdin().as_raw_fd();
    let mut termios = unsafe { std::mem::zeroed() };

    unsafe {
        if libc::tcgetattr(fd, &mut termios) != 0 {
            return Err(std::io::Error::last_os_error());
        }

        termios.c_lflag |= libc::ICANON | libc::ECHO;

        if libc::tcsetattr(fd, libc::TCSANOW, &termios) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }

    Ok(())
}

/// Windows raw mode
#[cfg(windows)]
fn enable_raw_mode() -> std::io::Result<()> {
    // TODO: Use Windows Console API
    Ok(())
}

/// Disable raw mode for Windows
#[cfg(windows)]
fn disable_raw_mode() -> std::io::Result<()> {
    // TODO: Pending implementation
    Ok(())
}

/// Read a key press
fn read_key() -> std::io::Result<KeyInput> {
    let mut buffer = [0; 4];
    let mut stdin = stdin();

    stdin.read_exact(&mut buffer[0..1])?;

    match buffer[0] {
        // Enter key
        b'\r' | b'\n' => Ok(KeyInput::Enter),

        // Escape key or escape sequence
        27 => {
            let mut temp_buffer = [0; 2];
            match stdin.read(&mut temp_buffer) {
                Ok(2) if temp_buffer[0] == b'[' => {
                    // ANSI escape sequence
                    match temp_buffer[1] {
                        b'A' => Ok(KeyInput::Up),   // Up arrow
                        b'B' => Ok(KeyInput::Down), // Down arrow
                        _ => Ok(KeyInput::Escape),
                    }
                }
                _ => Ok(KeyInput::Escape),
            }
        }

        // Regular characters
        b'q' | b'Q' => Ok(KeyInput::Escape), // q for quit
        b'k' | b'K' => Ok(KeyInput::Up),     // k for up (vim)
        b'j' | b'J' => Ok(KeyInput::Down),   // j for down (vim)

        // Printable ASCII
        c if (32..=126).contains(&c) => Ok(KeyInput::Char(c as char)),

        _ => Ok(KeyInput::Unknown),
    }
}

/// Clear screen and move cursor to top
fn clear_screen() {
    print!("\x1B[2J\x1B[H");
    io::stdout().flush().unwrap();
}

/// Hide the cursor
fn hide_cursor() {
    print!("\x1B[?25l");
    io::stdout().flush().unwrap();
}

/// Show the cursor
fn show_cursor() {
    print!("\x1B[?25h");
    io::stdout().flush().unwrap();
}

/// Generate options
#[derive(Debug, Clone, Copy)]
pub enum GenerateOption {
    Notice,
    ThirdPartyLicenses,
}

impl GenerateOption {
    /// Get the display name for the option
    pub fn display_name(&self) -> &'static str {
        match self {
            GenerateOption::Notice => "NOTICE file",
            GenerateOption::ThirdPartyLicenses => "THIRD_PARTY_LICENSES file",
        }
    }

    /// Get the filename
    pub fn filename(&self) -> &'static str {
        match self {
            GenerateOption::Notice => "NOTICE",
            GenerateOption::ThirdPartyLicenses => "THIRD_PARTY_LICENSES",
        }
    }

    /// Get the file extension
    pub fn extension(&self) -> &'static str {
        match self {
            GenerateOption::Notice => "",
            GenerateOption::ThirdPartyLicenses => ".md",
        }
    }

    /// Full filename with extension
    pub fn full_filename(&self) -> String {
        format!("{}{}", self.filename(), self.extension())
    }
}

/// Check if a file exists for the given option
pub fn file_exists(option: GenerateOption, path: &str) -> bool {
    let file_path = Path::new(path).join(option.full_filename());
    let exists = file_path.exists();

    log(
        LogLevel::Info,
        &format!(
            "Checking if {} exists at {}: {}",
            option.full_filename(),
            file_path.display(),
            exists
        ),
    );

    exists
}

/// Display interactive menu with real arrow key navigation
pub fn show_interactive_menu(path: &str) -> Option<GenerateOption> {
    let options = [GenerateOption::Notice, GenerateOption::ThirdPartyLicenses];
    let mut selected_index = 0;
    let raw_mode_available = enable_raw_mode().is_ok();

    if raw_mode_available {
        hide_cursor();
    }

    let cleanup = || {
        if raw_mode_available {
            show_cursor();
            let _ = disable_raw_mode();
        }
    };

    loop {
        if raw_mode_available {
            clear_screen();
        } else {
            for _ in 0..3 {
                println!();
            }
        }

        println!("{}", "📝 File Generation Options".bold().blue());
        println!("{}", "─".repeat(50).blue());
        println!();

        for (index, option) in options.iter().enumerate() {
            let action = if file_exists(*option, path) {
                "Update".yellow()
            } else {
                "Generate".green()
            };

            let indicator = if index == selected_index {
                "▶".bold().cyan()
            } else {
                " ".normal()
            };

            let line_content = format!("{}. {} {}", index + 1, action, option.display_name());

            if index == selected_index {
                println!("{} {}", indicator, line_content.bold().on_bright_black());
            } else {
                println!("{indicator} {line_content}");
            }
        }

        let cancel_content = format!("0. {}", "Cancel".red());
        if selected_index == options.len() {
            println!(
                "{} {}",
                "▶".bold().cyan(),
                cancel_content.bold().on_bright_black()
            );
        } else {
            println!("  {cancel_content}");
        }

        println!();
        if raw_mode_available {
            println!(
                "{}",
                "Use ↑/↓ arrows to navigate, Enter to select, q/Esc to cancel".dimmed()
            );
        } else {
            println!("{}", "Type 1, 2, or 0 to select, or q to cancel:".dimmed());
            print!("> ");
            io::stdout().flush().unwrap();
        }

        // Read input
        if raw_mode_available {
            // key input
            match read_key() {
                Ok(KeyInput::Up) => {
                    if selected_index > 0 {
                        selected_index -= 1;
                    } else {
                        selected_index = options.len(); // to Cancel option
                    }
                }
                Ok(KeyInput::Down) => {
                    if selected_index < options.len() {
                        selected_index += 1;
                    } else {
                        selected_index = 0; // to first option
                    }
                }
                Ok(KeyInput::Enter) => {
                    cleanup();
                    if selected_index < options.len() {
                        log(
                            LogLevel::Info,
                            &format!("User selected option: {:?}", options[selected_index]),
                        );
                        return Some(options[selected_index]);
                    } else {
                        println!("\n{}", "✋ Operation cancelled.".yellow());
                        return None;
                    }
                }
                Ok(KeyInput::Escape) => {
                    cleanup();
                    println!("\n{}", "✋ Operation cancelled.".yellow());
                    return None;
                }
                Ok(KeyInput::Char('1')) => {
                    cleanup();
                    log(LogLevel::Info, "User selected option 1 (NOTICE)");
                    return Some(GenerateOption::Notice);
                }
                Ok(KeyInput::Char('2')) => {
                    cleanup();
                    log(
                        LogLevel::Info,
                        "User selected option 2 (THIRD_PARTY_LICENSES)",
                    );
                    return Some(GenerateOption::ThirdPartyLicenses);
                }
                Ok(KeyInput::Char('0')) => {
                    cleanup();
                    println!("\n{}", "✋ Operation cancelled.".yellow());
                    return None;
                }
                Ok(KeyInput::Char('h') | KeyInput::Char('?')) => {
                    // Show help
                    clear_screen();
                    println!("\n{}", "📚 Help - Navigation Commands".bold().blue());
                    println!("{}", "─".repeat(40).blue());
                    println!("  {} Move selection up", "↑ Arrow or k".cyan());
                    println!("  {} Move selection down", "↓ Arrow or j".cyan());
                    println!("  {} Select current option", "Enter".green());
                    println!("  {} Quick select options", "1, 2, 0".yellow());
                    println!("  {} Cancel and exit", "q or Esc".red());
                    println!("  {} Show this help", "h or ?".blue());
                    println!("\nPress any key to continue...");
                    let _ = read_key();
                }
                Ok(_) | Err(_) => {
                    // Invalid key, continue loop
                    continue;
                }
            }
        } else {
            // Fallback to line-based input
            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(_) => {
                    let choice = input.trim().to_lowercase();
                    log(LogLevel::Info, &format!("User input: '{choice}'"));

                    match choice.as_str() {
                        "0" => {
                            println!("{}", "✋ Operation cancelled.".yellow());
                            return None;
                        }
                        "1" => {
                            log(LogLevel::Info, "User selected option 1 (NOTICE)");
                            return Some(GenerateOption::Notice);
                        }
                        "2" => {
                            log(
                                LogLevel::Info,
                                "User selected option 2 (THIRD_PARTY_LICENSES)",
                            );
                            return Some(GenerateOption::ThirdPartyLicenses);
                        }
                        "q" | "quit" | "exit" => {
                            println!("{}", "✋ Operation cancelled.".yellow());
                            return None;
                        }
                        _ => {
                            println!("{} Invalid input. Please use 1, 2, 0, or q.", "❌".red());
                            println!("Press Enter to continue...");
                            let mut _dummy = String::new();
                            let _ = io::stdin().read_line(&mut _dummy);
                        }
                    }
                }
                Err(_) => {
                    println!("{} Error reading input.", "❌".red());
                    let mut _dummy = String::new();
                    let _ = io::stdin().read_line(&mut _dummy);
                }
            }
        }
    }
}

/// Generate or update a NOTICE file
pub fn generate_notice_file(license_data: &[LicenseInfo], path: &str) {
    let file_path = Path::new(path).join(GenerateOption::Notice.full_filename());
    let exists = file_exists(GenerateOption::Notice, path);

    let action = if exists { "Updating" } else { "Generating" };

    log(
        LogLevel::Info,
        &format!(
            "{} NOTICE file at {} with {} dependencies",
            action,
            file_path.display(),
            license_data.len()
        ),
    );

    log_debug("License data for NOTICE file", &license_data);

    println!(
        "{} {} NOTICE file at {}...",
        "📄".bold(),
        action.green().bold(),
        file_path.display().to_string().blue()
    );

    // Generate NOTICE content
    let notice_content = generate_notice_content(license_data);

    // Write to file
    match fs::write(&file_path, notice_content) {
        Ok(_) => {
            println!(
                "{} NOTICE file generated successfully!",
                "✅".green().bold()
            );
            println!("   📍 Location: {}", file_path.display().to_string().blue());
        }
        Err(err) => {
            println!("{} Failed to write NOTICE file: {}", "❌".red().bold(), err);
            log(
                LogLevel::Error,
                &format!("Failed to write NOTICE file: {err}"),
            );
        }
    }
}

/// Generate the content for a NOTICE file
fn generate_notice_content(license_data: &[LicenseInfo]) -> String {
    let mut content = String::new();

    // Header
    content.push_str("NOTICE\n");
    content.push_str("======\n\n");
    content.push_str("This project includes third-party software components that are subject to separate copyright notices and license terms.\n");
    content.push_str("Your use of the source code for these components is subject to the terms and conditions of the following licenses.\n\n");

    // Group dependencies by license
    let mut license_groups: std::collections::HashMap<String, Vec<&LicenseInfo>> =
        std::collections::HashMap::new();

    for info in license_data {
        let license_key = info.get_license();
        license_groups.entry(license_key).or_default().push(info);
    }

    // Sort license groups
    let mut sorted_licenses: Vec<_> = license_groups.iter().collect();
    sorted_licenses.sort_by_key(|(license, _)| license.as_str());

    for (license, dependencies) in sorted_licenses {
        content.push_str(&format!("## {license} Licensed Components\n\n"));

        // Sort dependencies within each license group
        let mut sorted_deps = dependencies.clone();
        sorted_deps.sort_by_key(|dep| &dep.name);

        for dep in sorted_deps {
            content.push_str(&format!("* {} ({})\n", dep.name, dep.version));
        }
        content.push('\n');
    }

    // Footer
    content.push_str("---\n\n");
    content.push_str(&format!(
        "This NOTICE file contains {} third-party dependencies.\n",
        license_data.len()
    ));
    content.push_str("For detailed license information, see the THIRD_PARTY_LICENSES file.\n");
    content.push_str(&format!(
        "Generated at: {}\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    ));
    content.push_str("Generated by: Feluda (https://github.com/anistark/feluda)\n\n");

    // Feluda disclaimer. Once Feluda is stable, this can be updated.
    // TODO: Get it reviewed by legal counsel
    content.push_str("DISCLAIMER:\n");
    content.push_str("-----------\n");
    content.push_str("Feluda is still in early stages!\n");
    content.push_str("The license information may be incomplete, outdated, or incorrect. Users are responsible for:\n");
    content.push_str("• Verifying the accuracy of all license information\n");
    content.push_str("• Ensuring compliance with all applicable license terms\n");
    content.push_str("• Consulting with legal counsel for license compliance matters\n");
    content.push_str("• Checking the official package repositories for the most up-to-date license information\n\n");
    content.push_str("Feluda and its contributors disclaim all warranties and are not liable for any legal issues\n");
    content.push_str("arising from the use of this information. Use at your own risk.\n");

    content
}

/// Generate or update a THIRD_PARTY_LICENSES file
pub fn generate_third_party_licenses_file(license_data: &[LicenseInfo], path: &str) {
    let file_path = Path::new(path).join(GenerateOption::ThirdPartyLicenses.full_filename());
    let exists = file_exists(GenerateOption::ThirdPartyLicenses, path);

    let action = if exists { "Updating" } else { "Generating" };

    log(
        LogLevel::Info,
        &format!(
            "{} THIRD_PARTY_LICENSES file at {} with {} dependencies",
            action,
            file_path.display(),
            license_data.len()
        ),
    );

    log_debug("License data for THIRD_PARTY_LICENSES file", &license_data);

    println!(
        "{} {} THIRD_PARTY_LICENSES file at {}...",
        "📜".bold(),
        action.green().bold(),
        file_path.display().to_string().blue()
    );

    // Generate THIRD_PARTY_LICENSES content
    let (licenses_content, fetch_stats) = with_spinner(
        &format!(
            "Fetching license content for {} dependencies",
            license_data.len()
        ),
        |indicator| generate_third_party_licenses_content(license_data, indicator),
    );

    // Write to file
    match fs::write(&file_path, licenses_content) {
        Ok(_) => {
            println!(
                "{} THIRD_PARTY_LICENSES file generated successfully!",
                "✅".green().bold()
            );
            println!("   📍 Location: {}", file_path.display().to_string().blue());
            println!(
                "   📊 Dependencies: {}",
                license_data.len().to_string().cyan()
            );

            // Display license fetching statistics
            let (successfully_fetched, failed_to_fetch) = fetch_stats;
            println!(
                "   📄 Actual license texts fetched: {} ({:.1}%)",
                successfully_fetched.to_string().green(),
                (successfully_fetched as f64 / license_data.len() as f64) * 100.0
            );

            if failed_to_fetch > 0 {
                println!(
                    "   ⚠️  License texts not fetched: {} ({:.1}%)",
                    failed_to_fetch.to_string().yellow(),
                    (failed_to_fetch as f64 / license_data.len() as f64) * 100.0
                );
                println!(
                    "      {}",
                    "Templates or generic references used for these dependencies.".dimmed()
                );
            }
        }
        Err(err) => {
            println!(
                "{} Failed to write THIRD_PARTY_LICENSES file: {}",
                "❌".red().bold(),
                err
            );
            log(
                LogLevel::Error,
                &format!("Failed to write THIRD_PARTY_LICENSES file: {err}"),
            );
        }
    }
}

/// HTTP client for API requests
fn create_http_client() -> Option<Client> {
    Client::builder()
        .user_agent("feluda-license-checker/1.0")
        .timeout(Duration::from_secs(10))
        .build()
        .ok()
}

/// Rate limit delay to avoid hitting API limits
fn rate_limit_delay() {
    std::thread::sleep(Duration::from_millis(500));
}

/// Fetch the actual license content for a dependency
fn fetch_actual_license_content(name: &str, version: &str) -> Option<String> {
    log(
        LogLevel::Info,
        &format!("Attempting to fetch actual license content for {name} v{version}"),
    );

    // Fetch from crates.io for Rust packages
    if let Some(content) = fetch_license_from_crates_io(name, version) {
        return Some(content);
    }

    // Fetch from npm for Node.js packages
    if let Some(content) = fetch_license_from_npm(name, version) {
        return Some(content);
    }

    // Fetch from PyPI for Python packages
    if let Some(content) = fetch_license_from_pypi(name, version) {
        return Some(content);
    }

    // Fetch from Go proxy for Go modules
    if let Some(content) = fetch_license_from_go_proxy(name, version) {
        return Some(content);
    }

    // Fetch from GitHub if we can infer the repository
    if let Some(content) = fetch_license_from_github(name, version) {
        return Some(content);
    }

    log(
        LogLevel::Warn,
        &format!("Could not fetch actual license content for {name} v{version}"),
    );
    None
}

/// Fetch license content from crates.io
fn fetch_license_from_crates_io(name: &str, version: &str) -> Option<String> {
    log(
        LogLevel::Info,
        &format!("Trying to fetch license from crates.io for {name} v{version}"),
    );

    let client = create_http_client()?;
    rate_limit_delay();

    let api_url = format!("https://crates.io/api/v1/crates/{name}");
    let response = client.get(&api_url).send().ok()?;

    if !response.status().is_success() {
        log(
            LogLevel::Warn,
            &format!(
                "Failed to fetch crate info from crates.io: HTTP {}",
                response.status()
            ),
        );
        return None;
    }

    let crate_info: serde_json::Value = response.json().ok()?;

    let repository = crate_info.get("crate")?.get("repository")?.as_str()?;

    log(
        LogLevel::Info,
        &format!("Found repository for {name}: {repository}"),
    );

    if repository.contains("github.com") {
        return fetch_license_from_github_repo(repository);
    }

    None
}

/// Fetch license content from npm
fn fetch_license_from_npm(name: &str, version: &str) -> Option<String> {
    log(
        LogLevel::Info,
        &format!("Trying to fetch license from npm for {name} v{version}"),
    );

    let client = create_http_client()?;
    rate_limit_delay();

    let api_url = format!("https://registry.npmjs.org/{name}/{version}");
    let response = client.get(&api_url).send().ok()?;

    if !response.status().is_success() {
        log(
            LogLevel::Warn,
            &format!(
                "Failed to fetch package info from npm: HTTP {}",
                response.status()
            ),
        );
        return None;
    }

    let package_info: serde_json::Value = response.json().ok()?;

    if let Some(repository) = package_info.get("repository") {
        if let Some(url) = repository.get("url").and_then(|u| u.as_str()) {
            log(
                LogLevel::Info,
                &format!("Found repository for {name}: {url}"),
            );

            let clean_url = url
                .trim_start_matches("git+")
                .trim_end_matches(".git")
                .replace("git://", "https://");

            if clean_url.contains("github.com") {
                return fetch_license_from_github_repo(&clean_url);
            }
        }
    }

    None
}

/// Fetch license content from PyPI
fn fetch_license_from_pypi(name: &str, version: &str) -> Option<String> {
    log(
        LogLevel::Info,
        &format!("Trying to fetch license from PyPI for {name} v{version}"),
    );

    let client = create_http_client()?;
    rate_limit_delay();

    let api_url = format!("https://pypi.org/pypi/{name}/{version}/json");
    let response = client.get(&api_url).send().ok()?;

    if !response.status().is_success() {
        log(
            LogLevel::Warn,
            &format!(
                "Failed to fetch package info from PyPI: HTTP {}",
                response.status()
            ),
        );
        return None;
    }

    let package_info: serde_json::Value = response.json().ok()?;

    if let Some(project_urls) = package_info.get("info").and_then(|i| i.get("project_urls")) {
        if let Some(homepage) = project_urls.get("Homepage").and_then(|h| h.as_str()) {
            log(
                LogLevel::Info,
                &format!("Found homepage for {name}: {homepage}"),
            );

            if homepage.contains("github.com") {
                return fetch_license_from_github_repo(homepage);
            }
        }
    }

    None
}

/// Fetch license content from Go proxy
fn fetch_license_from_go_proxy(name: &str, version: &str) -> Option<String> {
    log(
        LogLevel::Info,
        &format!("Trying to fetch license from Go proxy for {name} v{version}"),
    );

    if name.starts_with("github.com/") {
        let repo_url = format!(
            "https://{}",
            name.split('/').take(3).collect::<Vec<_>>().join("/")
        );
        log(
            LogLevel::Info,
            &format!("Inferred GitHub repository: {repo_url}"),
        );
        return fetch_license_from_github_repo(&repo_url);
    }

    None
}

/// Fetch license content from a GitHub repository
fn fetch_license_from_github(name: &str, _version: &str) -> Option<String> {
    log(
        LogLevel::Info,
        &format!("Trying to infer GitHub repository for {name}"),
    );

    let possible_repos = vec![
        format!("https://github.com/{}/{}", name, name),
        format!("https://github.com/{}/lib{}", name, name),
        format!("https://github.com/{}/{}-rs", name, name),
        format!("https://github.com/{}/{}.js", name, name),
    ];

    for repo_url in possible_repos {
        log(LogLevel::Info, &format!("Trying repository: {repo_url}"));
        if let Some(content) = fetch_license_from_github_repo(&repo_url) {
            return Some(content);
        }
    }

    None
}

/// Fetch license content from GitHub repository
fn fetch_license_from_github_repo(repo_url: &str) -> Option<String> {
    log(
        LogLevel::Info,
        &format!("Fetching license from GitHub repo: {repo_url}"),
    );

    let parts: Vec<&str> = repo_url.trim_end_matches('/').split('/').collect();

    if parts.len() < 2 {
        log(
            LogLevel::Warn,
            &format!("Invalid GitHub URL format: {repo_url}"),
        );
        return None;
    }

    let owner = parts[parts.len() - 2];
    let repo = parts[parts.len() - 1];

    let client = create_http_client()?;
    rate_limit_delay();

    // Common license file names
    let license_files = [
        "LICENSE",
        "LICENSE.txt",
        "LICENSE.md",
        "license",
        "license.txt",
        "license.md",
        "COPYING",
        "COPYING.txt",
        "COPYRIGHT",
        "COPYRIGHT.txt",
    ];

    for license_file in &license_files {
        let api_url =
            format!("https://api.github.com/repos/{owner}/{repo}/contents/{license_file}");

        log(LogLevel::Info, &format!("Trying to fetch: {api_url}"));

        match client.get(&api_url).send() {
            Ok(response) => {
                if response.status().is_success() {
                    if let Ok(content_info) = response.json::<serde_json::Value>() {
                        if let Some(download_url) =
                            content_info.get("download_url").and_then(|u| u.as_str())
                        {
                            log(
                                LogLevel::Info,
                                &format!("Found license file, downloading from: {download_url}"),
                            );

                            rate_limit_delay();

                            match client.get(download_url).send() {
                                Ok(license_response) => {
                                    if license_response.status().is_success() {
                                        if let Ok(license_content) = license_response.text() {
                                            log(LogLevel::Info, &format!("Successfully fetched license content for {repo} from {license_file}"));
                                            return Some(license_content);
                                        }
                                    }
                                }
                                Err(err) => {
                                    log(
                                        LogLevel::Warn,
                                        &format!("Failed to download license file: {err}"),
                                    );
                                }
                            }
                        }
                    }
                } else if response.status().as_u16() == 404 {
                    continue;
                } else {
                    log(
                        LogLevel::Warn,
                        &format!("GitHub API error: HTTP {}", response.status()),
                    );
                }
            }
            Err(err) => {
                log(
                    LogLevel::Warn,
                    &format!("Failed to fetch from GitHub API: {err}"),
                );
            }
        }
    }

    log(
        LogLevel::Warn,
        &format!("No license file found in repository: {owner}/{repo}"),
    );
    None
}

/// Generate the content for a THIRD_PARTY_LICENSES file
fn generate_third_party_licenses_content(
    license_data: &[LicenseInfo],
    indicator: &crate::cli::LoadingIndicator,
) -> (String, (usize, usize)) {
    let mut content = String::new();

    let mut successfully_fetched = 0;
    let mut failed_to_fetch = 0;

    // Header
    content.push_str("# Third-Party Licenses\n\n");
    content.push_str("This project includes third-party libraries licensed under various open source licenses.\n");
    content.push_str("Below is a list of all dependencies, their versions, and license types.\n\n");
    content.push_str(&format!("**Total Dependencies:** {}\n", license_data.len()));
    content.push_str(&format!(
        "**Generated:** {}\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    ));
    content.push_str("---\n\n");

    // Sort dependencies alphabetically
    let mut sorted_deps: Vec<_> = license_data.iter().collect();
    sorted_deps.sort_by(|a, b| a.name.cmp(&b.name));

    indicator.update_progress("processing dependencies");

    for (index, dep) in sorted_deps.iter().enumerate() {
        indicator.update_progress(&format!("processing {}/{}", index + 1, sorted_deps.len()));

        content.push_str(&format!(
            "## {}. {} {}\n\n",
            index + 1,
            dep.name,
            dep.version
        ));

        // License information
        content.push_str(&format!("**License:** {}\n", dep.get_license()));

        // Add compatibility information if available
        match dep.compatibility {
            crate::licenses::LicenseCompatibility::Compatible => {
                content.push_str("**Compatibility:** ✅ Compatible\n");
            }
            crate::licenses::LicenseCompatibility::Incompatible => {
                content.push_str("**Compatibility:** ⚠️ Potentially Incompatible\n");
            }
            crate::licenses::LicenseCompatibility::Unknown => {
                content.push_str("**Compatibility:** ❓ Unknown\n");
            }
        }

        // Add restrictive warning if applicable
        if dep.is_restrictive {
            content.push_str("**⚠️ Note:** This license may have restrictive terms\n");
        }

        // Common package repository URLs based on the dependency type
        let repo_url = generate_package_url(&dep.name, &dep.version);
        if let Some(ref url) = repo_url {
            content.push_str(&format!("**Package URL:** {url}\n"));
        }

        // Copyright notice placeholder
        content.push_str(&format!(
            "**Copyright:** See {} package for copyright information\n",
            dep.name
        ));

        // License text
        content.push_str("\n### License Text\n\n");

        // Try to fetch the actual license content
        match fetch_actual_license_content(&dep.name, &dep.version) {
            Some(actual_license_content) => {
                successfully_fetched += 1;
                log(
                    LogLevel::Info,
                    &format!("Using actual license content for {}", dep.name),
                );

                content.push_str("*The following is the actual license text from the dependency's repository:*\n\n");
                content.push_str("```\n");
                content.push_str(&actual_license_content);
                content.push_str("\n```\n");
            }
            None => {
                failed_to_fetch += 1;
                log(
                    LogLevel::Warn,
                    &format!(
                        "Could not fetch actual license for {}, using fallback",
                        dep.name
                    ),
                );

                match dep.get_license().as_str() {
                    "MIT" => {
                        content.push_str("*Note: Could not fetch actual license text. Below is the standard MIT license template:*\n\n");
                        content.push_str(get_mit_license_text(&dep.name));
                    }
                    "Apache-2.0" => {
                        content.push_str("*Note: Could not fetch actual license text. Below is the standard Apache 2.0 license template:*\n\n");
                        content.push_str(get_apache_license_text());
                    }
                    "BSD-3-Clause" => {
                        content.push_str("*Note: Could not fetch actual license text. Below is the standard BSD 3-Clause license template:*\n\n");
                        content.push_str(get_bsd_license_text(&dep.name));
                    }
                    license if license.contains("MIT") => {
                        content.push_str("*Note: Could not fetch actual license text. Below is the standard MIT license template:*\n\n");
                        content.push_str(get_mit_license_text(&dep.name));
                    }
                    license if license.contains("Apache") => {
                        content.push_str("*Note: Could not fetch actual license text. Below is the standard Apache 2.0 license template:*\n\n");
                        content.push_str(get_apache_license_text());
                    }
                    _ => {
                        content.push_str(&format!(
                            "*Could not fetch the actual license text for {}.*\n\n",
                            dep.name
                        ));
                        content.push_str(&format!(
                            "For the full license text of {}, please refer to:\n",
                            dep.get_license()
                        ));
                        content.push_str(&format!(
                            "- The official {} license documentation\n",
                            dep.get_license()
                        ));
                        if let Some(ref url) = repo_url {
                            content.push_str(&format!("- The package repository: {url}\n"));
                        }
                        content.push_str("- The dependency's source code or package files\n\n");
                    }
                }
            }
        }

        content.push_str("\n---\n\n");
    }

    indicator.update_progress("finalizing document");

    content.push_str("## License Fetching Statistics\n\n");
    content.push_str(&format!(
        "**Total Dependencies Processed:** {}\n",
        license_data.len()
    ));
    content.push_str(&format!(
        "**Actual License Texts Fetched:** {} ({:.1}%)\n",
        successfully_fetched,
        (successfully_fetched as f64 / license_data.len() as f64) * 100.0
    ));
    content.push_str(&format!(
        "**License Texts Not Fetched:** {} ({:.1}%)\n",
        failed_to_fetch,
        (failed_to_fetch as f64 / license_data.len() as f64) * 100.0
    ));

    if successfully_fetched > 0 {
        content.push_str(&format!(
            "\n✅ Successfully fetched actual license texts for {successfully_fetched} dependencies.\n"
        ));
    }

    if failed_to_fetch > 0 {
        content.push_str(&format!(
            "\n⚠️ Could not fetch actual license texts for {failed_to_fetch} dependencies. Using fallback templates or generic references.\n"
        ));
        content.push_str(
            "Consider manually verifying the license information for these dependencies.\n",
        );
    }

    content.push('\n');

    // Footer with Feluda legal disclaimer
    content.push_str("## Legal Notice & Disclaimer\n\n");
    content.push_str("**IMPORTANT LEGAL DISCLAIMER:**\n\n");
    content.push_str("This file was automatically generated by [Feluda](https://github.com/anistark/feluda). Feluda is still in early stages of development.\n");
    content.push_str(
        "The license information contained herein may be incomplete, outdated, or incorrect.\n\n",
    );
    content.push_str("**USER RESPONSIBILITIES:**\n");
    content.push_str(
        "- **Verify Accuracy**: Users must independently verify all license information\n",
    );
    content.push_str("- **Legal Compliance**: Ensure compliance with all applicable license terms and conditions\n");
    content.push_str("- **Legal Counsel**: Consult with qualified legal counsel for license compliance matters\n");
    content.push_str("- **Stay Updated**: Check official package repositories for the most current license information\n");
    content.push_str(
        "- **Due Diligence**: Perform thorough license audits before commercial distribution\n\n",
    );
    content.push_str("**LIMITATION OF LIABILITY:**\n");
    content.push_str("Feluda, its contributors, and maintainers:\n");
    content.push_str("- Disclaim all warranties, express or implied\n");
    content.push_str("- Are not liable for any legal issues, damages, or losses arising from the use of this information\n");
    content.push_str(
        "- Do not guarantee the accuracy, completeness, or reliability of license information\n",
    );
    content.push_str(
        "- Are not responsible for license compliance decisions or their consequences\n\n",
    );
    content.push_str("**USE AT YOUR OWN RISK**\n\n");
    content.push_str("For the most up-to-date license information, please check the official package repositories.\n");
    content.push_str("This tool is provided as-is without any warranties or guarantees.\n\n");
    content.push_str("---\n\n");
    content.push_str("*This file was generated using [Feluda](https://github.com/anistark/feluda), an open-source dependency license checker.*\n");

    (content, (successfully_fetched, failed_to_fetch))
}

/// Generate package repository URL
fn generate_package_url(name: &str, version: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }

    // Go modules: domain/path structure
    if name.contains('/') && name.contains('.') {
        return Some(format!("https://pkg.go.dev/{name}"));
    }

    // npm scoped packages: @scope/name
    if name.starts_with('@') && name.contains('/') {
        return Some(format!("https://www.npmjs.com/package/{name}"));
    }

    // Python packages: Common Python indicators
    if name.starts_with("python-")
        || name.starts_with("django-")
        || name.starts_with("flask-")
        || name.starts_with("pytest-")
        || name.starts_with("py-")
        || name == "requests"
        || name == "numpy"
        || name == "pandas"
        || name == "click"
        || name == "boto3"
        || (name.chars().all(|c| c.is_lowercase() || c == '_') && name.contains('_'))
    {
        return Some(format!("https://pypi.org/project/{name}/"));
    }

    // npm packages: Common JavaScript indicators
    if name.starts_with("react-") ||
       name.starts_with("vue-") ||
       name.starts_with("angular-") ||
       name.starts_with("webpack-") ||
       name.starts_with("babel-") ||
       name.starts_with("eslint-") ||
       name.starts_with("express-") ||
       name.starts_with("node-") ||
       name == "express" ||
       name == "lodash" ||
       name == "axios" ||
       name == "moment" ||
       // npm version patterns
       version.starts_with('^') ||
       version.starts_with('~') ||
       version == "latest" ||
       version == "next"
    {
        return Some(format!("https://www.npmjs.com/package/{name}"));
    }

    // Rust crates: Version starts with digit
    if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Some(format!("https://crates.io/crates/{name}"));
    }

    None
}

/// Get MIT license text template
fn get_mit_license_text(_package_name: &str) -> &'static str {
    "MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the \"Software\"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

"
}

/// Get Apache 2.0 license text
fn get_apache_license_text() -> &'static str {
    "Apache License
Version 2.0, January 2004
http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

\"License\" shall mean the terms and conditions for use, reproduction,
and distribution as defined by Sections 1 through 9 of this document.

\"Licensor\" shall mean the copyright owner or entity granting the License.

\"Legal Entity\" shall mean the union of the acting entity and all
other entities that control, are controlled by, or are under common
control with that entity.

[License text continues - truncated for brevity]

For the complete Apache 2.0 license text, visit: http://www.apache.org/licenses/LICENSE-2.0

"
}

/// Get BSD 3-Clause license text template
fn get_bsd_license_text(_package_name: &str) -> &'static str {
    "BSD 3-Clause License

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS \"AS IS\"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

"
}

/// Main entry point for the generate command
pub fn handle_generate_command(
    path: String,
    language: Option<String>,
    project_license: Option<String>,
) {
    log(
        LogLevel::Info,
        &format!(
            "Starting generate command with path: {path} language: {language:?} project_license: {project_license:?}"
        ),
    );

    // Parse project dependencies first
    log(
        LogLevel::Info,
        &format!("Parsing dependencies for generate command in path: {path}"),
    );

    // Import necessary modules for dependency parsing and license detection
    let mut resolved_project_license = project_license;

    // If no project license is provided via CLI, try to detect it
    if resolved_project_license.is_none() {
        log(
            LogLevel::Info,
            "No project license specified, attempting to detect",
        );
        match detect_project_license(&path) {
            Ok(Some(detected)) => {
                log(
                    LogLevel::Info,
                    &format!("Detected project license: {detected}"),
                );
                resolved_project_license = Some(detected);
            }
            Ok(None) => {
                log(LogLevel::Warn, "Could not detect project license");
            }
            Err(e) => {
                log(
                    LogLevel::Error,
                    &format!("Error detecting project license: {e}"),
                );
            }
        }
    } else {
        log(
            LogLevel::Info,
            &format!(
                "Using provided project license: {}",
                resolved_project_license.as_ref().unwrap()
            ),
        );
    }

    // Parse and analyze dependencies
    let mut analyzed_data = match parse_root(&path, language.as_deref(), false, false) {
        Ok(data) => data,
        Err(e) => {
            println!("{} Failed to parse dependencies: {}", "❌".red().bold(), e);
            log(
                LogLevel::Error,
                &format!("Failed to parse dependencies: {e}"),
            );
            return;
        }
    };

    log_debug("Analyzed dependencies for generate command", &analyzed_data);

    // Update each dependency with compatibility information if project license is known
    if let Some(ref proj_license) = resolved_project_license {
        log(
            LogLevel::Info,
            &format!("Checking license compatibility against project license: {proj_license}"),
        );

        for info in &mut analyzed_data {
            if let Some(ref dep_license) = info.license {
                info.compatibility = is_license_compatible(dep_license, proj_license, false);
            } else {
                info.compatibility = LicenseCompatibility::Unknown;
            }
        }
    } else {
        // If no project license is known, mark all as unknown compatibility
        for info in &mut analyzed_data {
            info.compatibility = LicenseCompatibility::Unknown;
        }
    }

    // Check if we have any dependencies to process
    if analyzed_data.is_empty() {
        println!(
            "{} {}",
            "⚠️".yellow().bold(),
            "No dependencies found. Cannot generate files without dependency data.".yellow()
        );
        return;
    }

    println!(
        "\n{}",
        "🚀 Welcome to Feluda License File Generator!"
            .bold()
            .green()
    );
    println!(
        "{}",
        format!("Found {} dependencies to process.", analyzed_data.len()).dimmed()
    );

    match show_interactive_menu(&path) {
        Some(GenerateOption::Notice) => {
            generate_notice_file(&analyzed_data, &path);
        }
        Some(GenerateOption::ThirdPartyLicenses) => {
            generate_third_party_licenses_file(&analyzed_data, &path);
        }
        None => {
            log(LogLevel::Info, "User cancelled generate operation");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::licenses::LicenseCompatibility;
    use tempfile::TempDir;

    fn get_test_license_data() -> Vec<LicenseInfo> {
        vec![
            LicenseInfo {
                name: "serde".to_string(),
                version: "1.0.151".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "tokio".to_string(),
                version: "1.0.2".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ]
    }

    #[test]
    fn test_generate_option_display_name() {
        assert_eq!(GenerateOption::Notice.display_name(), "NOTICE file");
        assert_eq!(
            GenerateOption::ThirdPartyLicenses.display_name(),
            "THIRD_PARTY_LICENSES file"
        );
    }

    #[test]
    fn test_generate_option_filename() {
        assert_eq!(GenerateOption::Notice.filename(), "NOTICE");
        assert_eq!(
            GenerateOption::ThirdPartyLicenses.filename(),
            "THIRD_PARTY_LICENSES"
        );
    }

    #[test]
    fn test_generate_option_full_filename() {
        assert_eq!(GenerateOption::Notice.full_filename(), "NOTICE");
        assert_eq!(
            GenerateOption::ThirdPartyLicenses.full_filename(),
            "THIRD_PARTY_LICENSES.md"
        );
    }

    #[test]
    fn test_file_exists_false() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_str().unwrap();

        assert!(!file_exists(GenerateOption::Notice, path));
        assert!(!file_exists(GenerateOption::ThirdPartyLicenses, path));
    }

    #[test]
    fn test_file_exists_true() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_str().unwrap();

        std::fs::write(temp_dir.path().join("NOTICE"), "test notice").unwrap();
        std::fs::write(
            temp_dir.path().join("THIRD_PARTY_LICENSES.md"),
            "test licenses",
        )
        .unwrap();

        assert!(file_exists(GenerateOption::Notice, path));
        assert!(file_exists(GenerateOption::ThirdPartyLicenses, path));
    }

    #[test]
    fn test_generate_notice_file() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_str().unwrap();
        let license_data = get_test_license_data();
        generate_notice_file(&license_data, path);
    }

    #[test]
    fn test_generate_third_party_licenses_file() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_str().unwrap();
        let license_data = get_test_license_data();
        generate_third_party_licenses_file(&license_data, path);
    }

    #[test]
    fn test_handle_generate_command_empty_data() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_str().unwrap();
        handle_generate_command(path.to_string(), None, None);
    }

    #[test]
    fn test_generate_option_copy() {
        let option1 = GenerateOption::Notice;
        let option2 = option1;
        assert_eq!(option1.display_name(), option2.display_name());
    }

    #[test]
    fn test_generate_option_debug() {
        let option = GenerateOption::ThirdPartyLicenses;
        let debug_str = format!("{option:?}");
        assert!(debug_str.contains("ThirdPartyLicenses"));
    }

    #[test]
    fn test_generate_option_methods() {
        let notice = GenerateOption::Notice;
        let licenses = GenerateOption::ThirdPartyLicenses;

        assert_eq!(notice.display_name(), "NOTICE file");
        assert_eq!(licenses.display_name(), "THIRD_PARTY_LICENSES file");

        assert_eq!(notice.filename(), "NOTICE");
        assert_eq!(licenses.filename(), "THIRD_PARTY_LICENSES");

        assert_eq!(notice.extension(), "");
        assert_eq!(licenses.extension(), ".md");

        assert_eq!(notice.full_filename(), "NOTICE");
        assert_eq!(licenses.full_filename(), "THIRD_PARTY_LICENSES.md");
    }

    #[test]
    fn test_generate_option_copy_clone() {
        let notice1 = GenerateOption::Notice;
        let notice2 = notice1;

        assert_eq!(notice1.display_name(), notice2.display_name());
        assert_eq!(notice1.full_filename(), notice2.full_filename());
    }

    #[test]
    fn test_file_exists_with_different_paths() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().to_str().unwrap();

        // Test non-existent files
        assert!(!file_exists(GenerateOption::Notice, path));
        assert!(!file_exists(GenerateOption::ThirdPartyLicenses, path));

        // Create NOTICE file
        std::fs::write(temp_dir.path().join("NOTICE"), "test notice").unwrap();
        assert!(file_exists(GenerateOption::Notice, path));
        assert!(!file_exists(GenerateOption::ThirdPartyLicenses, path));

        // Create THIRD_PARTY_LICENSES.md file
        std::fs::write(
            temp_dir.path().join("THIRD_PARTY_LICENSES.md"),
            "test licenses",
        )
        .unwrap();
        assert!(file_exists(GenerateOption::Notice, path));
        assert!(file_exists(GenerateOption::ThirdPartyLicenses, path));
    }

    #[test]
    fn test_generate_package_url() {
        // Go modules
        assert_eq!(
            generate_package_url("github.com/gorilla/mux", "v1.8.0"),
            Some("https://pkg.go.dev/github.com/gorilla/mux".to_string())
        );

        // npm scoped packages
        assert_eq!(
            generate_package_url("@babel/core", "7.0.0"),
            Some("https://www.npmjs.com/package/@babel/core".to_string())
        );

        // Python packages
        assert_eq!(
            generate_package_url("python-dateutil", "v2.8.2"),
            Some("https://pypi.org/project/python-dateutil/".to_string())
        );
        assert_eq!(
            generate_package_url("requests", "2.28.1"),
            Some("https://pypi.org/project/requests/".to_string())
        );
        assert_eq!(
            generate_package_url("django-rest-framework", "3.14.0"),
            Some("https://pypi.org/project/django-rest-framework/".to_string())
        );

        // npm packages
        assert_eq!(
            generate_package_url("express", "4.18.0"),
            Some("https://www.npmjs.com/package/express".to_string())
        );
        assert_eq!(
            generate_package_url("react-router", "6.0.0"),
            Some("https://www.npmjs.com/package/react-router".to_string())
        );

        // npm packages
        assert_eq!(
            generate_package_url("some-package", "^4.18.0"),
            Some("https://www.npmjs.com/package/some-package".to_string())
        );
        assert_eq!(
            generate_package_url("another-pkg", "latest"),
            Some("https://www.npmjs.com/package/another-pkg".to_string())
        );

        // Rust crates
        assert_eq!(
            generate_package_url("serde", "1.0.0"),
            Some("https://crates.io/crates/serde".to_string())
        );
        assert_eq!(
            generate_package_url("tokio", "1.28.1"),
            Some("https://crates.io/crates/tokio".to_string())
        );

        // Unknown packages
        assert_eq!(generate_package_url("", "1.0.0"), None);
        assert_eq!(generate_package_url("UnknownPackage", "unknown"), None);
    }

    #[test]
    fn test_license_templates() {
        let mit_license = get_mit_license_text("test_package");
        assert!(mit_license.contains("MIT License"));
        assert!(mit_license.contains("Permission is hereby granted"));
        assert!(mit_license.contains("free of charge"));
        assert!(mit_license.contains("THE SOFTWARE IS PROVIDED \"AS IS\""));

        let apache_license = get_apache_license_text();
        assert!(apache_license.contains("Apache License"));
        assert!(apache_license.contains("Version 2.0"));
        assert!(apache_license.contains("January 2004"));
        assert!(apache_license.contains("http://www.apache.org/licenses/"));

        let bsd_license = get_bsd_license_text("test_package");
        assert!(bsd_license.contains("BSD 3-Clause License"));
        assert!(bsd_license.contains("Redistribution and use"));
        assert!(bsd_license.contains("Neither the name"));
    }

    #[test]
    fn test_generate_notice_content() {
        let test_data = vec![
            LicenseInfo {
                name: "package1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package3".to_string(),
                version: "1.5.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let content = generate_notice_content(&test_data);

        // Check header
        assert!(content.contains("NOTICE"));
        assert!(content.contains("======"));

        // Check license sections
        assert!(content.contains("MIT Licensed Components"));
        assert!(content.contains("Apache-2.0 Licensed Components"));

        // Check package listings
        assert!(content.contains("package1 (1.0.0)"));
        assert!(content.contains("package2 (2.0.0)"));
        assert!(content.contains("package3 (1.5.0)"));

        // Check footer
        assert!(content.contains("Generated by: Feluda"));
        assert!(content.contains("DISCLAIMER"));
        assert!(content.contains("Generated at:"));

        // Check dependency count
        assert!(content.contains("3 third-party dependencies"));
    }

    #[test]
    fn test_generate_notice_content_empty() {
        let test_data = vec![];
        let content = generate_notice_content(&test_data);

        assert!(content.contains("NOTICE"));
        assert!(content.contains("0 third-party dependencies"));
        assert!(content.contains("Generated by: Feluda"));
    }

    #[test]
    fn test_generate_notice_content_no_license() {
        let test_data = vec![LicenseInfo {
            name: "unknown_package".to_string(),
            version: "1.0.0".to_string(),
            license: None,
            is_restrictive: true,
            compatibility: LicenseCompatibility::Unknown,
            osi_status: crate::licenses::OsiStatus::Unknown,
        }];

        let content = generate_notice_content(&test_data);
        assert!(content.contains("No License Licensed Components"));
        assert!(content.contains("unknown_package (1.0.0)"));
    }

    #[test]
    fn test_create_http_client() {
        let client = create_http_client();
        assert!(client.is_some());

        if let Some(client) = client {
            let _ = client;
        }
    }

    #[test]
    fn test_rate_limit_delay() {
        let start = std::time::Instant::now();
        rate_limit_delay();
        let duration = start.elapsed();

        // Should take at least 500ms
        assert!(duration >= std::time::Duration::from_millis(500));
    }

    #[test]
    fn test_generate_notice_file_creation() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().to_str().unwrap();

        let license_data = vec![LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        generate_notice_file(&license_data, path);

        // Check that the file was created
        let notice_path = temp_dir.path().join("NOTICE");
        assert!(notice_path.exists());

        // Check file contents
        let content = std::fs::read_to_string(notice_path).unwrap();
        assert!(content.contains("NOTICE"));
        assert!(content.contains("test_package"));
        assert!(content.contains("MIT"));
    }

    #[test]
    fn test_generate_notice_file_update() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().to_str().unwrap();

        // Create existing NOTICE file
        let notice_path = temp_dir.path().join("NOTICE");
        std::fs::write(&notice_path, "Old notice content").unwrap();

        let license_data = vec![LicenseInfo {
            name: "new_package".to_string(),
            version: "2.0.0".to_string(),
            license: Some("Apache-2.0".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        generate_notice_file(&license_data, path);

        // Check that the file was updated
        let content = std::fs::read_to_string(notice_path).unwrap();
        assert!(content.contains("new_package"));
        assert!(content.contains("Apache-2.0"));
        assert!(!content.contains("Old notice content"));
    }

    #[test]
    fn test_generate_third_party_licenses_file_creation() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().to_str().unwrap();

        let license_data = vec![LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        generate_third_party_licenses_file(&license_data, path);

        // Check that the file was created
        let licenses_path = temp_dir.path().join("THIRD_PARTY_LICENSES.md");
        assert!(licenses_path.exists());

        // Check file contents
        let content = std::fs::read_to_string(licenses_path).unwrap();
        assert!(content.contains("# Third-Party Licenses"));
        assert!(content.contains("test_package"));
        assert!(content.contains("MIT"));
        assert!(content.contains("Legal Notice & Disclaimer"));
    }

    #[test]
    fn test_handle_generate_command_with_empty_data() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().to_str().unwrap();

        handle_generate_command(path.to_string(), None, None);
    }

    #[test]
    #[cfg(windows)]
    fn test_enable_disable_raw_mode_windows() {
        // On Windows, these should return Ok(())
        assert!(enable_raw_mode().is_ok());
        assert!(disable_raw_mode().is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_enable_disable_raw_mode_unix() {
        // On Unix, these might fail in test environment, but should not panic
        let _ = enable_raw_mode();
        let _ = disable_raw_mode();
    }

    #[test]
    fn test_fetch_actual_license_content_invalid_package() {
        // This should return None for invalid packages
        let result = fetch_actual_license_content("definitely_nonexistent_package_12345", "1.0.0");
        assert!(result.is_none());
    }
}
