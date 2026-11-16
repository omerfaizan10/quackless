//! Hand-written SQL tokenizer.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// Identifier or keyword. `quoted` identifiers keep their case and are never keywords.
    Word { value: String, quoted: bool },
    Number(String),
    Str(String),
    Sym(&'static str),
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub pos: usize,
}

const SYMS: [&str; 22] = [
    "<>", "!=", "<=", ">=", "||", "::", "=", "<", ">", "+", "-", "*", "/", "%", "(", ")", ",", ".", ";", "[", "]", "?",
];

pub fn tokenize(sql: &str) -> Result<Vec<Token>, String> {
    let b = sql.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // comments
        if c == b'-' && i + 1 < b.len() && b[i + 1] == b'-' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        let start = i;
        if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$' || b[i] >= 0x80) {
                i += 1;
            }
            out.push(Token { tok: Tok::Word { value: sql[start..i].to_ascii_lowercase(), quoted: false }, pos: start });
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && i + 1 < b.len() && b[i + 1].is_ascii_digit()) {
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                let save = i;
                i += 1;
                if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                    i += 1;
                }
                if i < b.len() && b[i].is_ascii_digit() {
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                } else {
                    i = save;
                }
            }
            out.push(Token { tok: Tok::Number(sql[start..i].to_string()), pos: start });
            continue;
        }
        if c == b'\'' {
            i += 1;
            let mut s = String::new();
            loop {
                if i >= b.len() {
                    return Err(format!("unterminated string literal at position {}", start));
                }
                if b[i] == b'\'' {
                    if i + 1 < b.len() && b[i + 1] == b'\'' {
                        s.push('\'');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                let ch = sql[i..].chars().next().unwrap();
                s.push(ch);
                i += ch.len_utf8();
            }
            out.push(Token { tok: Tok::Str(s), pos: start });
            continue;
        }
        if c == b'"' || c == b'`' {
            let close = c;
            i += 1;
            let s0 = i;
            while i < b.len() && b[i] != close {
                i += 1;
            }
            if i >= b.len() {
                return Err(format!("unterminated quoted identifier at position {}", start));
            }
            out.push(Token { tok: Tok::Word { value: sql[s0..i].to_string(), quoted: true }, pos: start });
            i += 1;
            continue;
        }
        let rest = &sql[i..];
        match SYMS.iter().find(|s| rest.starts_with(**s)) {
            Some(s) => {
                out.push(Token { tok: Tok::Sym(s), pos: start });
                i += s.len();
            }
            None => return Err(format!("unexpected character '{}' at position {}", rest.chars().next().unwrap(), start)),
        }
    }
    out.push(Token { tok: Tok::Eof, pos: sql.len() });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        let t = tokenize("SELECT a, 'it''s' FROM t WHERE x <= 1.5e3 -- c\n").unwrap();
        let kinds: Vec<Tok> = t.into_iter().map(|t| t.tok).collect();
        assert_eq!(kinds[0], Tok::Word { value: "select".into(), quoted: false });
        assert_eq!(kinds[3], Tok::Str("it's".into()));
        assert_eq!(kinds[8], Tok::Sym("<="));
        assert_eq!(kinds[9], Tok::Number("1.5e3".into()));
        assert_eq!(kinds[10], Tok::Eof);
    }
}
