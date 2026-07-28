//! Tokenizer for calculator expressions (Unicode operators + ASCII aliases).

use super::CalcError;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Multiply,
    Divide,
    Power,
    Percent,
    Factorial,
    Mod,
    LParen,
    RParen,
    Comma,
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    BitNand,
    BitNor,
    BitXnor,
    LShift,
    RShift,
    UrShift,
    Sqrt,
}

pub fn tokenize(input: &str) -> Result<Vec<Token>, CalcError> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // Multi-character ASCII operators first
        if matches_at(&chars, i, "**") {
            tokens.push(Token::Power);
            i += 2;
            continue;
        }
        if matches_at(&chars, i, "<<") {
            tokens.push(Token::LShift);
            i += 2;
            continue;
        }
        if matches_at(&chars, i, ">>>") {
            tokens.push(Token::UrShift);
            i += 3;
            continue;
        }
        if matches_at(&chars, i, ">>") {
            tokens.push(Token::RShift);
            i += 2;
            continue;
        }
        if matches_at(&chars, i, "&&") || matches_at(&chars, i, "AND") {
            // AND as word is handled via ident; && is BitAnd
            if matches_at(&chars, i, "&&") {
                tokens.push(Token::BitAnd);
                i += 2;
                continue;
            }
        }

        match c {
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' | '−' | '–' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' | '×' | '⋅' => {
                tokens.push(Token::Multiply);
                i += 1;
            }
            '/' | '÷' => {
                tokens.push(Token::Divide);
                i += 1;
            }
            '^' => {
                tokens.push(Token::Power);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            '!' => {
                tokens.push(Token::Factorial);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '√' => {
                tokens.push(Token::Sqrt);
                i += 1;
            }
            '∧' => {
                tokens.push(Token::BitAnd);
                i += 1;
            }
            '∨' => {
                tokens.push(Token::BitOr);
                i += 1;
            }
            '⊻' => {
                tokens.push(Token::BitXor);
                i += 1;
            }
            '¬' | '~' => {
                tokens.push(Token::BitNot);
                i += 1;
            }
            '⊼' => {
                tokens.push(Token::BitNand);
                i += 1;
            }
            '⊽' => {
                tokens.push(Token::BitNor);
                i += 1;
            }
            '⊙' => {
                tokens.push(Token::BitXnor);
                i += 1;
            }
            '≪' => {
                tokens.push(Token::LShift);
                i += 1;
            }
            '≫' => {
                tokens.push(Token::RShift);
                i += 1;
            }
            '⋙' => {
                tokens.push(Token::UrShift);
                i += 1;
            }
            'π' => {
                tokens.push(Token::Ident("π".into()));
                i += 1;
            }
            'τ' => {
                tokens.push(Token::Ident("τ".into()));
                i += 1;
            }
            'φ' | 'ϕ' => {
                tokens.push(Token::Ident("φ".into()));
                i += 1;
            }
            'e' | 'E' if is_standalone_e(&chars, i) => {
                tokens.push(Token::Ident("e".into()));
                i += 1;
            }
            '0'..='9' | '.' => {
                let (n, next) = read_number(&chars, i)?;
                tokens.push(Token::Number(n));
                i = next;
            }
            c if c.is_ascii_alphabetic() || c == '_' || is_fn_char(c) => {
                let (ident, next) = read_ident(&chars, i);
                match ident.as_str() {
                    "mod" | "MOD" => tokens.push(Token::Mod),
                    "and" | "AND" => tokens.push(Token::BitAnd),
                    "or" | "OR" => tokens.push(Token::BitOr),
                    "xor" | "XOR" => tokens.push(Token::BitXor),
                    "not" | "NOT" => tokens.push(Token::BitNot),
                    "nand" | "NAND" => tokens.push(Token::BitNand),
                    "nor" | "NOR" => tokens.push(Token::BitNor),
                    "xnor" | "XNOR" => tokens.push(Token::BitXnor),
                    other => tokens.push(Token::Ident(other.to_string())),
                }
                i = next;
            }
            _ => {
                return Err(CalcError::Syntax(format!("Unexpected character '{c}'")));
            }
        }
    }

    Ok(tokens)
}

fn matches_at(chars: &[char], i: usize, s: &str) -> bool {
    let s_chars: Vec<char> = s.chars().collect();
    if i + s_chars.len() > chars.len() {
        return false;
    }
    chars[i..i + s_chars.len()]
        .iter()
        .zip(s_chars.iter())
        .all(|(a, b)| a == b)
}

fn is_fn_char(c: char) -> bool {
    matches!(c, '⁻' | '¹' | '₀'..='₉')
}

fn is_standalone_e(chars: &[char], i: usize) -> bool {
    // 'e' as Euler's number only when not part of a larger identifier
    // and not scientific notation (handled in read_number).
    let prev_ok = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
    let next_ok = i + 1 >= chars.len()
        || (!chars[i + 1].is_ascii_alphanumeric() && chars[i + 1] != '_');
    prev_ok && next_ok
}

fn read_number(chars: &[char], start: usize) -> Result<(f64, usize), CalcError> {
    let mut i = start;
    let mut buf = String::new();

    // Hex / binary / octal prefixes
    if chars[i] == '0' && i + 1 < chars.len() {
        match chars[i + 1] {
            'x' | 'X' => {
                i += 2;
                let mut hex = String::new();
                while i < chars.len() && chars[i].is_ascii_hexdigit() {
                    hex.push(chars[i]);
                    i += 1;
                }
                if hex.is_empty() {
                    return Err(CalcError::Syntax("Invalid hex number".into()));
                }
                let n = u64::from_str_radix(&hex, 16)
                    .map_err(|_| CalcError::Syntax("Invalid hex number".into()))?;
                return Ok((n as f64, i));
            }
            'b' | 'B' => {
                i += 2;
                let mut bin = String::new();
                while i < chars.len() && (chars[i] == '0' || chars[i] == '1') {
                    bin.push(chars[i]);
                    i += 1;
                }
                if bin.is_empty() {
                    return Err(CalcError::Syntax("Invalid binary number".into()));
                }
                let n = u64::from_str_radix(&bin, 2)
                    .map_err(|_| CalcError::Syntax("Invalid binary number".into()))?;
                return Ok((n as f64, i));
            }
            'o' | 'O' => {
                i += 2;
                let mut oct = String::new();
                while i < chars.len() && chars[i] >= '0' && chars[i] <= '7' {
                    oct.push(chars[i]);
                    i += 1;
                }
                if oct.is_empty() {
                    return Err(CalcError::Syntax("Invalid octal number".into()));
                }
                let n = u64::from_str_radix(&oct, 8)
                    .map_err(|_| CalcError::Syntax("Invalid octal number".into()))?;
                return Ok((n as f64, i));
            }
            _ => {}
        }
    }

    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
        buf.push(chars[i]);
        i += 1;
    }

    // Scientific notation
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let peek = i + 1;
        if peek < chars.len()
            && (chars[peek].is_ascii_digit() || chars[peek] == '+' || chars[peek] == '-')
        {
            buf.push(chars[i]);
            i += 1;
            if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                buf.push(chars[i]);
                i += 1;
            }
            while i < chars.len() && chars[i].is_ascii_digit() {
                buf.push(chars[i]);
                i += 1;
            }
        }
    }

    let n = buf
        .parse::<f64>()
        .map_err(|_| CalcError::Syntax(format!("Invalid number '{buf}'")))?;
    Ok((n, i))
}

fn read_ident(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    let mut buf = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphanumeric() || c == '_' || is_fn_char(c) {
            buf.push(c);
            i += 1;
        } else {
            break;
        }
    }
    // Normalize inverse function superscripts: sin⁻¹ → asin, etc.
    let normalized = normalize_ident(&buf);
    (normalized, i)
}

fn normalize_ident(s: &str) -> String {
    match s {
        "sin⁻¹" | "asin" | "arcsin" => "asin".into(),
        "cos⁻¹" | "acos" | "arccos" => "acos".into(),
        "tan⁻¹" | "atan" | "arctan" => "atan".into(),
        "sinh⁻¹" | "asinh" => "asinh".into(),
        "cosh⁻¹" | "acosh" => "acosh".into(),
        "tanh⁻¹" | "atanh" => "atanh".into(),
        "pi" | "PI" => "π".into(),
        "tau" | "TAU" => "τ".into(),
        "phi" | "PHI" => "φ".into(),
        other => other.to_string(),
    }
}
