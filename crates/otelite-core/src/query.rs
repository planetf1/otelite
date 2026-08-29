//! Query parser for structured filter expressions
//!
//! Supports simple query syntax:
//! - `severity = ERROR`
//! - `duration > 500ms`
//! - `gen_ai.system = "anthropic"`
//! - `name contains "chat"`

use std::fmt;

/// Comparison operator for query predicates
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operator {
    /// Equality (=)
    Equal,
    /// Inequality (!=)
    NotEqual,
    /// Greater than (>)
    GreaterThan,
    /// Less than (<)
    LessThan,
    /// Greater than or equal (>=)
    GreaterThanOrEqual,
    /// Less than or equal (<=)
    LessThanOrEqual,
    /// Contains substring
    Contains,
}

impl fmt::Display for Operator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operator::Equal => write!(f, "="),
            Operator::NotEqual => write!(f, "!="),
            Operator::GreaterThan => write!(f, ">"),
            Operator::LessThan => write!(f, "<"),
            Operator::GreaterThanOrEqual => write!(f, ">="),
            Operator::LessThanOrEqual => write!(f, "<="),
            Operator::Contains => write!(f, "contains"),
        }
    }
}

/// Value type for query predicates
#[derive(Debug, Clone, PartialEq)]
pub enum QueryValue {
    /// String value
    String(String),
    /// Numeric value
    Number(f64),
    /// Duration in milliseconds
    Duration(u64),
}

impl fmt::Display for QueryValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryValue::String(s) => write!(f, "\"{}\"", s),
            QueryValue::Number(n) => write!(f, "{}", n),
            QueryValue::Duration(d) => write!(f, "{}ms", d),
        }
    }
}

/// A single query predicate (field operator value)
#[derive(Debug, Clone, PartialEq)]
pub struct QueryPredicate {
    /// Field name (e.g., "severity", "gen_ai.system")
    pub field: String,
    /// Comparison operator
    pub operator: Operator,
    /// Value to compare against
    pub value: QueryValue,
}

impl fmt::Display for QueryPredicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.field, self.operator, self.value)
    }
}

/// Error type for query parsing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// Empty query string
    EmptyQuery,
    /// Invalid syntax
    InvalidSyntax(String),
    /// Unknown operator
    UnknownOperator(String),
    /// Invalid value format
    InvalidValue(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::EmptyQuery => write!(f, "Query string is empty"),
            QueryError::InvalidSyntax(msg) => write!(f, "Invalid syntax: {}", msg),
            QueryError::UnknownOperator(op) => write!(f, "Unknown operator: {}", op),
            QueryError::InvalidValue(msg) => write!(f, "Invalid value: {}", msg),
        }
    }
}

impl std::error::Error for QueryError {}

/// Parse a query string into a list of predicates
///
/// # Examples
///
/// ```
/// use otelite_core::query::{parse_query, Operator, QueryValue};
///
/// let predicates = parse_query("severity = \"ERROR\"").unwrap();
/// assert_eq!(predicates.len(), 1);
/// assert_eq!(predicates[0].field, "severity");
/// assert_eq!(predicates[0].operator, Operator::Equal);
/// ```
pub fn parse_query(input: &str) -> Result<Vec<QueryPredicate>, QueryError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(QueryError::EmptyQuery);
    }

    let mut predicates = Vec::new();
    let mut rest = input;
    loop {
        match find_and_separator(rest) {
            Some((sep_start, sep_len)) => {
                predicates.push(parse_single_predicate(&rest[..sep_start])?);
                rest = &rest[sep_start + sep_len..];
            },
            None => {
                predicates.push(parse_single_predicate(rest)?);
                break;
            },
        }
    }

    Ok(predicates)
}

/// Find a space-delimited `and` token (case-insensitive) in `input`.
///
/// Returns the byte range of the token including one surrounding space
/// character on each side. A token starting at byte 0 is not treated as a
/// separator, so a field literally named `and` still parses as a
/// predicate (e.g. `and = "x"`).
fn find_and_separator(input: &str) -> Option<(usize, usize)> {
    let bytes = input.as_bytes();
    let mut i = 1usize;
    while i + 4 <= bytes.len() {
        if input.is_char_boundary(i)
            && bytes[i - 1] == b' '
            && bytes[i].eq_ignore_ascii_case(&b'a')
            && bytes[i + 1].eq_ignore_ascii_case(&b'n')
            && bytes[i + 2].eq_ignore_ascii_case(&b'd')
            && bytes[i + 3] == b' '
        {
            return Some((i - 1, 5));
        }
        i += 1;
    }
    None
}

fn parse_single_predicate(input: &str) -> Result<QueryPredicate, QueryError> {
    // Try to find operator
    let (field, operator, value_str) = if let Some(pos) = input.find(" contains ") {
        let field = input[..pos].trim();
        let value = input[pos + 10..].trim();
        (field, Operator::Contains, value)
    } else if let Some(pos) = input.find(" >= ") {
        let field = input[..pos].trim();
        let value = input[pos + 4..].trim();
        (field, Operator::GreaterThanOrEqual, value)
    } else if let Some(pos) = input.find(" <= ") {
        let field = input[..pos].trim();
        let value = input[pos + 4..].trim();
        (field, Operator::LessThanOrEqual, value)
    } else if let Some(pos) = input.find(" != ") {
        let field = input[..pos].trim();
        let value = input[pos + 4..].trim();
        (field, Operator::NotEqual, value)
    } else if let Some(pos) = input.find(" > ") {
        let field = input[..pos].trim();
        let value = input[pos + 3..].trim();
        (field, Operator::GreaterThan, value)
    } else if let Some(pos) = input.find(" < ") {
        let field = input[..pos].trim();
        let value = input[pos + 3..].trim();
        (field, Operator::LessThan, value)
    } else if let Some(pos) = input.find(" = ") {
        let field = input[..pos].trim();
        let value = input[pos + 3..].trim();
        (field, Operator::Equal, value)
    } else {
        return Err(QueryError::InvalidSyntax(
            "No valid operator found. Expected: =, !=, >, <, >=, <=, contains".to_string(),
        ));
    };

    if field.is_empty() {
        return Err(QueryError::InvalidSyntax("Field name is empty".to_string()));
    }

    if value_str.is_empty() {
        return Err(QueryError::InvalidValue("Value is empty".to_string()));
    }

    let value = parse_value(value_str)?;

    Ok(QueryPredicate {
        field: field.to_string(),
        operator,
        value,
    })
}

fn parse_value(input: &str) -> Result<QueryValue, QueryError> {
    let input = input.trim();

    if input.is_empty() {
        return Err(QueryError::InvalidValue("Value is empty".to_string()));
    }

    // Check for quoted string
    if (input.starts_with('"') && input.ends_with('"'))
        || (input.starts_with('\'') && input.ends_with('\''))
    {
        if input.len() < 2 {
            return Err(QueryError::InvalidValue(
                "Quoted string is too short".to_string(),
            ));
        }
        return Ok(QueryValue::String(input[1..input.len() - 1].to_string()));
    }

    // Check for duration (e.g., "500ms", "1s")
    if let Some(num_part) = input.strip_suffix("ms") {
        match num_part.parse::<u64>() {
            Ok(n) => return Ok(QueryValue::Duration(n)),
            Err(_) => {
                return Err(QueryError::InvalidValue(format!(
                    "Invalid duration format: {}",
                    input
                )))
            },
        }
    }

    if let Some(num_part) = input.strip_suffix('s') {
        if let Ok(n) = num_part.parse::<u64>() {
            return n
                .checked_mul(1000)
                .map(QueryValue::Duration)
                .ok_or_else(|| {
                    QueryError::InvalidValue(format!("Duration out of range: {}", input))
                });
        }
        // Fractional seconds, e.g. 1.5s -> 1500 ms. The `as u64` cast
        // saturates, so very large finite values are deterministic.
        if let Ok(n) = num_part.parse::<f64>() {
            if n.is_finite() && n >= 0.0 {
                return Ok(QueryValue::Duration((n * 1000.0) as u64));
            }
        }
        return Err(QueryError::InvalidValue(format!(
            "Invalid duration format: {}",
            input
        )));
    }

    // Try to parse as number
    if let Ok(n) = input.parse::<f64>() {
        return Ok(QueryValue::Number(n));
    }

    // Reject unquoted strings - require explicit quoting for string values
    Err(QueryError::InvalidValue(format!(
        "String values must be quoted: {}",
        input
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_equality() {
        let result = parse_query("severity = \"ERROR\"").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "severity");
        assert_eq!(result[0].operator, Operator::Equal);
        assert_eq!(result[0].value, QueryValue::String("ERROR".to_string()));
    }

    #[test]
    fn test_parse_quoted_string() {
        let result = parse_query("name = \"my service\"").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "name");
        assert_eq!(result[0].operator, Operator::Equal);
        assert_eq!(
            result[0].value,
            QueryValue::String("my service".to_string())
        );
    }

    #[test]
    fn test_parse_duration_ms() {
        let result = parse_query("duration > 500ms").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "duration");
        assert_eq!(result[0].operator, Operator::GreaterThan);
        assert_eq!(result[0].value, QueryValue::Duration(500));
    }

    #[test]
    fn test_parse_duration_seconds() {
        let result = parse_query("duration < 2s").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "duration");
        assert_eq!(result[0].operator, Operator::LessThan);
        assert_eq!(result[0].value, QueryValue::Duration(2000));
    }

    #[test]
    fn test_parse_numeric_value() {
        let result = parse_query("count >= 100").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "count");
        assert_eq!(result[0].operator, Operator::GreaterThanOrEqual);
        assert_eq!(result[0].value, QueryValue::Number(100.0));
    }

    #[test]
    fn test_parse_contains_operator() {
        let result = parse_query("name contains \"chat\"").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "name");
        assert_eq!(result[0].operator, Operator::Contains);
        assert_eq!(result[0].value, QueryValue::String("chat".to_string()));
    }

    #[test]
    fn test_parse_dotted_field_name() {
        let result = parse_query("gen_ai.system = \"anthropic\"").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "gen_ai.system");
        assert_eq!(result[0].operator, Operator::Equal);
        assert_eq!(result[0].value, QueryValue::String("anthropic".to_string()));
    }

    #[test]
    fn test_parse_not_equal() {
        let result = parse_query("status != 200").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "status");
        assert_eq!(result[0].operator, Operator::NotEqual);
        assert_eq!(result[0].value, QueryValue::Number(200.0));
    }

    #[test]
    fn test_parse_less_than_or_equal() {
        let result = parse_query("latency <= 1000ms").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "latency");
        assert_eq!(result[0].operator, Operator::LessThanOrEqual);
        assert_eq!(result[0].value, QueryValue::Duration(1000));
    }

    #[test]
    fn test_parse_multiple_predicates() {
        let result = parse_query("severity = \"ERROR\" AND duration > 500ms").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].field, "severity");
        assert_eq!(result[0].operator, Operator::Equal);
        assert_eq!(result[1].field, "duration");
        assert_eq!(result[1].operator, Operator::GreaterThan);
    }

    #[test]
    fn test_empty_query() {
        let result = parse_query("");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), QueryError::EmptyQuery);
    }

    #[test]
    fn test_invalid_syntax_no_operator() {
        let result = parse_query("severity ERROR");
        assert!(result.is_err());
        match result.unwrap_err() {
            QueryError::InvalidSyntax(_) => {},
            _ => panic!("Expected InvalidSyntax error"),
        }
    }

    #[test]
    fn test_invalid_duration_format() {
        let result = parse_query("duration > \"abc\"");
        // This should succeed - it's a valid string comparison
        assert!(result.is_ok());

        // Test actual invalid duration
        let result2 = parse_query("duration > abcms");
        assert!(result2.is_err());
        match result2.unwrap_err() {
            QueryError::InvalidValue(_) => {},
            _ => panic!("Expected InvalidValue error"),
        }
    }

    #[test]
    fn test_empty_field_name() {
        let result = parse_query(" = ERROR");
        assert!(result.is_err());
        match result.unwrap_err() {
            QueryError::InvalidSyntax(_) => {},
            _ => panic!("Expected InvalidSyntax error"),
        }
    }

    #[test]
    fn test_empty_value() {
        // Test with explicit empty value after operator
        let result = parse_query("severity = \"\"");
        assert!(result.is_ok()); // Empty string is valid

        // Test with whitespace-only value (no quotes)
        let result2 = parse_query("severity =");
        assert!(result2.is_err());
        // This triggers InvalidSyntax because the operator pattern doesn't match
        // when there's no space after the equals sign
    }

    // --- Edge cases from issue #11 -------------------------------------

    #[test]
    fn test_parse_escaped_inner_quote_is_literal() {
        // Documented behaviour: backslash escapes are NOT processed. The
        // value keeps the backslash verbatim and the last quote in the
        // input closes the string, so the issue's example yields
        // `hello \"world\` (trailing backslash retained).
        let result = parse_query(r#"severity = "hello \"world\""#).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].value,
            QueryValue::String(r#"hello \"world\"#.to_string())
        );
    }

    #[test]
    fn test_parse_scientific_notation() {
        let result = parse_query("count > 1.5e3").unwrap();
        assert_eq!(result[0].operator, Operator::GreaterThan);
        assert_eq!(result[0].value, QueryValue::Number(1500.0));
    }

    #[test]
    fn test_parse_negative_number() {
        let result = parse_query("value < -5").unwrap();
        assert_eq!(result[0].operator, Operator::LessThan);
        assert_eq!(result[0].value, QueryValue::Number(-5.0));
    }

    #[test]
    fn test_parse_fractional_seconds() {
        let result = parse_query("duration > 1.5s").unwrap();
        assert_eq!(result[0].value, QueryValue::Duration(1500));
    }

    #[test]
    fn test_parse_fractional_ms_rejected() {
        // Documented behaviour: only whole milliseconds are supported.
        let result = parse_query("duration > 1.5ms");
        match result.unwrap_err() {
            QueryError::InvalidValue(_) => {},
            other => panic!("Expected InvalidValue, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_large_duration_no_overflow() {
        let result = parse_query("duration > 999999ms").unwrap();
        assert_eq!(result[0].value, QueryValue::Duration(999999));
    }

    #[test]
    fn test_parse_overflowing_seconds_rejected() {
        // 184467440737095516s would overflow u64 milliseconds; must be a
        // clean error, not a panic.
        let result = parse_query("duration > 184467440737095516s");
        match result.unwrap_err() {
            QueryError::InvalidValue(msg) => {
                assert!(msg.contains("out of range"), "unexpected message: {}", msg)
            },
            other => panic!("Expected InvalidValue, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_multiple_spaces_around_operator() {
        let result = parse_query("severity  =  \"info\"").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "severity");
        assert_eq!(result[0].value, QueryValue::String("info".to_string()));
    }

    #[test]
    fn test_parse_tabs_are_consistent_error() {
        // Documented behaviour: tabs are not accepted as operator
        // whitespace; the query is rejected consistently.
        let result = parse_query("severity\t=\t\"info\"");
        match result.unwrap_err() {
            QueryError::InvalidSyntax(_) => {},
            other => panic!("Expected InvalidSyntax, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_leading_trailing_whitespace() {
        let result = parse_query("   severity = \"info\"   ").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "severity");
    }

    #[test]
    fn test_parse_three_predicates() {
        let result = parse_query("a = \"x\" AND b = \"y\" AND c = \"z\"").unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].field, "a");
        assert_eq!(result[1].field, "b");
        assert_eq!(result[2].field, "c");
    }

    #[test]
    fn test_parse_lower_case_and_is_separator() {
        let result = parse_query("a = \"x\" and b = \"y\"").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].field, "a");
        assert_eq!(result[1].field, "b");
        assert_eq!(result[1].value, QueryValue::String("y".to_string()));
    }

    #[test]
    fn test_parse_field_named_and() {
        // A field literally called `and` must still parse as one
        // predicate, not a separator error.
        let result = parse_query("and = \"x\"").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].field, "and");
        assert_eq!(result[0].value, QueryValue::String("x".to_string()));
    }

    #[test]
    fn test_parse_and_inside_quoted_string_rejected() {
        // Documented limitation: an `and` token inside a quoted value
        // still splits the query (same as uppercase AND did before), so
        // such values are rejected rather than silently mis-parsed.
        let result = parse_query("name = \"rock and roll\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_rhs_is_clear_error() {
        let result = parse_query("severity =");
        match result.unwrap_err() {
            QueryError::InvalidSyntax(_) => {},
            other => panic!("Expected InvalidSyntax, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_bare_field_is_clear_error() {
        let result = parse_query("severity");
        match result.unwrap_err() {
            QueryError::InvalidSyntax(_) => {},
            other => panic!("Expected InvalidSyntax, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_unclosed_quoted_string_is_clear_error() {
        let result = parse_query("severity = \"abc");
        match result.unwrap_err() {
            QueryError::InvalidValue(_) => {},
            other => panic!("Expected InvalidValue, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_bare_and_is_clear_error() {
        let result = parse_query("AND");
        match result.unwrap_err() {
            QueryError::InvalidSyntax(_) => {},
            other => panic!("Expected InvalidSyntax, got {:?}", other),
        }
    }
}
