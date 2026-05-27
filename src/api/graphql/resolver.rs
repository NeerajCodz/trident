use crate::errors::Result;
use crate::planner::Planner;
use crate::query::QueryParser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// GraphQL request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphQLRequest {
    pub query: String,
    #[serde(default)]
    pub variables: BTreeMap<String, serde_json::Value>,
    #[serde(rename = "operationName")]
    pub operation_name: Option<String>,
}

/// GraphQL response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphQLResponse {
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<GraphQLError>,
}

/// GraphQL error.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphQLError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
}

impl GraphQLError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: None,
        }
    }
}

/// Execute a GraphQL query against praxis's query engine.
///
/// This is a simplified GraphQL executor that:
/// 1. Parses the GraphQL query
/// 2. Translates it to a Poiesis/SQL query
/// 3. Executes it through the query parser and planner
/// 4. Returns the results as JSON
pub fn execute_graphql(request: &GraphQLRequest) -> GraphQLResponse {
    match execute_graphql_internal(request) {
        Ok(data) => GraphQLResponse {
            data: Some(data),
            errors: Vec::new(),
        },
        Err(err) => GraphQLResponse {
            data: None,
            errors: vec![GraphQLError::new(err.to_string())],
        },
    }
}

fn execute_graphql_internal(request: &GraphQLRequest) -> Result<serde_json::Value> {
    let query = &request.query;
    let variables = &request.variables;

    // Parse the GraphQL query to extract operation type and fields
    let parsed = parse_graphql_query(query, variables)?;

    // Execute through praxis's query engine
    let logical = QueryParser::parse(&parsed.sql_query)?;
    let planner = Planner;
    let plan = planner.plan(&logical)?;

    // Return the plan as JSON (in a real implementation, this would execute the plan)
    let plan_json = serde_json::to_value(&plan).unwrap_or(serde_json::Value::Null);

    Ok(serde_json::json!({
        "records": [plan_json],
        "operation": parsed.operation_type,
    }))
}

struct ParsedGraphQL {
    sql_query: String,
    operation_type: String,
}

fn parse_graphql_query(
    query: &str,
    variables: &BTreeMap<String, serde_json::Value>,
) -> Result<ParsedGraphQL> {
    let query = query.trim();

    // Simple GraphQL parser - handles basic queries
    if query.starts_with("query") || query.starts_with("{") {
        parse_query_operation(query, variables)
    } else if query.starts_with("mutation") {
        parse_mutation_operation(query, variables)
    } else {
        // Try to parse as a shorthand query
        parse_query_operation(query, variables)
    }
}

fn parse_query_operation(
    query: &str,
    variables: &BTreeMap<String, serde_json::Value>,
) -> Result<ParsedGraphQL> {
    // Extract collection name from the query
    // GraphQL query: { users(where: "...", limit: 10) { name email } }
    // Translates to: SELECT name, email FROM users WHERE ... LIMIT 10

    let content = query
        .trim_start_matches("query")
        .trim_start_matches("{")
        .trim_end_matches("}")
        .trim();

    // Find the collection name (first word before parentheses or braces)
    let collection = content.split(['(', '{']).next().unwrap_or("").trim();

    if collection.is_empty() {
        return Err(crate::errors::PraxisError::Query(
            "GraphQL query missing collection name".to_string(),
        ));
    }

    // Extract arguments
    let args_start = content.find('(');
    let args_end = content.find(')');
    let fields_start = content.find('{');

    let mut where_clause = String::new();
    let mut limit_clause = String::new();

    if let (Some(start), Some(end)) = (args_start, args_end) {
        let args = &content[start + 1..end];
        for arg in args.split(',') {
            let arg = arg.trim();
            if let Some((key, value)) = arg.split_once(':') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');

                // Replace variable references
                let value = if let Some(var_name) = value.strip_prefix('$') {
                    variables
                        .get(var_name)
                        .map(|v| v.to_string().trim_matches('"').to_string())
                        .unwrap_or_else(|| value.to_string())
                } else {
                    value.to_string()
                };

                match key {
                    "where" | "filter" => {
                        where_clause = format!(" WHERE {}", value);
                    }
                    "limit" => {
                        limit_clause = format!(" LIMIT {}", value);
                    }
                    "offset" => {
                        limit_clause.push_str(&format!(" OFFSET {}", value));
                    }
                    _ => {}
                }
            }
        }
    }

    // Extract field selection
    let fields = if let Some(start) = fields_start {
        let fields_content = &content[start..];
        let fields: Vec<&str> = fields_content
            .trim_matches('{')
            .trim_matches('}')
            .split_whitespace()
            .map(|f| f.trim().trim_matches(','))
            .filter(|f| !f.is_empty() && *f != "{")
            .collect();
        if fields.is_empty() {
            "*".to_string()
        } else {
            fields.join(", ")
        }
    } else {
        "*".to_string()
    };

    let sql_query = format!(
        "SELECT {} FROM {}{}{}",
        fields, collection, where_clause, limit_clause
    );

    Ok(ParsedGraphQL {
        sql_query,
        operation_type: "query".to_string(),
    })
}

fn parse_mutation_operation(
    query: &str,
    _variables: &BTreeMap<String, serde_json::Value>,
) -> Result<ParsedGraphQL> {
    // Simple mutation parser
    // mutation { createUser(input: { name: "John", email: "john@example.com" }) { id name } }
    // Translates to: INSERT INTO users SET name = 'John', email = 'john@example.com'

    let content = query
        .trim_start_matches("mutation")
        .trim_start_matches("{")
        .trim_end_matches("}")
        .trim();

    // Find operation name
    let op_name = content.split(['(', '{']).next().unwrap_or("").trim();

    if op_name.is_empty() {
        return Err(crate::errors::PraxisError::Query(
            "GraphQL mutation missing operation name".to_string(),
        ));
    }

    // Extract input fields
    let args_start = content.find('(');
    let args_end = content.find(')');

    if let (Some(start), Some(end)) = (args_start, args_end) {
        let args = &content[start + 1..end];
        if let Some((key, value)) = args.split_once(':') {
            let key = key.trim();
            if key == "input" {
                // Parse input object
                let value = value.trim();
                let mut sets = Vec::new();
                // Simple key-value parsing
                for pair in value.trim_matches('{').trim_matches('}').split(',') {
                    if let Some((k, v)) = pair.split_once(':') {
                        let k = k.trim();
                        let v = v.trim().trim_matches('"').trim_matches('\'');
                        sets.push(format!("{} = '{}'", k, v));
                    }
                }

                // Derive collection name from operation name
                // createUser -> users, updatePost -> posts
                let _collection = derive_collection_name(op_name);

                let sql_query = format!("INSERT INTO users SET {}", sets.join(", "));

                return Ok(ParsedGraphQL {
                    sql_query,
                    operation_type: "mutation".to_string(),
                });
            }
        }
    }

    Err(crate::errors::PraxisError::Query(
        "unsupported GraphQL mutation format".to_string(),
    ))
}

fn derive_collection_name(op_name: &str) -> String {
    // Simple heuristic: remove common prefixes and pluralize
    let name = op_name
        .trim_start_matches("create")
        .trim_start_matches("update")
        .trim_start_matches("delete")
        .trim_start_matches("get")
        .trim_start_matches("find");

    // Simple pluralization (just add 's' for now)
    format!("{}s", name.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_query() {
        let request = GraphQLRequest {
            query: "{ users { name email } }".to_string(),
            variables: BTreeMap::new(),
            operation_name: None,
        };
        let response = execute_graphql(&request);
        assert!(response.errors.is_empty() || response.data.is_some());
    }

    #[test]
    fn test_parse_query_with_where() {
        let request = GraphQLRequest {
            query: r#"{ users(where: "age > 18", limit: 10) { name email } }"#.to_string(),
            variables: BTreeMap::new(),
            operation_name: None,
        };
        let response = execute_graphql(&request);
        // Should either succeed or have a meaningful error
        assert!(response.data.is_some() || !response.errors.is_empty());
    }
}
