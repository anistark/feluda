use quick_xml::escape::unescape;
use quick_xml::events::{BytesRef, BytesText, Event};
use quick_xml::reader::Reader;
use rayon::prelude::*;
use regex::Regex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::config::FeludaConfig;
use crate::debug::{log, log_error, LogLevel};
use crate::licenses::{
    detect_license_from_content, fetch_licenses_from_github, is_license_restrictive,
    normalize_license_id, LicenseCompatibility, LicenseInfo,
};
use crate::purl::Ecosystem;

#[derive(Debug, Clone)]
struct JavaDependency {
    group_id: String,
    artifact_id: String,
    version: String,
}

pub fn analyze_java_licenses(
    file_path: &str,
    config: &FeludaConfig,
    no_local: bool,
) -> Vec<LicenseInfo> {
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
        &format!("Found {} direct Java dependencies", deps.len()),
    );

    // Expand to the full transitive set by walking POMs from Maven Central,
    // mirroring the transitive resolution done for Rust/Go/Node. Bounded by the
    // configured max dependency depth; falls back to the direct dependencies if
    // the registry is unreachable.
    let deps = resolve_transitive_dependencies(deps, config.dependencies.max_depth, no_local);

    log(
        LogLevel::Info,
        &format!(
            "Resolved {} Java dependencies (including transitive)",
            deps.len()
        ),
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
            let license =
                fetch_maven_license(&dep.group_id, &dep.artifact_id, &dep.version, no_local);
            let is_restrictive =
                is_license_restrictive(&Some(license.clone()), &known_licenses, config.strict);

            LicenseInfo {
                name: format!("{}:{}", dep.group_id, dep.artifact_id),
                version: dep.version.clone(),
                license: Some(license.clone()),
                is_restrictive,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: crate::licenses::get_osi_status(&license),
                ecosystem: Ecosystem::Maven,
                sub_project: None,
                qualifiers: Default::default(),
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

    let properties = extract_effective_pom_properties(&content);
    let managed_versions = extract_dependency_management(&content, &properties);
    let mut deps = extract_pom_dependencies(&content, &properties, &managed_versions, false);

    // Deduplicate
    deps.sort_by(|a, b| {
        a.group_id
            .cmp(&b.group_id)
            .then(a.artifact_id.cmp(&b.artifact_id))
    });
    deps.dedup_by(|a, b| a.group_id == b.group_id && a.artifact_id == b.artifact_id);

    deps
}

/// Decode a text event and resolve any entities it still contains.
///
/// Since quick-xml 0.42 event content is already `&str` (the reader rejects
/// input that is not UTF-8), so only unescaping remains. Malformed content
/// yields an empty string, matching the previous `unescape().unwrap_or_default()`
/// behaviour.
fn decode_text(e: &BytesText) -> String {
    unescape(e).map(|s| s.into_owned()).unwrap_or_default()
}

/// Resolve a standalone entity reference event into the text it denotes.
///
/// Since quick-xml 0.38 a text run is split around every `&ref;` or `&#nn;`,
/// which arrives as its own `Event::GeneralRef` holding just the name between
/// the delimiters. Rebuilding the reference and unescaping it handles named and
/// numeric forms with one code path. Unknown entities resolve to nothing.
fn decode_entity_ref(e: &BytesRef) -> String {
    let name: &str = e;
    unescape(&format!("&{name};"))
        .map(|s| s.into_owned())
        .unwrap_or_default()
}

fn extract_pom_properties(content: &str) -> HashMap<String, String> {
    let mut props = HashMap::new();
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut in_properties = false;
    let mut current_key: Option<String> = None;
    // Text arrives in fragments whenever entity references split a run, so
    // accumulate until the closing tag rather than inserting per event.
    let mut current_value = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.name().as_ref().to_string();
                if name == "properties" {
                    in_properties = true;
                } else if in_properties {
                    current_key = Some(name);
                    current_value.clear();
                }
            }
            Ok(Event::Text(e)) => {
                if current_key.is_some() {
                    current_value.push_str(&decode_text(&e));
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if current_key.is_some() {
                    current_value.push_str(&decode_entity_ref(&e));
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name().as_ref().to_string();
                if name == "properties" {
                    in_properties = false;
                }
                if in_properties {
                    if let Some(key) = current_key.take() {
                        props.insert(key, std::mem::take(&mut current_value));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    props
}

/// Coordinates declared at the top level of a POM, plus its `<parent>` block.
#[derive(Debug, Default)]
struct PomCoordinates {
    group_id: Option<String>,
    artifact_id: Option<String>,
    version: Option<String>,
    parent_group_id: Option<String>,
    parent_artifact_id: Option<String>,
    parent_version: Option<String>,
}

fn extract_pom_coordinates(content: &str) -> PomCoordinates {
    let mut coords = PomCoordinates::default();
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut path: Vec<String> = Vec::new();
    // Entity references split a text run into several events, so the element's
    // text is accumulated here and committed when its closing tag arrives.
    let mut buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                path.push(e.name().as_ref().to_string());
                buf.clear();
            }
            Ok(Event::Text(e)) => {
                buf.push_str(&decode_text(&e));
            }
            Ok(Event::GeneralRef(e)) => {
                buf.push_str(&decode_entity_ref(&e));
            }
            Ok(Event::End(_)) => {
                let slot = match path
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    ["project", "groupId"] => Some(&mut coords.group_id),
                    ["project", "artifactId"] => Some(&mut coords.artifact_id),
                    ["project", "version"] => Some(&mut coords.version),
                    ["project", "parent", "groupId"] => Some(&mut coords.parent_group_id),
                    ["project", "parent", "artifactId"] => Some(&mut coords.parent_artifact_id),
                    ["project", "parent", "version"] => Some(&mut coords.parent_version),
                    _ => None,
                };
                if let Some(slot) = slot {
                    let val = buf.trim();
                    if !val.is_empty() {
                        *slot = Some(val.to_string());
                    }
                }
                buf.clear();
                path.pop();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    coords
}

/// `<properties>` plus the Maven built-in `project.*` properties derivable from
/// the POM itself, so placeholders like `${project.version}` resolve.
/// `project.groupId` and `project.version` fall back to the parent's values
/// when absent, matching Maven's inheritance rules.
fn extract_effective_pom_properties(content: &str) -> HashMap<String, String> {
    let mut props = extract_pom_properties(content);
    let coords = extract_pom_coordinates(content);

    let group_id = coords.group_id.or_else(|| coords.parent_group_id.clone());
    let version = coords.version.or_else(|| coords.parent_version.clone());

    let builtins = [
        ("project.groupId", group_id),
        ("project.artifactId", coords.artifact_id),
        ("project.version", version),
        ("project.parent.groupId", coords.parent_group_id),
        ("project.parent.artifactId", coords.parent_artifact_id),
        ("project.parent.version", coords.parent_version),
    ];
    for (key, value) in builtins {
        if let Some(value) = value {
            let value = resolve_property(&value, &props);
            // A value that is itself an unresolvable placeholder (e.g. an
            // inherited `${revision}`) would only propagate the placeholder;
            // leave the property undefined instead.
            if !value.contains("${") {
                props.insert(key.to_string(), value);
            }
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

/// Extract dependencies from a POM's `<dependencies>` blocks.
///
/// When `transitive` is set the caller is reading a *dependency's* POM rather
/// than the project's own, so the scopes Maven does not propagate (`provided`,
/// `system`) and `optional` dependencies are dropped. `test` scope is always
/// dropped.
fn extract_pom_dependencies(
    content: &str,
    properties: &HashMap<String, String>,
    managed_versions: &HashMap<String, String>,
    transitive: bool,
) -> Vec<JavaDependency> {
    let mut deps = Vec::new();

    // Find <dependencies> blocks outside of <dependencyManagement>
    let content_stripped = strip_dependency_management(content);

    let dep_re = Regex::new(r"(?s)<dependency>(.*?)</dependency>").unwrap();

    for cap in dep_re.captures_iter(&content_stripped) {
        let block = &cap[1];

        if let Some(scope) = extract_xml_tag(block, "scope") {
            let scope = scope.to_ascii_lowercase();
            // `test` scope is never a runtime concern.
            if scope == "test" {
                continue;
            }
            // `provided` and `system` are not inherited by downstream projects.
            if transitive && (scope == "provided" || scope == "system") {
                continue;
            }
        }

        // Optional dependencies are not propagated transitively.
        if transitive {
            if let Some(optional) = extract_xml_tag(block, "optional") {
                if optional.eq_ignore_ascii_case("true") {
                    continue;
                }
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

        // A transitive version we still cannot resolve (e.g. a property defined
        // only in a parent POM we did not fetch) is not actionable — skip it
        // rather than emit a bogus coordinate.
        if transitive && version.contains("${") {
            continue;
        }

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
// TRANSITIVE DEPENDENCY RESOLUTION
// =============================================================================

/// Resolve the full transitive dependency set for a list of direct deps by
/// walking POMs from Maven Central breadth-first, up to `max_depth` levels.
///
/// This mirrors the transitive resolution performed for Rust/Go/Node. Java has
/// no universal lockfile and the build tool may be absent, so the graph is
/// reconstructed from published POMs. If the registry is unreachable a POM
/// fetch simply yields no children, so the result degrades to the direct deps.
fn resolve_transitive_dependencies(
    direct: Vec<JavaDependency>,
    max_depth: u32,
    no_local: bool,
) -> Vec<JavaDependency> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut resolved: Vec<JavaDependency> = Vec::new();
    let mut queue: VecDeque<(JavaDependency, u32)> = VecDeque::new();

    // Seed with the direct dependencies at depth 0.
    for dep in direct {
        let key = format!("{}:{}", dep.group_id, dep.artifact_id);
        if visited.insert(key) {
            queue.push_back((dep.clone(), 0));
            resolved.push(dep);
        }
    }

    while let Some((dep, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        for child in
            fetch_pom_transitive_deps(&dep.group_id, &dep.artifact_id, &dep.version, no_local)
        {
            let key = format!("{}:{}", child.group_id, child.artifact_id);
            // First occurrence wins, approximating Maven's nearest-first order.
            if visited.insert(key) {
                resolved.push(child.clone());
                queue.push_back((child, depth + 1));
            }
        }
    }

    resolved
}

/// Fetch an artifact's POM and extract the dependencies it propagates
/// transitively (compile/runtime scope, non-optional), resolving `${properties}`
/// and `<dependencyManagement>` declared within that POM.
fn fetch_pom_transitive_deps(
    group_id: &str,
    artifact_id: &str,
    version: &str,
    no_local: bool,
) -> Vec<JavaDependency> {
    let content = match fetch_pom_content(group_id, artifact_id, version, no_local) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let properties = extract_effective_pom_properties(&content);
    let managed_versions = extract_dependency_management(&content, &properties);
    extract_pom_dependencies(&content, &properties, &managed_versions, true)
}

// =============================================================================
// MAVEN CENTRAL LICENSE LOOKUP
// =============================================================================

/// Resolve the license of one artifact, local files first.
///
/// A POM declares the license its authors chose; a jar only carries the license texts that were
/// bundled into it, and several of those can be present at once (an EPL-2.0 artifact that also ships
/// the GPLv2 text for the Classpath Exception). Reading the jar first therefore answers `GPL-2.0` for
/// those artifacts, which is both wrong and marked restrictive. So the order is: the **local POM
/// chain, parents included**, then the jar next to it, then the registry. Most multi-module projects
/// (Guava, `commons-*`, slf4j, logback, Netty, the ee4j APIs) declare the license only in a parent, so
/// stopping at the artifact's own POM would still let the jar answer first for exactly those. Both
/// local steps are files, so an offline run stays offline, and `no_local` skips them, as `--no-local`
/// promises.
pub(crate) fn fetch_maven_license(
    group_id: &str,
    artifact_id: &str,
    version: &str,
    no_local: bool,
) -> String {
    if !no_local {
        if let Some(license) =
            resolve_pom_license(group_id, artifact_id, version, 0, PomSource::LocalOnly)
        {
            return license;
        }
        if let Some(license) = fetch_license_from_local_jar(group_id, artifact_id, version) {
            return license;
        }
    }

    if let Some(license) = fetch_license_from_pom(group_id, artifact_id, version, no_local) {
        return license;
    }

    "Unknown".to_string()
}

/// License files conventionally bundled inside a jar, in priority order. Maven artifacts
/// place them under `META-INF/`; some older jars keep them at the archive root.
const JAR_LICENSE_ENTRIES: &[&str] = &[
    "META-INF/LICENSE",
    "META-INF/LICENSE.txt",
    "META-INF/LICENSE.md",
    "META-INF/COPYING",
    "LICENSE",
    "LICENSE.txt",
    "LICENSE.md",
    "COPYING",
];

/// Probe the locally cached jar for a bundled license file and content-detect it.
///
/// The jar lives at `<repo>/<group as path>/<artifact>/<version>/<artifact>-<version>.jar`,
/// where the repo is `MAVEN_REPO_LOCAL` or the default `~/.m2/repository`.
fn fetch_license_from_local_jar(
    group_id: &str,
    artifact_id: &str,
    version: &str,
) -> Option<String> {
    detect_license_in_jar(&local_artifact_path(group_id, artifact_id, version, "jar")?)
}

/// Where one file of an artifact lives in the local Maven repository.
///
/// A concrete version is required to locate anything on disk, so `RELEASE` and an empty version
/// answer `None`.
fn local_artifact_path(
    group_id: &str,
    artifact_id: &str,
    version: &str,
    extension: &str,
) -> Option<PathBuf> {
    if version.is_empty() || version == "RELEASE" {
        return None;
    }

    let repo = maven_local_repo()?;
    Some(
        repo.join(group_id.replace('.', "/"))
            .join(artifact_id)
            .join(version)
            .join(format!("{artifact_id}-{version}.{extension}")),
    )
}

/// The local POMs already read, including the ones that were not there, keyed by file path.
///
/// The transitive walk and the license lookup ask about the same coordinates several times over —
/// once per dependency that walks up to a shared parent such as `commons-parent` — and each answer
/// costs a read and a parse. The file cannot change in a way that matters within one pass, and
/// `feluda watch` empties this map before each re-scan (see [`reset_maven_caches`]), so a `mvn
/// install` that lands mid-watch is picked up by the next pass rather than cached as absent.
fn local_pom_cache() -> &'static Mutex<HashMap<PathBuf, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Read `<repo>/<group as path>/<artifact>/<version>/<artifact>-<version>.pom`, if it is there.
fn read_local_pom(group_id: &str, artifact_id: &str, version: &str) -> Option<String> {
    let path = local_artifact_path(group_id, artifact_id, version, "pom")?;

    if let Ok(cache) = local_pom_cache().lock() {
        if let Some(answer) = cache.get(&path) {
            return answer.clone();
        }
    }

    // A truncated file, an HTML error page a proxy saved under the POM's name, or a zero-byte
    // leftover would otherwise be accepted as "the answer", returning no license and suppressing
    // both the jar and the network fallback. An interrupted download cut inside `<dependencies>`
    // still carries the opening tag, so the closing tag is what proves the document is whole.
    let answer = fs::read_to_string(&path)
        .ok()
        .filter(|content| content.contains("<project") && content.contains("</project>"));

    if let Ok(mut cache) = local_pom_cache().lock() {
        cache.insert(path, answer.clone());
    }

    answer
}

fn maven_local_repo() -> Option<PathBuf> {
    if let Ok(repo) = std::env::var("MAVEN_REPO_LOCAL") {
        return Some(PathBuf::from(repo));
    }
    dirs::home_dir().map(|home| home.join(".m2").join("repository"))
}

fn detect_license_in_jar(jar_path: &Path) -> Option<String> {
    let file = fs::File::open(jar_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    for entry_name in JAR_LICENSE_ENTRIES {
        if let Ok(mut entry) = archive.by_name(entry_name) {
            let mut content = String::new();
            if entry.read_to_string(&mut content).is_ok() {
                if let Some(spdx) = detect_license_from_content(&content) {
                    log(
                        LogLevel::Info,
                        &format!("Detected {spdx} from jar entry {entry_name}"),
                    );
                    return Some(spdx);
                }
            }
        }
    }
    None
}

/// Maximum number of parent POMs to follow when resolving a license. Guards
/// against cycles and unbounded chains in malformed metadata.
const MAX_POM_PARENT_DEPTH: usize = 5;

fn fetch_license_from_pom(
    group_id: &str,
    artifact_id: &str,
    version: &str,
    no_local: bool,
) -> Option<String> {
    resolve_pom_license(
        group_id,
        artifact_id,
        version,
        0,
        PomSource::Registry { no_local },
    )
}

/// Where the parent-POM walk may read a POM from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PomSource {
    /// Files already in the local repository, and nothing else: no network at any level.
    LocalOnly,
    /// The local repository first, then the registry. `no_local` is `--no-local`, which skips the
    /// local read.
    Registry { no_local: bool },
}

/// Resolve a license from an artifact's POM, walking up the `<parent>` chain
/// when the artifact's own POM declares no `<licenses>`. Many widely used
/// libraries (Guava, Apache Commons, slf4j, logback) inherit their license
/// from a parent POM rather than declaring it directly.
fn resolve_pom_license(
    group_id: &str,
    artifact_id: &str,
    version: &str,
    depth: usize,
    source: PomSource,
) -> Option<String> {
    if depth > MAX_POM_PARENT_DEPTH {
        return None;
    }

    let pom_content = match source {
        PomSource::LocalOnly => read_local_pom(group_id, artifact_id, version)?,
        PomSource::Registry { no_local } => {
            fetch_pom_content(group_id, artifact_id, version, no_local)?
        }
    };

    // A license declared directly on this POM takes precedence.
    if let Some(license) = extract_license_from_pom_content(&pom_content) {
        return Some(license);
    }

    // Otherwise follow the parent POM, where the license is often declared.
    let parent = resolve_parent_coordinate(&pom_content, version)?;
    resolve_pom_license(
        &parent.group_id,
        &parent.artifact_id,
        &parent.version,
        depth + 1,
        source,
    )
}

/// Process-wide cache of fetched POM bodies, keyed by `group:artifact:version`.
/// The transitive walk and the license lookup both fetch the same POMs, so
/// caching roughly halves the network traffic for a scan.
fn pom_content_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fetch_pom_content(
    group_id: &str,
    artifact_id: &str,
    version: &str,
    no_local: bool,
) -> Option<String> {
    // The POM next to the jar in the local repository is the one the project was built from, and
    // reading it keeps an offline scan offline, exactly as it does for the jar.
    if !no_local {
        if let Some(content) = read_local_pom(group_id, artifact_id, version) {
            return Some(content);
        }
    }

    let key = format!("{group_id}:{artifact_id}:{version}");

    if let Ok(cache) = pom_content_cache().lock() {
        if let Some(cached) = cache.get(&key) {
            return cached.clone();
        }
    }

    let result = fetch_pom_content_uncached(group_id, artifact_id, version);

    if let Ok(mut cache) = pom_content_cache().lock() {
        cache.insert(key, result.clone());
    }

    result
}

/// The endpoint in use, once it has been read.
///
/// A cell rather than an `OnceLock<String>` so that [`reset_maven_caches`] can empty it: `feluda
/// watch` re-scans in the same process, and a `[maven] endpoint` edited while it runs has to be
/// picked up by the next pass.
fn maven_endpoint_cell() -> &'static Mutex<Option<String>> {
    static ENDPOINT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    ENDPOINT.get_or_init(|| Mutex::new(None))
}

/// Base URL of the Maven repository the POM lookups read.
///
/// It comes from `[maven] endpoint`, which `FELUDA_MAVEN_ENDPOINT` overrides through the usual
/// figment chain, the way `FELUDA_CLEARLYDEFINED_ENDPOINT` moves the ClearlyDefined URL; a test can
/// therefore answer from a stub on localhost instead of Maven Central. Read once per pass: loading
/// the configuration per dependency would log a line per POM, and `feluda watch` clears the cache
/// before each re-scan.
fn maven_endpoint() -> String {
    let cell = maven_endpoint_cell();
    if let Ok(cached) = cell.lock() {
        if let Some(endpoint) = cached.as_ref() {
            return endpoint.clone();
        }
    }

    let endpoint = crate::config::load_config()
        .map(|config| config.maven.endpoint)
        // `FELUDA_MAVEN_ENDPOINT=` is how CI says "unset", and figment keeps it as an empty
        // string; an empty base URL would make every uncached artifact Unknown.
        .ok()
        .filter(|endpoint| !endpoint.trim().is_empty())
        // A mirror URL copied from Nexus or Artifactory ends in a slash, and joining it as
        // `.../maven-public//com/...` makes some repositories answer 404 for everything.
        .map(|endpoint| endpoint.trim_end_matches('/').to_string())
        .unwrap_or_else(|| FeludaConfig::default().maven.endpoint);

    if let Ok(mut cached) = cell.lock() {
        *cached = Some(endpoint.clone());
    }

    endpoint
}

/// Forget what the local Maven repository and the configuration answered last time, and keep what the
/// next pass cannot get differently.
///
/// `feluda watch` calls this before every pass. Without it a `pom.xml` edit re-scans the tree before
/// `mvn` has downloaded the new dependency, the miss is cached, and every later pass asks the registry
/// (or reports `Unknown` offline) even once the file is on disk.
///
/// A local POM's miss only says what `mvn` has downloaded *so far*, so the whole local cache goes, and
/// the endpoint is configuration rather than an answer, so it goes too. A POM's fetched content is kept
/// — clearing it as well made every pass ask the registry again for every POM outside `~/.m2` — but
/// only for a coordinate that cannot move between two passes: `RELEASE`, an empty version and a
/// `-SNAPSHOT` are re-resolved by the next pass, so their content is forgotten with the misses.
/// A hit fetched from an earlier `[maven] endpoint` also survives: the coordinate names the artifact,
/// and a mirror serving different content for the same coordinate is not something a per-process
/// cache can tell apart.
pub(crate) fn reset_maven_caches() {
    if let Ok(mut cache) = local_pom_cache().lock() {
        cache.clear();
    }
    if let Ok(mut cache) = pom_content_cache().lock() {
        cache.retain(|key, content| {
            content.is_some()
                && key.rsplit(':').next().is_some_and(|version| {
                    !version.is_empty() && version != "RELEASE" && !version.ends_with("-SNAPSHOT")
                })
        });
    }
    if let Ok(mut endpoint) = maven_endpoint_cell().lock() {
        *endpoint = None;
    }
}

fn fetch_pom_content_uncached(group_id: &str, artifact_id: &str, version: &str) -> Option<String> {
    let group_path = group_id.replace('.', "/");
    let effective_version = if version == "RELEASE" || version.is_empty() {
        fetch_latest_version(group_id, artifact_id)?
    } else {
        version.to_string()
    };

    let pom_url = format!(
        "{}/{group_path}/{artifact_id}/{effective_version}/{artifact_id}-{effective_version}.pom",
        maven_endpoint()
    );

    log(LogLevel::Info, &format!("Fetching POM: {pom_url}"));

    let response = crate::http_stats::get(&pom_url).ok()?;
    if !response.status().is_success() {
        return None;
    }

    response.text().ok()
}

fn extract_license_from_pom_content(content: &str) -> Option<String> {
    // Extract <licenses><license><name>...</name>
    let re = Regex::new(r"(?s)<licenses>.*?<license>.*?<name>(.*?)</name>.*?</license>").ok()?;
    // A `<name>` is written by the artifact's authors, not in SPDX: "Apache License, Version 2.0",
    // "EPL 2.0", "GPL2 w/ CPE". Everything downstream — the registry lookup, the restrictive list,
    // the CycloneDX writer — speaks ids, and a prose copyleft name matches none of them, so it would
    // classify as permissive. Normalize here, where the prose enters.
    re.captures(content)
        .map(|c| normalize_license_id(c[1].trim()))
        .filter(|s| !s.is_empty())
}

/// Maven `<parent>` coordinate, used to follow the POM inheritance chain.
struct ParentCoordinate {
    group_id: String,
    artifact_id: String,
    version: String,
}

fn extract_parent_from_pom_content(content: &str) -> Option<ParentCoordinate> {
    let parent_re = Regex::new(r"(?s)<parent>(.*?)</parent>").ok()?;
    let parent_block = parent_re.captures(content)?.get(1)?.as_str();

    Some(ParentCoordinate {
        group_id: extract_pom_tag(parent_block, "groupId")?,
        artifact_id: extract_pom_tag(parent_block, "artifactId")?,
        version: extract_pom_tag(parent_block, "version")?,
    })
}

/// Extract the `<parent>` coordinate with `${...}` placeholders expanded.
///
/// `own_version` is the version the POM was fetched at: when the parent
/// version resolves to nothing concrete (CI-friendly `${revision}` defined
/// only in the parent POM itself), the child's version is the best available
/// guess, since such setups version the whole reactor together. A wrong guess
/// merely fails the next fetch, which is where an unexpanded placeholder
/// ended up anyway.
fn resolve_parent_coordinate(content: &str, own_version: &str) -> Option<ParentCoordinate> {
    let parent = extract_parent_from_pom_content(content)?;
    let properties = extract_effective_pom_properties(content);

    let version = resolve_property(&parent.version, &properties);
    Some(ParentCoordinate {
        group_id: resolve_property(&parent.group_id, &properties),
        artifact_id: resolve_property(&parent.artifact_id, &properties),
        version: if version.contains("${") {
            own_version.to_string()
        } else {
            version
        },
    })
}

fn extract_pom_tag(content: &str, tag: &str) -> Option<String> {
    let re = Regex::new(&format!(r"(?s)<{tag}>(.*?)</{tag}>")).ok()?;
    re.captures(content)
        .map(|c| c[1].trim().to_string())
        .filter(|s| !s.is_empty())
}

fn fetch_latest_version(group_id: &str, artifact_id: &str) -> Option<String> {
    let url = format!(
        "https://search.maven.org/solrsearch/select?q=g:{group_id}+AND+a:{artifact_id}&rows=1&wt=json"
    );

    let response = crate::http_stats::get(&url).ok()?;
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

    fn write_jar(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for &(entry_name, content) in entries {
            zip.start_file(entry_name, SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn test_detect_license_in_jar() {
        let temp_dir = TempDir::new().unwrap();
        let jar_path = temp_dir.path().join("guava-32.0.jar");
        write_jar(
            &jar_path,
            &[(
                "META-INF/LICENSE",
                b"Apache License\nVersion 2.0, January 2004",
            )],
        );

        assert_eq!(
            detect_license_in_jar(&jar_path),
            Some("Apache-2.0".to_string())
        );
    }

    #[test]
    fn test_detect_license_in_jar_no_license_entry() {
        let temp_dir = TempDir::new().unwrap();
        let jar_path = temp_dir.path().join("nolicense-1.0.jar");
        write_jar(&jar_path, &[("com/example/Main.class", b"not a license")]);

        assert_eq!(detect_license_in_jar(&jar_path), None);
    }

    #[test]
    fn test_detect_license_in_jar_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        assert_eq!(
            detect_license_in_jar(&temp_dir.path().join("does-not-exist.jar")),
            None
        );
    }

    #[test]
    fn test_fetch_license_from_local_jar_requires_concrete_version() {
        // Without a concrete version the jar can't be located, so no probing happens.
        assert_eq!(fetch_license_from_local_jar("g", "a", ""), None);
        assert_eq!(fetch_license_from_local_jar("g", "a", "RELEASE"), None);
    }

    #[test]
    fn test_fetch_license_from_local_jar() {
        let temp_dir = TempDir::new().unwrap();
        let jar_path = temp_dir
            .path()
            .join("com/example/cachedpkg/1.0.0/cachedpkg-1.0.0.jar");
        write_jar(
            &jar_path,
            &[(
                "META-INF/LICENSE",
                b"MIT License\n\nCopyright (c) 2024 Example",
            )],
        );

        temp_env::with_var(
            "MAVEN_REPO_LOCAL",
            Some(temp_dir.path().to_str().unwrap()),
            || {
                let license = fetch_license_from_local_jar("com.example", "cachedpkg", "1.0.0");
                assert_eq!(license, Some("MIT".to_string()));

                let missing = fetch_license_from_local_jar("com.example", "nonexistent", "1.0.0");
                assert_eq!(missing, None);
            },
        );
    }

    /// The artifact's own POM declares nothing, its parent — also on disk — declares GPL-2.0, and the
    /// jar next to it bundles the MIT text. The parent chain has to answer: most multi-module projects
    /// declare the license only in a parent, and reading the jar first would report whatever that jar
    /// happens to bundle.
    #[test]
    fn test_local_parent_pom_answers_before_the_jar() {
        let temp_dir = TempDir::new().unwrap();
        let repo = temp_dir.path();
        let child_pom = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>parentpom</artifactId>
    <version>1.0</version>
  </parent>
  <artifactId>childmod</artifactId>
</project>
"#;
        let parent_pom = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>parentpom</artifactId>
  <version>1.0</version>
  <licenses>
    <license>
      <name>GPL-2.0</name>
    </license>
  </licenses>
</project>
"#;
        let write = |path: std::path::PathBuf, content: &str| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        };
        write(
            repo.join("com/example/childmod/1.0/childmod-1.0.pom"),
            child_pom,
        );
        write(
            repo.join("com/example/parentpom/1.0/parentpom-1.0.pom"),
            parent_pom,
        );
        write_jar(
            &repo.join("com/example/childmod/1.0/childmod-1.0.jar"),
            &[(
                "META-INF/LICENSE",
                b"MIT License\n\nCopyright (c) 2024 Example",
            )],
        );

        temp_env::with_var("MAVEN_REPO_LOCAL", Some(repo.to_str().unwrap()), || {
            assert_eq!(
                fetch_maven_license("com.example", "childmod", "1.0", false),
                "GPL-2.0",
                "the parent POM on disk must answer before the jar bundles its own text"
            );
        });
    }

    /// `--no-local` skips the whole local tier, the parent chain included: the contract grew from one
    /// POM to a chain, and the flag has to cover what it grew into.
    #[test]
    fn test_no_local_skips_the_local_parent_chain() {
        let temp_dir = TempDir::new().unwrap();
        let repo = temp_dir.path();
        let child_pom = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <parent>
    <groupId>com.example</groupId>
    <artifactId>parentpom</artifactId>
    <version>1.0</version>
  </parent>
  <artifactId>childmod</artifactId>
</project>
"#;
        let parent_pom = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <groupId>com.example</groupId>
  <artifactId>parentpom</artifactId>
  <version>1.0</version>
  <licenses>
    <license>
      <name>GPL-2.0</name>
    </license>
  </licenses>
</project>
"#;
        for (path, content) in [
            ("com/example/childmod/1.0/childmod-1.0.pom", child_pom),
            ("com/example/parentpom/1.0/parentpom-1.0.pom", parent_pom),
        ] {
            let path = repo.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        temp_env::with_vars(
            [
                ("MAVEN_REPO_LOCAL", Some(repo.to_str().unwrap())),
                // Nothing listens on port 1, so the registry tier fails instead of reaching the
                // network: the test stays offline and the answer is deterministic.
                ("FELUDA_MAVEN_ENDPOINT", Some("http://127.0.0.1:1/maven2")),
            ],
            || {
                reset_maven_caches();

                assert_eq!(
                    fetch_maven_license("com.example", "childmod", "1.0", false),
                    "GPL-2.0",
                    "the local parent chain answers before the jar"
                );
                // `--no-local` must skip the chain too: with no registry answer, the result is
                // unknown rather than the local parent's license.
                assert_eq!(
                    fetch_maven_license("com.example", "childmod", "1.0", true),
                    "Unknown"
                );

                reset_maven_caches();
            },
        );
    }

    /// A dependency `mvn` downloads while `feluda watch` is running must be seen by the next pass:
    /// the miss is cached, so the cache has to be emptied at the start of a pass.
    #[test]
    fn test_reset_maven_caches_forgets_a_missed_local_pom() {
        let temp_dir = TempDir::new().unwrap();
        let repo = temp_dir.path();
        let artifact = repo.join("com/example/latepkg/1.0");
        fs::create_dir_all(&artifact).unwrap();

        temp_env::with_var("MAVEN_REPO_LOCAL", Some(repo.to_str().unwrap()), || {
            // Pass one: not on disk yet, and the miss is what gets cached.
            assert_eq!(read_local_pom("com.example", "latepkg", "1.0"), None);

            // `mvn install` lands between passes. Without a reset the cached miss outlives the
            // file and every later pass still answers "not there".
            fs::write(artifact.join("latepkg-1.0.pom"), "<project></project>").unwrap();
            assert_eq!(
                read_local_pom("com.example", "latepkg", "1.0"),
                None,
                "the miss is cached for the rest of the pass"
            );

            reset_maven_caches();

            assert_eq!(
                read_local_pom("com.example", "latepkg", "1.0"),
                Some("<project></project>".to_string()),
                "the next pass has to see the file `mvn` downloaded"
            );
        });
    }

    /// A released POM does not change for a given coordinate, so the content a pass already fetched
    /// has to survive the reset; a miss says something about the network and a `RELEASE` or
    /// `-SNAPSHOT` coordinate is re-resolved every pass, so neither survives.
    #[test]
    fn test_reset_keeps_a_fetched_pom_and_forgets_a_miss() {
        let hit = "com.example:hit:1.0";
        let miss = "com.example:miss:1.0";
        let floating = "com.example:floating:RELEASE";
        let snapshot = "com.example:snapshot:2.0-SNAPSHOT";
        {
            let mut cache = pom_content_cache().lock().unwrap();
            cache.insert(hit.to_string(), Some("<project></project>".to_string()));
            cache.insert(miss.to_string(), None);
            cache.insert(
                floating.to_string(),
                Some("<project></project>".to_string()),
            );
            cache.insert(
                snapshot.to_string(),
                Some("<project></project>".to_string()),
            );
        }

        reset_maven_caches();

        {
            let cache = pom_content_cache().lock().unwrap();
            assert_eq!(
                cache.get(hit),
                Some(&Some("<project></project>".to_string())),
                "the next pass must not ask the registry again for a POM it already has"
            );
            assert!(
                !cache.contains_key(miss),
                "a miss is a statement about the network, not about the artifact"
            );
            assert!(
                !cache.contains_key(floating),
                "`RELEASE` re-resolves to a new version between passes"
            );
            assert!(
                !cache.contains_key(snapshot),
                "a `-SNAPSHOT` is republished under the same coordinate"
            );
        }
        reset_maven_caches();
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

        // The `<name>` is prose; what leaves this function is the SPDX id the rest of the pipeline
        // speaks, because a prose copyleft name matches neither the registry nor the restrictive list.
        let license = extract_license_from_pom_content(content);
        assert_eq!(license, Some("Apache-2.0".to_string()));
    }

    #[test]
    fn test_transitive_filters_optional_and_provided() {
        let content = r#"<project>
  <dependencies>
    <dependency>
      <groupId>com.example</groupId>
      <artifactId>compile-dep</artifactId>
      <version>1.0.0</version>
    </dependency>
    <dependency>
      <groupId>com.example</groupId>
      <artifactId>optional-dep</artifactId>
      <version>1.0.0</version>
      <optional>true</optional>
    </dependency>
    <dependency>
      <groupId>com.example</groupId>
      <artifactId>provided-dep</artifactId>
      <version>1.0.0</version>
      <scope>provided</scope>
    </dependency>
  </dependencies>
</project>"#;

        let props = HashMap::new();
        let managed = HashMap::new();

        // Transitive resolution drops optional and provided/system deps.
        let transitive = extract_pom_dependencies(content, &props, &managed, true);
        let ids: Vec<&str> = transitive.iter().map(|d| d.artifact_id.as_str()).collect();
        assert_eq!(ids, vec!["compile-dep"]);

        // Direct parsing keeps them (only `test` scope is dropped).
        let direct = extract_pom_dependencies(content, &props, &managed, false);
        assert_eq!(direct.len(), 3);
    }

    #[test]
    fn test_transitive_skips_unresolved_property_version() {
        let content = r#"<project>
  <dependencies>
    <dependency>
      <groupId>com.example</groupId>
      <artifactId>resolved</artifactId>
      <version>1.0.0</version>
    </dependency>
    <dependency>
      <groupId>com.example</groupId>
      <artifactId>unresolved</artifactId>
      <version>${some.parent.version}</version>
    </dependency>
  </dependencies>
</project>"#;

        let props = HashMap::new();
        let managed = HashMap::new();

        let transitive = extract_pom_dependencies(content, &props, &managed, true);
        let ids: Vec<&str> = transitive.iter().map(|d| d.artifact_id.as_str()).collect();
        assert_eq!(ids, vec!["resolved"]);
    }

    #[test]
    fn test_extract_pom_coordinates() {
        let content = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <parent>
    <groupId>com.example</groupId>
    <artifactId>example-parent</artifactId>
    <version>2.0.0</version>
  </parent>
  <artifactId>example-child</artifactId>
  <version>1.0.0</version>
</project>"#;

        let coords = extract_pom_coordinates(content);
        assert_eq!(coords.group_id, None);
        assert_eq!(coords.artifact_id, Some("example-child".to_string()));
        assert_eq!(coords.version, Some("1.0.0".to_string()));
        assert_eq!(coords.parent_group_id, Some("com.example".to_string()));
        assert_eq!(
            coords.parent_artifact_id,
            Some("example-parent".to_string())
        );
        assert_eq!(coords.parent_version, Some("2.0.0".to_string()));
    }

    #[test]
    fn test_pom_text_resolves_xml_entities() {
        // Guards the decode-then-unescape path in decode_text: predefined XML
        // entities must be resolved in both properties and coordinates.
        let content = r#"<project>
  <artifactId>my&amp;app</artifactId>
  <version>1.0.0</version>
  <properties>
    <escaped.prop>a&amp;b&lt;c&gt;d</escaped.prop>
    <numeric.prop>x&#38;y&#x26;z</numeric.prop>
  </properties>
</project>"#;

        let props = extract_pom_properties(content);
        assert_eq!(props.get("escaped.prop"), Some(&"a&b<c>d".to_string()));
        assert_eq!(props.get("numeric.prop"), Some(&"x&y&z".to_string()));

        let coords = extract_pom_coordinates(content);
        assert_eq!(coords.artifact_id, Some("my&app".to_string()));
    }

    #[test]
    fn test_extract_pom_coordinates_ignores_dependency_tags() {
        // groupId/version inside <dependencies> must not be mistaken for the
        // project's own coordinates.
        let content = r#"<project>
  <artifactId>my-app</artifactId>
  <version>1.0.0</version>
  <dependencies>
    <dependency>
      <groupId>com.google.guava</groupId>
      <artifactId>guava</artifactId>
      <version>31.1-jre</version>
    </dependency>
  </dependencies>
</project>"#;

        let coords = extract_pom_coordinates(content);
        assert_eq!(coords.group_id, None);
        assert_eq!(coords.artifact_id, Some("my-app".to_string()));
        assert_eq!(coords.version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_effective_properties_include_project_builtins() {
        let content = r#"<project>
  <groupId>com.example</groupId>
  <artifactId>my-app</artifactId>
  <version>1.2.3</version>
</project>"#;

        let props = extract_effective_pom_properties(content);
        assert_eq!(props.get("project.groupId").unwrap(), "com.example");
        assert_eq!(props.get("project.artifactId").unwrap(), "my-app");
        assert_eq!(props.get("project.version").unwrap(), "1.2.3");
    }

    #[test]
    fn test_effective_properties_inherit_from_parent() {
        // A child module without its own groupId/version inherits both from
        // the parent, as Maven does.
        let content = r#"<project>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>example-parent</artifactId>
    <version>2.0.0</version>
  </parent>
  <artifactId>example-child</artifactId>
</project>"#;

        let props = extract_effective_pom_properties(content);
        assert_eq!(props.get("project.groupId").unwrap(), "com.example");
        assert_eq!(props.get("project.version").unwrap(), "2.0.0");
        assert_eq!(props.get("project.parent.version").unwrap(), "2.0.0");
    }

    #[test]
    fn test_effective_properties_resolve_placeholder_version() {
        // CI-friendly versioning: <version>${revision}</version> with the
        // revision property defined in the same POM.
        let content = r#"<project>
  <groupId>com.example</groupId>
  <artifactId>my-app</artifactId>
  <version>${revision}</version>
  <properties>
    <revision>3.1.4</revision>
  </properties>
</project>"#;

        let props = extract_effective_pom_properties(content);
        assert_eq!(props.get("project.version").unwrap(), "3.1.4");
    }

    #[test]
    fn test_effective_properties_skip_unresolvable_placeholder() {
        // The parent's ${revision} is defined in a POM we haven't fetched;
        // project.version must stay undefined rather than hold the placeholder.
        let content = r#"<project>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>example-parent</artifactId>
    <version>${revision}</version>
  </parent>
  <artifactId>example-child</artifactId>
</project>"#;

        let props = extract_effective_pom_properties(content);
        assert!(!props.contains_key("project.version"));
        assert!(!props.contains_key("project.parent.version"));
        assert_eq!(props.get("project.groupId").unwrap(), "com.example");
    }

    #[test]
    fn test_parse_maven_pom_project_version_dependency() {
        // Sibling-module dependencies are commonly versioned as
        // ${project.version}; they must resolve, not fall through as literals.
        let temp_dir = TempDir::new().unwrap();
        let pom_path = temp_dir.path().join("pom.xml");

        fs::write(
            &pom_path,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project>
    <groupId>com.example</groupId>
    <artifactId>my-app</artifactId>
    <version>1.2.3</version>
    <dependencies>
        <dependency>
            <groupId>com.example</groupId>
            <artifactId>my-lib</artifactId>
            <version>${project.version}</version>
        </dependency>
    </dependencies>
</project>"#,
        )
        .unwrap();

        let deps = parse_maven_pom(pom_path.to_str().unwrap());
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].version, "1.2.3");
    }

    #[test]
    fn test_resolve_parent_coordinate_from_properties() {
        let content = r#"<project>
  <parent>
    <groupId>${parent.group}</groupId>
    <artifactId>example-parent</artifactId>
    <version>${revision}</version>
  </parent>
  <artifactId>example-child</artifactId>
  <properties>
    <parent.group>com.example</parent.group>
    <revision>2.5.0</revision>
  </properties>
</project>"#;

        let parent = resolve_parent_coordinate(content, "9.9.9").unwrap();
        assert_eq!(parent.group_id, "com.example");
        assert_eq!(parent.artifact_id, "example-parent");
        assert_eq!(parent.version, "2.5.0");
    }

    #[test]
    fn test_resolve_parent_coordinate_falls_back_to_own_version() {
        // ${revision} defined only in the parent POM: fall back to the
        // child's own fetched version instead of an unexpanded placeholder.
        let content = r#"<project>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>example-parent</artifactId>
    <version>${revision}</version>
  </parent>
  <artifactId>example-child</artifactId>
</project>"#;

        let parent = resolve_parent_coordinate(content, "4.0.1").unwrap();
        assert_eq!(parent.group_id, "com.example");
        assert_eq!(parent.version, "4.0.1");
    }

    #[test]
    fn test_resolve_parent_coordinate_literal_version() {
        let content = r#"<project>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>example-parent</artifactId>
    <version>2.0.0</version>
  </parent>
  <artifactId>example-child</artifactId>
</project>"#;

        let parent = resolve_parent_coordinate(content, "9.9.9").unwrap();
        assert_eq!(parent.version, "2.0.0");
    }

    #[test]
    fn test_extract_parent_from_pom_content() {
        let content = r#"<project>
  <parent>
    <groupId>com.google.guava</groupId>
    <artifactId>guava-parent</artifactId>
    <version>31.1-jre</version>
  </parent>
  <artifactId>guava</artifactId>
</project>"#;

        let parent = extract_parent_from_pom_content(content).unwrap();
        assert_eq!(parent.group_id, "com.google.guava");
        assert_eq!(parent.artifact_id, "guava-parent");
        assert_eq!(parent.version, "31.1-jre");
    }

    #[test]
    fn test_extract_parent_from_pom_content_none() {
        let content = r#"<project>
  <artifactId>standalone</artifactId>
</project>"#;
        assert!(extract_parent_from_pom_content(content).is_none());
    }

    #[test]
    fn test_extract_license_prefers_direct_over_parent() {
        // A POM that declares both a direct license and a parent: the direct
        // license wins, so resolution never needs to follow the parent.
        let content = r#"<project>
  <parent>
    <groupId>org.example</groupId>
    <artifactId>example-parent</artifactId>
    <version>1.0.0</version>
  </parent>
  <licenses>
    <license>
      <name>MIT License</name>
    </license>
  </licenses>
</project>"#;

        assert_eq!(
            extract_license_from_pom_content(content),
            Some("MIT".to_string())
        );
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
