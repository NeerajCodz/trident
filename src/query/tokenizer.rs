use crate::errors::{PraxisError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(String),
    String(String),
    Number(String),
    Symbol(char),
    Operator(String),
}

impl Token {
    pub fn is_word(&self, expected: &str) -> bool {
        matches!(self, Token::Word(word) if word.eq_ignore_ascii_case(expected))
    }

    pub fn text(&self) -> String {
        match self {
            Token::Word(value)
            | Token::String(value)
            | Token::Number(value)
            | Token::Operator(value) => value.clone(),
            Token::Symbol(value) => value.to_string(),
        }
    }
}

pub fn tokenize(input: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.peek().copied() {
        match character {
            ';' | ',' | '(' | ')' | '*' => {
                tokens.push(Token::Symbol(character));
                chars.next();
            }
            '\'' | '"' => tokens.push(Token::String(read_quoted(&mut chars)?)),
            '=' | '!' | '<' | '>' => tokens.push(Token::Operator(read_operator(&mut chars))),
            character if character.is_ascii_digit() || character == '-' => {
                tokens.push(Token::Number(read_while(&mut chars, |value| {
                    value.is_ascii_digit() || matches!(value, '.' | '-' | ':')
                })));
            }
            character if character.is_whitespace() => {
                chars.next();
            }
            character if is_identifier_start(character) => {
                tokens.push(Token::Word(read_while(&mut chars, is_identifier_continue)));
            }
            _ => {
                return Err(PraxisError::Query(format!(
                    "unsupported query character '{character}'"
                )));
            }
        }
    }
    Ok(tokens)
}

fn read_quoted(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<String> {
    let Some(quote) = chars.next() else {
        return Err(PraxisError::Query("expected quote".into()));
    };
    let mut output = String::new();
    while let Some(character) = chars.next() {
        if character == quote {
            return Ok(output);
        }
        if character == '\\' {
            if let Some(escaped) = chars.next() {
                output.push(escaped);
            }
        } else {
            output.push(character);
        }
    }
    Err(PraxisError::Query("unterminated string literal".into()))
}

fn read_operator(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let first = chars.next().expect("operator char exists");
    let mut output = first.to_string();
    if matches!(chars.peek(), Some('=')) {
        output.push('=');
        chars.next();
    }
    output
}

fn read_while(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    accepts: impl Fn(char) -> bool,
) -> String {
    let mut output = String::new();
    while let Some(character) = chars.peek().copied() {
        if !accepts(character) {
            break;
        }
        output.push(character);
        chars.next();
    }
    output
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_' || character == '`'
}

fn is_identifier_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-' | '`')
}
