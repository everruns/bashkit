pub mod ir;

use ir::*;
use oxc_allocator::Allocator;
use oxc_ast::ast;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::zapcode_browser::error::{Result, ZapcodeError};

pub fn parse(source: &str) -> Result<Program> {
    // Auto-wrap trailing object literals: `{ key: value }` → `({ key: value })`
    // This avoids the JS ambiguity where `{` at statement start is a block.
    let source = wrap_trailing_object(source);

    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, &source, source_type).parse();

    if !ret.errors.is_empty() {
        let msgs: Vec<String> = ret.errors.iter().map(|e| e.to_string()).collect();
        return Err(ZapcodeError::ParseError(msgs.join("\n")));
    }

    let mut lowerer = AstLowerer::new(&source);
    lowerer.lower_program(&ret.program)?;

    Ok(Program {
        body: lowerer.body,
        functions: lowerer.functions,
    })
}

/// If the source ends with a `{ ... }` block that looks like an object literal
/// (contains `key: value` or `key,` patterns), wrap it in `(...)` so oxc
/// parses it as an expression instead of a block statement.
fn wrap_trailing_object(source: &str) -> String {
    let trimmed = source.trim_end();

    // Must end with `}`
    if !trimmed.ends_with('}') {
        return source.to_string();
    }

    // Find the matching `{`
    //
    // Limitation: this brace scanner is a simple depth-counting heuristic that
    // does not account for braces inside string literals, comments, or template
    // literals. This is acceptable for preprocessing because:
    //   1. The input is AI-generated tool output (simple expressions), not
    //      arbitrary user code likely to contain embedded brace characters.
    //   2. This is only a heuristic for trailing-object extraction — if the
    //      heuristic produces malformed output, oxc will catch the parse error
    //      downstream, so correctness is never silently lost.
    //
    // TODO: revisit this heuristic if real-world failures are reported (e.g.
    // strings containing unbalanced braces causing incorrect extraction).
    let mut depth = 0;
    let mut open_pos = None;
    for (i, ch) in trimmed.char_indices().rev() {
        match ch {
            '}' => depth += 1,
            '{' => {
                depth -= 1;
                if depth == 0 {
                    open_pos = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let open_pos = match open_pos {
        Some(pos) => pos,
        None => return source.to_string(),
    };

    // The `{` must be at the start of a statement (preceded by newline, semicolon, or start)
    let before = trimmed[..open_pos].trim_end();
    if !before.is_empty() {
        let last_char = before.chars().last().unwrap();
        // If preceded by =, (, return, =>, etc. — it's already in expression context
        if matches!(last_char, '=' | '(' | ',' | ':' | '>' | '[') {
            return source.to_string();
        }
        // If preceded by a keyword that takes a block, don't wrap
        let last_word = before
            .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or("");
        if matches!(
            last_word,
            "if" | "else"
                | "for"
                | "while"
                | "do"
                | "try"
                | "catch"
                | "finally"
                | "class"
                | "function"
                | "switch"
        ) {
            return source.to_string();
        }
    }

    // Check the content between braces looks like object literal syntax
    let inner = &trimmed[open_pos + 1..trimmed.len() - 1].trim();
    if inner.is_empty() {
        return source.to_string();
    }

    // Heuristic: contains `identifier:` pattern (key-value) or commas between identifiers
    let looks_like_object = inner.contains(':') || {
        // Check for shorthand properties: `{ a, b }` pattern
        inner.split(',').all(|part| {
            let p = part.trim();
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == ' ')
        })
    };

    if !looks_like_object {
        return source.to_string();
    }

    // Wrap in parentheses with a semicolon to prevent it being parsed
    // as a function call on the preceding expression (e.g. `1({a})`)
    let close_pos = source.rfind('}').unwrap();
    let mut result = String::with_capacity(source.len() + 3);
    result.push_str(&source[..open_pos]);
    result.push_str(";(");
    result.push_str(&source[open_pos..=close_pos]);
    result.push(')');
    if close_pos + 1 < source.len() {
        result.push_str(&source[close_pos + 1..]);
    }
    result
}

struct AstLowerer<'a> {
    #[allow(dead_code)]
    source: &'a str,
    body: Vec<Statement>,
    functions: Vec<FunctionDef>,
}

impl<'a> AstLowerer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            body: Vec::new(),
            functions: Vec::new(),
        }
    }

    fn span(&self, s: oxc_span::Span) -> Span {
        s.into()
    }

    fn unsupported(&self, span: oxc_span::Span, desc: &str) -> ZapcodeError {
        ZapcodeError::UnsupportedSyntax {
            span: format!("{}..{}", span.start, span.end),
            description: desc.to_string(),
        }
    }

    fn lower_program(&mut self, program: &ast::Program<'_>) -> Result<()> {
        // Handle directives (e.g., "use strict", but also bare string literals)
        for directive in &program.directives {
            let span = self.span(directive.span);
            let expr = Expr::StringLit(directive.directive.to_string());
            self.body.push(Statement::Expression { expr, span });
        }
        for stmt in &program.body {
            let s = self.lower_statement(stmt)?;
            self.body.push(s);
        }
        Ok(())
    }

    fn lower_statement(&mut self, stmt: &ast::Statement<'_>) -> Result<Statement> {
        match stmt {
            ast::Statement::VariableDeclaration(decl) => self.lower_var_decl(decl),
            ast::Statement::ExpressionStatement(expr_stmt) => {
                let span = self.span(expr_stmt.span);
                let expr = self.lower_expr(&expr_stmt.expression)?;
                Ok(Statement::Expression { expr, span })
            }
            ast::Statement::ReturnStatement(ret) => {
                let span = self.span(ret.span);
                let value = match &ret.argument {
                    Some(arg) => Some(self.lower_expr(arg)?),
                    None => None,
                };
                Ok(Statement::Return { value, span })
            }
            ast::Statement::IfStatement(if_stmt) => self.lower_if(if_stmt),
            ast::Statement::WhileStatement(while_stmt) => {
                let span = self.span(while_stmt.span);
                let test = self.lower_expr(&while_stmt.test)?;
                let body = self.lower_statement_as_block(&while_stmt.body)?;
                Ok(Statement::While { test, body, span })
            }
            ast::Statement::DoWhileStatement(do_while) => {
                let span = self.span(do_while.span);
                let body = self.lower_statement_as_block(&do_while.body)?;
                let test = self.lower_expr(&do_while.test)?;
                Ok(Statement::DoWhile { body, test, span })
            }
            ast::Statement::ForStatement(for_stmt) => self.lower_for(for_stmt),
            ast::Statement::ForInStatement(s) => {
                Err(self.unsupported(s.span, "for...in loops are not supported, use for...of"))
            }
            ast::Statement::ForOfStatement(for_of) => self.lower_for_of(for_of),
            ast::Statement::BlockStatement(block) => {
                let span = self.span(block.span);
                let body = self.lower_statements(&block.body)?;
                Ok(Statement::Block { body, span })
            }
            ast::Statement::ThrowStatement(throw) => {
                let span = self.span(throw.span);
                let value = self.lower_expr(&throw.argument)?;
                Ok(Statement::Throw { value, span })
            }
            ast::Statement::TryStatement(try_stmt) => self.lower_try(try_stmt),
            ast::Statement::BreakStatement(s) => Ok(Statement::Break {
                span: self.span(s.span),
            }),
            ast::Statement::ContinueStatement(s) => Ok(Statement::Continue {
                span: self.span(s.span),
            }),
            ast::Statement::FunctionDeclaration(func) => self.lower_func_decl(func),
            ast::Statement::ClassDeclaration(class) => self.lower_class_decl(class),
            ast::Statement::SwitchStatement(switch) => self.lower_switch(switch),
            ast::Statement::EmptyStatement(_) => Ok(Statement::Expression {
                expr: Expr::UndefinedLit,
                span: Span { start: 0, end: 0 },
            }),
            ast::Statement::LabeledStatement(labeled) => self.lower_statement(&labeled.body),
            ast::Statement::TSTypeAliasDeclaration(s) => Ok(Statement::Expression {
                expr: Expr::UndefinedLit,
                span: self.span(s.span),
            }),
            ast::Statement::TSInterfaceDeclaration(s) => Ok(Statement::Expression {
                expr: Expr::UndefinedLit,
                span: self.span(s.span),
            }),
            ast::Statement::TSEnumDeclaration(s) => {
                Err(self.unsupported(s.span, "TypeScript enums are not supported"))
            }
            ast::Statement::ImportDeclaration(s) => Err(ZapcodeError::SandboxViolation(format!(
                "import declarations are forbidden in the sandbox (at {}..{})",
                s.span.start, s.span.end
            ))),
            ast::Statement::ExportDefaultDeclaration(s) => {
                Err(ZapcodeError::SandboxViolation(format!(
                    "export declarations are forbidden in the sandbox (at {}..{})",
                    s.span.start, s.span.end
                )))
            }
            ast::Statement::ExportNamedDeclaration(s) => {
                Err(ZapcodeError::SandboxViolation(format!(
                    "export declarations are forbidden in the sandbox (at {}..{})",
                    s.span.start, s.span.end
                )))
            }
            ast::Statement::ExportAllDeclaration(s) => {
                Err(ZapcodeError::SandboxViolation(format!(
                    "export declarations are forbidden in the sandbox (at {}..{})",
                    s.span.start, s.span.end
                )))
            }
            ast::Statement::DebuggerStatement(s) => {
                Err(self.unsupported(s.span, "unsupported statement type"))
            }
            ast::Statement::WithStatement(s) => {
                Err(self.unsupported(s.span, "unsupported statement type"))
            }
            ast::Statement::TSModuleDeclaration(s) => {
                Err(self.unsupported(s.span, "unsupported statement type"))
            }
            ast::Statement::TSGlobalDeclaration(s) => {
                Err(self.unsupported(s.span, "unsupported statement type"))
            }
            ast::Statement::TSImportEqualsDeclaration(s) => {
                Err(self.unsupported(s.span, "unsupported statement type"))
            }
            ast::Statement::TSExportAssignment(s) => {
                Err(self.unsupported(s.span, "unsupported statement type"))
            }
            ast::Statement::TSNamespaceExportDeclaration(s) => {
                Err(self.unsupported(s.span, "unsupported statement type"))
            }
        }
    }

    fn lower_statements(&mut self, stmts: &[ast::Statement<'_>]) -> Result<Vec<Statement>> {
        stmts.iter().map(|s| self.lower_statement(s)).collect()
    }

    fn lower_statement_as_block(&mut self, stmt: &ast::Statement<'_>) -> Result<Vec<Statement>> {
        match stmt {
            ast::Statement::BlockStatement(block) => self.lower_statements(&block.body),
            other => Ok(vec![self.lower_statement(other)?]),
        }
    }

    fn lower_var_decl(&mut self, decl: &ast::VariableDeclaration<'_>) -> Result<Statement> {
        let span = self.span(decl.span);
        let kind = match decl.kind {
            ast::VariableDeclarationKind::Const => VarKind::Const,
            ast::VariableDeclarationKind::Let => VarKind::Let,
            ast::VariableDeclarationKind::Var => VarKind::Var,
            ast::VariableDeclarationKind::Using | ast::VariableDeclarationKind::AwaitUsing => {
                return Err(self.unsupported(decl.span, "using declarations are not supported"));
            }
        };
        let mut declarations = Vec::new();
        for declarator in &decl.declarations {
            let pattern = self.lower_binding_pattern(&declarator.id)?;
            let init = match &declarator.init {
                Some(expr) => Some(self.lower_expr(expr)?),
                None => None,
            };
            declarations.push(VarDeclarator { pattern, init });
        }
        Ok(Statement::VariableDecl {
            kind,
            declarations,
            span,
        })
    }

    fn lower_binding_pattern(&mut self, pat: &ast::BindingPattern<'_>) -> Result<AssignTarget> {
        match pat {
            ast::BindingPattern::BindingIdentifier(id) => {
                Ok(AssignTarget::Ident(id.name.to_string()))
            }
            ast::BindingPattern::ObjectPattern(obj) => {
                let mut fields = Vec::new();
                for prop in &obj.properties {
                    let key = property_key_to_string(&prop.key);
                    let alias = match &prop.value {
                        ast::BindingPattern::BindingIdentifier(id) => {
                            let name = id.name.to_string();
                            if name != key {
                                Some(name)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    let default = match &prop.value {
                        ast::BindingPattern::AssignmentPattern(assign) => {
                            Some(self.lower_expr(&assign.right)?)
                        }
                        _ => None,
                    };
                    fields.push(DestructureField {
                        key,
                        alias,
                        default,
                    });
                }
                Ok(AssignTarget::ObjectDestructure(fields))
            }
            ast::BindingPattern::ArrayPattern(arr) => {
                let mut elements = Vec::new();
                for elem in &arr.elements {
                    match elem {
                        Some(pat) => elements.push(Some(self.lower_binding_pattern(pat)?)),
                        None => elements.push(None),
                    }
                }
                Ok(AssignTarget::ArrayDestructure(elements))
            }
            ast::BindingPattern::AssignmentPattern(assign) => {
                self.lower_binding_pattern(&assign.left)
            }
        }
    }

    fn lower_binding_pattern_to_param(
        &mut self,
        pat: &ast::BindingPattern<'_>,
    ) -> Result<ParamPattern> {
        match pat {
            ast::BindingPattern::BindingIdentifier(id) => {
                Ok(ParamPattern::Ident(id.name.to_string()))
            }
            ast::BindingPattern::ObjectPattern(obj) => {
                let mut fields = Vec::new();
                for prop in &obj.properties {
                    let key = property_key_to_string(&prop.key);
                    let alias = match &prop.value {
                        ast::BindingPattern::BindingIdentifier(id) => {
                            let name = id.name.to_string();
                            if name != key {
                                Some(name)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    fields.push(DestructureField {
                        key,
                        alias,
                        default: None,
                    });
                }
                Ok(ParamPattern::ObjectDestructure(fields))
            }
            ast::BindingPattern::ArrayPattern(arr) => {
                let mut elems = Vec::new();
                for elem in &arr.elements {
                    match elem {
                        Some(p) => elems.push(Some(self.lower_binding_pattern_to_param(p)?)),
                        None => elems.push(None),
                    }
                }
                Ok(ParamPattern::ArrayDestructure(elems))
            }
            ast::BindingPattern::AssignmentPattern(assign) => {
                let inner = self.lower_binding_pattern_to_param(&assign.left)?;
                let default = self.lower_expr(&assign.right)?;
                Ok(ParamPattern::DefaultValue {
                    pattern: Box::new(inner),
                    default,
                })
            }
        }
    }

    fn lower_if(&mut self, if_stmt: &ast::IfStatement<'_>) -> Result<Statement> {
        let span = self.span(if_stmt.span);
        let test = self.lower_expr(&if_stmt.test)?;
        let consequent = self.lower_statement_as_block(&if_stmt.consequent)?;
        let alternate = match &if_stmt.alternate {
            Some(alt) => Some(self.lower_statement_as_block(alt)?),
            None => None,
        };
        Ok(Statement::If {
            test,
            consequent,
            alternate,
            span,
        })
    }

    fn lower_for(&mut self, for_stmt: &ast::ForStatement<'_>) -> Result<Statement> {
        let span = self.span(for_stmt.span);
        let init = match &for_stmt.init {
            Some(init) => match init {
                ast::ForStatementInit::VariableDeclaration(decl) => {
                    Some(Box::new(self.lower_var_decl(decl)?))
                }
                other => {
                    if let Some(expr_ref) = other.as_expression() {
                        let expr = self.lower_expr(expr_ref)?;
                        Some(Box::new(Statement::Expression { expr, span }))
                    } else {
                        return Err(ZapcodeError::CompileError(
                            "unsupported for-loop initializer".to_string(),
                        ));
                    }
                }
            },
            None => None,
        };
        let test = match &for_stmt.test {
            Some(t) => Some(self.lower_expr(t)?),
            None => None,
        };
        let update = match &for_stmt.update {
            Some(u) => Some(self.lower_expr(u)?),
            None => None,
        };
        let body = self.lower_statement_as_block(&for_stmt.body)?;
        Ok(Statement::For {
            init,
            test,
            update,
            body,
            span,
        })
    }

    fn lower_for_of(&mut self, for_of: &ast::ForOfStatement<'_>) -> Result<Statement> {
        let span = self.span(for_of.span);
        let binding = match &for_of.left {
            ast::ForStatementLeft::VariableDeclaration(decl) => {
                if let Some(declarator) = decl.declarations.first() {
                    match &declarator.id {
                        ast::BindingPattern::BindingIdentifier(id) => {
                            ForBinding::Ident(id.name.to_string())
                        }
                        _ => {
                            let pat = self.lower_binding_pattern_to_param(&declarator.id)?;
                            ForBinding::Destructure(pat)
                        }
                    }
                } else {
                    return Err(self.unsupported(for_of.span, "empty for-of binding"));
                }
            }
            _ => return Err(self.unsupported(for_of.span, "unsupported for-of left-hand side")),
        };
        let iterable = self.lower_expr(&for_of.right)?;
        let body = self.lower_statement_as_block(&for_of.body)?;
        Ok(Statement::ForOf {
            binding,
            iterable,
            body,
            span,
        })
    }

    fn lower_try(&mut self, try_stmt: &ast::TryStatement<'_>) -> Result<Statement> {
        let span = self.span(try_stmt.span);
        let try_body = self.lower_statements(&try_stmt.block.body)?;
        let (catch_param, catch_body) = match &try_stmt.handler {
            Some(handler) => {
                let param = handler.param.as_ref().and_then(|p| match &p.pattern {
                    ast::BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
                    _ => None,
                });
                let body = self.lower_statements(&handler.body.body)?;
                (param, body)
            }
            None => (None, Vec::new()),
        };
        let finally_body = match &try_stmt.finalizer {
            Some(block) => Some(self.lower_statements(&block.body)?),
            None => None,
        };
        Ok(Statement::TryCatch {
            try_body,
            catch_param,
            catch_body,
            finally_body,
            span,
        })
    }

    fn lower_func_decl(&mut self, func: &ast::Function<'_>) -> Result<Statement> {
        let span = self.span(func.span);
        let func_def = self.lower_function(func)?;
        let func_index = self.functions.len();
        self.functions.push(func_def);
        Ok(Statement::FunctionDecl { func_index, span })
    }

    fn lower_function(&mut self, func: &ast::Function<'_>) -> Result<FunctionDef> {
        let name = func.id.as_ref().map(|id| id.name.to_string());
        let params = self.lower_formal_params(&func.params)?;
        let body = match &func.body {
            Some(body) => self.lower_statements(&body.statements)?,
            None => Vec::new(),
        };
        Ok(FunctionDef {
            name,
            params,
            body,
            is_async: func.r#async,
            is_generator: func.generator,
            is_arrow: false,
            span: self.span(func.span),
        })
    }

    fn lower_formal_params(
        &mut self,
        params: &ast::FormalParameters<'_>,
    ) -> Result<Vec<ParamPattern>> {
        let mut result = Vec::new();
        for param in &params.items {
            let pat = self.lower_binding_pattern_to_param(&param.pattern)?;
            result.push(pat);
        }
        if let Some(rest) = &params.rest {
            match &rest.rest.argument {
                ast::BindingPattern::BindingIdentifier(id) => {
                    result.push(ParamPattern::Rest(id.name.to_string()));
                }
                _ => {
                    return Err(self.unsupported(
                        rest.span,
                        "complex rest parameter patterns are not supported",
                    ));
                }
            }
        }
        Ok(result)
    }

    fn lower_class_decl(&mut self, class: &ast::Class<'_>) -> Result<Statement> {
        let span = self.span(class.span);
        let name = class
            .id
            .as_ref()
            .map(|id| id.name.to_string())
            .unwrap_or_else(|| "AnonymousClass".to_string());

        let super_class = match &class.super_class {
            Some(expr) => {
                if let ast::Expression::Identifier(id) = expr {
                    Some(id.name.to_string())
                } else {
                    return Err(self.unsupported(
                        class.span,
                        "computed super class expressions are not supported",
                    ));
                }
            }
            None => None,
        };

        let (constructor, methods, static_methods) = self.lower_class_body(&class.body)?;

        Ok(Statement::ClassDecl {
            name,
            super_class,
            constructor,
            methods,
            static_methods,
            span,
        })
    }

    fn lower_class_expr(&mut self, class: &ast::Class<'_>) -> Result<Expr> {
        let name = class.id.as_ref().map(|id| id.name.to_string());

        let super_class = match &class.super_class {
            Some(expr) => {
                if let ast::Expression::Identifier(id) = expr {
                    Some(id.name.to_string())
                } else {
                    return Err(self.unsupported(
                        class.span,
                        "computed super class expressions are not supported",
                    ));
                }
            }
            None => None,
        };

        let (constructor, methods, static_methods) = self.lower_class_body(&class.body)?;

        Ok(Expr::ClassExpr {
            name,
            super_class,
            constructor,
            methods,
            static_methods,
        })
    }

    fn lower_class_body(&mut self, body: &ast::ClassBody<'_>) -> Result<ClassBodyParts> {
        let mut constructor = None;
        let mut methods = Vec::new();
        let mut static_methods = Vec::new();

        for element in &body.body {
            match element {
                ast::ClassElement::MethodDefinition(method) => {
                    let method_name = match &method.key {
                        ast::PropertyKey::StaticIdentifier(id) => id.name.to_string(),
                        ast::PropertyKey::StringLiteral(s) => s.value.to_string(),
                        _ => continue, // skip computed method names
                    };

                    let func = &method.value;
                    let params = self.lower_formal_params(&func.params)?;
                    let body_stmts = match &func.body {
                        Some(body) => self.lower_statements(&body.statements)?,
                        None => Vec::new(),
                    };

                    let func_def = FunctionDef {
                        name: Some(method_name.clone()),
                        params,
                        body: body_stmts,
                        is_async: func.r#async,
                        is_generator: false,
                        is_arrow: false,
                        span: self.span(func.span),
                    };

                    match method.kind {
                        ast::MethodDefinitionKind::Constructor => {
                            constructor = Some(Box::new(func_def));
                        }
                        ast::MethodDefinitionKind::Method => {
                            if method.r#static {
                                static_methods.push(ClassMethod {
                                    name: method_name,
                                    func: func_def,
                                });
                            } else {
                                methods.push(ClassMethod {
                                    name: method_name,
                                    func: func_def,
                                });
                            }
                        }
                        ast::MethodDefinitionKind::Get | ast::MethodDefinitionKind::Set => {
                            // Getters/setters: treat as regular methods for now
                            if method.r#static {
                                static_methods.push(ClassMethod {
                                    name: method_name,
                                    func: func_def,
                                });
                            } else {
                                methods.push(ClassMethod {
                                    name: method_name,
                                    func: func_def,
                                });
                            }
                        }
                    }
                }
                ast::ClassElement::PropertyDefinition(_) => {
                    // Class property declarations (e.g., `name: string;`) are type-level
                    // and are handled at runtime through constructor assignments.
                    // Skip them in the IR.
                }
                ast::ClassElement::AccessorProperty(s) => {
                    return Err(self
                        .unsupported(s.span, "accessor properties in classes are not supported"));
                }
                ast::ClassElement::TSIndexSignature(_) => {
                    // TypeScript-only, skip
                }
                ast::ClassElement::StaticBlock(s) => {
                    return Err(self.unsupported(s.span, "static blocks are not supported"));
                }
            }
        }

        Ok((constructor, methods, static_methods))
    }

    fn lower_switch(&mut self, switch: &ast::SwitchStatement<'_>) -> Result<Statement> {
        let span = self.span(switch.span);
        let discriminant = self.lower_expr(&switch.discriminant)?;
        let mut cases = Vec::new();
        for case in &switch.cases {
            let test = match &case.test {
                Some(t) => Some(self.lower_expr(t)?),
                None => None,
            };
            let consequent = self.lower_statements(&case.consequent)?;
            cases.push(SwitchCase { test, consequent });
        }
        Ok(Statement::Switch {
            discriminant,
            cases,
            span,
        })
    }

    fn lower_expr(&mut self, expr: &ast::Expression<'_>) -> Result<Expr> {
        match expr {
            ast::Expression::NumericLiteral(lit) => Ok(Expr::NumberLit(lit.value)),
            ast::Expression::StringLiteral(lit) => Ok(Expr::StringLit(lit.value.to_string())),
            ast::Expression::BooleanLiteral(lit) => Ok(Expr::BoolLit(lit.value)),
            ast::Expression::NullLiteral(_) => Ok(Expr::NullLit),
            ast::Expression::TemplateLiteral(tpl) => {
                let quasis: Vec<String> =
                    tpl.quasis.iter().map(|q| q.value.raw.to_string()).collect();
                let exprs: Result<Vec<Expr>> =
                    tpl.expressions.iter().map(|e| self.lower_expr(e)).collect();
                Ok(Expr::TemplateLit {
                    quasis,
                    exprs: exprs?,
                })
            }
            ast::Expression::RegExpLiteral(re) => Ok(Expr::RegExpLit {
                pattern: format!("{:?}", re.regex.pattern),
                flags: re.regex.flags.to_string(),
            }),
            ast::Expression::Identifier(id) => {
                let name = id.name.to_string();
                match name.as_str() {
                    "undefined" => Ok(Expr::UndefinedLit),
                    "NaN" => Ok(Expr::NumberLit(f64::NAN)),
                    "Infinity" => Ok(Expr::NumberLit(f64::INFINITY)),
                    "eval" => Err(ZapcodeError::SandboxViolation(
                        "eval is forbidden in the sandbox".to_string(),
                    )),
                    "Function" => Err(ZapcodeError::SandboxViolation(
                        "Function constructor is forbidden in the sandbox".to_string(),
                    )),
                    "process" => Err(ZapcodeError::SandboxViolation(
                        "process is forbidden in the sandbox".to_string(),
                    )),
                    "globalThis" | "global" => Err(ZapcodeError::SandboxViolation(
                        "globalThis/global is forbidden in the sandbox".to_string(),
                    )),
                    "require" => Err(ZapcodeError::SandboxViolation(
                        "require is forbidden in the sandbox".to_string(),
                    )),
                    _ => Ok(Expr::Ident(name)),
                }
            }
            ast::Expression::ArrayExpression(arr) => {
                let mut elements = Vec::new();
                for elem in &arr.elements {
                    match elem {
                        ast::ArrayExpressionElement::SpreadElement(spread) => {
                            let expr = self.lower_expr(&spread.argument)?;
                            elements.push(Some(Expr::Spread(Box::new(expr))));
                        }
                        ast::ArrayExpressionElement::Elision(_) => {
                            elements.push(None);
                        }
                        other => {
                            let expr_ref = other.to_expression();
                            let expr = self.lower_expr(expr_ref)?;
                            elements.push(Some(expr));
                        }
                    }
                }
                Ok(Expr::Array(elements))
            }
            ast::Expression::ObjectExpression(obj) => {
                let mut props = Vec::new();
                for prop in &obj.properties {
                    match prop {
                        ast::ObjectPropertyKind::ObjectProperty(p) => {
                            let key = self.lower_property_key(&p.key)?;
                            let computed = p.computed;

                            if p.shorthand {
                                props.push(ObjProperty {
                                    kind: PropKind::Shorthand,
                                    key: key.clone(),
                                    value: Expr::Ident(key),
                                    computed: false,
                                });
                            } else if p.method {
                                let value = self.lower_expr(&p.value)?;
                                props.push(ObjProperty {
                                    kind: PropKind::Method,
                                    key,
                                    value,
                                    computed,
                                });
                            } else {
                                let value = self.lower_expr(&p.value)?;
                                props.push(ObjProperty {
                                    kind: PropKind::Init,
                                    key,
                                    value,
                                    computed,
                                });
                            }
                        }
                        ast::ObjectPropertyKind::SpreadProperty(spread) => {
                            let expr = self.lower_expr(&spread.argument)?;
                            props.push(ObjProperty {
                                kind: PropKind::Spread,
                                key: String::new(),
                                value: expr,
                                computed: false,
                            });
                        }
                    }
                }
                Ok(Expr::Object(props))
            }
            ast::Expression::BinaryExpression(bin) => {
                let op = lower_binary_op(bin.operator)?;
                let left = self.lower_expr(&bin.left)?;
                let right = self.lower_expr(&bin.right)?;
                Ok(Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                })
            }
            ast::Expression::UnaryExpression(unary) => {
                if matches!(unary.operator, ast::UnaryOperator::Typeof) {
                    let operand = self.lower_expr(&unary.argument)?;
                    return Ok(Expr::TypeOf(Box::new(operand)));
                }
                if matches!(unary.operator, ast::UnaryOperator::Delete) {
                    return Err(self.unsupported(unary.span, "delete operator is not supported"));
                }
                let op = match unary.operator {
                    ast::UnaryOperator::UnaryNegation => UnaryOp::Neg,
                    ast::UnaryOperator::LogicalNot => UnaryOp::Not,
                    ast::UnaryOperator::BitwiseNot => UnaryOp::BitNot,
                    ast::UnaryOperator::Void => UnaryOp::Void,
                    ast::UnaryOperator::UnaryPlus => {
                        let operand = self.lower_expr(&unary.argument)?;
                        return Ok(Expr::Binary {
                            op: BinOp::Mul,
                            left: Box::new(operand),
                            right: Box::new(Expr::NumberLit(1.0)),
                        });
                    }
                    _ => {
                        return Err(self.unsupported(unary.span, "unsupported unary operator"));
                    }
                };
                let operand = self.lower_expr(&unary.argument)?;
                Ok(Expr::Unary {
                    op,
                    operand: Box::new(operand),
                })
            }
            ast::Expression::UpdateExpression(update) => {
                let op = match update.operator {
                    ast::UpdateOperator::Increment => UpdateOp::Increment,
                    ast::UpdateOperator::Decrement => UpdateOp::Decrement,
                };
                let operand = self.lower_simple_assign_target(&update.argument)?;
                Ok(Expr::Update {
                    op,
                    prefix: update.prefix,
                    operand: Box::new(operand),
                })
            }
            ast::Expression::LogicalExpression(logical) => {
                let op = match logical.operator {
                    ast::LogicalOperator::And => LogicalOp::And,
                    ast::LogicalOperator::Or => LogicalOp::Or,
                    ast::LogicalOperator::Coalesce => LogicalOp::NullishCoalescing,
                };
                let left = self.lower_expr(&logical.left)?;
                let right = self.lower_expr(&logical.right)?;
                Ok(Expr::Logical {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                })
            }
            ast::Expression::ConditionalExpression(cond) => {
                let test = self.lower_expr(&cond.test)?;
                let consequent = self.lower_expr(&cond.consequent)?;
                let alternate = self.lower_expr(&cond.alternate)?;
                Ok(Expr::Conditional {
                    test: Box::new(test),
                    consequent: Box::new(consequent),
                    alternate: Box::new(alternate),
                })
            }
            ast::Expression::AssignmentExpression(assign) => {
                let op = lower_assign_op(assign.operator);
                let target = self.lower_assignment_target(&assign.left)?;
                let value = self.lower_expr(&assign.right)?;
                Ok(Expr::Assignment {
                    op,
                    target: Box::new(target),
                    value: Box::new(value),
                })
            }
            ast::Expression::SequenceExpression(seq) => {
                let exprs: Result<Vec<Expr>> =
                    seq.expressions.iter().map(|e| self.lower_expr(e)).collect();
                Ok(Expr::Sequence(exprs?))
            }
            ast::Expression::CallExpression(call) => {
                let callee = self.lower_expr(&call.callee)?;
                let args = self.lower_args(&call.arguments)?;
                Ok(Expr::Call {
                    callee: Box::new(callee),
                    args,
                    optional: call.optional,
                })
            }
            ast::Expression::NewExpression(new_expr) => {
                let callee = self.lower_expr(&new_expr.callee)?;
                let args = self.lower_args(&new_expr.arguments)?;
                Ok(Expr::New {
                    callee: Box::new(callee),
                    args,
                })
            }
            ast::Expression::StaticMemberExpression(member) => {
                let object = self.lower_expr(&member.object)?;
                let property = member.property.name.to_string();
                Ok(Expr::Member {
                    object: Box::new(object),
                    property,
                    optional: member.optional,
                })
            }
            ast::Expression::ComputedMemberExpression(member) => {
                let object = self.lower_expr(&member.object)?;
                let property = self.lower_expr(&member.expression)?;
                Ok(Expr::ComputedMember {
                    object: Box::new(object),
                    property: Box::new(property),
                    optional: member.optional,
                })
            }
            ast::Expression::PrivateFieldExpression(s) => {
                Err(self.unsupported(s.span, "private fields are not supported"))
            }
            ast::Expression::ArrowFunctionExpression(arrow) => {
                let params = self.lower_formal_params(&arrow.params)?;
                let body = if arrow.expression {
                    match arrow.body.statements.first() {
                        Some(ast::Statement::ExpressionStatement(expr)) => {
                            let ret_expr = self.lower_expr(&expr.expression)?;
                            vec![Statement::Return {
                                value: Some(ret_expr),
                                span: self.span(arrow.span),
                            }]
                        }
                        _ => self.lower_statements(&arrow.body.statements)?,
                    }
                } else {
                    self.lower_statements(&arrow.body.statements)?
                };
                let func_index = self.functions.len();
                self.functions.push(FunctionDef {
                    name: None,
                    params,
                    body,
                    is_async: arrow.r#async,
                    is_generator: false,
                    is_arrow: true,
                    span: self.span(arrow.span),
                });
                Ok(Expr::ArrowFunction { func_index })
            }
            ast::Expression::FunctionExpression(func) => {
                let func_def = self.lower_function(func)?;
                let func_index = self.functions.len();
                self.functions.push(func_def);
                Ok(Expr::FunctionExpr { func_index })
            }
            ast::Expression::AwaitExpression(await_expr) => {
                let expr = self.lower_expr(&await_expr.argument)?;
                Ok(Expr::Await(Box::new(expr)))
            }
            ast::Expression::ParenthesizedExpression(paren) => self.lower_expr(&paren.expression),
            ast::Expression::ChainExpression(chain) => self.lower_chain_expr(&chain.expression),
            ast::Expression::TaggedTemplateExpression(s) => {
                Err(self.unsupported(s.span, "tagged template expressions are not supported"))
            }
            ast::Expression::ThisExpression(_) => Ok(Expr::Ident("this".to_string())),
            ast::Expression::Super(_) => Ok(Expr::Ident("super".to_string())),
            ast::Expression::YieldExpression(yield_expr) => {
                let value = match &yield_expr.argument {
                    Some(arg) => Some(Box::new(self.lower_expr(arg)?)),
                    None => None,
                };
                Ok(Expr::Yield {
                    value,
                    delegate: yield_expr.delegate,
                })
            }
            ast::Expression::ClassExpression(class) => self.lower_class_expr(class),
            ast::Expression::MetaProperty(s) => {
                Err(self.unsupported(s.span, "meta properties are not supported"))
            }
            ast::Expression::ImportExpression(s) => Err(ZapcodeError::SandboxViolation(format!(
                "dynamic import() is forbidden in the sandbox (at {}..{})",
                s.span.start, s.span.end
            ))),
            ast::Expression::TSAsExpression(ts) => self.lower_expr(&ts.expression),
            ast::Expression::TSSatisfiesExpression(ts) => self.lower_expr(&ts.expression),
            ast::Expression::TSNonNullExpression(ts) => self.lower_expr(&ts.expression),
            ast::Expression::TSTypeAssertion(ts) => self.lower_expr(&ts.expression),
            ast::Expression::TSInstantiationExpression(ts) => self.lower_expr(&ts.expression),
            _ => Err(ZapcodeError::UnsupportedSyntax {
                span: "unknown".to_string(),
                description: "unsupported expression type".to_string(),
            }),
        }
    }

    fn lower_chain_expr(&mut self, expr: &ast::ChainElement<'_>) -> Result<Expr> {
        match expr {
            ast::ChainElement::CallExpression(call) => {
                let callee = self.lower_expr(&call.callee)?;
                let args = self.lower_args(&call.arguments)?;
                Ok(Expr::Call {
                    callee: Box::new(callee),
                    args,
                    optional: call.optional,
                })
            }
            ast::ChainElement::StaticMemberExpression(member) => {
                let object = self.lower_expr(&member.object)?;
                Ok(Expr::Member {
                    object: Box::new(object),
                    property: member.property.name.to_string(),
                    optional: member.optional,
                })
            }
            ast::ChainElement::ComputedMemberExpression(member) => {
                let object = self.lower_expr(&member.object)?;
                let property = self.lower_expr(&member.expression)?;
                Ok(Expr::ComputedMember {
                    object: Box::new(object),
                    property: Box::new(property),
                    optional: member.optional,
                })
            }
            ast::ChainElement::PrivateFieldExpression(s) => {
                Err(self.unsupported(s.span, "private fields are not supported"))
            }
            ast::ChainElement::TSNonNullExpression(ts) => self.lower_expr(&ts.expression),
        }
    }

    fn lower_args(&mut self, args: &[ast::Argument<'_>]) -> Result<Vec<Expr>> {
        let mut result = Vec::new();
        for arg in args {
            match arg {
                ast::Argument::SpreadElement(spread) => {
                    let expr = self.lower_expr(&spread.argument)?;
                    result.push(Expr::Spread(Box::new(expr)));
                }
                other => {
                    let expr_ref = other.to_expression();
                    let expr = self.lower_expr(expr_ref)?;
                    result.push(expr);
                }
            }
        }
        Ok(result)
    }

    fn lower_property_key(&mut self, key: &ast::PropertyKey<'_>) -> Result<String> {
        Ok(property_key_to_string_from_key(key))
    }

    fn lower_assignment_target(&mut self, target: &ast::AssignmentTarget<'_>) -> Result<Expr> {
        match target {
            ast::AssignmentTarget::AssignmentTargetIdentifier(id) => {
                Ok(Expr::Ident(id.name.to_string()))
            }
            ast::AssignmentTarget::StaticMemberExpression(member) => {
                let object = self.lower_expr(&member.object)?;
                Ok(Expr::Member {
                    object: Box::new(object),
                    property: member.property.name.to_string(),
                    optional: false,
                })
            }
            ast::AssignmentTarget::ComputedMemberExpression(member) => {
                let object = self.lower_expr(&member.object)?;
                let property = self.lower_expr(&member.expression)?;
                Ok(Expr::ComputedMember {
                    object: Box::new(object),
                    property: Box::new(property),
                    optional: false,
                })
            }
            _ => Err(ZapcodeError::CompileError(
                "unsupported assignment target".to_string(),
            )),
        }
    }

    fn lower_simple_assign_target(
        &mut self,
        target: &ast::SimpleAssignmentTarget<'_>,
    ) -> Result<Expr> {
        match target {
            ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                Ok(Expr::Ident(id.name.to_string()))
            }
            ast::SimpleAssignmentTarget::StaticMemberExpression(member) => {
                let object = self.lower_expr(&member.object)?;
                Ok(Expr::Member {
                    object: Box::new(object),
                    property: member.property.name.to_string(),
                    optional: false,
                })
            }
            ast::SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                let object = self.lower_expr(&member.object)?;
                let property = self.lower_expr(&member.expression)?;
                Ok(Expr::ComputedMember {
                    object: Box::new(object),
                    property: Box::new(property),
                    optional: false,
                })
            }
            _ => Err(ZapcodeError::CompileError(
                "unsupported update target".to_string(),
            )),
        }
    }
}

fn property_key_to_string(key: &ast::PropertyKey<'_>) -> String {
    property_key_to_string_from_key(key)
}

fn property_key_to_string_from_key(key: &ast::PropertyKey<'_>) -> String {
    match key {
        ast::PropertyKey::StaticIdentifier(id) => id.name.to_string(),
        ast::PropertyKey::StringLiteral(s) => s.value.to_string(),
        ast::PropertyKey::NumericLiteral(n) => n.value.to_string(),
        _ => "<computed>".to_string(),
    }
}

fn lower_binary_op(op: ast::BinaryOperator) -> Result<BinOp> {
    match op {
        ast::BinaryOperator::Addition => Ok(BinOp::Add),
        ast::BinaryOperator::Subtraction => Ok(BinOp::Sub),
        ast::BinaryOperator::Multiplication => Ok(BinOp::Mul),
        ast::BinaryOperator::Division => Ok(BinOp::Div),
        ast::BinaryOperator::Remainder => Ok(BinOp::Rem),
        ast::BinaryOperator::Exponential => Ok(BinOp::Pow),
        ast::BinaryOperator::Equality => Ok(BinOp::Eq),
        ast::BinaryOperator::Inequality => Ok(BinOp::Neq),
        ast::BinaryOperator::StrictEquality => Ok(BinOp::StrictEq),
        ast::BinaryOperator::StrictInequality => Ok(BinOp::StrictNeq),
        ast::BinaryOperator::LessThan => Ok(BinOp::Lt),
        ast::BinaryOperator::LessEqualThan => Ok(BinOp::Lte),
        ast::BinaryOperator::GreaterThan => Ok(BinOp::Gt),
        ast::BinaryOperator::GreaterEqualThan => Ok(BinOp::Gte),
        ast::BinaryOperator::BitwiseAnd => Ok(BinOp::BitAnd),
        ast::BinaryOperator::BitwiseOR => Ok(BinOp::BitOr),
        ast::BinaryOperator::BitwiseXOR => Ok(BinOp::BitXor),
        ast::BinaryOperator::ShiftLeft => Ok(BinOp::Shl),
        ast::BinaryOperator::ShiftRight => Ok(BinOp::Shr),
        ast::BinaryOperator::ShiftRightZeroFill => Ok(BinOp::Ushr),
        ast::BinaryOperator::In => Ok(BinOp::In),
        ast::BinaryOperator::Instanceof => Ok(BinOp::InstanceOf),
    }
}

fn lower_assign_op(op: ast::AssignmentOperator) -> AssignOp {
    match op {
        ast::AssignmentOperator::Assign => AssignOp::Assign,
        ast::AssignmentOperator::Addition => AssignOp::AddAssign,
        ast::AssignmentOperator::Subtraction => AssignOp::SubAssign,
        ast::AssignmentOperator::Multiplication => AssignOp::MulAssign,
        ast::AssignmentOperator::Division => AssignOp::DivAssign,
        ast::AssignmentOperator::Remainder => AssignOp::RemAssign,
        ast::AssignmentOperator::Exponential => AssignOp::PowAssign,
        ast::AssignmentOperator::BitwiseAnd => AssignOp::BitAndAssign,
        ast::AssignmentOperator::BitwiseOR => AssignOp::BitOrAssign,
        ast::AssignmentOperator::BitwiseXOR => AssignOp::BitXorAssign,
        ast::AssignmentOperator::ShiftLeft => AssignOp::ShlAssign,
        ast::AssignmentOperator::ShiftRight => AssignOp::ShrAssign,
        ast::AssignmentOperator::ShiftRightZeroFill => AssignOp::UshrAssign,
        ast::AssignmentOperator::LogicalNullish => AssignOp::NullishAssign,
        ast::AssignmentOperator::LogicalAnd => AssignOp::AndAssign,
        ast::AssignmentOperator::LogicalOr => AssignOp::OrAssign,
    }
}
