//! The stanza format Debian tooling writes everything in.
//!
//! `/var/lib/dpkg/status` and the machine readable `copyright` file are the same shape: records
//! separated by blank lines, each record a list of `Field: value` lines, with continuation lines
//! indented by one space. One parser serves both.

/// A single record: its fields in the order they appeared.
///
/// Field names are matched case-insensitively, because Debian's own tooling does. Values keep
/// their continuation lines, joined with newlines, since a DEP-5 `License` field carries the short
/// name on the first line and the full license text underneath.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Stanza {
    fields: Vec<(String, String)>,
}

impl Stanza {
    /// The value of a field, or `None` when the stanza does not carry it.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Every value of a field, in the order the fields appeared.
    ///
    /// Debian's own files state each field once, but the Python metadata that reuses this format
    /// repeats `Classifier` freely, and only the whole set says what a distribution is.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.fields
            .iter()
            .filter(move |(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The first line of a field, which is the part that carries the value when the rest of the
    /// field is prose: a `License` short name above its text, a `Description` summary above its
    /// long form.
    pub fn first_line(&self, name: &str) -> Option<&str> {
        self.get(name)
            .and_then(|value| value.lines().next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// Split a deb822 document into its stanzas.
///
/// Malformed lines are skipped rather than failing the parse: these files come off other people's
/// systems, and one unreadable line in `/var/lib/dpkg/status` should not cost the report every
/// package after it.
pub fn parse_stanzas(content: &str) -> Vec<Stanza> {
    let mut stanzas = Vec::new();
    let mut current = Stanza::default();

    for line in content.lines() {
        // A comment, which DEP-5 permits anywhere.
        if line.starts_with('#') {
            continue;
        }

        if line.trim().is_empty() {
            if !current.is_empty() {
                stanzas.push(std::mem::take(&mut current));
            }
            continue;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            // A continuation line. `.` alone marks a deliberately blank line in the value.
            let continuation = line.trim();
            let continuation = if continuation == "." {
                ""
            } else {
                continuation
            };
            if let Some((_, value)) = current.fields.last_mut() {
                value.push('\n');
                value.push_str(continuation);
            }
            continue;
        }

        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            if !name.is_empty() {
                current
                    .fields
                    .push((name.to_string(), value.trim().to_string()));
            }
        }
    }

    if !current.is_empty() {
        stanzas.push(current);
    }
    stanzas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_splits_on_blank_lines() {
        let stanzas = parse_stanzas("Package: one\nVersion: 1\n\nPackage: two\nVersion: 2\n");
        assert_eq!(stanzas.len(), 2);
        assert_eq!(stanzas[0].get("Package"), Some("one"));
        assert_eq!(stanzas[1].get("Version"), Some("2"));
    }

    #[test]
    fn test_field_lookup_is_case_insensitive() {
        let stanzas = parse_stanzas("Package: one\n");
        assert_eq!(stanzas[0].get("package"), Some("one"));
        assert_eq!(stanzas[0].get("PACKAGE"), Some("one"));
        assert_eq!(stanzas[0].get("Source"), None);
    }

    #[test]
    fn test_continuation_lines_join_into_the_value() {
        // A DEP-5 License field: short name first, licence text underneath, with `.` for the
        // blank lines that would otherwise end the stanza.
        let stanzas =
            parse_stanzas("License: GPL-2+\n This program is free software.\n .\n See it.\n");
        assert_eq!(
            stanzas[0].get("License"),
            Some("GPL-2+\nThis program is free software.\n\nSee it.")
        );
        assert_eq!(stanzas[0].first_line("License"), Some("GPL-2+"));
    }

    #[test]
    fn test_every_value_of_a_repeated_field_is_available() {
        let stanzas = parse_stanzas(
            "Name: pkg\n\
             Classifier: Programming Language :: Python :: 3\n\
             Classifier: License :: OSI Approved :: MIT License\n",
        );
        let classifiers: Vec<&str> = stanzas[0].all("Classifier").collect();
        assert_eq!(
            classifiers,
            [
                "Programming Language :: Python :: 3",
                "License :: OSI Approved :: MIT License"
            ]
        );
        // `get` still answers with the first, which is what the single-valued callers want.
        assert_eq!(
            stanzas[0].get("classifier"),
            Some("Programming Language :: Python :: 3")
        );
        assert_eq!(stanzas[0].all("Source").count(), 0);
    }

    #[test]
    fn test_repeated_blank_lines_do_not_make_empty_stanzas() {
        let stanzas = parse_stanzas("\n\nPackage: one\n\n\n\nPackage: two\n\n");
        assert_eq!(stanzas.len(), 2);
    }

    #[test]
    fn test_unparseable_lines_are_skipped() {
        let stanzas = parse_stanzas("Package: one\nthis line has no colon\nVersion: 1\n");
        assert_eq!(stanzas.len(), 1);
        assert_eq!(stanzas[0].get("Package"), Some("one"));
        assert_eq!(stanzas[0].get("Version"), Some("1"));
    }

    #[test]
    fn test_comments_are_ignored() {
        let stanzas = parse_stanzas("# a comment\nPackage: one\n");
        assert_eq!(stanzas[0].get("Package"), Some("one"));
    }

    #[test]
    fn test_empty_document_has_no_stanzas() {
        assert!(parse_stanzas("").is_empty());
        assert!(parse_stanzas("\n\n\n").is_empty());
    }
}
