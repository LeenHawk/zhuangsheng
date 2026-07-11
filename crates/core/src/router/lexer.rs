#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Dot,
    Bang,
    And,
    Or,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    In,
    Identifier(String),
    String(String),
    Number(String),
    Null,
    Bool(bool),
    End,
}

pub fn lex(source: &str) -> Result<Vec<Token>, String> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            byte if byte.is_ascii_whitespace() => index += 1,
            b'(' => push(&mut tokens, Token::LeftParen, &mut index),
            b')' => push(&mut tokens, Token::RightParen, &mut index),
            b'[' => push(&mut tokens, Token::LeftBracket, &mut index),
            b']' => push(&mut tokens, Token::RightBracket, &mut index),
            b',' => push(&mut tokens, Token::Comma, &mut index),
            b'.' => push(&mut tokens, Token::Dot, &mut index),
            b'!' if bytes.get(index + 1) == Some(&b'=') => {
                tokens.push(Token::NotEqual);
                index += 2;
            }
            b'!' => push(&mut tokens, Token::Bang, &mut index),
            b'&' if bytes.get(index + 1) == Some(&b'&') => {
                tokens.push(Token::And);
                index += 2;
            }
            b'|' if bytes.get(index + 1) == Some(&b'|') => {
                tokens.push(Token::Or);
                index += 2;
            }
            b'=' if bytes.get(index + 1) == Some(&b'=') => {
                tokens.push(Token::Equal);
                index += 2;
            }
            b'<' if bytes.get(index + 1) == Some(&b'=') => {
                tokens.push(Token::LessEqual);
                index += 2;
            }
            b'>' if bytes.get(index + 1) == Some(&b'=') => {
                tokens.push(Token::GreaterEqual);
                index += 2;
            }
            b'<' => push(&mut tokens, Token::Less, &mut index),
            b'>' => push(&mut tokens, Token::Greater, &mut index),
            b'"' => tokens.push(Token::String(scan_string(source, &mut index)?)),
            b'-' | b'0'..=b'9' => tokens.push(Token::Number(scan_number(source, &mut index)?)),
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let identifier = scan_identifier(source, &mut index);
                tokens.push(match identifier.as_str() {
                    "true" => Token::Bool(true),
                    "false" => Token::Bool(false),
                    "null" => Token::Null,
                    "in" => Token::In,
                    _ => Token::Identifier(identifier),
                });
            }
            _ => return Err(format!("unexpected token at byte {index}")),
        }
    }
    tokens.push(Token::End);
    Ok(tokens)
}

fn scan_string(source: &str, index: &mut usize) -> Result<String, String> {
    let bytes = source.as_bytes();
    let start = *index;
    *index += 1;
    let mut escaped = false;
    while *index < bytes.len() {
        let byte = bytes[*index];
        *index += 1;
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return serde_json::from_str(&source[start..*index])
                .map_err(|error| format!("invalid string literal: {error}"));
        } else if byte < 0x20 {
            return Err("control character in string literal".into());
        }
    }
    Err("unterminated string literal".into())
}

fn scan_number(source: &str, index: &mut usize) -> Result<String, String> {
    let bytes = source.as_bytes();
    let start = *index;
    if bytes[*index] == b'-' {
        *index += 1;
    }
    if bytes.get(*index) == Some(&b'0') {
        *index += 1;
    } else {
        let digits = *index;
        while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
            *index += 1;
        }
        if digits == *index {
            return Err(format!("invalid number at byte {start}"));
        }
    }
    if bytes.get(*index) == Some(&b'.') {
        *index += 1;
        let digits = *index;
        while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
            *index += 1;
        }
        if digits == *index {
            return Err("fraction requires digits".into());
        }
    }
    if matches!(bytes.get(*index), Some(b'e' | b'E')) {
        *index += 1;
        if matches!(bytes.get(*index), Some(b'+' | b'-')) {
            *index += 1;
        }
        let digits = *index;
        while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
            *index += 1;
        }
        if digits == *index {
            return Err("exponent requires digits".into());
        }
    }
    Ok(source[start..*index].into())
}

fn scan_identifier(source: &str, index: &mut usize) -> String {
    let start = *index;
    let bytes = source.as_bytes();
    while bytes
        .get(*index)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        *index += 1;
    }
    source[start..*index].into()
}

fn push(tokens: &mut Vec<Token>, token: Token, index: &mut usize) {
    tokens.push(token);
    *index += 1;
}
