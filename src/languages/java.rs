use quick_xml::events::Event;
use quick_xml::reader::Reader;
use rayon::prelude::*;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::config::FeludaConfig;
use crate::debug::{log, log_error, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, is_license_restrictive, LicenseCompatibility, LicenseInfo,
};

#[derive(Debug, Clone)]
struct JavaDependency {
    group_id: String,
    artifact_id: String,
    version: String,
}

pub fn analyze_java_licenses(file_path: &str, config: &FeludaConfig) -> Vec<LicenseInfo> {
    log(
        LogLevel::Info,
        &format!("Analyzing Java dependencies from: {file_path}"),
    );

    let project_dir = Path::new(file_path).parent().unwrap_or(Path::new("."));

    let deps = if file_path.ends_with("pom.xml") {
        parse_maven_pom(file_path)
    } else if file_path.ends_with("build.gradle") || file_path.ends_with("build.gradle.kts") {
        parse_gradle_build(file_path, project_dir)
    } else {
        Vec::new()
    };

    if deps.is_empty() {
        log(LogLevel::Warn, "No Java dependencies found");
        return Vec::new();
    }

    log(
        LogLevel::Info,
        &format!("Found {} Java dependencies", deps.len()),
    );

    let known_licenses = match fetch_licenses_from_github() {
        Ok(licenses) => licenses,
        Err(err) => {
            log_error("Failed to fetch licenses from GitHub", &err);
            HashMap::new()
        }
    };

    deps.par_iter()
        .map(|dep| {
            let license = fetch_maven_license(&dep.group_id, &dep.artifact_id, &dep.version);
            let is_restrictive =
                is_license_restrictive(&Some(license.clone()), &known_licenses, config.strict);

            LicenseInfo {
                name: format!("{}:{}", dep.group_id, dep.artifact_id),
                version: dep.version.clone(),
                license: Some(license.clone()),
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: crate::licenses::get_osi_status(&license),
            }
        })
        .collect()
}

// =============================================================================
// MAVEN POM PARSING
// =============================================================================

fn parse_maven_pom(pom_path: &str) -> Vec<JavaDependency> {
    let content = match fs::read_to_string(pom_path) {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("Failed to read pom.xml: {pom_path}"), &e);
            return Vec::new();
        }
    };

    let properties = extract_pom_properties(&content);
    let managed_versions = extract_dependency_management(&content, &properties);
    let mut deps = extract_pom_dependencies(&content, &properties, &managed_versions);

    // Deduplicate
    deps.sort_by(|a, b| {
        a.group_id
            .cmp(&b.group_id)
            .then(a.artifact_id.cmp(&b.artifact_id))
    });
    deps.dedup_by(|a, b| a.group_id == b.group_id && a.artifact_id == b.artifact_id);

    deps
}

fn extract_pom_properties(content: &str) -> HashMap<String, String> {
    let mut props = HashMap::new();
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut in_properties = false;
    let mut current_key: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "properties" {
                    in_properties = true;
                } else if in_properties {
                    current_key = Some(name);
                }
            }
            Ok(Event::Text(e)) => {
                if let Some(ref key) = current_key {
                    let val = e.unescape().unwrap_or_default().to_string();
                    props.insert(key.clone(), val);
                }
            }
            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "properties" {
                    in_properties = false;
                }
                if in_properties {
                    current_key = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    props
}

fn extract_dependency_management(
    content: &str,
    properties: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut managed = HashMap::new();

    let dm_re = Regex::new(
        r"(?s)<dependencyManagement>.*?<dependencies>(.*?)</dependencies>.*?</dependencyManagement>",
    )
    .unwrap();
    let dep_re = Regex::new(r"(?s)<dependency>(.*?)</dependency>").unwrap();

    if let Some(dm_cap) = dm_re.captures(content) {
        let dm_section = &dm_cap[1];
        for dep_cap in dep_re.captures_iter(dm_section) {
            let dep_block = &dep_cap[1];
            if let (Some(g), Some(a), Some(v)) = (
                extract_xml_tag(dep_block, "groupId"),
                extract_xml_tag(dep_block, "artifactId"),
                extract_xml_tag(dep_block, "version"),
            ) {
                let g = resolve_property(&g, properties);
                let a = resolve_property(&a, properties);
                let v = resolve_property(&v, properties);
                managed.insert(format!("{g}:{a}"), v);
            }
        }
    }

    managed
}

fn extract_pom_dependencies(
    content: &str,
    properties: &HashMap<String, String>,
    managed_versions: &HashMap<String, String>,
) -> Vec<JavaDependency> {
    let mut deps = Vec::new();

    // Find <dependencies> blocks outside of <dependencyManagement>
    let content_stripped = strip_dependency_management(content);

    let dep_re = Regex::new(r"(?s)<dependency>(.*?)</dependency>").unwrap();

    for cap in dep_re.captures_iter(&content_stripped) {
        let block = &cap[1];

        // Skip test-scoped dependencies
        if let Some(scope) = extract_xml_tag(block, "scope") {
            if scope == "test" {
                continue;
            }
        }

        let group_id = extract_xml_tag(block, "groupId").unwrap_or_default();
        let artifact_id = extract_xml_tag(block, "artifactId").unwrap_or_default();
        let version_raw = extract_xml_tag(block, "version").unwrap_or_default();

        if group_id.is_empty() || artifact_id.is_empty() {
            continue;
        }

        let group_id = resolve_property(&group_id, properties);
        let artifact_id = resolve_property(&artifact_id, properties);
        let version = if version_raw.is_empty() {
            let key = format!("{group_id}:{artifact_id}");
            managed_versions
                .get(&key)
                .cloned()
                .unwrap_or_else(|| "RELEASE".to_string())
        } else {
            resolve_property(&version_raw, properties)
        };

        deps.push(JavaDependency {
            group_id,
            artifact_id,
            version,
        });
    }

    deps
}

fn strip_dependency_management(content: &str) -> String {
    let dm_re = Regex::new(r"(?s)<dependencyManagement>.*?</dependencyManagement>").unwrap();
    dm_re.replace_all(content, "").to_string()
}

fn extract_xml_tag(block: &str, tag: &str) -> Option<String> {
    let pattern = format!(r"<{tag}>(.*?)</{tag}>");
    let re = Regex::new(&pattern).ok()?;
    re.captures(block)
        .map(|c| c[1].trim().to_string())
        .filter(|s| !s.is_empty())
}

fn resolve_property(value: &str, properties: &HashMap<String, String>) -> String {
    let prop_re = Regex::new(r"\$\{([^}]+)\}").unwrap();
    let mut result = value.to_string();
    for cap in prop_re.captures_iter(value) {
        let key = &cap[1];
        if let Some(resolved) = properties.get(key) {
            result = result.replace(&cap[0], resolved);
        }
    }
    result
}

// =============================================================================
// GRADLE BUILD PARSING
// =============================================================================

fn parse_gradle_build(build_path: &str, project_dir: &Path) -> Vec<JavaDependency> {
    let content = match fs::read_to_string(build_path) {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("Failed to read {build_path}"), &e);
            return Vec::new();
        }
    };

    let mut deps = parse_gradle_dependencies(&content);

    // Also try to read gradle.properties for version variables
    let props = read_gradle_properties(project_dir);
    for dep in &mut deps {
        dep.version = resolve_gradle_variable(&dep.version, &props);
    }

    deps.sort_by(|a, b| {
        a.group_id
            .cmp(&b.group_id)
            .then(a.artifact_id.cmp(&b.artifact_id))
    });
    deps.dedup_by(|a, b| a.group_id == b.group_id && a.artifact_id == b.artifact_id);

    deps
}

fn parse_gradle_dependencies(content: &str) -> Vec<JavaDependency> {
    let mut deps = Vec::new();

    // Match: implementation 'group:artifact:version' or implementation("group:artifact:version")
    // Also: api, compileOnly, runtimeOnly, annotationProcessor
    let coord_re = Regex::new(
        r#"(?m)^\s*(?:implementation|api|compileOnly|runtimeOnly|annotationProcessor|compile)\s*[\(\s]['"]([^'"]+)['"][,\s\)]"#,
    )
    .unwrap();

    // Match: implementation(group: 'com.example', name: 'lib', version: '1.0')
    let named_re = Regex::new(
        r#"(?s)(?:implementation|api|compileOnly|runtimeOnly|annotationProcessor|compile)\s*\(\s*group\s*:\s*['"]([^'"]+)['"]\s*,\s*name\s*:\s*['"]([^'"]+)['"]\s*,\s*version\s*:\s*['"]([^'"]+)['"]\s*\)"#,
    )
    .unwrap();

    for cap in coord_re.captures_iter(content) {
        let coord = &cap[1];
        if let Some(dep) = parse_gradle_coordinate(coord) {
            deps.push(dep);
        }
    }

    for cap in named_re.captures_iter(content) {
        deps.push(JavaDependency {
            group_id: cap[1].to_string(),
            artifact_id: cap[2].to_string(),
            version: cap[3].to_string(),
        });
    }

    deps
}

fn parse_gradle_coordinate(coord: &str) -> Option<JavaDependency> {
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 2 {
        return None;
    }

    let group_id = parts[0].trim().to_string();
    let artifact_id = parts[1].trim().to_string();
    let version = parts
        .get(2)
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| "RELEASE".to_string());

    if group_id.is_empty() || artifact_id.is_empty() {
        return None;
    }

    Some(JavaDependency {
        group_id,
        artifact_id,
        version,
    })
}

fn read_gradle_properties(project_dir: &Path) -> HashMap<String, String> {
    let mut props = HashMap::new();
    let props_path = project_dir.join("gradle.properties");

    if let Ok(content) = fs::read_to_string(&props_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                props.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }

    props
}

fn resolve_gradle_variable(value: &str, props: &HashMap<String, String>) -> String {
    // Handle ${propName} and $propName style references
    let re = Regex::new(r"\$\{?([A-Za-z_][A-Za-z0-9_.]*)\}?").unwrap();
    let mut result = value.to_string();
    for cap in re.captures_iter(value) {
        let key = &cap[1];
        if let Some(resolved) = props.get(key) {
            result = result.replace(&cap[0], resolved);
        }
    }
    result
}

// =============================================================================
// MAVEN CENTRAL LICENSE LOOKUP
// =============================================================================

fn fetch_maven_license(group_id: &str, artifact_id: &str, version: &str) -> String {
    // Try fetching the POM from Maven Central and extracting license info
    if let Some(license) = fetch_license_from_pom(group_id, artifact_id, version) {
        return license;
    }

    // Fallback: Maven Central search API
    if let Some(license) = fetch_license_from_search_api(group_id, artifact_id) {
        return license;
    }

    "Unknown".to_string()
}

fn fetch_license_from_pom(group_id: &str, artifact_id: &str, version: &str) -> Option<String> {
    let group_path = group_id.replace('.', "/");
    let effective_version = if version == "RELEASE" || version.is_empty() {
        fetch_latest_version(group_id, artifact_id)?
    } else {
        version.to_string()
    };

    let pom_url = format!(
        "https://repo1.maven.org/maven2/{group_path}/{artifact_id}/{effective_version}/{artifact_id}-{effective_version}.pom"
    );

    log(LogLevel::Info, &format!("Fetching POM: {pom_url}"));

    let response = reqwest::blocking::get(&pom_url).ok()?;
    if !response.status().is_success() {
        return None;
    }

    let pom_content = response.text().ok()?;
    extract_license_from_pom_content(&pom_content)
}

fn extract_license_from_pom_content(content: &str) -> Option<String> {
    // Extract <licenses><license><name>...</name>
    let re = Regex::new(r"(?s)<licenses>.*?<license>.*?<name>(.*?)</name>.*?</license>").ok()?;
    re.captures(content)
        .map(|c| c[1].trim().to_string())
        .filter(|s| !s.is_empty())
}

fn fetch_latest_version(group_id: &str, artifact_id: &str) -> Option<String> {
    let url = format!(
        "https://search.maven.org/solrsearch/select?q=g:{group_id}+AND+a:{artifact_id}&rows=1&wt=json"
    );

    let response = reqwest::blocking::get(&url).ok()?;
    if !response.status().is_success() {
        return None;
    }

    let json: serde_json::Value = response.json().ok()?;
    json["response"]["docs"]
        .as_array()?
        .first()
        .and_then(|doc| doc["latestVersion"].as_str())
        .map(String::from)
}

fn fetch_license_from_search_api(group_id: &str, artifact_id: &str) -> Option<String> {
    let url = format!(
        "https://search.maven.org/solrsearch/select?q=g:{group_id}+AND+a:{artifact_id}&rows=1&wt=json"
    );

    log(
        LogLevel::Info,
        &format!("Querying Maven Central search for {group_id}:{artifact_id}"),
    );

    let response = reqwest::blocking::get(&url).ok()?;
    if !response.status().is_success() {
        return None;
    }

    let json: serde_json::Value = response.json().ok()?;
    json["response"]["docs"]
        .as_array()?
        .first()
        .and_then(|doc| doc["p"].as_str().or(doc["packaging"].as_str()))
        .map(String::from)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_gradle_coordinate_full() {
        let dep = parse_gradle_coordinate("com.google.guava:guava:31.1-jre").unwrap();
        assert_eq!(dep.group_id, "com.google.guava");
        assert_eq!(dep.artifact_id, "guava");
        assert_eq!(dep.version, "31.1-jre");
    }

    #[test]
    fn test_parse_gradle_coordinate_no_version() {
        let dep = parse_gradle_coordinate("org.springframework:spring-core").unwrap();
        assert_eq!(dep.group_id, "org.springframework");
        assert_eq!(dep.artifact_id, "spring-core");
        assert_eq!(dep.version, "RELEASE");
    }

    #[test]
    fn test_parse_gradle_coordinate_invalid() {
        assert!(parse_gradle_coordinate("not-a-coordinate").is_none());
        assert!(parse_gradle_coordinate("").is_none());
    }

    #[test]
    fn test_parse_gradle_dependencies_groovy() {
        let content = r#"
dependencies {
    implementation 'com.google.guava:guava:31.1-jre'
    implementation("org.apache.commons:commons-lang3:3.12.0")
    api 'org.slf4j:slf4j-api:1.7.36'
    testImplementation 'junit:junit:4.13.2'
    compileOnly 'org.projectlombok:lombok:1.18.24'
}
"#;
        let deps = parse_gradle_dependencies(content);
        assert!(deps.iter().any(|d| d.artifact_id == "guava"));
        assert!(deps.iter().any(|d| d.artifact_id == "commons-lang3"));
        assert!(deps.iter().any(|d| d.artifact_id == "slf4j-api"));
        // testImplementation is not in our pattern so not included
        assert!(deps.iter().any(|d| d.artifact_id == "lombok"));
    }

    #[test]
    fn test_parse_gradle_dependencies_named() {
        let content = r#"
dependencies {
    implementation(group: 'org.apache.kafka', name: 'kafka-clients', version: '3.4.0')
}
"#;
        let deps = parse_gradle_dependencies(content);
        assert!(deps.iter().any(|d| d.artifact_id == "kafka-clients"));
    }

    #[test]
    fn test_extract_xml_tag() {
        let block = "<groupId>com.example</groupId><artifactId>mylib</artifactId>";
        assert_eq!(
            extract_xml_tag(block, "groupId"),
            Some("com.example".to_string())
        );
        assert_eq!(
            extract_xml_tag(block, "artifactId"),
            Some("mylib".to_string())
        );
        assert_eq!(extract_xml_tag(block, "version"), None);
    }

    #[test]
    fn test_resolve_property() {
        let mut props = HashMap::new();
        props.insert("spring.version".to_string(), "5.3.20".to_string());

        assert_eq!(
            resolve_property("${spring.version}", &props),
            "5.3.20".to_string()
        );
        assert_eq!(resolve_property("literal", &props), "literal".to_string());
    }

    #[test]
    fn test_parse_maven_pom_basic() {
        let temp_dir = TempDir::new().unwrap();
        let pom_path = temp_dir.path().join("pom.xml");

        fs::write(
            &pom_path,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project>
    <groupId>com.example</groupId>
    <artifactId>my-app</artifactId>
    <version>1.0.0</version>
    <dependencies>
        <dependency>
            <groupId>com.google.guava</groupId>
            <artifactId>guava</artifactId>
            <version>31.1-jre</version>
        </dependency>
        <dependency>
            <groupId>junit</groupId>
            <artifactId>junit</artifactId>
            <version>4.13.2</version>
            <scope>test</scope>
        </dependency>
    </dependencies>
</project>"#,
        )
        .unwrap();

        let deps = parse_maven_pom(pom_path.to_str().unwrap());
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].group_id, "com.google.guava");
        assert_eq!(deps[0].artifact_id, "guava");
        assert_eq!(deps[0].version, "31.1-jre");
    }

    #[test]
    fn test_parse_maven_pom_with_properties() {
        let temp_dir = TempDir::new().unwrap();
        let pom_path = temp_dir.path().join("pom.xml");

        fs::write(
            &pom_path,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project>
    <groupId>com.example</groupId>
    <artifactId>my-app</artifactId>
    <version>1.0.0</version>
    <properties>
        <guava.version>31.1-jre</guava.version>
    </properties>
    <dependencies>
        <dependency>
            <groupId>com.google.guava</groupId>
            <artifactId>guava</artifactId>
            <version>${guava.version}</version>
        </dependency>
    </dependencies>
</project>"#,
        )
        .unwrap();

        let deps = parse_maven_pom(pom_path.to_str().unwrap());
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].version, "31.1-jre");
    }

    #[test]
    fn test_parse_maven_pom_dependency_management() {
        let temp_dir = TempDir::new().unwrap();
        let pom_path = temp_dir.path().join("pom.xml");

        fs::write(
            &pom_path,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project>
    <groupId>com.example</groupId>
    <artifactId>my-app</artifactId>
    <version>1.0.0</version>
    <dependencyManagement>
        <dependencies>
            <dependency>
                <groupId>org.springframework</groupId>
                <artifactId>spring-core</artifactId>
                <version>5.3.20</version>
            </dependency>
        </dependencies>
    </dependencyManagement>
    <dependencies>
        <dependency>
            <groupId>org.springframework</groupId>
            <artifactId>spring-core</artifactId>
        </dependency>
    </dependencies>
</project>"#,
        )
        .unwrap();

        let deps = parse_maven_pom(pom_path.to_str().unwrap());
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].artifact_id, "spring-core");
        assert_eq!(deps[0].version, "5.3.20");
    }

    #[test]
    fn test_extract_license_from_pom_content() {
        let content = r#"<licenses>
  <license>
    <name>Apache License, Version 2.0</name>
    <url>https://www.apache.org/licenses/LICENSE-2.0.txt</url>
  </license>
</licenses>"#;

        let license = extract_license_from_pom_content(content);
        assert_eq!(license, Some("Apache License, Version 2.0".to_string()));
    }

    #[test]
    fn test_read_gradle_properties() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("gradle.properties"),
            "guavaVersion=31.1-jre\n# comment\nspringVersion=5.3.20\n",
        )
        .unwrap();

        let props = read_gradle_properties(temp_dir.path());
        assert_eq!(props.get("guavaVersion").unwrap(), "31.1-jre");
        assert_eq!(props.get("springVersion").unwrap(), "5.3.20");
        assert!(!props.contains_key("# comment"));
    }

    #[test]
    fn test_resolve_gradle_variable() {
        let mut props = HashMap::new();
        props.insert("guavaVersion".to_string(), "31.1-jre".to_string());

        assert_eq!(
            resolve_gradle_variable("${guavaVersion}", &props),
            "31.1-jre"
        );
        assert_eq!(resolve_gradle_variable("1.0.0", &props), "1.0.0");
    }
}
