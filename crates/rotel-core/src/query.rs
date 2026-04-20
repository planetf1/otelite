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
/// use rotel_core::query::{parse_query, Operator, QueryValue};
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

    // Split by AND (case-insensitive) for multiple predicates
    let parts: Vec<&str> = input.split(" AND ").collect();

    for part in parts {
        let predicate = parse_single_predicate(part.trim())?;
        predicates.push(predicate);
    }

    Ok(predicates)
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
        match num_part.parse::<u64>() {
            Ok(n) => return Ok(QueryValue::Duration(n * 1000)),
            Err(_) => {
                return Err(QueryError::InvalidValue(format!(
                    "Invalid duration format: {}",
                    input
                )))
            },
        }
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
}
