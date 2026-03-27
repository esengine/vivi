use crate::ast::*;
use vivi_lexer::{Spanned, Token};

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("{message}")]
pub struct ParseError {
    pub message: String,
    #[label("{label}")]
    pub span: std::ops::Range<usize>,
    pub label: String,
    #[source_code]
    pub source_code: String,
}

pub struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
    source: String,
}

impl Parser {
    pub fn new(tokens: Vec<Spanned>, source: String) -> Self {
        Self {
            tokens,
            pos: 0,
            source,
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        self.skip_newlines();
        while !self.is_at_end() {
            let item = self.parse_item()?;
            items.push(item);
            self.skip_newlines();
        }
        Ok(Program { items })
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        match self.peek_token() {
            Some(Token::Component) => Ok(Item::Component(self.parse_component()?)),
            Some(Token::System) => Ok(Item::System(self.parse_system()?)),
            Some(Token::World) => Ok(Item::World(self.parse_world()?)),
            Some(Token::Fn) => Ok(Item::Fn(self.parse_fn_def()?)),
            Some(Token::Extern) => Ok(Item::Extern(self.parse_extern_block()?)),
            Some(Token::EntityKw) => Ok(Item::Entity(self.parse_entity_def()?)),
            Some(other) => Err(self.error(format!("expected top-level item, found `{other}`"))),
            None => Err(self.error_eof("expected top-level item")),
        }
    }

    fn parse_component(&mut self) -> Result<ComponentDef, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::Component)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        self.skip_newlines();

        let mut fields = Vec::new();
        while !self.check(&Token::RBrace) {
            let field_start = self.current_span().start;
            let field_name = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;
            let field_end = self.previous_span().end;
            fields.push(Field {
                name: field_name,
                ty,
                span: field_start..field_end,
            });
            self.skip_newlines();
            // optional comma
            if self.check(&Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.expect(Token::RBrace)?;
        let end = self.previous_span().end;
        Ok(ComponentDef {
            name,
            fields,
            span: start..end,
        })
    }

    fn parse_system(&mut self) -> Result<SystemDef, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::System)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        self.skip_newlines();

        // Check if system has query/each or just bare statements
        if self.check(&Token::Query) {
            let query = self.parse_query()?;
            self.skip_newlines();
            let each = self.parse_each()?;
            self.skip_newlines();
            self.expect(Token::RBrace)?;
            let end = self.previous_span().end;
            Ok(SystemDef {
                name,
                query: Some(query),
                each: Some(each),
                body: vec![],
                span: start..end,
            })
        } else {
            // Bare system: just statements, no query/each
            let body = self.parse_block_body()?;
            self.expect(Token::RBrace)?;
            let end = self.previous_span().end;
            Ok(SystemDef {
                name,
                query: None,
                each: None,
                body,
                span: start..end,
            })
        }
    }

    fn parse_query(&mut self) -> Result<QueryDef, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::Query)?;
        self.expect(Token::LBrace)?;
        self.skip_newlines();

        let mut entries = Vec::new();
        while !self.check(&Token::RBrace) {
            let entry_start = self.current_span().start;
            let access = match self.peek_token() {
                Some(Token::Read) => {
                    self.advance();
                    Access::Read
                }
                Some(Token::Write) => {
                    self.advance();
                    Access::Write
                }
                _ => return Err(self.error("expected `read` or `write` in query".into())),
            };
            let component = self.expect_ident()?;
            let entry_end = self.previous_span().end;
            entries.push(QueryEntry {
                access,
                component,
                span: entry_start..entry_end,
            });
            self.skip_newlines();
        }
        self.expect(Token::RBrace)?;
        let end = self.previous_span().end;
        Ok(QueryDef {
            entries,
            span: start..end,
        })
    }

    fn parse_each(&mut self) -> Result<EachBlock, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::Each)?;
        self.expect(Token::LParen)?;

        let mut params = Vec::new();
        while !self.check(&Token::RParen) {
            let param_start = self.current_span().start;
            let name = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let component = self.expect_ident()?;
            let param_end = self.previous_span().end;
            params.push(EachParam {
                name,
                component,
                span: param_start..param_end,
            });
            if self.check(&Token::Comma) {
                self.advance();
            }
        }
        self.expect(Token::RParen)?;
        self.expect(Token::LBrace)?;
        self.skip_newlines();

        let body = self.parse_block_body()?;
        self.expect(Token::RBrace)?;
        let end = self.previous_span().end;
        Ok(EachBlock {
            params,
            body,
            span: start..end,
        })
    }

    fn parse_block_body(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.check(&Token::RBrace) {
            let stmt = self.parse_stmt()?;
            stmts.push(stmt);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek_token() {
            Some(Token::Let) => self.parse_let(),
            Some(Token::If) => self.parse_if(),
            Some(Token::While) => self.parse_while(),
            Some(Token::Return) => self.parse_return(),
            _ => {
                let expr = self.parse_expr()?;
                // Check for assignment
                if self.check(&Token::Eq) {
                    let assign_start = expr.span().start;
                    self.advance();
                    let value = self.parse_expr()?;
                    let end = value.span().end;
                    Ok(Stmt::Assign(AssignStmt {
                        target: expr,
                        value,
                        span: assign_start..end,
                    }))
                } else {
                    Ok(Stmt::Expr(expr))
                }
            }
        }
    }

    fn parse_let(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::Let)?;
        let name = self.expect_ident()?;

        let ty = if self.check(&Token::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(Token::Eq)?;
        let value = self.parse_expr()?;
        let end = value.span().end;
        Ok(Stmt::Let(LetStmt {
            name,
            ty,
            value,
            span: start..end,
        }))
    }

    fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::If)?;
        let condition = self.parse_expr()?;
        self.expect(Token::LBrace)?;
        self.skip_newlines();
        let then_body = self.parse_block_body()?;
        self.expect(Token::RBrace)?;

        let else_body = if self.check(&Token::Else) {
            self.advance();
            self.expect(Token::LBrace)?;
            self.skip_newlines();
            let body = self.parse_block_body()?;
            self.expect(Token::RBrace)?;
            Some(body)
        } else {
            None
        };

        let end = self.previous_span().end;
        Ok(Stmt::If(IfStmt {
            condition,
            then_body,
            else_body,
            span: start..end,
        }))
    }

    fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::While)?;
        let condition = self.parse_expr()?;
        self.expect(Token::LBrace)?;
        self.skip_newlines();
        let body = self.parse_block_body()?;
        self.expect(Token::RBrace)?;
        let end = self.previous_span().end;
        Ok(Stmt::While(WhileStmt {
            condition,
            body,
            span: start..end,
        }))
    }

    fn parse_return(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::Return)?;
        let end;
        let value = if self.check(&Token::Newline) || self.check(&Token::RBrace) || self.is_at_end()
        {
            end = self.previous_span().end;
            None
        } else {
            let expr = self.parse_expr()?;
            end = expr.span().end;
            Some(expr)
        };
        Ok(Stmt::Return(value, start..end))
    }

    // Expression parsing with precedence climbing
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while self.check(&Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            let span = left.span().start..right.span().end;
            left = Expr::BinOp(Box::new(left), BinOp::Or, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison()?;
        while self.check(&Token::And) {
            self.advance();
            let right = self.parse_comparison()?;
            let span = left.span().start..right.span().end;
            left = Expr::BinOp(Box::new(left), BinOp::And, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_additive()?;
        if let Some(op) = self.peek_comparison_op() {
            self.advance();
            let right = self.parse_additive()?;
            let span = left.span().start..right.span().end;
            left = Expr::BinOp(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn peek_comparison_op(&self) -> Option<BinOp> {
        match self.peek_token()? {
            Token::EqEq => Some(BinOp::Eq),
            Token::NotEq => Some(BinOp::NotEq),
            Token::Lt => Some(BinOp::Lt),
            Token::Gt => Some(BinOp::Gt),
            Token::LtEq => Some(BinOp::LtEq),
            Token::GtEq => Some(BinOp::GtEq),
            _ => None,
        }
    }

    fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative()?;
        while let Some(op) = self.peek_additive_op() {
            self.advance();
            let right = self.parse_multiplicative()?;
            let span = left.span().start..right.span().end;
            left = Expr::BinOp(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn peek_additive_op(&self) -> Option<BinOp> {
        match self.peek_token()? {
            Token::Plus => Some(BinOp::Add),
            Token::Minus => Some(BinOp::Sub),
            _ => None,
        }
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        while let Some(op) = self.peek_multiplicative_op() {
            self.advance();
            let right = self.parse_unary()?;
            let span = left.span().start..right.span().end;
            left = Expr::BinOp(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn peek_multiplicative_op(&self) -> Option<BinOp> {
        match self.peek_token()? {
            Token::Star => Some(BinOp::Mul),
            Token::Slash => Some(BinOp::Div),
            _ => None,
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek_token() {
            Some(Token::Minus) => {
                let start = self.current_span().start;
                self.advance();
                let expr = self.parse_unary()?;
                let end = expr.span().end;
                Ok(Expr::UnaryOp(UnaryOp::Neg, Box::new(expr), start..end))
            }
            Some(Token::Not) => {
                let start = self.current_span().start;
                self.advance();
                let expr = self.parse_unary()?;
                let end = expr.span().end;
                Ok(Expr::UnaryOp(UnaryOp::Not, Box::new(expr), start..end))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        while self.check(&Token::Dot) {
            self.advance();
            let field = self.expect_ident()?;
            let end = self.previous_span().end;
            let span = expr.span().start..end;
            expr = Expr::FieldAccess(Box::new(expr), field, span);
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek_token() {
            Some(Token::IntLit(_)) => {
                if let Token::IntLit(v) = self.advance().token {
                    Ok(Expr::IntLit(v, self.previous_span()))
                } else {
                    unreachable!()
                }
            }
            Some(Token::FloatLit(_)) => {
                if let Token::FloatLit(v) = self.advance().token {
                    Ok(Expr::FloatLit(v, self.previous_span()))
                } else {
                    unreachable!()
                }
            }
            Some(Token::True) => {
                self.advance();
                Ok(Expr::BoolLit(true, self.previous_span()))
            }
            Some(Token::False) => {
                self.advance();
                Ok(Expr::BoolLit(false, self.previous_span()))
            }
            Some(Token::Ident(_)) => {
                if let Token::Ident(name) = self.advance().token {
                    let start = self.previous_span().start;
                    // Check for function call: ident(
                    if self.check(&Token::LParen) {
                        self.advance(); // consume (
                        let mut args = Vec::new();
                        while !self.check(&Token::RParen) {
                            args.push(self.parse_expr()?);
                            if self.check(&Token::Comma) {
                                self.advance();
                            }
                        }
                        self.expect(Token::RParen)?;
                        let end = self.previous_span().end;
                        Ok(Expr::Call(name, args, start..end))
                    } else {
                        Ok(Expr::Ident(name, self.previous_span()))
                    }
                } else {
                    unreachable!()
                }
            }
            Some(Token::LParen) => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            Some(other) => Err(self.error(format!("expected expression, found `{other}`"))),
            None => Err(self.error_eof("expected expression")),
        }
    }

    fn parse_type(&mut self) -> Result<TypeName, ParseError> {
        match self.peek_token() {
            Some(Token::I32) => {
                self.advance();
                Ok(TypeName::I32)
            }
            Some(Token::I64) => {
                self.advance();
                Ok(TypeName::I64)
            }
            Some(Token::F32) => {
                self.advance();
                Ok(TypeName::F32)
            }
            Some(Token::F64) => {
                self.advance();
                Ok(TypeName::F64)
            }
            Some(Token::Bool) => {
                self.advance();
                Ok(TypeName::Bool)
            }
            Some(Token::EntityType) => {
                self.advance();
                Ok(TypeName::Entity)
            }
            _ => Err(self.error("expected type".into())),
        }
    }

    fn parse_extern_block(&mut self) -> Result<ExternBlock, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::Extern)?;

        // Optional module name: extern "physics" { ... }
        let module_name = if let Some(Token::Ident(name)) = self.peek_token() {
            // We don't have string literals, so use bare ident as module name
            self.advance();
            name
        } else {
            "host".to_string()
        };

        self.expect(Token::LBrace)?;
        self.skip_newlines();

        let mut functions = Vec::new();
        while !self.check(&Token::RBrace) {
            let fn_start = self.current_span().start;
            self.expect(Token::Fn)?;
            let name = self.expect_ident()?;
            self.expect(Token::LParen)?;

            let mut params = Vec::new();
            while !self.check(&Token::RParen) {
                let param_start = self.current_span().start;
                let param_name = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;
                let param_end = self.previous_span().end;
                params.push(FnParam {
                    name: param_name,
                    ty,
                    span: param_start..param_end,
                });
                if self.check(&Token::Comma) {
                    self.advance();
                }
            }
            self.expect(Token::RParen)?;

            let return_ty = if self.check(&Token::Arrow) {
                self.advance();
                Some(self.parse_type()?)
            } else {
                None
            };

            let fn_end = self.previous_span().end;
            functions.push(ExternFn {
                name,
                params,
                return_ty,
                span: fn_start..fn_end,
            });
            self.skip_newlines();
        }
        self.expect(Token::RBrace)?;
        let end = self.previous_span().end;

        Ok(ExternBlock {
            module_name,
            functions,
            span: start..end,
        })
    }

    fn parse_entity_def(&mut self) -> Result<EntityDef, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::EntityKw)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        self.skip_newlines();

        let mut components = Vec::new();
        while !self.check(&Token::RBrace) {
            let comp_start = self.current_span().start;
            let comp_name = self.expect_ident()?;
            self.expect(Token::LBrace)?;
            self.skip_newlines();

            let mut fields = Vec::new();
            while !self.check(&Token::RBrace) {
                let field_name = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let value = self.parse_expr()?;
                fields.push((field_name, value));
                self.skip_newlines();
                if self.check(&Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                }
            }
            self.expect(Token::RBrace)?;
            let comp_end = self.previous_span().end;
            components.push(EntityComponent {
                component: comp_name,
                fields,
                span: comp_start..comp_end,
            });
            self.skip_newlines();
        }
        self.expect(Token::RBrace)?;
        let end = self.previous_span().end;

        Ok(EntityDef {
            name,
            components,
            span: start..end,
        })
    }

    fn parse_fn_def(&mut self) -> Result<FnDef, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::Fn)?;
        let name = self.expect_ident()?;
        self.expect(Token::LParen)?;

        let mut params = Vec::new();
        while !self.check(&Token::RParen) {
            let param_start = self.current_span().start;
            let param_name = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;
            let param_end = self.previous_span().end;
            params.push(FnParam {
                name: param_name,
                ty,
                span: param_start..param_end,
            });
            if self.check(&Token::Comma) {
                self.advance();
            }
        }
        self.expect(Token::RParen)?;

        let return_ty = if self.check(&Token::Arrow) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(Token::LBrace)?;
        self.skip_newlines();
        let body = self.parse_block_body()?;
        self.expect(Token::RBrace)?;
        let end = self.previous_span().end;

        Ok(FnDef {
            name,
            params,
            return_ty,
            body,
            span: start..end,
        })
    }

    fn parse_world(&mut self) -> Result<WorldDef, ParseError> {
        let start = self.current_span().start;
        self.expect(Token::World)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        self.skip_newlines();

        let mut systems = Vec::new();
        if self.check(&Token::Systems) {
            self.advance();
            self.expect(Token::LBrace)?;
            self.skip_newlines();
            while !self.check(&Token::RBrace) {
                systems.push(self.expect_ident()?);
                self.skip_newlines();
                if self.check(&Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                }
            }
            self.expect(Token::RBrace)?;
            self.skip_newlines();
        }

        self.expect(Token::RBrace)?;
        let end = self.previous_span().end;
        Ok(WorldDef {
            name,
            systems,
            span: start..end,
        })
    }

    // Helper methods
    fn peek_token(&self) -> Option<Token> {
        self.tokens.get(self.pos).map(|s| s.token.clone())
    }

    fn check(&self, token: &Token) -> bool {
        self.peek_token()
            .map(|t| std::mem::discriminant(&t) == std::mem::discriminant(token))
            .unwrap_or(false)
    }

    fn advance(&mut self) -> Spanned {
        let tok = self.tokens[self.pos].clone();
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: Token) -> Result<Spanned, ParseError> {
        if self.check(&expected) {
            Ok(self.advance())
        } else {
            match self.peek_token() {
                Some(found) => Err(self.error(format!("expected `{expected}`, found `{found}`"))),
                None => Err(self.error_eof(&format!("expected `{expected}`"))),
            }
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek_token() {
            Some(Token::Ident(_)) => {
                if let Token::Ident(name) = self.advance().token {
                    Ok(name)
                } else {
                    unreachable!()
                }
            }
            Some(other) => Err(self.error(format!("expected identifier, found `{other}`"))),
            None => Err(self.error_eof("expected identifier")),
        }
    }

    fn skip_newlines(&mut self) {
        while self.check(&Token::Newline) {
            self.advance();
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn current_span(&self) -> std::ops::Range<usize> {
        if self.pos < self.tokens.len() {
            self.tokens[self.pos].span.clone()
        } else if let Some(last) = self.tokens.last() {
            last.span.end..last.span.end
        } else {
            0..0
        }
    }

    fn previous_span(&self) -> std::ops::Range<usize> {
        if self.pos > 0 {
            self.tokens[self.pos - 1].span.clone()
        } else {
            0..0
        }
    }

    fn error(&self, message: String) -> ParseError {
        let span = self.current_span();
        ParseError {
            message,
            span: span.clone(),
            label: "here".into(),
            source_code: self.source.clone(),
        }
    }

    fn error_eof(&self, context: &str) -> ParseError {
        let span = if let Some(last) = self.tokens.last() {
            last.span.end..last.span.end
        } else {
            0..0
        };
        ParseError {
            message: format!("unexpected end of file, {context}"),
            span,
            label: "here".into(),
            source_code: self.source.clone(),
        }
    }
}
