pub mod ast;
pub mod parser;

use std::collections::HashSet;
use vivi_lexer::lex;

// Embedded standard library modules
const STD_MATH: &str = include_str!("../../../std/vivi/math.vivi");
const STD_RENDER: &str = include_str!("../../../std/vivi/render.vivi");

fn get_std_module(path: &[String]) -> Option<&'static str> {
    if path.len() == 2 && path[0] == "std" {
        match path[1].as_str() {
            "math" => Some(STD_MATH),
            "render" => Some(STD_RENDER),
            _ => None,
        }
    } else {
        None
    }
}

pub fn parse(source: &str) -> Result<ast::Program, Box<dyn std::error::Error>> {
    let tokens = lex(source)?;
    let mut parser = parser::Parser::new(tokens, source.to_string());
    let mut program = parser.parse_program()?;

    // Resolve use declarations
    resolve_uses(&mut program)?;

    Ok(program)
}

fn resolve_uses(program: &mut ast::Program) -> Result<(), Box<dyn std::error::Error>> {
    let mut resolved: HashSet<String> = HashSet::new();
    let mut i = 0;
    while i < program.items.len() {
        if let ast::Item::Use(use_decl) = &program.items[i] {
            let path_key = use_decl.path.join(".");
            if resolved.contains(&path_key) {
                // Already imported, skip
                program.items.remove(i);
                continue;
            }

            let module_src = get_std_module(&use_decl.path)
                .ok_or_else(|| format!("unknown module `{path_key}`"))?;

            let mut module_program = {
                let tokens = lex(module_src)?;
                let mut p = parser::Parser::new(tokens, module_src.to_string());
                p.parse_program()?
            };

            // Recursively resolve uses in the imported module
            resolve_uses(&mut module_program)?;

            resolved.insert(path_key);

            // Replace the Use item with the module's items
            program.items.remove(i);
            for (j, item) in module_program.items.into_iter().enumerate() {
                program.items.insert(i + j, item);
            }
            // Don't increment i — re-check from the same position
        } else {
            i += 1;
        }
    }
    Ok(())
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
