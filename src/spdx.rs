//! SPDX expression parsing and evaluation.
//!
//! Handles compound expressions like `MIT OR Apache-2.0`, `(MIT AND BSD-2-Clause)`,
//! and `GPL-2.0-only WITH Classpath-exception-2.0`.
//!
//! Operator semantics used by Feluda:
//!   - `OR`  — user may choose any alternative; compatible/non-restrictive if ANY component qualifies.
//!   - `AND` — all licenses apply simultaneously; compatible/non-restrictive only if ALL qualify.
//!   - `WITH`— exception modifier; treated as an annotation on the base license.

/// A parsed SPDX expression tree.
#[derive(Debug, Clone, PartialEq)]
pub enum SpdxExpression {
    License(String),
    With { license: String, exception: String },
    Or(Box<SpdxExpression>, Box<SpdxExpression>),
    And(Box<SpdxExpression>, Box<SpdxExpression>),
}

impl SpdxExpression {
    /// Returns all individual license IDs mentioned in the expression (no exceptions).
    #[allow(dead_code)]
    pub fn license_ids(&self) -> Vec<String> {
        match self {
            Self::License(id) => vec![id.clone()],
            Self::With { license, .. } => vec![license.clone()],
            Self::Or(a, b) | Self::And(a, b) => {
                let mut ids = a.license_ids();
                ids.extend(b.license_ids());
                ids
            }
        }
    }
}

/// Parse an SPDX expression string into an [`SpdxExpression`] tree.
///
/// Returns the original string wrapped in `License` if parsing fails, so call
/// sites degrade gracefully rather than erroring out.
pub fn parse(input: &str) -> SpdxExpression {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return SpdxExpression::License(input.to_string());
    }

    let tokens = tokenize(trimmed);
    let mut pos = 0;
    parse_or_expr(&tokens, &mut pos).unwrap_or_else(|| SpdxExpression::License(input.to_string()))
}

/// Strictly parse an SPDX expression, returning `None` when the input is not a
/// well-formed expression — unlike [`parse`], which degrades to a literal `License`
/// so lenient call sites never error.
///
/// "Well-formed" requires every token to be consumed, so bare prose such as
/// `header value` (two ids with no operator between them) is rejected. That makes
/// this the right validator for source-header tag values, where the text after the
/// `SPDX-License-Identifier:` marker might be a real expression or just a sentence
/// that happens to mention it.
pub fn parse_strict(input: &str) -> Option<SpdxExpression> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let tokens = tokenize(trimmed);
    let mut pos = 0;
    let expr = parse_or_expr(&tokens, &mut pos)?;

    // Reject when the parser stopped before consuming every token: leftover tokens
    // mean the input was prose or otherwise malformed, not a valid expression.
    (pos == tokens.len()).then_some(expr)
}

/// Returns `true` when `input` looks like a compound SPDX expression (contains
/// ` OR `, ` AND `, ` WITH `, or parentheses) rather than a plain license ID.
pub fn is_compound(input: &str) -> bool {
    input.contains(" OR ")
        || input.contains(" AND ")
        || input.contains(" WITH ")
        || input.contains('(')
}

// ── The SPDX license list ────────────────────────────────────────────────────

/// The SPDX license and exception ids, as CycloneDX's schema lists them for `license.id`.
const LISTED_IDS: &str = include_str!("../config/spdx_license_ids.txt");

/// Every listed id, keyed by its lowercase spelling, since SPDX matches ids case insensitively but
/// a document has to spell them the way the list does.
fn listed_ids() -> &'static std::collections::HashMap<String, &'static str> {
    static IDS: std::sync::OnceLock<std::collections::HashMap<String, &'static str>> =
        std::sync::OnceLock::new();
    IDS.get_or_init(|| {
        LISTED_IDS
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|id| (id.to_ascii_lowercase(), id))
            .collect()
    })
}

/// The SPDX list's spelling of `id`, or `None` when the list has no such license or exception.
///
/// `mit` gives `MIT`. A free form name such as `SEE LICENSE IN LICENSE.txt`, or an id the list
/// does not carry yet, gives `None`: CycloneDX only accepts listed ids as `license.id`, so those
/// belong in `license.name`.
pub fn listed_id(id: &str) -> Option<&'static str> {
    listed_ids().get(&id.trim().to_ascii_lowercase()).copied()
}

/// The list's spelling of a license id inside an expression, or `None` when it is not listed.
///
/// Document local `LicenseRef-` and `DocumentRef-` ids are kept as they are, since SPDX allows
/// them anywhere in an expression, and a trailing `+` ("or later") is allowed on a listed license.
fn listed_license(id: &str) -> Option<String> {
    if id.starts_with("LicenseRef-") || id.starts_with("DocumentRef-") {
        return Some(id.to_string());
    }
    listed_id(id).map(str::to_string).or_else(|| {
        let base = id.strip_suffix('+')?;
        listed_id(base).map(|base| format!("{base}+"))
    })
}

/// Rewrite a well formed expression with every license in the list's spelling.
///
/// A license the list does not carry is handed to `unlisted`, whose answer takes its place, or
/// which rejects the whole expression by returning `None`. Exceptions have no document local
/// form in SPDX 2.x, so an unlisted exception always rejects it. `None` also means the input is
/// not an expression at all: prose such as `SEE LICENSE IN LICENSE.txt`, a dangling operator or an
/// unclosed parenthesis.
pub fn rewrite_expression(
    input: &str,
    mut unlisted: impl FnMut(&str) -> Option<String>,
) -> Option<String> {
    let tokens = tokenize(input.trim());
    // The parser forgives a `WITH` with nothing after it and an unclosed parenthesis, so both are
    // ruled out before its verdict is trusted.
    let balanced = tokens.iter().filter(|t| **t == Token::LParen).count()
        == tokens.iter().filter(|t| **t == Token::RParen).count();
    let dangling = matches!(tokens.last(), Some(Token::Or | Token::And | Token::With));
    if !balanced || dangling || parse_strict(input).is_none() {
        return None;
    }

    let mut written = String::new();
    let mut previous: Option<&Token> = None;
    for token in &tokens {
        let text = match token {
            Token::Id(id) if previous == Some(&Token::With) => listed_id(id)?.to_string(),
            Token::Id(id) => match listed_license(id) {
                Some(license) => license,
                None => unlisted(id)?,
            },
            Token::Or => "OR".to_string(),
            Token::And => "AND".to_string(),
            Token::With => "WITH".to_string(),
            Token::LParen => "(".to_string(),
            Token::RParen => ")".to_string(),
        };
        let joins = matches!(previous, None | Some(Token::LParen)) || *token == Token::RParen;
        if !joins {
            written.push(' ');
        }
        written.push_str(&text);
        previous = Some(token);
    }
    Some(written)
}

/// The expression with every license in the list's spelling, or `None` unless every license and
/// exception in it is listed. A single id counts as an expression.
pub fn listed_expression(input: &str) -> Option<String> {
    rewrite_expression(input, |_| None)
}

// ── Tokeniser ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Id(String),
    Or,
    And,
    With,
    LParen,
    RParen,
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            ' ' | '\t' => {
                chars.next();
            }
            _ => {
                // Peek-based accumulation so delimiters are never consumed by this branch.
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c == ' ' || c == '\t' || c == '(' || c == ')' {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                match word.as_str() {
                    "OR" => tokens.push(Token::Or),
                    "AND" => tokens.push(Token::And),
                    "WITH" => tokens.push(Token::With),
                    _ => tokens.push(Token::Id(word)),
                }
            }
        }
    }
    tokens
}

// ── Recursive descent parser ─────────────────────────────────────────────────

fn parse_or_expr(tokens: &[Token], pos: &mut usize) -> Option<SpdxExpression> {
    let mut left = parse_and_expr(tokens, pos)?;

    while *pos < tokens.len() {
        if tokens[*pos] == Token::Or {
            *pos += 1;
            let right = parse_and_expr(tokens, pos)?;
            left = SpdxExpression::Or(Box::new(left), Box::new(right));
        } else {
            break;
        }
    }
    Some(left)
}

fn parse_and_expr(tokens: &[Token], pos: &mut usize) -> Option<SpdxExpression> {
    let mut left = parse_with_expr(tokens, pos)?;

    while *pos < tokens.len() {
        if tokens[*pos] == Token::And {
            *pos += 1;
            let right = parse_with_expr(tokens, pos)?;
            left = SpdxExpression::And(Box::new(left), Box::new(right));
        } else {
            break;
        }
    }
    Some(left)
}

fn parse_with_expr(tokens: &[Token], pos: &mut usize) -> Option<SpdxExpression> {
    let base = parse_primary(tokens, pos)?;

    if *pos < tokens.len() && tokens[*pos] == Token::With {
        *pos += 1;
        if let Some(Token::Id(exception)) = tokens.get(*pos) {
            let exception = exception.clone();
            *pos += 1;
            if let SpdxExpression::License(license) = base {
                return Some(SpdxExpression::With { license, exception });
            }
        }
    }
    Some(base)
}

fn parse_primary(tokens: &[Token], pos: &mut usize) -> Option<SpdxExpression> {
    match tokens.get(*pos)? {
        Token::LParen => {
            *pos += 1;
            let expr = parse_or_expr(tokens, pos)?;
            if tokens.get(*pos) == Some(&Token::RParen) {
                *pos += 1;
            }
            Some(expr)
        }
        Token::Id(id) => {
            let id = id.clone();
            *pos += 1;
            Some(SpdxExpression::License(id))
        }
        _ => None,
    }
}

// ── Compatibility / restrictiveness evaluation ────────────────────────────────

/// Evaluate compatibility of an SPDX expression against the project license.
///
/// - `OR`  → compatible if ANY branch is compatible.
/// - `AND` → compatible only if ALL branches are compatible.
/// - Plain or `WITH` → delegate to the base license check.
pub fn expression_compatibility(
    expr: &SpdxExpression,
    project_license: &str,
    strict: bool,
    check_fn: &dyn Fn(&str, &str, bool) -> crate::licenses::LicenseCompatibility,
) -> crate::licenses::LicenseCompatibility {
    use crate::licenses::LicenseCompatibility;

    match expr {
        SpdxExpression::License(id) => check_fn(id, project_license, strict),
        SpdxExpression::With { license, .. } => check_fn(license, project_license, strict),

        SpdxExpression::Or(a, b) => {
            let ca = expression_compatibility(a, project_license, strict, check_fn);
            let cb = expression_compatibility(b, project_license, strict, check_fn);
            match (ca, cb) {
                (LicenseCompatibility::Compatible, _) | (_, LicenseCompatibility::Compatible) => {
                    LicenseCompatibility::Compatible
                }
                (LicenseCompatibility::Unknown, _) | (_, LicenseCompatibility::Unknown) => {
                    LicenseCompatibility::Unknown
                }
                _ => LicenseCompatibility::Incompatible,
            }
        }

        SpdxExpression::And(a, b) => {
            let ca = expression_compatibility(a, project_license, strict, check_fn);
            let cb = expression_compatibility(b, project_license, strict, check_fn);
            match (ca, cb) {
                (LicenseCompatibility::Incompatible, _)
                | (_, LicenseCompatibility::Incompatible) => LicenseCompatibility::Incompatible,
                (LicenseCompatibility::Compatible, LicenseCompatibility::Compatible) => {
                    LicenseCompatibility::Compatible
                }
                _ => LicenseCompatibility::Unknown,
            }
        }
    }
}

/// Evaluate restrictiveness of an SPDX expression.
///
/// - `OR`  → not restrictive if ANY branch is not restrictive (user can choose the permissive option).
/// - `AND` → restrictive if ANY branch is restrictive (all licenses apply).
pub fn expression_is_restrictive(expr: &SpdxExpression, check_fn: &dyn Fn(&str) -> bool) -> bool {
    match expr {
        SpdxExpression::License(id) => check_fn(id),
        SpdxExpression::With { license, .. } => check_fn(license),
        SpdxExpression::Or(a, b) => {
            expression_is_restrictive(a, check_fn) && expression_is_restrictive(b, check_fn)
        }
        SpdxExpression::And(a, b) => {
            expression_is_restrictive(a, check_fn) || expression_is_restrictive(b, check_fn)
        }
    }
}

/// Evaluate OSI status of an SPDX expression.
///
/// - `OR`  → approved if ANY branch is approved.
/// - `AND` → approved only if ALL branches are approved.
pub fn expression_osi_status(
    expr: &SpdxExpression,
    check_fn: &dyn Fn(&str) -> crate::licenses::OsiStatus,
) -> crate::licenses::OsiStatus {
    use crate::licenses::OsiStatus;

    match expr {
        SpdxExpression::License(id) => check_fn(id),
        SpdxExpression::With { license, .. } => check_fn(license),

        SpdxExpression::Or(a, b) => {
            let sa = expression_osi_status(a, check_fn);
            let sb = expression_osi_status(b, check_fn);
            match (sa, sb) {
                (OsiStatus::Approved, _) | (_, OsiStatus::Approved) => OsiStatus::Approved,
                (OsiStatus::Unknown, _) | (_, OsiStatus::Unknown) => OsiStatus::Unknown,
                _ => OsiStatus::NotApproved,
            }
        }

        SpdxExpression::And(a, b) => {
            let sa = expression_osi_status(a, check_fn);
            let sb = expression_osi_status(b, check_fn);
            match (sa, sb) {
                (OsiStatus::NotApproved, _) | (_, OsiStatus::NotApproved) => OsiStatus::NotApproved,
                (OsiStatus::Approved, OsiStatus::Approved) => OsiStatus::Approved,
                _ => OsiStatus::Unknown,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_bundled_list_is_well_formed() {
        let ids: Vec<&str> = LISTED_IDS
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        // Every id once, whatever its case, or lookups would depend on which spelling came last.
        assert_eq!(listed_ids().len(), ids.len());
        assert!(
            ids.len() > 600,
            "only {} ids; is the list truncated?",
            ids.len()
        );
        for id in &ids {
            assert_eq!(id.trim(), *id);
            assert!(!id.contains(' '), "{id:?}");
        }
    }

    #[test]
    fn test_listed_id_uses_the_lists_spelling() {
        assert_eq!(listed_id("MIT"), Some("MIT"));
        assert_eq!(listed_id("mit"), Some("MIT"));
        assert_eq!(listed_id(" apache-2.0 "), Some("Apache-2.0"));
        // Deprecated ids are still on the list, and exceptions are too.
        assert_eq!(listed_id("GPL-2.0+"), Some("GPL-2.0+"));
        assert_eq!(
            listed_id("Classpath-exception-2.0"),
            Some("Classpath-exception-2.0")
        );

        assert_eq!(listed_id("SEE LICENSE IN LICENSE.txt"), None);
        assert_eq!(listed_id("Custom-1.0"), None);
        assert_eq!(listed_id("LicenseRef-acme"), None);
        assert_eq!(listed_id(""), None);
    }

    #[test]
    fn test_listed_expressions() {
        let listed = |input: &str| listed_expression(input);

        assert_eq!(
            listed("MIT OR Apache-2.0").as_deref(),
            Some("MIT OR Apache-2.0")
        );
        assert_eq!(
            listed("(MIT AND BSD-3-Clause) OR GPL-2.0-only").as_deref(),
            Some("(MIT AND BSD-3-Clause) OR GPL-2.0-only")
        );
        assert_eq!(
            listed("GPL-2.0-only WITH classpath-exception-2.0").as_deref(),
            Some("GPL-2.0-only WITH Classpath-exception-2.0")
        );
        assert_eq!(
            listed("LicenseRef-acme OR mit").as_deref(),
            Some("LicenseRef-acme OR MIT")
        );
        assert_eq!(
            listed("apache-2.0+ AND MIT").as_deref(),
            Some("Apache-2.0+ AND MIT")
        );
        assert_eq!(listed(" ( MIT  OR ISC ) ").as_deref(), Some("(MIT OR ISC)"));
        // A single id is an expression too.
        assert_eq!(listed("mit").as_deref(), Some("MIT"));

        // Prose joined by an operator is not an expression.
        assert_eq!(listed("Apache License 2.0 OR MIT"), None);
        assert_eq!(listed("SEE LICENSE IN LICENSE.txt"), None);
        assert_eq!(listed("Custom-1.0 OR MIT"), None);
        // An exception has to be a listed exception.
        assert_eq!(listed("MIT WITH Custom-exception"), None);
        // Malformed shapes the lenient parser would forgive.
        assert_eq!(listed("MIT OR"), None);
        assert_eq!(listed("MIT WITH"), None);
        assert_eq!(listed("(MIT OR Apache-2.0"), None);
    }

    #[test]
    fn test_unlisted_licenses_are_rewritten_in_place() {
        let referenced = rewrite_expression("Custom-1.0 OR (mit AND Other-2)", |id| {
            Some(format!("LicenseRef-{id}"))
        });
        assert_eq!(
            referenced.as_deref(),
            Some("LicenseRef-Custom-1.0 OR (MIT AND LicenseRef-Other-2)")
        );

        // An unlisted exception has nowhere to go.
        assert_eq!(
            rewrite_expression("MIT WITH Custom-exception", |id| Some(id.to_string())),
            None
        );
    }

    #[test]
    fn test_parse_simple() {
        assert_eq!(parse("MIT"), SpdxExpression::License("MIT".to_string()));
    }

    #[test]
    fn test_parse_or() {
        assert_eq!(
            parse("MIT OR Apache-2.0"),
            SpdxExpression::Or(
                Box::new(SpdxExpression::License("MIT".to_string())),
                Box::new(SpdxExpression::License("Apache-2.0".to_string())),
            )
        );
    }

    #[test]
    fn test_parse_strict_accepts_valid() {
        assert!(parse_strict("MIT").is_some());
        assert!(parse_strict("MIT OR Apache-2.0").is_some());
        assert!(parse_strict("(MIT OR Apache-2.0)").is_some());
        assert!(parse_strict("GPL-2.0-only WITH Classpath-exception-2.0").is_some());
    }

    #[test]
    fn test_parse_strict_rejects_prose_and_empty() {
        // Two bare ids with no operator leave tokens unconsumed → rejected.
        assert!(parse_strict("header value").is_none());
        assert!(parse_strict("this is not a license").is_none());
        assert!(parse_strict("").is_none());
        assert!(parse_strict("   ").is_none());
    }

    #[test]
    fn test_parse_and() {
        assert_eq!(
            parse("MIT AND BSD-2-Clause"),
            SpdxExpression::And(
                Box::new(SpdxExpression::License("MIT".to_string())),
                Box::new(SpdxExpression::License("BSD-2-Clause".to_string())),
            )
        );
    }

    #[test]
    fn test_parse_with() {
        assert_eq!(
            parse("GPL-2.0-only WITH Classpath-exception-2.0"),
            SpdxExpression::With {
                license: "GPL-2.0-only".to_string(),
                exception: "Classpath-exception-2.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_parenthesized() {
        let expr = parse("(MIT OR Apache-2.0)");
        assert_eq!(
            expr,
            SpdxExpression::Or(
                Box::new(SpdxExpression::License("MIT".to_string())),
                Box::new(SpdxExpression::License("Apache-2.0".to_string())),
            )
        );
    }

    #[test]
    fn test_parse_nested() {
        let expr = parse("(MIT OR Apache-2.0) AND BSD-2-Clause");
        assert_eq!(
            expr,
            SpdxExpression::And(
                Box::new(SpdxExpression::Or(
                    Box::new(SpdxExpression::License("MIT".to_string())),
                    Box::new(SpdxExpression::License("Apache-2.0".to_string())),
                )),
                Box::new(SpdxExpression::License("BSD-2-Clause".to_string())),
            )
        );
    }

    #[test]
    fn test_parse_triple_or() {
        let expr = parse("MIT OR Apache-2.0 OR BSD-2-Clause");
        let ids = match expr {
            SpdxExpression::Or(left, right) => {
                let mut ids = left.license_ids();
                ids.extend(right.license_ids());
                ids
            }
            _ => panic!("expected Or"),
        };
        assert!(ids.contains(&"MIT".to_string()));
        assert!(ids.contains(&"Apache-2.0".to_string()));
        assert!(ids.contains(&"BSD-2-Clause".to_string()));
    }

    #[test]
    fn test_license_ids_or() {
        let expr = parse("MIT OR Apache-2.0");
        let ids = expr.license_ids();
        assert_eq!(ids, vec!["MIT", "Apache-2.0"]);
    }

    #[test]
    fn test_license_ids_with() {
        let expr = parse("GPL-2.0-only WITH Classpath-exception-2.0");
        assert_eq!(expr.license_ids(), vec!["GPL-2.0-only"]);
    }

    #[test]
    fn test_is_compound() {
        assert!(is_compound("MIT OR Apache-2.0"));
        assert!(is_compound("MIT AND BSD-2-Clause"));
        assert!(is_compound("GPL-2.0-only WITH Classpath-exception-2.0"));
        assert!(is_compound("(MIT OR Apache-2.0)"));
        assert!(!is_compound("MIT"));
        assert!(!is_compound("Apache-2.0"));
    }

    #[test]
    fn test_expression_compatibility_or_one_compatible() {
        use crate::licenses::LicenseCompatibility;

        let expr = parse("MIT OR GPL-3.0");
        let result = expression_compatibility(&expr, "MIT", false, &|dep, proj, _| {
            if dep == "MIT" && proj == "MIT" {
                LicenseCompatibility::Compatible
            } else {
                LicenseCompatibility::Incompatible
            }
        });
        assert_eq!(result, LicenseCompatibility::Compatible);
    }

    #[test]
    fn test_expression_compatibility_and_one_incompatible() {
        use crate::licenses::LicenseCompatibility;

        let expr = parse("MIT AND GPL-3.0");
        let result = expression_compatibility(&expr, "MIT", false, &|dep, proj, _| {
            if dep == "MIT" && proj == "MIT" {
                LicenseCompatibility::Compatible
            } else {
                LicenseCompatibility::Incompatible
            }
        });
        assert_eq!(result, LicenseCompatibility::Incompatible);
    }

    #[test]
    fn test_expression_is_restrictive_or_one_permissive() {
        let expr = parse("MIT OR GPL-3.0");
        let result = expression_is_restrictive(&expr, &|id| id == "GPL-3.0");
        assert!(
            !result,
            "OR with one permissive option should not be restrictive"
        );
    }

    #[test]
    fn test_expression_is_restrictive_and_one_restrictive() {
        let expr = parse("MIT AND GPL-3.0");
        let result = expression_is_restrictive(&expr, &|id| id == "GPL-3.0");
        assert!(
            result,
            "AND with one restrictive component should be restrictive"
        );
    }

    #[test]
    fn test_expression_osi_status_or_one_approved() {
        use crate::licenses::OsiStatus;

        let expr = parse("MIT OR LicenseRef-Custom");
        let result = expression_osi_status(&expr, &|id| {
            if id == "MIT" {
                OsiStatus::Approved
            } else {
                OsiStatus::Unknown
            }
        });
        assert_eq!(result, OsiStatus::Approved);
    }

    #[test]
    fn test_expression_osi_status_and_one_not_approved() {
        use crate::licenses::OsiStatus;

        let expr = parse("MIT AND LicenseRef-Custom");
        let result = expression_osi_status(&expr, &|id| {
            if id == "MIT" {
                OsiStatus::Approved
            } else {
                OsiStatus::NotApproved
            }
        });
        assert_eq!(result, OsiStatus::NotApproved);
    }
}
