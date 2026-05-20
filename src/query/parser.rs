use crate::errors::{Result, TridentError};
use crate::query::{Token, tokenize};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum QueryLanguage {
    Sql,
    Pql,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogicalPlan {
    pub language: QueryLanguage,
    pub operation: QueryOperation,
    pub collection: String,
    pub filter: Option<Predicate>,
    pub filter_expression: Option<BooleanExpression>,
    pub joins: Vec<JoinClause>,
    pub full_text: Option<FullTextClause>,
    pub vector: Option<VectorClause>,
    pub traversal: Option<TraversalClause>,
    pub rank_by: Option<String>,
    pub order_by: Option<OrderClause>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub distinct: bool,
    pub group_by: Option<Vec<String>>,
    pub having: Option<BooleanExpression>,
    pub as_of: Option<AsOfClause>,
    pub mutation: Option<MutationPlan>,
    pub ddl: Option<DdlPlan>,
    pub explain_analyze: bool,
    /// Common Table Expressions (WITH clause)
    pub ctes: Vec<CteDefinition>,
    /// Window functions in SELECT
    pub window_functions: Vec<WindowFunction>,
    /// Query parameters ($1, $2, :name, @name)
    pub params: QueryParams,
}

/// A Common Table Expression (CTE) definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CteDefinition {
    pub name: String,
    pub query: Box<LogicalPlan>,
    pub recursive: bool,
}

/// A window function in a SELECT clause.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowFunction {
    /// Function name: ROW_NUMBER, RANK, DENSE_RANK, LAG, LEAD, FIRST_VALUE, LAST_VALUE, SUM, COUNT, etc.
    pub function: String,
    /// Arguments to the function (e.g., column name for LAG/LEAD)
    pub args: Vec<String>,
    /// PARTITION BY columns
    pub partition_by: Vec<String>,
    /// ORDER BY columns (with direction)
    pub order_by: Vec<OrderClause>,
    /// Alias for the result column
    pub alias: String,
}

/// Query parameters for parameterized queries.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryParams {
    /// Positional parameters ($1, $2, etc.)
    pub positional: Vec<String>,
    /// Named parameters (:name, @name)
    pub named: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum QueryOperation {
    Select,
    Insert,
    Update,
    Delete,
    Watch,
    Explain,
    CreateCollection,
    AlterCollection,
    DropCollection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DdlPlan {
    CreateCollection {
        name: String,
        attributes: Vec<AttributeDdl>,
    },
    AlterCollection {
        name: String,
        actions: Vec<AlterAction>,
    },
    DropCollection {
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttributeDdl {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AlterAction {
    AddAttribute {
        name: String,
        type_name: String,
        nullable: bool,
    },
    DropAttribute {
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Predicate {
    pub attribute: String,
    pub operator: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BooleanExpression {
    Predicate(Predicate),
    And(Box<BooleanExpression>, Box<BooleanExpression>),
    Or(Box<BooleanExpression>, Box<BooleanExpression>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JoinClause {
    pub collection: String,
    pub left_attribute: String,
    pub right_attribute: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FullTextClause {
    pub attribute: String,
    pub query: String,
    pub top: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorClause {
    pub target: String,
    pub top: usize,
    pub threshold: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraversalClause {
    pub target: String,
    pub hops: usize,
    pub edge_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderClause {
    pub attribute: String,
    pub descending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AsOfClause {
    Commit(String),
    Branch(String),
    Timestamp(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MutationPlan {
    Insert {
        key: String,
        attributes: BTreeMap<String, Value>,
    },
    Update {
        key: String,
        attributes: BTreeMap<String, Value>,
    },
    Delete {
        key: String,
    },
}

pub struct QueryParser;

impl QueryParser {
    pub fn parse(input: &str) -> Result<LogicalPlan> {
        Self::parse_with_params(input, &QueryParams::default())
    }

    /// Parse a query with bound parameters.
    pub fn parse_with_params(input: &str, params: &QueryParams) -> Result<LogicalPlan> {
        let trimmed = input.trim();
        let upper = trimmed.to_ascii_uppercase();

        // Handle WITH clause (CTEs)
        if upper.starts_with("WITH") {
            let mut plan = parse_cte(trimmed)?;
            plan.params = params.clone();
            return Ok(plan);
        }

        if upper.starts_with("EXPLAIN") {
            let rest = trimmed["EXPLAIN".len()..].trim();
            let (rest, explain_analyze) = if rest.to_ascii_uppercase().starts_with("ANALYZE") {
                (rest["ANALYZE".len()..].trim(), true)
            } else {
                (rest, false)
            };
            let plan = Self::parse(rest)?;
            Ok(LogicalPlan {
                operation: QueryOperation::Explain,
                explain_analyze,
                ..plan
            })
        } else if upper.starts_with("SELECT") {
            parse_sql(trimmed)
        } else if upper.starts_with("INSERT")
            || upper.starts_with("UPDATE")
            || upper.starts_with("DELETE")
        {
            parse_sql_mutation(trimmed)
        } else if upper.starts_with("CREATE") {
            parse_ddl_create(trimmed)
        } else if upper.starts_with("ALTER") {
            parse_ddl_alter(trimmed)
        } else if upper.starts_with("DROP") {
            parse_ddl_drop(trimmed)
        } else if upper.starts_with("FIND") || upper.starts_with("WATCH") {
            parse_pql(trimmed)
        } else {
            Err(TridentError::Query(
                "expected SQL SELECT, DDL, or PQL FIND statement".into(),
            ))
        }
    }
}

fn parse_sql(input: &str) -> Result<LogicalPlan> {
    let tokens = tokenize(input)?;
    let from_index = tokens
        .iter()
        .position(|token| token.is_word("FROM"))
        .ok_or_else(|| TridentError::Query("SQL query missing FROM".into()))?;
    let collection = token_text(
        tokens
            .get(from_index + 1)
            .ok_or_else(|| TridentError::Query("SQL query missing collection".into()))?,
    );
    let filter_expression = tokens
        .iter()
        .position(|token| token.is_word("WHERE"))
        .map(|index| parse_boolean_expression(&tokens[index + 1..]))
        .transpose()?;
    let filter = first_predicate(filter_expression.as_ref()).cloned();

    Ok(LogicalPlan {
        language: QueryLanguage::Sql,
        operation: QueryOperation::Select,
        collection,
        filter,
        filter_expression,
        joins: parse_joins(&tokens)?,
        full_text: parse_full_text(&tokens)?,
        vector: None,
        traversal: None,
        rank_by: None,
        order_by: parse_order_by(&tokens),
        limit: parse_limit(&tokens),
        offset: parse_offset(&tokens),
        distinct: tokens.iter().any(|t| t.is_word("DISTINCT")),
        group_by: parse_group_by(&tokens),
        having: None,
        as_of: None,
        mutation: None,
        ddl: None,
        explain_analyze: false,
        ctes: Vec::new(),
        window_functions: Vec::new(),
        params: QueryParams::default(),
    })
}

fn parse_sql_mutation(input: &str) -> Result<LogicalPlan> {
    let tokens = tokenize(input)?;
    let command = tokens
        .first()
        .ok_or_else(|| TridentError::Query("empty mutation".into()))?;
    if command.is_word("INSERT") {
        let collection = token_text(expect_token(&tokens, 2, "INSERT INTO missing collection")?);
        let key = token_text(expect_token(&tokens, 3, "INSERT INTO missing record key")?);
        let set_index = find_token(&tokens, "SET")?;
        let attributes = parse_assignments(&tokens[set_index + 1..])?;
        mutation_plan(
            QueryOperation::Insert,
            collection,
            MutationPlan::Insert { key, attributes },
        )
    } else if command.is_word("UPDATE") {
        let collection = token_text(expect_token(&tokens, 1, "UPDATE missing collection")?);
        let key = token_text(expect_token(&tokens, 2, "UPDATE missing record key")?);
        let set_index = find_token(&tokens, "SET")?;
        let attributes = parse_assignments(&tokens[set_index + 1..])?;
        mutation_plan(
            QueryOperation::Update,
            collection,
            MutationPlan::Update { key, attributes },
        )
    } else if command.is_word("DELETE") {
        let from_index = find_token(&tokens, "FROM")?;
        let collection = token_text(expect_token(
            &tokens,
            from_index + 1,
            "DELETE missing collection",
        )?);
        let key = if let Some(where_index) = tokens.iter().position(|token| token.is_word("WHERE"))
        {
            token_text(expect_token(
                &tokens,
                where_index + 3,
                "DELETE WHERE id = missing key",
            )?)
        } else {
            token_text(expect_token(
                &tokens,
                from_index + 2,
                "DELETE missing record key",
            )?)
        };
        mutation_plan(
            QueryOperation::Delete,
            collection,
            MutationPlan::Delete { key },
        )
    } else {
        Err(TridentError::Query("unsupported SQL mutation".into()))
    }
}

fn parse_pql(input: &str) -> Result<LogicalPlan> {
    let tokens = tokenize(input)?;
    if tokens
        .first()
        .map(|token| {
            token.is_word("SELECT")
                || token.is_word("INSERT")
                || token.is_word("UPDATE")
                || token.is_word("DELETE")
        })
        .unwrap_or(false)
    {
        return if tokens
            .first()
            .map(|token| token.is_word("SELECT"))
            .unwrap_or(false)
        {
            parse_sql(input)
        } else {
            parse_sql_mutation(input)
        };
    }
    let operation = if tokens
        .first()
        .map(|token| token.is_word("WATCH"))
        .unwrap_or(false)
    {
        QueryOperation::Watch
    } else {
        QueryOperation::Select
    };
    let collection = tokens
        .get(1)
        .map(token_text)
        .ok_or_else(|| TridentError::Query("PQL FIND missing collection".into()))?;

    let vector = tokens
        .iter()
        .position(|token| token.is_word("SIMILAR"))
        .map(|index| VectorClause {
            target: tokens.get(index + 2).map(token_text).unwrap_or_default(),
            top: tokens
                .iter()
                .position(|token| token.is_word("TOP"))
                .and_then(|top_index| tokens.get(top_index + 1))
                .and_then(|value| token_text(value).parse().ok())
                .unwrap_or(10),
            threshold: tokens
                .iter()
                .position(|token| token.is_word("THRESHOLD"))
                .and_then(|threshold_index| tokens.get(threshold_index + 1))
                .map(token_text),
        });

    let traversal = tokens
        .iter()
        .position(|token| token.is_word("TRAVERSE"))
        .map(|index| TraversalClause {
            target: tokens.get(index + 1).map(token_text).unwrap_or_default(),
            hops: tokens
                .iter()
                .position(|token| token.is_word("HOPS"))
                .and_then(|hop_index| tokens.get(hop_index.saturating_sub(1)))
                .and_then(|value| token_text(value).parse().ok())
                .unwrap_or(1),
            edge_label: tokens
                .iter()
                .position(|token| token.is_word("VIA"))
                .and_then(|via_index| tokens.get(via_index + 1))
                .map(token_text),
        });

    let filter_expression = tokens
        .iter()
        .position(|token| token.is_word("WHERE"))
        .map(|index| parse_boolean_expression(&tokens[index + 1..]))
        .transpose()?;
    let filter = first_predicate(filter_expression.as_ref()).cloned();

    let rank_by = tokens
        .iter()
        .position(|token| token.is_word("RANK"))
        .and_then(|index| {
            let by_index = index + 2;
            tokens
                .get(by_index..)
                .map(|rest| collect_until(rest, &["AS", "LIMIT"]).join(" "))
        });

    let as_of = parse_as_of(&tokens);

    Ok(LogicalPlan {
        language: QueryLanguage::Pql,
        operation,
        collection,
        filter,
        filter_expression,
        joins: Vec::new(),
        full_text: parse_full_text(&tokens)?,
        vector,
        traversal,
        rank_by,
        order_by: parse_order_by(&tokens),
        limit: parse_limit(&tokens),
        offset: parse_offset(&tokens),
        distinct: false,
        group_by: None,
        having: None,
        as_of,
        mutation: None,
        ddl: None,
        explain_analyze: false,
        ctes: Vec::new(),
        window_functions: Vec::new(),
        params: QueryParams::default(),
    })
}

fn parse_as_of(tokens: &[Token]) -> Option<AsOfClause> {
    let as_index = tokens.iter().position(|token| token.is_word("AS"))?;
    let kind = tokens.get(as_index + 2)?;
    let value = token_text(tokens.get(as_index + 3)?);
    if kind.is_word("COMMIT") {
        Some(AsOfClause::Commit(value))
    } else if kind.is_word("BRANCH") {
        Some(AsOfClause::Branch(value))
    } else if kind.is_word("TIMESTAMP") {
        Some(AsOfClause::Timestamp(value))
    } else {
        None
    }
}

fn parse_order_by(tokens: &[Token]) -> Option<OrderClause> {
    let order_index = tokens.iter().position(|token| token.is_word("ORDER"))?;
    let attribute = token_text(tokens.get(order_index + 2)?);
    let descending = tokens
        .get(order_index + 3)
        .map(|direction| direction.is_word("DESC"))
        .unwrap_or(false);
    Some(OrderClause {
        attribute,
        descending,
    })
}

fn parse_limit(tokens: &[Token]) -> Option<usize> {
    tokens
        .iter()
        .position(|token| token.is_word("LIMIT"))
        .and_then(|index| tokens.get(index + 1))
        .and_then(|limit| token_text(limit).parse().ok())
}

fn parse_offset(tokens: &[Token]) -> Option<usize> {
    tokens
        .iter()
        .position(|token| token.is_word("OFFSET"))
        .and_then(|index| tokens.get(index + 1))
        .and_then(|val| token_text(val).parse().ok())
}

fn parse_group_by(tokens: &[Token]) -> Option<Vec<String>> {
    let gb_index = tokens.iter().position(|t| t.is_word("GROUP"))?;
    if !tokens.get(gb_index + 1).map_or(false, |t| t.is_word("BY")) {
        return None;
    }
    let mut attributes = Vec::new();
    let mut cursor = gb_index + 2;
    while cursor < tokens.len() {
        match &tokens[cursor] {
            Token::Word(word) => {
                let upper = word.to_ascii_uppercase();
                if matches!(
                    upper.as_str(),
                    "HAVING" | "ORDER" | "LIMIT" | "OFFSET" | "UNION" | "INTERSECT" | "EXCEPT"
                ) {
                    break;
                }
                attributes.push(word.clone());
                cursor += 1;
            }
            Token::Symbol(',') => {
                cursor += 1;
            }
            _ => break,
        }
    }
    if attributes.is_empty() {
        None
    } else {
        Some(attributes)
    }
}

fn collect_until(tokens: &[Token], stop_words: &[&str]) -> Vec<String> {
    let end = tokens
        .iter()
        .position(|token| stop_words.iter().any(|stop| token.is_word(stop)))
        .unwrap_or(tokens.len());
    tokens[..end].iter().map(token_text).collect()
}

fn mutation_plan(
    operation: QueryOperation,
    collection: String,
    mutation: MutationPlan,
) -> Result<LogicalPlan> {
    Ok(LogicalPlan {
        language: QueryLanguage::Sql,
        operation,
        collection,
        filter: None,
        filter_expression: None,
        joins: Vec::new(),
        full_text: None,
        vector: None,
        traversal: None,
        rank_by: None,
        order_by: None,
        limit: None,
        offset: None,
        distinct: false,
        group_by: None,
        having: None,
        as_of: None,
        mutation: Some(mutation),
        ddl: None,
        explain_analyze: false,
        ctes: Vec::new(),
        window_functions: Vec::new(),
        params: QueryParams::default(),
    })
}

fn ddl_plan(operation: QueryOperation, collection: String, ddl: DdlPlan) -> Result<LogicalPlan> {
    Ok(LogicalPlan {
        language: QueryLanguage::Sql,
        operation,
        collection,
        filter: None,
        filter_expression: None,
        joins: Vec::new(),
        full_text: None,
        vector: None,
        traversal: None,
        rank_by: None,
        order_by: None,
        limit: None,
        offset: None,
        distinct: false,
        group_by: None,
        having: None,
        as_of: None,
        mutation: None,
        ddl: Some(ddl),
        explain_analyze: false,
        ctes: Vec::new(),
        window_functions: Vec::new(),
        params: QueryParams::default(),
    })
}

fn parse_ddl_create(input: &str) -> Result<LogicalPlan> {
    let tokens = tokenize(input)?;
    let col_index = tokens
        .iter()
        .position(|t| t.is_word("COLLECTION"))
        .ok_or_else(|| TridentError::Query("CREATE missing COLLECTION keyword".into()))?;
    let name = token_text(
        tokens
            .get(col_index + 1)
            .ok_or_else(|| TridentError::Query("CREATE COLLECTION missing name".into()))?,
    );
    let attributes = if let Some(paren_index) = tokens.iter().position(|t| matches!(t, Token::Symbol('('))) {
        parse_attribute_defs(&tokens[paren_index + 1..])?
    } else {
        Vec::new()
    };
    ddl_plan(
        QueryOperation::CreateCollection,
        name.clone(),
        DdlPlan::CreateCollection { name, attributes },
    )
}

fn parse_ddl_alter(input: &str) -> Result<LogicalPlan> {
    let tokens = tokenize(input)?;
    let col_index = tokens
        .iter()
        .position(|t| t.is_word("COLLECTION"))
        .ok_or_else(|| TridentError::Query("ALTER missing COLLECTION keyword".into()))?;
    let name = token_text(
        tokens
            .get(col_index + 1)
            .ok_or_else(|| TridentError::Query("ALTER COLLECTION missing name".into()))?,
    );
    let mut actions = Vec::new();
    let mut cursor = col_index + 2;
    while cursor < tokens.len() {
        if tokens[cursor].is_word("ADD") {
            cursor += 1;
            if cursor < tokens.len() && tokens[cursor].is_word("ATTRIBUTE") {
                cursor += 1;
            }
            let attr_name = token_text(
                tokens
                    .get(cursor)
                    .ok_or_else(|| TridentError::Query("ADD missing attribute name".into()))?,
            );
            cursor += 1;
            let type_name = token_text(
                tokens
                    .get(cursor)
                    .ok_or_else(|| TridentError::Query("ADD missing attribute type".into()))?,
            );
            cursor += 1;
            let nullable = if cursor < tokens.len() && tokens[cursor].is_word("NULLABLE") {
                cursor += 1;
                true
            } else {
                false
            };
            actions.push(AlterAction::AddAttribute {
                name: attr_name,
                type_name,
                nullable,
            });
        } else if tokens[cursor].is_word("DROP") {
            cursor += 1;
            if cursor < tokens.len() && tokens[cursor].is_word("ATTRIBUTE") {
                cursor += 1;
            }
            let attr_name = token_text(
                tokens
                    .get(cursor)
                    .ok_or_else(|| TridentError::Query("DROP missing attribute name".into()))?,
            );
            cursor += 1;
            actions.push(AlterAction::DropAttribute { name: attr_name });
        } else {
            cursor += 1;
        }
    }
    ddl_plan(
        QueryOperation::AlterCollection,
        name.clone(),
        DdlPlan::AlterCollection { name, actions },
    )
}

fn parse_ddl_drop(input: &str) -> Result<LogicalPlan> {
    let tokens = tokenize(input)?;
    let col_index = tokens
        .iter()
        .position(|t| t.is_word("COLLECTION"))
        .ok_or_else(|| TridentError::Query("DROP missing COLLECTION keyword".into()))?;
    let name = token_text(
        tokens
            .get(col_index + 1)
            .ok_or_else(|| TridentError::Query("DROP COLLECTION missing name".into()))?,
    );
    ddl_plan(
        QueryOperation::DropCollection,
        name.clone(),
        DdlPlan::DropCollection { name },
    )
}

fn parse_attribute_defs(tokens: &[Token]) -> Result<Vec<AttributeDdl>> {
    let mut attributes = Vec::new();
    let mut cursor = 0;
    while cursor < tokens.len() {
        if matches!(tokens[cursor], Token::Symbol(')')) {
            break;
        }
        let attr_name = token_text(&tokens[cursor]);
        cursor += 1;
        let type_name = if cursor < tokens.len() {
            token_text(&tokens[cursor])
        } else {
            "string".to_string()
        };
        cursor += 1;
        let nullable = if cursor < tokens.len() && tokens[cursor].is_word("NULLABLE") {
            cursor += 1;
            true
        } else {
            false
        };
        attributes.push(AttributeDdl {
            name: attr_name,
            type_name,
            nullable,
        });
        if cursor < tokens.len() && matches!(tokens[cursor], Token::Symbol(',')) {
            cursor += 1;
        }
    }
    Ok(attributes)
}

fn expect_token<'a>(tokens: &'a [Token], index: usize, message: &str) -> Result<&'a Token> {
    tokens
        .get(index)
        .ok_or_else(|| TridentError::Query(message.into()))
}

fn find_token(tokens: &[Token], needle: &str) -> Result<usize> {
    tokens
        .iter()
        .position(|token| token.is_word(needle))
        .ok_or_else(|| TridentError::Query(format!("missing {needle} clause")))
}

fn parse_assignments(tokens: &[Token]) -> Result<BTreeMap<String, Value>> {
    let joined = tokens.iter().map(token_text).collect::<Vec<_>>().join(" ");
    let mut attributes = BTreeMap::new();
    for assignment in joined
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let Some((key, raw_value)) = assignment.split_once('=') else {
            return Err(TridentError::Query(format!(
                "assignment '{assignment}' must use key=value"
            )));
        };
        attributes.insert(key.trim().into(), parse_literal(raw_value.trim()));
    }
    Ok(attributes)
}

/// Parse a literal value from a query string into a JSON value.
fn parse_literal(raw: &str) -> Value {
    let trimmed = raw.trim().trim_matches('\'').trim_matches('"');
    serde_json::from_str(raw).unwrap_or_else(|_| {
        trimmed
            .parse::<i64>()
            .map(Value::from)
            .or_else(|_| trimmed.parse::<f64>().map(Value::from))
            .or_else(|_| trimmed.parse::<bool>().map(Value::from))
            .unwrap_or_else(|_| Value::from(trimmed))
    })
}

fn token_text(token: &Token) -> String {
    token.text().trim_matches('`').to_string()
}

fn parse_boolean_expression(tokens: &[Token]) -> Result<BooleanExpression> {
    let mut parser = BooleanParser { tokens, cursor: 0 };
    parser.parse_or()
}

fn first_predicate(expression: Option<&BooleanExpression>) -> Option<&Predicate> {
    match expression? {
        BooleanExpression::Predicate(predicate) => Some(predicate),
        BooleanExpression::And(left, _) | BooleanExpression::Or(left, _) => {
            first_predicate(Some(left))
        }
    }
}

fn parse_joins(tokens: &[Token]) -> Result<Vec<JoinClause>> {
    let mut joins = Vec::new();
    let mut cursor = 0;
    while let Some(index) = tokens[cursor..]
        .iter()
        .position(|token| token.is_word("JOIN"))
    {
        let index = cursor + index;
        let collection = token_text(expect_token(tokens, index + 1, "JOIN missing collection")?);
        let on_index = tokens[index..]
            .iter()
            .position(|token| token.is_word("ON"))
            .map(|relative| index + relative)
            .ok_or_else(|| TridentError::Query("JOIN missing ON".into()))?;
        joins.push(JoinClause {
            collection,
            left_attribute: token_text(expect_token(
                tokens,
                on_index + 1,
                "JOIN missing left side",
            )?),
            right_attribute: token_text(expect_token(
                tokens,
                on_index + 3,
                "JOIN missing right side",
            )?),
        });
        cursor = on_index + 4;
    }
    Ok(joins)
}

fn parse_full_text(tokens: &[Token]) -> Result<Option<FullTextClause>> {
    let Some(index) = tokens.iter().position(|token| token.is_word("MATCH")) else {
        return Ok(None);
    };
    Ok(Some(FullTextClause {
        attribute: token_text(expect_token(tokens, index + 1, "MATCH missing attribute")?),
        query: token_text(expect_token(tokens, index + 2, "MATCH missing query")?),
        top: tokens
            .iter()
            .position(|token| token.is_word("TOP"))
            .and_then(|top_index| tokens.get(top_index + 1))
            .and_then(|value| token_text(value).parse().ok()),
    }))
}

struct BooleanParser<'a> {
    tokens: &'a [Token],
    cursor: usize,
}

impl<'a> BooleanParser<'a> {
    fn parse_or(&mut self) -> Result<BooleanExpression> {
        let mut expression = self.parse_and()?;
        while self.consume_word("OR") {
            let right = self.parse_and()?;
            expression = BooleanExpression::Or(Box::new(expression), Box::new(right));
        }
        Ok(expression)
    }

    fn parse_and(&mut self) -> Result<BooleanExpression> {
        let mut expression = self.parse_primary()?;
        while self.consume_word("AND") {
            let right = self.parse_primary()?;
            expression = BooleanExpression::And(Box::new(expression), Box::new(right));
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<BooleanExpression> {
        if self.consume_symbol('(') {
            let expression = self.parse_or()?;
            self.expect_symbol(')')?;
            return Ok(expression);
        }
        Ok(BooleanExpression::Predicate(Predicate {
            attribute: token_text(self.expect("WHERE predicate missing attribute")?),
            operator: token_text(self.expect("WHERE predicate missing operator")?),
            value: token_text(self.expect("WHERE predicate missing value")?),
        }))
    }

    fn consume_word(&mut self, expected: &str) -> bool {
        if self
            .peek()
            .map(|token| token.is_word(expected))
            .unwrap_or(false)
        {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn consume_symbol(&mut self, expected: char) -> bool {
        if matches!(self.peek(), Some(Token::Symbol(symbol)) if *symbol == expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn expect_symbol(&mut self, expected: char) -> Result<()> {
        if self.consume_symbol(expected) {
            Ok(())
        } else {
            Err(TridentError::Query(format!("expected '{expected}'")))
        }
    }

    fn expect(&mut self, message: &str) -> Result<&'a Token> {
        let token = self
            .tokens
            .get(self.cursor)
            .ok_or_else(|| TridentError::Query(message.into()))?;
        self.cursor += 1;
        Ok(token)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }
}

/// Parse a WITH clause (Common Table Expressions).
///
/// Syntax: WITH [RECURSIVE] name AS (query) [, name2 AS (query2)] SELECT ...
fn parse_cte(input: &str) -> Result<LogicalPlan> {
    let tokens = tokenize(input)?;
    let mut cursor = 0;

    // Skip WITH
    cursor += 1;

    // Check for RECURSIVE
    let recursive = tokens
        .get(cursor)
        .map(|t| t.is_word("RECURSIVE"))
        .unwrap_or(false);
    if recursive {
        cursor += 1;
    }

    // Parse CTE definitions
    let mut ctes = Vec::new();
    loop {
        // CTE name
        let name = token_text(
            tokens
                .get(cursor)
                .ok_or_else(|| TridentError::Query("CTE missing name".into()))?,
        );
        cursor += 1;

        // AS keyword
        if !tokens
            .get(cursor)
            .map(|t| t.is_word("AS"))
            .unwrap_or(false)
        {
            return Err(TridentError::Query("CTE missing AS keyword".into()));
        }
        cursor += 1;

        // Opening paren
        if !matches!(tokens.get(cursor), Some(Token::Symbol('('))) {
            return Err(TridentError::Query("CTE missing opening parenthesis".into()));
        }
        cursor += 1;

        // Find matching closing paren
        let mut depth = 1;
        let query_start = cursor;
        while cursor < tokens.len() && depth > 0 {
            match &tokens[cursor] {
                Token::Symbol('(') => depth += 1,
                Token::Symbol(')') => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            cursor += 1;
        }

        // Parse the inner query
        let query_tokens = &tokens[query_start..cursor];
        let query_str = query_tokens.iter().map(token_text).collect::<Vec<_>>().join(" ");
        let inner_query = QueryParser::parse(&query_str)?;

        ctes.push(CteDefinition {
            name,
            query: Box::new(inner_query),
            recursive,
        });

        cursor += 1; // skip closing paren

        // Check for comma (more CTEs) or break
        if matches!(tokens.get(cursor), Some(Token::Symbol(','))) {
            cursor += 1;
        } else {
            break;
        }
    }

    // Parse the main query (everything after the CTEs)
    let main_query_str = tokens[cursor..]
        .iter()
        .map(token_text)
        .collect::<Vec<_>>()
        .join(" ");
    let mut main_plan = QueryParser::parse(&main_query_str)?;
    main_plan.ctes = ctes;

    Ok(main_plan)
}

/// Parse window functions from SELECT tokens.
/// Looks for patterns like: function(args) OVER (PARTITION BY ... ORDER BY ...) AS alias
pub fn parse_window_functions(tokens: &[Token]) -> Vec<WindowFunction> {
    let mut windows = Vec::new();
    let mut cursor = 0;

    while cursor < tokens.len() {
        // Look for OVER keyword
        if tokens[cursor].is_word("OVER") {
            // Walk backwards to find the function call
            let func_end = cursor;
            let mut func_start = cursor;

            // Find function name (before the opening paren)
            if func_start > 0 {
                func_start -= 1;
                // Check if there's a closing paren before OVER
                if matches!(tokens.get(func_start), Some(Token::Symbol(')'))) {
                    // Find matching opening paren
                    let mut depth = 1;
                    func_start -= 1;
                    while func_start > 0 && depth > 0 {
                        match &tokens[func_start] {
                            Token::Symbol(')') => depth += 1,
                            Token::Symbol('(') => depth -= 1,
                            _ => {}
                        }
                        if depth > 0 {
                            func_start -= 1;
                        }
                    }
                    if func_start > 0 {
                        func_start -= 1; // include function name
                    }
                }
            }

            let func_name = token_text(&tokens[func_start]);

            // Parse args (between parens)
            let args_start = func_start + 2; // skip func name and '('
            let args_end = func_end - 1; // before ')'
            let args: Vec<String> = tokens[args_start..args_end]
                .iter()
                .filter(|t| !matches!(t, Token::Symbol(',')))
                .map(token_text)
                .collect();

            cursor += 1; // skip OVER

            // Parse (PARTITION BY ... ORDER BY ...)
            let mut partition_by = Vec::new();
            let mut order_by = Vec::new();

            if matches!(tokens.get(cursor), Some(Token::Symbol('('))) {
                cursor += 1; // skip '('

                while cursor < tokens.len() {
                    if matches!(tokens.get(cursor), Some(Token::Symbol(')'))) {
                        cursor += 1;
                        break;
                    }

                    if tokens[cursor].is_word("PARTITION") {
                        cursor += 2; // skip PARTITION BY
                        while cursor < tokens.len()
                            && !tokens[cursor].is_word("ORDER")
                            && !matches!(tokens.get(cursor), Some(Token::Symbol(')')))
                        {
                            if !matches!(tokens.get(cursor), Some(Token::Symbol(','))) {
                                partition_by.push(token_text(&tokens[cursor]));
                            }
                            cursor += 1;
                        }
                    } else if tokens[cursor].is_word("ORDER") {
                        cursor += 2; // skip ORDER BY
                        while cursor < tokens.len()
                            && !matches!(tokens.get(cursor), Some(Token::Symbol(')')))
                        {
                            if !matches!(tokens.get(cursor), Some(Token::Symbol(','))) {
                                let attr = token_text(&tokens[cursor]);
                                let desc = tokens
                                    .get(cursor + 1)
                                    .map(|t| t.is_word("DESC"))
                                    .unwrap_or(false);
                                order_by.push(OrderClause {
                                    attribute: attr,
                                    descending: desc,
                                });
                                if desc {
                                    cursor += 1;
                                }
                            }
                            cursor += 1;
                        }
                    } else {
                        cursor += 1;
                    }
                }
            }

            // Parse optional AS alias
            let alias = if tokens
                .get(cursor)
                .map(|t| t.is_word("AS"))
                .unwrap_or(false)
            {
                cursor += 1;
                token_text(
                    tokens
                        .get(cursor)
                        .unwrap_or(&Token::Word(String::new())),
                )
            } else {
                func_name.clone()
            };

            windows.push(WindowFunction {
                function: func_name,
                args,
                partition_by,
                order_by,
                alias,
            });
        } else {
            cursor += 1;
        }
    }

    windows
}

/// Substitute parameters in a query string.
///
/// Supports:
/// - Positional: $1, $2, etc.
/// - Named: :name, @name
pub fn substitute_params(query: &str, params: &QueryParams) -> String {
    let mut result = query.to_string();

    // Substitute positional params ($1, $2, ...)
    for (i, value) in params.positional.iter().enumerate() {
        let placeholder = format!("${}", i + 1);
        result = result.replace(&placeholder, value);
    }

    // Substitute named params (:name, @name)
    for (name, value) in &params.named {
        result = result.replace(&format!(":{name}"), value);
        result = result.replace(&format!("@{name}"), value);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_cte() {
        let sql = "WITH active AS (SELECT * FROM users WHERE status = 'active') SELECT * FROM active";
        let plan = QueryParser::parse(sql).unwrap();
        assert_eq!(plan.ctes.len(), 1);
        assert_eq!(plan.ctes[0].name, "active");
        assert_eq!(plan.collection, "active");
    }

    #[test]
    fn test_parse_multiple_ctes() {
        let sql = "WITH a AS (SELECT * FROM users), b AS (SELECT * FROM orders) SELECT * FROM a";
        let plan = QueryParser::parse(sql).unwrap();
        assert_eq!(plan.ctes.len(), 2);
        assert_eq!(plan.ctes[0].name, "a");
        assert_eq!(plan.ctes[1].name, "b");
    }

    #[test]
    fn test_parse_recursive_cte() {
        let sql = "WITH RECURSIVE tree AS (SELECT * FROM nodes WHERE parent IS NULL UNION ALL SELECT n.* FROM nodes n JOIN tree t ON n.parent = t.id) SELECT * FROM tree";
        let plan = QueryParser::parse(sql).unwrap();
        assert_eq!(plan.ctes.len(), 1);
        assert!(plan.ctes[0].recursive);
    }

    #[test]
    fn test_substitute_params_positional() {
        let query = "SELECT * FROM users WHERE id = $1 AND name = $2";
        let mut params = QueryParams::default();
        params.positional.push("42".to_string());
        params.positional.push("'Alice'".to_string());
        let result = substitute_params(query, &params);
        assert_eq!(result, "SELECT * FROM users WHERE id = 42 AND name = 'Alice'");
    }

    #[test]
    fn test_substitute_params_named() {
        let query = "SELECT * FROM users WHERE id = :user_id";
        let mut params = QueryParams::default();
        params.named.insert("user_id".to_string(), "42".to_string());
        let result = substitute_params(query, &params);
        assert_eq!(result, "SELECT * FROM users WHERE id = 42");
    }
}
