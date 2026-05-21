//! Cypher-Lite lexer. Hand-rolled; no external parser-combinator crates.

use crate::cypher::error::CypherError;

/// A token with its source span (byte offsets into the original query string).
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub token: T,
    pub start: usize,
    pub end: usize,
}

impl<T> Spanned<T> {
    pub fn span(&self) -> (usize, usize) {
        (self.start, self.end)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // --- Keywords (supported) ---
    Match,
    Where,
    Return,
    Order,
    By,
    Limit,
    Skip,
    Distinct,
    And,
    Or,
    Not,
    Is,
    Null,
    Contains,
    Starts,
    Ends,
    With,
    As,
    Asc,
    Desc,
    True,
    False,
    In,
    // --- Keywords (forbidden — detected so we can emit Unsupported) ---
    Unwind,
    Merge,
    Create,
    Delete,
    Set,
    Remove,
    Optional,
    // --- Symbols ---
    LParen,   // (
    RParen,   // )
    LBracket, // [
    RBracket, // ]
    LBrace,   // {
    RBrace,   // }
    Comma,    // ,
    Dot,      // .
    Colon,    // :
    Semicolon, // ;
    Minus,    // -
    Star,     // *
    Pipe,     // |
    Arrow,    // ->
    LeftArrow, // <-
    DoubleDash, // --
    Lt,       // <
    Gt,       // >
    Le,       // <=
    Ge,       // >=
    Eq,       // =
    Neq,      // != or <>
    RegexEq,  // =~
    // --- Literals ---
    StringLit(String),
    IntLit(i64),
    FloatLit(f64),
    // --- Identifiers ---
    Ident(String),
}

/// Tokenize `query` and return the token stream with spans.
/// Returns `Err(CypherError::Parse)` on the first lexing error.
pub fn tokenize(query: &str) -> Result<Vec<Spanned<Token>>, CypherError> {
    let bytes = query.as_bytes();
    let len = bytes.len();
    let mut pos = 0usize;
    let mut tokens = Vec::new();

    while pos < len {
        // Skip whitespace.
        if bytes[pos].is_ascii_whitespace() {
            pos += 1;
            continue;
        }

        // Skip line comments `//`.
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            while pos < len && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }

        // Skip block comments `/* ... */`.
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'*' {
            pos += 2;
            loop {
                if pos + 1 >= len {
                    return Err(CypherError::parse("unterminated block comment", (pos, pos)));
                }
                if bytes[pos] == b'*' && bytes[pos + 1] == b'/' {
                    pos += 2;
                    break;
                }
                pos += 1;
            }
            continue;
        }

        let start = pos;

        // Two-char symbols first.
        if pos + 1 < len {
            let two = &bytes[pos..pos + 2];
            let tok = match two {
                b"->" => Some(Token::Arrow),
                b"<-" => Some(Token::LeftArrow),
                b"--" => Some(Token::DoubleDash),
                b"<=" => Some(Token::Le),
                b">=" => Some(Token::Ge),
                b"!=" => Some(Token::Neq),
                b"<>" => Some(Token::Neq),
                b"=~" => Some(Token::RegexEq),
                _ => None,
            };
            if let Some(t) = tok {
                tokens.push(Spanned { token: t, start, end: pos + 2 });
                pos += 2;
                continue;
            }
        }

        // Single-char symbols.
        let single = match bytes[pos] {
            b'(' => Some(Token::LParen),
            b')' => Some(Token::RParen),
            b'[' => Some(Token::LBracket),
            b']' => Some(Token::RBracket),
            b'{' => Some(Token::LBrace),
            b'}' => Some(Token::RBrace),
            b',' => Some(Token::Comma),
            b'.' => Some(Token::Dot),
            b':' => Some(Token::Colon),
            b';' => Some(Token::Semicolon),
            b'-' => Some(Token::Minus),
            b'*' => Some(Token::Star),
            b'|' => Some(Token::Pipe),
            b'<' => Some(Token::Lt),
            b'>' => Some(Token::Gt),
            b'=' => Some(Token::Eq),
            _ => None,
        };
        if let Some(t) = single {
            tokens.push(Spanned { token: t, start, end: pos + 1 });
            pos += 1;
            continue;
        }

        // String literals (single or double quoted).
        if bytes[pos] == b'\'' || bytes[pos] == b'"' {
            let quote = bytes[pos];
            pos += 1;
            let mut s = String::new();
            loop {
                if pos >= len {
                    return Err(CypherError::parse("unterminated string literal", (start, pos)));
                }
                if bytes[pos] == quote {
                    pos += 1;
                    break;
                }
                if bytes[pos] == b'\\' {
                    pos += 1;
                    if pos >= len {
                        return Err(CypherError::parse("unterminated escape sequence", (start, pos)));
                    }
                    match bytes[pos] {
                        b'n' => s.push('\n'),
                        b't' => s.push('\t'),
                        b'\\' => s.push('\\'),
                        b'\'' => s.push('\''),
                        b'"' => s.push('"'),
                        b'r' => s.push('\r'),
                        other => {
                            s.push('\\');
                            s.push(other as char);
                        }
                    }
                    pos += 1;
                } else {
                    // Multi-byte UTF-8 characters: copy raw bytes.
                    let ch_start = pos;
                    pos += 1;
                    while pos < len && (bytes[pos] & 0xC0) == 0x80 {
                        pos += 1;
                    }
                    s.push_str(&query[ch_start..pos]);
                }
            }
            tokens.push(Spanned { token: Token::StringLit(s), start, end: pos });
            continue;
        }

        // Backtick-quoted identifiers.
        if bytes[pos] == b'`' {
            pos += 1;
            let id_start = pos;
            while pos < len && bytes[pos] != b'`' {
                pos += 1;
            }
            if pos >= len {
                return Err(CypherError::parse("unterminated backtick identifier", (start, pos)));
            }
            let ident = query[id_start..pos].to_string();
            pos += 1; // skip closing backtick
            tokens.push(Spanned { token: Token::Ident(ident), start, end: pos });
            continue;
        }

        // Numbers: integer or float.
        if bytes[pos].is_ascii_digit() || (bytes[pos] == b'-' && pos + 1 < len && bytes[pos + 1].is_ascii_digit()) {
            // Note: bare `-` before a digit — handle only when clearly numeric context.
            // In practice the parser passes the Minus token for ambiguous cases.
            let num_start = pos;
            if bytes[pos] == b'-' {
                pos += 1;
            }
            while pos < len && bytes[pos].is_ascii_digit() {
                pos += 1;
            }
            let mut is_float = false;
            if pos < len && bytes[pos] == b'.' && pos + 1 < len && bytes[pos + 1].is_ascii_digit() {
                is_float = true;
                pos += 1;
                while pos < len && bytes[pos].is_ascii_digit() {
                    pos += 1;
                }
            }
            // Exponent
            if pos < len && (bytes[pos] == b'e' || bytes[pos] == b'E') {
                is_float = true;
                pos += 1;
                if pos < len && (bytes[pos] == b'+' || bytes[pos] == b'-') {
                    pos += 1;
                }
                while pos < len && bytes[pos].is_ascii_digit() {
                    pos += 1;
                }
            }
            let num_str = &query[num_start..pos];
            if is_float {
                let f: f64 = num_str.parse().map_err(|_| {
                    CypherError::parse(format!("invalid float literal `{num_str}`"), (start, pos))
                })?;
                tokens.push(Spanned { token: Token::FloatLit(f), start, end: pos });
            } else {
                let i: i64 = num_str.parse().map_err(|_| {
                    CypherError::parse(format!("invalid integer literal `{num_str}`"), (start, pos))
                })?;
                tokens.push(Spanned { token: Token::IntLit(i), start, end: pos });
            }
            continue;
        }

        // Identifiers and keywords (ASCII + underscore; also accept leading $ for params).
        if bytes[pos].is_ascii_alphabetic() || bytes[pos] == b'_' || bytes[pos] == b'$' {
            let id_start = pos;
            while pos < len && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_' || bytes[pos] == b'$') {
                pos += 1;
            }
            let raw = &query[id_start..pos];
            let tok = keyword_or_ident(raw);
            tokens.push(Spanned { token: tok, start, end: pos });
            continue;
        }

        return Err(CypherError::parse(
            format!("unexpected character `{}`", bytes[pos] as char),
            (pos, pos + 1),
        ));
    }

    Ok(tokens)
}

fn keyword_or_ident(raw: &str) -> Token {
    match raw.to_ascii_uppercase().as_str() {
        "MATCH" => Token::Match,
        "WHERE" => Token::Where,
        "RETURN" => Token::Return,
        "ORDER" => Token::Order,
        "BY" => Token::By,
        "LIMIT" => Token::Limit,
        "SKIP" => Token::Skip,
        "DISTINCT" => Token::Distinct,
        "AND" => Token::And,
        "OR" => Token::Or,
        "NOT" => Token::Not,
        "IS" => Token::Is,
        "NULL" => Token::Null,
        "CONTAINS" => Token::Contains,
        "STARTS" => Token::Starts,
        "ENDS" => Token::Ends,
        "WITH" => Token::With,
        "AS" => Token::As,
        "ASC" => Token::Asc,
        "DESC" => Token::Desc,
        "TRUE" => Token::True,
        "FALSE" => Token::False,
        "IN" => Token::In,
        // Forbidden-but-recognised keywords.
        "UNWIND" => Token::Unwind,
        "MERGE" => Token::Merge,
        "CREATE" => Token::Create,
        "DELETE" => Token::Delete,
        "SET" => Token::Set,
        "REMOVE" => Token::Remove,
        "OPTIONAL" => Token::Optional,
        _ => Token::Ident(raw.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(q: &str) -> Vec<Token> {
        tokenize(q).unwrap().into_iter().map(|s| s.token).collect()
    }

    #[test]
    fn keywords_case_insensitive() {
        assert_eq!(toks("match"), vec![Token::Match]);
        assert_eq!(toks("MATCH"), vec![Token::Match]);
        assert_eq!(toks("Match"), vec![Token::Match]);
    }

    #[test]
    fn symbols() {
        let t = toks("->  <-  --  <=  >=  !=  <>  =~");
        assert_eq!(t, vec![
            Token::Arrow, Token::LeftArrow, Token::DoubleDash,
            Token::Le, Token::Ge, Token::Neq, Token::Neq, Token::RegexEq,
        ]);
    }

    #[test]
    fn string_escapes() {
        let t = toks(r#"'hello\nworld'"#);
        assert_eq!(t, vec![Token::StringLit("hello\nworld".into())]);
    }

    #[test]
    #[allow(clippy::approx_constant)] // 3.14 is a test literal, not an approximation of PI
    fn integer_and_float() {
        let t = toks("42 3.14");
        assert_eq!(t, vec![Token::IntLit(42), Token::FloatLit(3.14)]);
    }

    #[test]
    fn backtick_ident() {
        let t = toks("`my weird ident`");
        assert_eq!(t, vec![Token::Ident("my weird ident".into())]);
    }

    #[test]
    fn spans_are_accurate() {
        let tokens = tokenize("MATCH (n)").unwrap();
        assert_eq!(tokens[0].start, 0);
        assert_eq!(tokens[0].end, 5); // "MATCH"
        assert_eq!(tokens[1].start, 6); // "("
    }

    #[test]
    fn forbidden_keywords_are_recognised() {
        assert_eq!(toks("UNWIND"), vec![Token::Unwind]);
        assert_eq!(toks("MERGE"), vec![Token::Merge]);
        assert_eq!(toks("CREATE"), vec![Token::Create]);
        assert_eq!(toks("DELETE"), vec![Token::Delete]);
        assert_eq!(toks("SET"), vec![Token::Set]);
        assert_eq!(toks("REMOVE"), vec![Token::Remove]);
        assert_eq!(toks("OPTIONAL"), vec![Token::Optional]);
    }
}
