//! CycloneDX XML, read as the JSON it stands for.
//!
//! CycloneDX defines one model with a JSON and an XML encoding, so an XML BOM is mapped onto the
//! JSON shape and ingest and validation read it with the same code. Only what those two read is
//! mapped: identity, licenses, the metadata the validator checks, and the dependency graph.
//!
//! An enriched copy of an XML input is written in XML by splicing the original text: a resolved
//! component's `<licenses>` is replaced, or inserted where the schema's sequence puts it, and every
//! other byte stays where it was.
//!
//! `feluda sbom cyclonedx --format xml` writes the same [`CycloneDxBom`] the JSON writer does, in
//! the element order each version's XSD requires.

use quick_xml::escape::{escape, unescape};
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;
use serde_json::{json, Map, Value as JsonValue};
use std::ops::Range;

use super::cyclonedx::{
    CycloneDxBom, CycloneDxComponent, CycloneDxLicenseChoice, CycloneDxToolsChoice,
};

/// The namespace every CycloneDX XML BOM declares, followed by its version.
const NAMESPACE: &str = "http://cyclonedx.org/schema/bom/";

/// An element, with its children, as the parser saw it.
#[derive(Debug, Default)]
struct Node {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|child| child.name == name)
    }

    fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |child| child.name == name)
    }

    fn child_text(&self, name: &str) -> Option<String> {
        self.child(name)
            .map(|child| child.text.trim().to_string())
            .filter(|text| !text.is_empty())
    }
}

/// Whether `content` is an XML document whose root is a CycloneDX `bom`.
pub fn looks_like_cyclonedx_xml(content: &str) -> bool {
    content.trim_start().starts_with('<') && root_start(content).is_some_and(|root| is_bom(&root))
}

/// The root element's start tag.
fn root_start(content: &str) -> Option<BytesStart<'static>> {
    let mut reader = Reader::from_str(content);
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) | Ok(Event::Empty(start)) => return Some(start.into_owned()),
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn is_bom(start: &BytesStart) -> bool {
    local_name(start.name().as_ref()) == "bom" && spec_version(start).is_some()
}

/// The version a `bom` element's namespace names: `1.6` for `http://cyclonedx.org/schema/bom/1.6`.
fn spec_version(start: &BytesStart) -> Option<String> {
    start.attributes().flatten().find_map(|attribute| {
        let key: &str = attribute.key.as_ref();
        if key != "xmlns" && !key.starts_with("xmlns:") {
            return None;
        }
        let value = attribute.normalized_value(XmlVersion::Implicit1_0).ok()?;
        value.strip_prefix(NAMESPACE).map(str::to_string)
    })
}

/// The name without its namespace prefix.
fn local_name(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_string()
}

/// Parse the whole document into a tree.
fn parse_tree(content: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(content);
    let mut stack: Vec<Node> = Vec::new();

    let open = |start: &BytesStart| -> Result<Node, String> {
        let mut attributes = Vec::new();
        for attribute in start.attributes() {
            let attribute = attribute.map_err(|e| format!("invalid XML attribute: {e}"))?;
            let key: &str = attribute.key.as_ref();
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            let value = attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|e| format!("invalid XML attribute value: {e}"))?;
            attributes.push((local_name(key), value.into_owned()));
        }
        Ok(Node {
            name: local_name(start.name().as_ref()),
            attributes,
            ..Default::default()
        })
    };

    loop {
        let event = reader
            .read_event()
            .map_err(|e| format!("invalid XML at byte {}: {e}", reader.error_position()))?;
        match event {
            Event::Start(start) => stack.push(open(&start)?),
            Event::Empty(start) => {
                let node = open(&start)?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => return Ok(node),
                }
            }
            Event::Text(text) => {
                if let Some(node) = stack.last_mut() {
                    let text = unescape(&text).map_err(|e| format!("invalid XML text: {e}"))?;
                    node.text.push_str(&text);
                }
            }
            Event::GeneralRef(reference) => {
                if let Some(node) = stack.last_mut() {
                    let name: &str = &reference;
                    if let Ok(text) = unescape(&format!("&{name};")) {
                        node.text.push_str(&text);
                    }
                }
            }
            Event::CData(data) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&data.into_inner());
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or("unbalanced XML end tag")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => return Ok(node),
                }
            }
            Event::Eof => return Err("XML document ended before its root element closed".into()),
            _ => {}
        }
    }
}

/// Parse a CycloneDX XML BOM into the CycloneDX JSON shape.
pub fn parse(content: &str) -> Result<JsonValue, String> {
    let root_tag = root_start(content).ok_or("not an XML document")?;
    let spec_version = spec_version(&root_tag).ok_or("the root element is not a CycloneDX bom")?;
    let root = parse_tree(content)?;

    let mut bom = Map::new();
    bom.insert("bomFormat".to_string(), json!("CycloneDX"));
    bom.insert("specVersion".to_string(), json!(spec_version));
    if let Some(serial) = root.attribute("serialNumber") {
        bom.insert("serialNumber".to_string(), json!(serial));
    }
    if let Some(version) = root.attribute("version") {
        let version = version
            .parse::<u64>()
            .map(JsonValue::from)
            .unwrap_or_else(|_| json!(version));
        bom.insert("version".to_string(), version);
    }
    if let Some(metadata) = root.child("metadata") {
        bom.insert("metadata".to_string(), metadata_json(metadata));
    }
    if let Some(components) = root.child("components") {
        bom.insert("components".to_string(), components_json(components));
    }
    if let Some(dependencies) = root.child("dependencies") {
        let dependencies: Vec<JsonValue> = dependencies
            .children("dependency")
            .map(dependency_json)
            .collect();
        bom.insert("dependencies".to_string(), JsonValue::Array(dependencies));
    }
    Ok(JsonValue::Object(bom))
}

fn metadata_json(metadata: &Node) -> JsonValue {
    let mut object = Map::new();
    if let Some(timestamp) = metadata.child_text("timestamp") {
        object.insert("timestamp".to_string(), json!(timestamp));
    }
    if let Some(lifecycles) = metadata.child("lifecycles") {
        // A lifecycle is a predefined `phase`, or a `name` and `description` of one's own.
        let lifecycles: Vec<JsonValue> = lifecycles
            .children("lifecycle")
            .map(|lifecycle| {
                let mut entry = Map::new();
                for field in ["phase", "name", "description"] {
                    if let Some(value) = lifecycle.child_text(field) {
                        entry.insert(field.to_string(), json!(value));
                    }
                }
                JsonValue::Object(entry)
            })
            .collect();
        object.insert("lifecycles".to_string(), JsonValue::Array(lifecycles));
    }
    if let Some(tools) = metadata.child("tools") {
        // 1.4 and earlier list `<tool>`s; 1.5 onwards may hold components and services instead.
        let legacy: Vec<JsonValue> = tools
            .children("tool")
            .map(|tool| {
                let mut entry = Map::new();
                for field in ["vendor", "name", "version"] {
                    if let Some(value) = tool.child_text(field) {
                        entry.insert(field.to_string(), json!(value));
                    }
                }
                JsonValue::Object(entry)
            })
            .collect();
        let tools_json = if legacy.is_empty() {
            let mut entry = Map::new();
            if let Some(components) = tools.child("components") {
                entry.insert("components".to_string(), components_json(components));
            }
            if let Some(services) = tools.child("services") {
                let services: Vec<JsonValue> = services
                    .children("service")
                    .map(|service| json!({ "name": service.child_text("name") }))
                    .collect();
                entry.insert("services".to_string(), JsonValue::Array(services));
            }
            JsonValue::Object(entry)
        } else {
            JsonValue::Array(legacy)
        };
        object.insert("tools".to_string(), tools_json);
    }
    if let Some(component) = metadata.child("component") {
        object.insert("component".to_string(), component_json(component));
    }
    JsonValue::Object(object)
}

fn components_json(components: &Node) -> JsonValue {
    JsonValue::Array(
        components
            .children("component")
            .map(component_json)
            .collect(),
    )
}

fn component_json(component: &Node) -> JsonValue {
    let mut object = Map::new();
    for attribute in ["type", "bom-ref", "mime-type"] {
        if let Some(value) = component.attribute(attribute) {
            object.insert(attribute.to_string(), json!(value));
        }
    }
    for field in [
        "group",
        "name",
        "version",
        "description",
        "scope",
        "copyright",
        "cpe",
        "purl",
        "author",
        "publisher",
    ] {
        if let Some(value) = component.child_text(field) {
            object.insert(field.to_string(), json!(value));
        }
    }
    if let Some(supplier) = component.child("supplier") {
        object.insert(
            "supplier".to_string(),
            json!({ "name": supplier.child_text("name") }),
        );
    }
    if let Some(licenses) = component.child("licenses") {
        object.insert("licenses".to_string(), licenses_json(licenses));
    }
    if let Some(hashes) = component.child("hashes") {
        let hashes: Vec<JsonValue> = hashes
            .children("hash")
            .map(|hash| json!({ "alg": hash.attribute("alg"), "content": hash.text.trim() }))
            .collect();
        object.insert("hashes".to_string(), JsonValue::Array(hashes));
    }
    if let Some(references) = component.child("externalReferences") {
        let references: Vec<JsonValue> = references
            .children("reference")
            .map(|reference| {
                json!({ "type": reference.attribute("type"), "url": reference.child_text("url") })
            })
            .collect();
        object.insert(
            "externalReferences".to_string(),
            JsonValue::Array(references),
        );
    }
    if let Some(properties) = component.child("properties") {
        let properties: Vec<JsonValue> = properties
            .children("property")
            .map(|property| {
                json!({ "name": property.attribute("name"), "value": property.text.trim() })
            })
            .collect();
        object.insert("properties".to_string(), JsonValue::Array(properties));
    }
    if let Some(nested) = component.child("components") {
        object.insert("components".to_string(), components_json(nested));
    }
    JsonValue::Object(object)
}

/// `<licenses>` holds `<license>` and `<expression>` elements, which JSON writes as
/// `{"license": {...}}` and `{"expression": "..."}`.
fn licenses_json(licenses: &Node) -> JsonValue {
    let mut entries = Vec::new();
    for child in &licenses.children {
        match child.name.as_str() {
            "license" => {
                let mut license = Map::new();
                for field in ["id", "name", "url"] {
                    if let Some(value) = child.child_text(field) {
                        license.insert(field.to_string(), json!(value));
                    }
                }
                for attribute in ["acknowledgement", "bom-ref"] {
                    if let Some(value) = child.attribute(attribute) {
                        license.insert(attribute.to_string(), json!(value));
                    }
                }
                entries.push(json!({ "license": license }));
            }
            "expression" => {
                let mut expression = Map::new();
                expression.insert("expression".to_string(), json!(child.text.trim()));
                for attribute in ["acknowledgement", "bom-ref"] {
                    if let Some(value) = child.attribute(attribute) {
                        expression.insert(attribute.to_string(), json!(value));
                    }
                }
                entries.push(JsonValue::Object(expression));
            }
            _ => {}
        }
    }
    JsonValue::Array(entries)
}

fn dependency_json(dependency: &Node) -> JsonValue {
    let depends_on: Vec<&str> = dependency
        .children("dependency")
        .filter_map(|child| child.attribute("ref"))
        .collect();
    json!({ "ref": dependency.attribute("ref"), "dependsOn": depends_on })
}

// =============================================================================
// ENRICHED COPY
// =============================================================================

/// Elements the schema's component sequence puts after `<licenses>`. A new `<licenses>` goes in
/// before the first of these, or before `</component>` when the component has none.
const AFTER_LICENSES: [&str; 18] = [
    "copyright",
    "cpe",
    "purl",
    "omniborId",
    "swhid",
    "swid",
    "modified",
    "pedigree",
    "externalReferences",
    "properties",
    "components",
    "evidence",
    "releaseNotes",
    "modelCard",
    "data",
    "cryptoProperties",
    "tags",
    "signature",
];

/// Where a top level component's licenses are, or where they would go.
#[derive(Debug, Default)]
struct ComponentSpan {
    /// The byte range of its `<licenses>` element.
    licenses: Option<Range<usize>>,
    /// The byte offset a new `<licenses>` is inserted at.
    insert_at: Option<usize>,
    /// The leading whitespace of its first child, to indent an inserted element the same way.
    child_indent: Option<String>,
    /// The namespace prefix its elements use, `cdx:` or nothing.
    prefix: String,
}

/// Find each top level `bom > components > component` in the original text.
fn component_spans(content: &str) -> Result<Vec<ComponentSpan>, String> {
    let mut reader = Reader::from_str(content);
    let mut path: Vec<String> = Vec::new();
    let mut spans: Vec<ComponentSpan> = Vec::new();

    loop {
        let before = reader.buffer_position() as usize;
        let event = reader
            .read_event()
            .map_err(|e| format!("invalid XML at byte {}: {e}", reader.error_position()))?;
        let after = reader.buffer_position() as usize;
        // Whitespace between the previous tag and this one belongs to this one's line.
        let tag_start = before + content[before..after].find('<').unwrap_or(0);

        let in_component = path.len() == 3 && path[1] == "components" && path[2] == "component";
        let is_empty = matches!(event, Event::Empty(_));
        match &event {
            Event::Start(start) | Event::Empty(start) => {
                let raw = start.name().as_ref().to_string();
                let name = local_name(&raw);
                if path.len() == 2 && path[1] == "components" && name == "component" {
                    let prefix = raw.strip_suffix("component").unwrap_or("").to_string();
                    spans.push(ComponentSpan {
                        prefix,
                        ..Default::default()
                    });
                }
                if in_component {
                    let span = spans.last_mut().ok_or("component without a span")?;
                    if span.child_indent.is_none() {
                        span.child_indent = Some(indent_before(content, tag_start));
                    }
                    if name == "licenses" {
                        span.licenses = Some(tag_start..tag_start);
                    } else if span.licenses.is_none()
                        && span.insert_at.is_none()
                        && AFTER_LICENSES.contains(&name.as_str())
                    {
                        span.insert_at = Some(tag_start);
                    }
                    if name == "licenses" && is_empty {
                        span.licenses = Some(tag_start..after);
                    }
                }
                if !is_empty {
                    path.push(name);
                }
            }
            Event::End(end) => {
                let name = local_name(end.name().as_ref());
                path.pop();
                let closed_child =
                    path.len() == 3 && path[1] == "components" && path[2] == "component";
                if closed_child && name == "licenses" {
                    if let Some(range) = spans.last_mut().and_then(|span| span.licenses.as_mut()) {
                        range.end = after;
                    }
                }
                let closed_component = path.len() == 2 && path[1] == "components";
                if closed_component && name == "component" {
                    let span = spans.last_mut().ok_or("component without a span")?;
                    if span.insert_at.is_none() {
                        span.insert_at = Some(tag_start);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(spans)
}

/// The whitespace between the start of the line and `offset`, when that is all there is.
fn indent_before(content: &str, offset: usize) -> String {
    let line_start = content[..offset].rfind('\n').map_or(0, |i| i + 1);
    let indent = &content[line_start..offset];
    if indent.chars().all(char::is_whitespace) {
        indent.to_string()
    } else {
        String::new()
    }
}

/// A license choice as a CycloneDX XML `<licenses>` element.
fn licenses_element(choice: &CycloneDxLicenseChoice, prefix: &str) -> String {
    let acknowledgement = |value: &Option<String>| {
        value
            .as_deref()
            .map(|value| format!(" acknowledgement=\"{}\"", escape(value)))
            .unwrap_or_default()
    };
    let inner = match choice {
        CycloneDxLicenseChoice::Expression {
            expression,
            acknowledgement: ack,
        } => format!(
            "<{prefix}expression{}>{}</{prefix}expression>",
            acknowledgement(ack),
            escape(expression.as_str())
        ),
        CycloneDxLicenseChoice::License { license } => {
            let mut fields = String::new();
            if let Some(id) = &license.id {
                fields.push_str(&format!("<{prefix}id>{}</{prefix}id>", escape(id.as_str())));
            }
            if let Some(name) = &license.name {
                fields.push_str(&format!(
                    "<{prefix}name>{}</{prefix}name>",
                    escape(name.as_str())
                ));
            }
            if let Some(url) = &license.url {
                fields.push_str(&format!(
                    "<{prefix}url>{}</{prefix}url>",
                    escape(url.as_str())
                ));
            }
            format!(
                "<{prefix}license{}>{fields}</{prefix}license>",
                acknowledgement(&license.acknowledgement)
            )
        }
    };
    format!("<{prefix}licenses>{inner}</{prefix}licenses>")
}

/// The original text with the given components' licenses set.
///
/// `licenses` pairs a component's position among the top level components with what to state.
pub fn patch(
    content: &str,
    licenses: &[(usize, CycloneDxLicenseChoice)],
) -> Result<String, String> {
    let spans = component_spans(content)?;

    // Every edit is a byte range and its replacement; applied back to front, earlier offsets
    // stay valid.
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for (position, choice) in licenses {
        let Some(span) = spans.get(*position) else {
            continue;
        };
        let element = licenses_element(choice, &span.prefix);
        if let Some(range) = &span.licenses {
            edits.push((range.clone(), element));
        } else if let Some(offset) = span.insert_at {
            let indent = span.child_indent.clone().unwrap_or_default();
            let line_start = content[..offset].rfind('\n').map_or(0, |i| i + 1);
            if !indent.is_empty() && indent_before(content, offset).len() == offset - line_start {
                // Its own line, indented like the component's other children.
                edits.push((line_start..line_start, format!("{indent}{element}\n")));
            } else {
                edits.push((offset..offset, element));
            }
        }
    }

    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut output = content.to_string();
    for (range, replacement) in edits {
        output.replace_range(range, &replacement);
    }
    Ok(output)
}

// =============================================================================
// WRITING
// =============================================================================

/// Builds an indented XML document one line at a time.
struct XmlWriter {
    out: String,
    depth: usize,
}

impl XmlWriter {
    fn line(&mut self, text: &str) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn open(&mut self, tag: &str) {
        self.line(&format!("<{tag}>"));
        self.depth += 1;
    }

    fn close(&mut self, name: &str) {
        self.depth -= 1;
        self.line(&format!("</{name}>"));
    }

    /// `<name>text</name>`, escaped.
    fn text(&mut self, name: &str, text: &str) {
        self.line(&format!("<{name}>{}</{name}>", escape(text)));
    }

    fn optional(&mut self, name: &str, text: &Option<String>) {
        if let Some(text) = text {
            self.text(name, text);
        }
    }
}

/// A BOM as CycloneDX XML.
///
/// Elements follow the order the XSD's sequences give them, since XML Schema rejects any other.
/// What the JSON writer leaves out because it is empty, this leaves out too.
pub fn write(bom: &CycloneDxBom) -> String {
    let mut xml = XmlWriter {
        out: String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"),
        depth: 0,
    };

    let mut root = format!(
        "bom xmlns=\"{NAMESPACE}{}\"",
        escape(bom.spec_version.as_str())
    );
    if let Some(serial) = &bom.serial_number {
        root.push_str(&format!(" serialNumber=\"{}\"", escape(serial.as_str())));
    }
    if let Some(version) = bom.version {
        root.push_str(&format!(" version=\"{version}\""));
    }
    xml.open(&root);

    if let Some(metadata) = &bom.metadata {
        xml.open("metadata");
        if let Some(timestamp) = &metadata.timestamp {
            xml.text(
                "timestamp",
                &timestamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            );
        }
        if !metadata.lifecycles.is_empty() {
            xml.open("lifecycles");
            for lifecycle in &metadata.lifecycles {
                xml.open("lifecycle");
                xml.text("phase", &lifecycle.phase);
                xml.close("lifecycle");
            }
            xml.close("lifecycles");
        }
        match &metadata.tools {
            Some(CycloneDxToolsChoice::Legacy(tools)) if !tools.is_empty() => {
                xml.open("tools");
                for tool in tools {
                    xml.open("tool");
                    xml.optional("vendor", &tool.vendor);
                    xml.text("name", &tool.name);
                    xml.optional("version", &tool.version);
                    xml.close("tool");
                }
                xml.close("tools");
            }
            Some(CycloneDxToolsChoice::Nested(tools))
                if !tools.components.is_empty() || !tools.services.is_empty() =>
            {
                xml.open("tools");
                if !tools.components.is_empty() {
                    xml.open("components");
                    for tool in &tools.components {
                        xml.open(&format!(
                            "component type=\"{}\"",
                            escape(tool.component_type.as_str())
                        ));
                        xml.text("name", &tool.name);
                        xml.optional("version", &tool.version);
                        xml.close("component");
                    }
                    xml.close("components");
                }
                if !tools.services.is_empty() {
                    xml.open("services");
                    for service in &tools.services {
                        xml.open("service");
                        xml.text("name", &service.name);
                        xml.optional("version", &service.version);
                        xml.close("service");
                    }
                    xml.close("services");
                }
                xml.close("tools");
            }
            _ => {}
        }
        if !metadata.authors.is_empty() {
            xml.open("authors");
            for author in &metadata.authors {
                xml.open("author");
                xml.optional("name", &author.name);
                xml.optional("email", &author.email);
                xml.close("author");
            }
            xml.close("authors");
        }
        if let Some(component) = &metadata.component {
            write_component(&mut xml, component);
        }
        xml.close("metadata");
    }

    if !bom.components.is_empty() {
        xml.open("components");
        for component in &bom.components {
            write_component(&mut xml, component);
        }
        xml.close("components");
    }

    xml.close("bom");
    xml.out
}

/// A component, its children in the schema's order: name, version, description, scope,
/// licenses, copyright, purl, externalReferences.
fn write_component(xml: &mut XmlWriter, component: &CycloneDxComponent) {
    xml.open(&format!(
        "component type=\"{}\"",
        escape(component.component_type.as_str())
    ));
    xml.text("name", &component.name);
    xml.optional("version", &component.version);
    xml.optional("description", &component.description);
    xml.optional("scope", &component.scope);
    if !component.licenses.is_empty() {
        xml.open("licenses");
        for license in &component.licenses {
            // One choice is one line, the same element an enriched copy splices in.
            let element = licenses_element(license, "");
            let inner = element
                .strip_prefix("<licenses>")
                .and_then(|element| element.strip_suffix("</licenses>"))
                .unwrap_or(&element);
            xml.line(inner);
        }
        xml.close("licenses");
    }
    xml.optional("copyright", &component.copyright);
    xml.optional("purl", &component.purl);
    if !component.external_references.is_empty() {
        xml.open("externalReferences");
        for reference in &component.external_references {
            xml.open(&format!(
                "reference type=\"{}\"",
                escape(reference.ref_type.as_str())
            ));
            xml.text("url", &reference.url);
            xml.optional("comment", &reference.comment);
            xml.close("reference");
        }
        xml.close("externalReferences");
    }
    xml.close("component");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sbom::cyclonedx::CycloneDxLicense;

    const BOM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<bom xmlns="http://cyclonedx.org/schema/bom/1.6" serialNumber="urn:uuid:3e671687-395b-41f5-a30f-a58921a69b79" version="1">
  <metadata>
    <timestamp>2026-10-07T10:00:00Z</timestamp>
    <tools>
      <components>
        <component type="application">
          <name>cdxgen</name>
          <version>12.3.0</version>
        </component>
      </components>
    </tools>
    <component type="application" bom-ref="app">
      <name>app</name>
    </component>
  </metadata>
  <components>
    <component type="library" bom-ref="pkg:npm/%40babel/core@7.24.0">
      <group>@babel</group>
      <name>core</name>
      <version>7.24.0</version>
      <licenses>
        <license acknowledgement="declared"><id>MIT</id></license>
      </licenses>
      <purl>pkg:npm/%40babel/core@7.24.0</purl>
    </component>
    <component type="library">
      <name>jackson-databind</name>
      <version>2.17.0</version>
      <licenses>
        <expression>Apache-2.0 OR LGPL-2.1-only</expression>
      </licenses>
      <purl>pkg:maven/com.fasterxml.jackson.core/jackson-databind@2.17.0</purl>
      <components>
        <component type="library"><name>nested</name></component>
      </components>
    </component>
    <component type="library">
      <name>mystery &amp; co</name>
      <version>1.0.0</version>
      <purl>pkg:cargo/mystery@1.0.0</purl>
    </component>
    <component type="library">
      <name>last</name>
    </component>
  </components>
  <dependencies>
    <dependency ref="app">
      <dependency ref="pkg:npm/%40babel/core@7.24.0"/>
    </dependency>
  </dependencies>
</bom>
"#;

    #[test]
    fn test_detects_cyclonedx_xml() {
        assert!(looks_like_cyclonedx_xml(BOM));
        assert!(!looks_like_cyclonedx_xml(
            "<project><name>x</name></project>"
        ));
        assert!(!looks_like_cyclonedx_xml("{\"bomFormat\": \"CycloneDX\"}"));
    }

    #[test]
    fn test_parses_into_the_json_shape() {
        let bom = parse(BOM).unwrap();
        assert_eq!(bom["bomFormat"], "CycloneDX");
        assert_eq!(bom["specVersion"], "1.6");
        assert_eq!(bom["version"], 1);
        assert_eq!(bom["metadata"]["timestamp"], "2026-10-07T10:00:00Z");
        assert_eq!(bom["metadata"]["tools"]["components"][0]["name"], "cdxgen");
        assert_eq!(bom["metadata"]["component"]["name"], "app");

        let components = bom["components"].as_array().unwrap();
        assert_eq!(components.len(), 4);
        assert_eq!(components[0]["type"], "library");
        assert_eq!(components[0]["group"], "@babel");
        assert_eq!(components[0]["purl"], "pkg:npm/%40babel/core@7.24.0");
        assert_eq!(
            components[0]["licenses"],
            json!([{ "license": { "id": "MIT", "acknowledgement": "declared" } }])
        );
        assert_eq!(
            components[1]["licenses"],
            json!([{ "expression": "Apache-2.0 OR LGPL-2.1-only" }])
        );
        assert_eq!(components[1]["components"][0]["name"], "nested");
        assert_eq!(components[2]["name"], "mystery & co");
        assert_eq!(
            bom["dependencies"][0]["dependsOn"],
            json!(["pkg:npm/%40babel/core@7.24.0"])
        );
    }

    #[test]
    fn test_legacy_tools_and_prefixed_namespaces() {
        let bom = parse(
            r#"<cdx:bom xmlns:cdx="http://cyclonedx.org/schema/bom/1.4" version="1">
<cdx:metadata><cdx:tools><cdx:tool><cdx:vendor>Acme</cdx:vendor><cdx:name>scan</cdx:name></cdx:tool></cdx:tools></cdx:metadata>
<cdx:components><cdx:component type="library"><cdx:name>a</cdx:name></cdx:component></cdx:components>
</cdx:bom>"#,
        )
        .unwrap();
        assert_eq!(bom["specVersion"], "1.4");
        assert_eq!(
            bom["metadata"]["tools"],
            json!([{ "vendor": "Acme", "name": "scan" }])
        );
        assert_eq!(bom["components"][0]["name"], "a");
    }

    #[test]
    fn test_malformed_xml_is_an_error() {
        let error = parse(r#"<bom xmlns="http://cyclonedx.org/schema/bom/1.6"><components></bom>"#)
            .unwrap_err();
        assert!(error.contains("invalid XML"), "{error}");
    }

    fn id(id: &str, acknowledgement: Option<&str>) -> CycloneDxLicenseChoice {
        CycloneDxLicenseChoice::License {
            license: CycloneDxLicense {
                id: Some(id.to_string()),
                name: None,
                url: None,
                acknowledgement: acknowledgement.map(str::to_string),
            },
        }
    }

    #[test]
    fn test_patch_replaces_and_inserts_in_schema_order() {
        let patched = patch(
            BOM,
            &[
                (1, id("MIT", Some("concluded"))),
                (2, id("Apache-2.0", None)),
                (
                    3,
                    CycloneDxLicenseChoice::Expression {
                        expression: "MIT OR Apache-2.0".to_string(),
                        acknowledgement: None,
                    },
                ),
            ],
        )
        .unwrap();

        // Replaced where it stood.
        assert!(patched.contains(
            "<version>2.17.0</version>\n      <licenses><license acknowledgement=\"concluded\"><id>MIT</id></license></licenses>\n      <purl>"
        ));
        assert!(!patched.contains("LGPL-2.1-only"));
        // Inserted before `<purl>`, which the schema orders after `<licenses>`.
        assert!(patched.contains(
            "<version>1.0.0</version>\n      <licenses><license><id>Apache-2.0</id></license></licenses>\n      <purl>pkg:cargo/mystery"
        ));
        // Inserted before the end tag when nothing follows.
        assert!(patched.contains(
            "<name>last</name>\n      <licenses><expression>MIT OR Apache-2.0</expression></licenses>\n    </component>"
        ));
        // The nested component and the untouched first one are as they were.
        assert!(patched.contains("<component type=\"library\"><name>nested</name></component>"));
        assert!(patched.contains("<license acknowledgement=\"declared\"><id>MIT</id></license>"));

        let reparsed = parse(&patched).unwrap();
        assert_eq!(
            reparsed["components"][2]["licenses"][0]["license"]["id"],
            "Apache-2.0"
        );
        assert_eq!(reparsed["components"][2]["name"], "mystery & co");
    }

    #[test]
    fn test_patch_escapes_names() {
        let choice = CycloneDxLicenseChoice::License {
            license: CycloneDxLicense {
                id: None,
                name: Some("Acme <Commercial> & Co".to_string()),
                url: None,
                acknowledgement: None,
            },
        };
        let patched = patch(BOM, &[(3, choice)]).unwrap();
        assert!(patched.contains("<name>Acme &lt;Commercial&gt; &amp; Co</name>"));
        assert_eq!(
            parse(&patched).unwrap()["components"][3]["licenses"][0]["license"]["name"],
            "Acme <Commercial> & Co"
        );
    }

    #[test]
    fn test_write_round_trips_through_parse() {
        use crate::sbom::spdx::{SpdxDocument, SpdxPackage};
        use crate::sbom::CycloneDxVersion;

        let mut document = SpdxDocument::new("demo");
        for (name, license) in [
            ("listed", "MIT"),
            ("either", "MIT OR Apache-2.0"),
            ("titled", "Acme <Commercial> & Co"),
        ] {
            document.add_package(
                SpdxPackage::new(name, &document.document_namespace)
                    .with_version("1.0.0")
                    .with_purl(format!("pkg:npm/{name}@1.0.0"))
                    .with_license(license),
            );
        }

        document.sbom_type = Some(crate::sbom::spdx::SbomKind::Source);
        for version in [CycloneDxVersion::V1_4, CycloneDxVersion::V1_6] {
            let bom = crate::sbom::cyclonedx::convert_spdx_to_cyclonedx(&document, version);
            let written = write(&bom);
            assert!(written.starts_with(&format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<bom xmlns=\"http://cyclonedx.org/schema/bom/{}\"",
                version.as_str()
            )));

            // Read back, it is the JSON writer's BOM.
            let parsed = parse(&written).unwrap();
            let json = serde_json::to_value(&bom).unwrap();
            assert_eq!(parsed["specVersion"], json["specVersion"]);
            assert_eq!(parsed["serialNumber"], json["serialNumber"]);
            for (read, expected) in parsed["components"]
                .as_array()
                .unwrap()
                .iter()
                .zip(json["components"].as_array().unwrap())
            {
                for field in ["type", "name", "version", "scope", "purl", "licenses"] {
                    assert_eq!(read[field], expected[field], "{field} in {written}");
                }
            }
            assert_eq!(
                parsed["metadata"].get("lifecycles"),
                json["metadata"].get("lifecycles")
            );
            let tools = &parsed["metadata"]["tools"];
            if version == CycloneDxVersion::V1_4 {
                assert_eq!(tools[0]["name"], "feluda");
            } else {
                assert_eq!(tools["components"][0]["name"], "feluda");
            }
        }
    }
}
