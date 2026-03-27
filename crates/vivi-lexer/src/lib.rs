use logos::Logos;
use std::ops::Range;

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t]+")]
#[logos(skip r"//[^\n]*")]
pub enum Token {
    // Keywords
    #[token("component")]
    Component,
    #[token("system")]
    System,
    #[token("query")]
    Query,
    #[token("read")]
    Read,
    #[token("write")]
    Write,
    #[token("each")]
    Each,
    #[token("world")]
    World,
    #[token("systems")]
    Systems,
    #[token("entity")]
    EntityKw,
    #[token("extern")]
    Extern,
    #[token("fn")]
    Fn,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("while")]
    While,
    #[token("let")]
    Let,
    #[token("return")]
    Return,
    #[token("global")]
    Global,
    #[token("spawn")]
    Spawn,
    #[token("despawn")]
    Despawn,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("and")]
    And,
    #[token("or")]
    Or,
    #[token("not")]
    Not,

    // Types
    #[token("i32")]
    I32,
    #[token("i64")]
    I64,
    #[token("f32")]
    F32,
    #[token("f64")]
    F64,
    #[token("bool")]
    Bool,
    #[token("Entity")]
    EntityType,

    // Punctuation
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token(":")]
    Colon,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("=")]
    Eq,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("->")]
    Arrow,

    // Literals
    #[regex(r"[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    FloatLit(f64),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    IntLit(i64),

    // Identifier
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    // Newline (significant for statement separation)
    #[regex(r"\n(\r?\n)*")]
    Newline,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Component => write!(f, "component"),
            Token::System => write!(f, "system"),
            Token::Query => write!(f, "query"),
            Token::Read => write!(f, "read"),
            Token::Write => write!(f, "write"),
            Token::Each => write!(f, "each"),
            Token::World => write!(f, "world"),
            Token::Systems => write!(f, "systems"),
            Token::EntityKw => write!(f, "entity"),
            Token::Extern => write!(f, "extern"),
            Token::Fn => write!(f, "fn"),
            Token::If => write!(f, "if"),
            Token::Else => write!(f, "else"),
            Token::While => write!(f, "while"),
            Token::Let => write!(f, "let"),
            Token::Return => write!(f, "return"),
            Token::Global => write!(f, "global"),
            Token::Spawn => write!(f, "spawn"),
            Token::Despawn => write!(f, "despawn"),
            Token::True => write!(f, "true"),
            Token::False => write!(f, "false"),
            Token::And => write!(f, "and"),
            Token::Or => write!(f, "or"),
            Token::Not => write!(f, "not"),
            Token::I32 => write!(f, "i32"),
            Token::I64 => write!(f, "i64"),
            Token::F32 => write!(f, "f32"),
            Token::F64 => write!(f, "f64"),
            Token::Bool => write!(f, "bool"),
            Token::EntityType => write!(f, "Entity"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::Colon => write!(f, ":"),
            Token::Comma => write!(f, ","),
            Token::Dot => write!(f, "."),
            Token::Eq => write!(f, "="),
            Token::EqEq => write!(f, "=="),
            Token::NotEq => write!(f, "!="),
            Token::Lt => write!(f, "<"),
            Token::Gt => write!(f, ">"),
            Token::LtEq => write!(f, "<="),
            Token::GtEq => write!(f, ">="),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Arrow => write!(f, "->"),
            Token::FloatLit(v) => write!(f, "{v}"),
            Token::IntLit(v) => write!(f, "{v}"),
            Token::Ident(s) => write!(f, "{s}"),
            Token::Newline => write!(f, "newline"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Spanned {
    pub token: Token,
    pub span: Range<usize>,
}

pub fn lex(source: &str) -> Result<Vec<Spanned>, LexError> {
    let mut lexer = Token::lexer(source);
    let mut tokens = Vec::new();

    while let Some(result) = lexer.next() {
        match result {
            Ok(token) => {
                tokens.push(Spanned {
                    token,
                    span: lexer.span(),
                });
            }
            Err(()) => {
                return Err(LexError {
                    span: lexer.span(),
                    source_code: source.to_string(),
                });
            }
        }
    }

    Ok(tokens)
}

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("unexpected character")]
pub struct LexError {
    #[label("here")]
    pub span: Range<usize>,
    #[source_code]
    pub source_code: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_component() {
        let tokens = lex("component Position { x: f32 }").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| &t.token).collect();
        assert!(matches!(kinds[0], Token::Component));
        assert!(matches!(kinds[1], Token::Ident(s) if s == "Position"));
        assert!(matches!(kinds[2], Token::LBrace));
        assert!(matches!(kinds[3], Token::Ident(s) if s == "x"));
        assert!(matches!(kinds[4], Token::Colon));
        assert!(matches!(kinds[5], Token::F32));
        assert!(matches!(kinds[6], Token::RBrace));
    }

    #[test]
    fn test_lex_float() {
        let tokens = lex("3.14").unwrap();
        assert!(matches!(&tokens[0].token, Token::FloatLit(v) if (*v - 3.14).abs() < 1e-10));
    }
}
