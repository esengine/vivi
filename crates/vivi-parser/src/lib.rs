pub mod ast;
pub mod parser;

use std::collections::HashSet;
use vivi_lexer::lex;

// Embedded standard library modules
const STD_MATH: &str = include_str!("../../../std/vivi/math.vivi");
const STD_RENDER: &str = include_str!("../../../std/vivi/render.vivi");
const STD_INPUT: &str = include_str!("../../../std/vivi/input.vivi");

fn get_std_module(path: &[String]) -> Option<&'static str> {
    if path.len() == 2 && path[0] == "std" {
        match path[1].as_str() {
            "math" => Some(STD_MATH),
            "render" => Some(STD_RENDER),
            "input" => Some(STD_INPUT),
            _ => None,
        }
    } else {
        None
    }
}

pub fn parse(source: &str) -> Result<ast::Program, Box<dyn std::error::Error>> {
    parse_file(source, None).map(|(program, _)| program)
}

pub fn parse_with_modules(source: &str) -> Result<(ast::Program, Vec<String>), Box<dyn std::error::Error>> {
    parse_file(source, None)
}

/// Parse with a base directory for resolving local `use ./path` imports.
pub fn parse_file(source: &str, base_dir: Option<&std::path::Path>) -> Result<(ast::Program, Vec<String>), Box<dyn std::error::Error>> {
    let tokens = lex(source)?;
    let mut parser = parser::Parser::new(tokens, source.to_string());
    let mut program = parser.parse_program()?;

    let mut used_modules = Vec::new();
    let mut resolved = HashSet::new();
    resolve_uses(&mut program, &mut used_modules, &mut resolved, base_dir)?;

    Ok((program, used_modules))
}

fn resolve_uses(
    program: &mut ast::Program,
    used_modules: &mut Vec<String>,
    resolved: &mut HashSet<String>,
    base_dir: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut i = 0;
    while i < program.items.len() {
        if let ast::Item::Use(use_decl) = &program.items[i] {
            let path_key = use_decl.path.join(".");
            if resolved.contains(&path_key) {
                program.items.remove(i);
                continue;
            }

            let module_src: String;
            let is_local;

            if let Some(std_src) = get_std_module(&use_decl.path) {
                // Standard library module
                module_src = std_src.to_string();
                is_local = false;
            } else {
                // Local file import: use ./path/to/module → ./path/to/module.vivi
                let file_path = use_decl.path.join("/") + ".vivi";
                let full_path = if let Some(dir) = base_dir {
                    dir.join(&file_path)
                } else {
                    std::path::PathBuf::from(&file_path)
                };
                module_src = std::fs::read_to_string(&full_path)
                    .map_err(|e| format!("cannot load module `{path_key}`: {} ({})", e, full_path.display()))?;
                is_local = true;
            }

            let mut module_program = {
                let tokens = lex(&module_src)?;
                let mut p = parser::Parser::new(tokens, module_src.clone());
                p.parse_program()?
            };

            // Resolve uses in the imported module (with same base_dir for local, or None for std)
            let nested_base = if is_local {
                base_dir
            } else {
                None
            };
            resolve_uses(&mut module_program, used_modules, resolved, nested_base)?;

            resolved.insert(path_key.clone());
            used_modules.push(path_key);

            program.items.remove(i);
            for (j, item) in module_program.items.into_iter().enumerate() {
                program.items.insert(i + j, item);
            }
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
