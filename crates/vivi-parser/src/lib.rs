pub mod ast;
pub mod parser;

use vivi_lexer::lex;

pub fn parse(source: &str) -> Result<ast::Program, Box<dyn std::error::Error>> {
    let tokens = lex(source)?;
    let mut parser = parser::Parser::new(tokens, source.to_string());
    let program = parser.parse_program()?;
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_component() {
        let src = "component Position {\n    x: f32\n    y: f32\n}";
        let prog = parse(src).unwrap();
        assert_eq!(prog.items.len(), 1);
        if let ast::Item::Component(c) = &prog.items[0] {
            assert_eq!(c.name, "Position");
            assert_eq!(c.fields.len(), 2);
        } else {
            panic!("expected component");
        }
    }

    #[test]
    fn test_parse_system() {
        let src = r#"system Movement {
    query {
        write Position
        read Velocity
    }
    each(pos: Position, vel: Velocity) {
        pos.x = pos.x + vel.dx
    }
}"#;
        let prog = parse(src).unwrap();
        assert_eq!(prog.items.len(), 1);
        if let ast::Item::System(s) = &prog.items[0] {
            assert_eq!(s.name, "Movement");
            assert_eq!(s.query.as_ref().unwrap().entries.len(), 2);
            assert_eq!(s.each.as_ref().unwrap().params.len(), 2);
        } else {
            panic!("expected system");
        }
    }
}
