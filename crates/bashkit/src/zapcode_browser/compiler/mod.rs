pub mod instruction;

use std::collections::{HashMap, HashSet};

use crate::zapcode_browser::error::{Result, ZapcodeError};
use crate::zapcode_browser::parser::ir::*;
use instruction::*;

/// Compiled program ready for VM execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompiledProgram {
    pub instructions: Vec<Instruction>,
    pub functions: Vec<CompiledFunction>,
    pub local_names: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompiledFunction {
    pub name: Option<String>,
    pub params: Vec<ParamPattern>,
    pub instructions: Vec<Instruction>,
    pub local_count: usize,
    pub local_names: Vec<String>,
    pub is_async: bool,
    pub is_generator: bool,
}

struct Compiler {
    instructions: Vec<Instruction>,
    locals: Vec<String>,
    local_indices: HashMap<String, usize>,
    functions: Vec<CompiledFunction>,
    loop_stack: Vec<LoopInfo>,
    external_functions: HashSet<String>,
}

struct LoopInfo {
    break_patches: Vec<usize>,
    continue_patches: Vec<usize>,
}

impl Compiler {
    fn new(external_functions: HashSet<String>) -> Self {
        Self {
            instructions: Vec::new(),
            locals: Vec::new(),
            local_indices: HashMap::new(),
            functions: Vec::new(),
            loop_stack: Vec::new(),
            external_functions,
        }
    }

    fn emit(&mut self, instr: Instruction) -> usize {
        let idx = self.instructions.len();
        self.instructions.push(instr);
        idx
    }

    fn current_offset(&self) -> usize {
        self.instructions.len()
    }

    fn patch_jump(&mut self, instr_idx: usize, target: usize) {
        match &mut self.instructions[instr_idx] {
            Instruction::Jump(t)
            | Instruction::JumpIfFalse(t)
            | Instruction::JumpIfTrue(t)
            | Instruction::JumpIfNullish(t) => {
                *t = target;
            }
            Instruction::SetupTry(catch_target, _) => {
                *catch_target = target;
            }
            _ => {}
        }
    }

    fn declare_local(&mut self, name: &str) -> usize {
        if let Some(&idx) = self.local_indices.get(name) {
            return idx;
        }
        let idx = self.locals.len();
        self.locals.push(name.to_string());
        self.local_indices.insert(name.to_string(), idx);
        idx
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        self.local_indices.get(name).copied()
    }

    fn compile_program(&mut self, program: &Program) -> Result<()> {
        // First pass: compile all function definitions
        for func_def in &program.functions {
            let compiled = self.compile_function_def(func_def)?;
            self.functions.push(compiled);
        }

        // Second pass: compile body
        // For the last statement, if it's an expression, keep the value on the stack
        let len = program.body.len();
        for (i, stmt) in program.body.iter().enumerate() {
            let is_last = i == len - 1;
            if is_last {
                if let Statement::Expression { expr, .. } = stmt {
                    self.compile_expr(expr)?;
                    // Don't pop — leave value on stack as program result
                } else {
                    self.compile_statement(stmt)?;
                }
            } else {
                self.compile_statement(stmt)?;
            }
        }

        Ok(())
    }

    fn compile_function_def(&mut self, func: &FunctionDef) -> Result<CompiledFunction> {
        let mut func_compiler = Compiler::new(self.external_functions.clone());

        // Set up parameters as locals
        for param in &func.params {
            match param {
                ParamPattern::Ident(name) => {
                    func_compiler.declare_local(name);
                }
                ParamPattern::Rest(name) => {
                    func_compiler.declare_local(name);
                }
                ParamPattern::DefaultValue { pattern, .. } => {
                    if let ParamPattern::Ident(name) = pattern.as_ref() {
                        func_compiler.declare_local(name);
                    }
                }
                ParamPattern::ObjectDestructure(fields) => {
                    for field in fields {
                        let name = field.alias.as_ref().unwrap_or(&field.key);
                        func_compiler.declare_local(name);
                    }
                }
                ParamPattern::ArrayDestructure(elems) => {
                    for elem in elems.iter().flatten() {
                        if let ParamPattern::Ident(name) = elem {
                            func_compiler.declare_local(name);
                        }
                    }
                }
            }
        }

        for stmt in &func.body {
            func_compiler.compile_statement(stmt)?;
        }

        // Implicit return undefined
        func_compiler.emit(Instruction::Push(Constant::Undefined));
        func_compiler.emit(Instruction::Return);

        Ok(CompiledFunction {
            name: func.name.clone(),
            params: func.params.clone(),
            instructions: func_compiler.instructions,
            local_count: func_compiler.locals.len(),
            local_names: func_compiler.locals,
            is_async: func.is_async,
            is_generator: func.is_generator,
        })
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<()> {
        match stmt {
            Statement::VariableDecl { declarations, .. } => {
                for decl in declarations {
                    self.compile_var_declarator(decl)?;
                }
            }
            Statement::Expression { expr, .. } => {
                self.compile_expr(expr)?;
                self.emit(Instruction::Pop);
            }
            Statement::Return { value, .. } => {
                match value {
                    Some(expr) => self.compile_expr(expr)?,
                    None => {
                        self.emit(Instruction::Push(Constant::Undefined));
                    }
                }
                self.emit(Instruction::Return);
            }
            Statement::If {
                test,
                consequent,
                alternate,
                ..
            } => {
                self.compile_expr(test)?;
                let jump_else = self.emit(Instruction::JumpIfFalse(0));

                for s in consequent {
                    self.compile_statement(s)?;
                }

                if let Some(alt) = alternate {
                    let jump_end = self.emit(Instruction::Jump(0));
                    let else_target = self.current_offset();
                    self.patch_jump(jump_else, else_target);

                    for s in alt {
                        self.compile_statement(s)?;
                    }
                    let end_target = self.current_offset();
                    self.patch_jump(jump_end, end_target);
                } else {
                    let else_target = self.current_offset();
                    self.patch_jump(jump_else, else_target);
                }
            }
            Statement::While { test, body, .. } => {
                let loop_start = self.current_offset();
                self.loop_stack.push(LoopInfo {
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                });

                self.compile_expr(test)?;
                let exit_jump = self.emit(Instruction::JumpIfFalse(0));

                for s in body {
                    self.compile_statement(s)?;
                }

                self.emit(Instruction::Jump(loop_start));
                let loop_end = self.current_offset();
                self.patch_jump(exit_jump, loop_end);

                let loop_info = self.loop_stack.pop().unwrap();
                for patch in loop_info.break_patches {
                    self.patch_jump(patch, loop_end);
                }
                for patch in loop_info.continue_patches {
                    self.patch_jump(patch, loop_start);
                }
            }
            Statement::DoWhile { body, test, .. } => {
                let loop_start = self.current_offset();
                self.loop_stack.push(LoopInfo {
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                });

                for s in body {
                    self.compile_statement(s)?;
                }

                let continue_target = self.current_offset();
                self.compile_expr(test)?;
                self.emit(Instruction::JumpIfTrue(loop_start));

                let loop_end = self.current_offset();
                let loop_info = self.loop_stack.pop().unwrap();
                for patch in loop_info.break_patches {
                    self.patch_jump(patch, loop_end);
                }
                for patch in loop_info.continue_patches {
                    self.patch_jump(patch, continue_target);
                }
            }
            Statement::For {
                init,
                test,
                update,
                body,
                ..
            } => {
                if let Some(init) = init {
                    self.compile_statement(init)?;
                }

                let loop_start = self.current_offset();
                self.loop_stack.push(LoopInfo {
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                });

                let exit_jump = if let Some(test) = test {
                    self.compile_expr(test)?;
                    Some(self.emit(Instruction::JumpIfFalse(0)))
                } else {
                    None
                };

                for s in body {
                    self.compile_statement(s)?;
                }

                let continue_target = self.current_offset();
                if let Some(update) = update {
                    self.compile_expr(update)?;
                    self.emit(Instruction::Pop);
                }

                self.emit(Instruction::Jump(loop_start));
                let loop_end = self.current_offset();

                if let Some(exit) = exit_jump {
                    self.patch_jump(exit, loop_end);
                }

                let loop_info = self.loop_stack.pop().unwrap();
                for patch in loop_info.break_patches {
                    self.patch_jump(patch, loop_end);
                }
                for patch in loop_info.continue_patches {
                    self.patch_jump(patch, continue_target);
                }
            }
            Statement::ForOf {
                binding,
                iterable,
                body,
                ..
            } => {
                self.compile_expr(iterable)?;
                self.emit(Instruction::GetIterator);

                let loop_start = self.current_offset();
                self.loop_stack.push(LoopInfo {
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                });

                self.emit(Instruction::Dup);
                self.emit(Instruction::IteratorNext);
                self.emit(Instruction::IteratorDone);
                let exit_jump = self.emit(Instruction::JumpIfTrue(0));

                // Bind the value
                match binding {
                    ForBinding::Ident(name) => {
                        let idx = self.declare_local(name);
                        self.emit(Instruction::StoreLocal(idx));
                    }
                    ForBinding::Destructure(_) => {
                        self.emit(Instruction::Pop); // TODO: destructure
                    }
                }

                for s in body {
                    self.compile_statement(s)?;
                }

                self.emit(Instruction::Jump(loop_start));
                let loop_end = self.current_offset();
                self.patch_jump(exit_jump, loop_end);
                self.emit(Instruction::Pop); // pop iterator

                let loop_info = self.loop_stack.pop().unwrap();
                for patch in loop_info.break_patches {
                    self.patch_jump(patch, loop_end);
                }
                for patch in loop_info.continue_patches {
                    self.patch_jump(patch, loop_start);
                }
            }
            Statement::Block { body, .. } => {
                for s in body {
                    self.compile_statement(s)?;
                }
            }
            Statement::Throw { value, .. } => {
                self.compile_expr(value)?;
                self.emit(Instruction::Throw);
            }
            Statement::TryCatch {
                try_body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                let setup = self.emit(Instruction::SetupTry(0, None));

                for s in try_body {
                    self.compile_statement(s)?;
                }
                self.emit(Instruction::EndTry);
                let jump_past_catch = self.emit(Instruction::Jump(0));

                // Catch block
                let catch_start = self.current_offset();
                self.patch_jump(setup, catch_start);

                if let Some(param) = catch_param {
                    let idx = self.declare_local(param);
                    self.emit(Instruction::StoreLocal(idx));
                } else {
                    self.emit(Instruction::Pop); // discard error
                }

                for s in catch_body {
                    self.compile_statement(s)?;
                }

                let after_catch = self.current_offset();
                self.patch_jump(jump_past_catch, after_catch);

                if let Some(finally) = finally_body {
                    for s in finally {
                        self.compile_statement(s)?;
                    }
                }
            }
            Statement::Break { .. } => {
                let idx = self.emit(Instruction::Jump(0));
                if let Some(loop_info) = self.loop_stack.last_mut() {
                    loop_info.break_patches.push(idx);
                }
            }
            Statement::Continue { .. } => {
                let idx = self.emit(Instruction::Jump(0));
                if let Some(loop_info) = self.loop_stack.last_mut() {
                    loop_info.continue_patches.push(idx);
                }
            }
            Statement::FunctionDecl { func_index, .. } => {
                self.emit(Instruction::CreateClosure(*func_index));
                let name = if *func_index < self.functions.len() {
                    self.functions[*func_index].name.clone()
                } else {
                    None
                };
                if let Some(name) = name {
                    // Store as both local and global so recursion works
                    self.emit(Instruction::Dup);
                    let idx = self.declare_local(&name);
                    self.emit(Instruction::StoreLocal(idx));
                    self.emit(Instruction::StoreGlobal(name));
                } else {
                    self.emit(Instruction::Pop);
                }
            }
            Statement::ClassDecl {
                name,
                super_class,
                constructor,
                methods,
                static_methods,
                ..
            } => {
                self.compile_class(
                    Some(name),
                    super_class.as_deref(),
                    constructor.as_deref(),
                    methods,
                    static_methods,
                )?;
                // Store the class as both local and global
                self.emit(Instruction::Dup);
                let idx = self.declare_local(name);
                self.emit(Instruction::StoreLocal(idx));
                self.emit(Instruction::StoreGlobal(name.clone()));
            }
            Statement::Switch {
                discriminant,
                cases,
                ..
            } => {
                self.compile_expr(discriminant)?;
                let mut case_jumps = Vec::new();
                let mut default_jump = None;

                // Compile test expressions and jumps
                for case in cases {
                    if let Some(test) = &case.test {
                        self.emit(Instruction::Dup);
                        self.compile_expr(test)?;
                        self.emit(Instruction::StrictEq);
                        let jump = self.emit(Instruction::JumpIfTrue(0));
                        case_jumps.push(jump);
                    } else {
                        default_jump = Some(case_jumps.len());
                        case_jumps.push(0); // placeholder
                    }
                }

                let jump_end = self.emit(Instruction::Jump(0));

                // Compile case bodies
                let mut body_starts = Vec::new();
                for case in cases {
                    body_starts.push(self.current_offset());
                    for s in &case.consequent {
                        self.compile_statement(s)?;
                    }
                }

                let end = self.current_offset();
                self.emit(Instruction::Pop); // pop discriminant

                // Patch jumps
                for (i, &jump) in case_jumps.iter().enumerate() {
                    if jump != 0 {
                        self.patch_jump(jump, body_starts[i]);
                    }
                }
                if let Some(default_idx) = default_jump {
                    // Jump to default case
                    self.patch_jump(jump_end, body_starts[default_idx]);
                } else {
                    self.patch_jump(jump_end, end);
                }
            }
        }
        Ok(())
    }

    fn compile_var_declarator(&mut self, decl: &VarDeclarator) -> Result<()> {
        match &decl.pattern {
            AssignTarget::Ident(name) => {
                let idx = self.declare_local(name);
                match &decl.init {
                    Some(expr) => {
                        self.compile_expr(expr)?;
                        self.emit(Instruction::StoreLocal(idx));
                    }
                    None => {
                        self.emit(Instruction::Push(Constant::Undefined));
                        self.emit(Instruction::StoreLocal(idx));
                    }
                }
            }
            AssignTarget::ObjectDestructure(fields) => {
                if let Some(expr) = &decl.init {
                    self.compile_expr(expr)?;
                } else {
                    self.emit(Instruction::Push(Constant::Undefined));
                }
                for field in fields {
                    self.emit(Instruction::Dup);
                    self.emit(Instruction::GetProperty(field.key.clone()));
                    let name = field.alias.as_ref().unwrap_or(&field.key);
                    let idx = self.declare_local(name);
                    self.emit(Instruction::StoreLocal(idx));
                }
                self.emit(Instruction::Pop); // pop source object
            }
            AssignTarget::ArrayDestructure(elems) => {
                if let Some(expr) = &decl.init {
                    self.compile_expr(expr)?;
                } else {
                    self.emit(Instruction::Push(Constant::Undefined));
                }
                for (i, elem) in elems.iter().enumerate() {
                    if let Some(target) = elem {
                        self.emit(Instruction::Dup);
                        self.emit(Instruction::Push(Constant::Int(i as i64)));
                        self.emit(Instruction::GetIndex);
                        match target {
                            AssignTarget::Ident(name) => {
                                let idx = self.declare_local(name);
                                self.emit(Instruction::StoreLocal(idx));
                            }
                            _ => {
                                self.emit(Instruction::Pop); // TODO: nested destructure
                            }
                        }
                    }
                }
                self.emit(Instruction::Pop); // pop source array
            }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<()> {
        match expr {
            Expr::NumberLit(n) => {
                if *n == (*n as i64) as f64 && !n.is_nan() && n.is_finite() {
                    self.emit(Instruction::Push(Constant::Int(*n as i64)));
                } else {
                    self.emit(Instruction::Push(Constant::Float(*n)));
                }
            }
            Expr::StringLit(s) => {
                self.emit(Instruction::Push(Constant::String(s.clone())));
            }
            Expr::BoolLit(b) => {
                self.emit(Instruction::Push(Constant::Bool(*b)));
            }
            Expr::NullLit => {
                self.emit(Instruction::Push(Constant::Null));
            }
            Expr::UndefinedLit => {
                self.emit(Instruction::Push(Constant::Undefined));
            }
            Expr::TemplateLit { quasis, exprs } => {
                let mut parts = 0;
                for (i, quasi) in quasis.iter().enumerate() {
                    if !quasi.is_empty() {
                        self.emit(Instruction::Push(Constant::String(quasi.clone())));
                        parts += 1;
                    }
                    if i < exprs.len() {
                        self.compile_expr(&exprs[i])?;
                        parts += 1;
                    }
                }
                if parts == 0 {
                    self.emit(Instruction::Push(Constant::String(String::new())));
                } else if parts > 1 {
                    self.emit(Instruction::ConcatStrings(parts));
                }
            }
            Expr::RegExpLit { .. } => {
                // RegExp not fully supported — push as string for now
                self.emit(Instruction::Push(Constant::Undefined));
            }
            Expr::Ident(name) => {
                if name == "this" {
                    self.emit(Instruction::LoadThis);
                } else if let Some(idx) = self.resolve_local(name) {
                    self.emit(Instruction::LoadLocal(idx));
                } else {
                    self.emit(Instruction::LoadGlobal(name.clone()));
                }
            }
            Expr::Array(elements) => {
                let mut count = 0;
                for elem in elements {
                    match elem {
                        Some(e) => {
                            self.compile_expr(e)?;
                            count += 1;
                        }
                        None => {
                            self.emit(Instruction::Push(Constant::Undefined));
                            count += 1;
                        }
                    }
                }
                self.emit(Instruction::CreateArray(count));
            }
            Expr::Object(props) => {
                let mut count = 0;
                for prop in props {
                    match prop.kind {
                        PropKind::Spread => {
                            self.compile_expr(&prop.value)?;
                            self.emit(Instruction::Spread);
                            count += 1;
                        }
                        _ => {
                            self.emit(Instruction::Push(Constant::String(prop.key.clone())));
                            self.compile_expr(&prop.value)?;
                            count += 1;
                        }
                    }
                }
                self.emit(Instruction::CreateObject(count));
            }
            Expr::Spread(expr) => {
                self.compile_expr(expr)?;
                self.emit(Instruction::Spread);
            }
            Expr::Binary { op, left, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                let instr = match op {
                    BinOp::Add => Instruction::Add,
                    BinOp::Sub => Instruction::Sub,
                    BinOp::Mul => Instruction::Mul,
                    BinOp::Div => Instruction::Div,
                    BinOp::Rem => Instruction::Rem,
                    BinOp::Pow => Instruction::Pow,
                    BinOp::Eq => Instruction::Eq,
                    BinOp::Neq => Instruction::Neq,
                    BinOp::StrictEq => Instruction::StrictEq,
                    BinOp::StrictNeq => Instruction::StrictNeq,
                    BinOp::Lt => Instruction::Lt,
                    BinOp::Lte => Instruction::Lte,
                    BinOp::Gt => Instruction::Gt,
                    BinOp::Gte => Instruction::Gte,
                    BinOp::BitAnd => Instruction::BitAnd,
                    BinOp::BitOr => Instruction::BitOr,
                    BinOp::BitXor => Instruction::BitXor,
                    BinOp::Shl => Instruction::Shl,
                    BinOp::Shr => Instruction::Shr,
                    BinOp::Ushr => Instruction::Ushr,
                    BinOp::In => Instruction::In,
                    BinOp::InstanceOf => Instruction::InstanceOf,
                };
                self.emit(instr);
            }
            Expr::Unary { op, operand } => {
                self.compile_expr(operand)?;
                match op {
                    UnaryOp::Neg => {
                        self.emit(Instruction::Neg);
                    }
                    UnaryOp::Not => {
                        self.emit(Instruction::Not);
                    }
                    UnaryOp::BitNot => {
                        self.emit(Instruction::BitNot);
                    }
                    UnaryOp::Void => {
                        self.emit(Instruction::Void);
                    }
                }
            }
            Expr::Update {
                op,
                prefix,
                operand,
            } => {
                // Load current value
                self.compile_expr(operand)?;

                if !prefix {
                    self.emit(Instruction::Dup); // keep pre-value
                }

                match op {
                    UpdateOp::Increment => {
                        self.emit(Instruction::Increment);
                    }
                    UpdateOp::Decrement => {
                        self.emit(Instruction::Decrement);
                    }
                }

                if *prefix {
                    self.emit(Instruction::Dup); // keep post-value
                }

                // Store back
                self.compile_store(operand)?;

                if !prefix {
                    // Swap to get pre-value on top
                    // Actually the dup before increment already has it
                }
            }
            Expr::Logical { op, left, right } => match op {
                LogicalOp::And => {
                    self.compile_expr(left)?;
                    self.emit(Instruction::Dup);
                    let skip = self.emit(Instruction::JumpIfFalse(0));
                    self.emit(Instruction::Pop);
                    self.compile_expr(right)?;
                    let end = self.current_offset();
                    self.patch_jump(skip, end);
                }
                LogicalOp::Or => {
                    self.compile_expr(left)?;
                    self.emit(Instruction::Dup);
                    let skip = self.emit(Instruction::JumpIfTrue(0));
                    self.emit(Instruction::Pop);
                    self.compile_expr(right)?;
                    let end = self.current_offset();
                    self.patch_jump(skip, end);
                }
                LogicalOp::NullishCoalescing => {
                    self.compile_expr(left)?;
                    self.emit(Instruction::Dup);
                    let skip = self.emit(Instruction::JumpIfNullish(0));
                    let jump_end = self.emit(Instruction::Jump(0));
                    let nullish_target = self.current_offset();
                    self.patch_jump(skip, nullish_target);
                    self.emit(Instruction::Pop);
                    self.compile_expr(right)?;
                    let end = self.current_offset();
                    self.patch_jump(jump_end, end);
                }
            },
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.compile_expr(test)?;
                let jump_else = self.emit(Instruction::JumpIfFalse(0));
                self.compile_expr(consequent)?;
                let jump_end = self.emit(Instruction::Jump(0));
                let else_target = self.current_offset();
                self.patch_jump(jump_else, else_target);
                self.compile_expr(alternate)?;
                let end = self.current_offset();
                self.patch_jump(jump_end, end);
            }
            Expr::Assignment { op, target, value } => {
                match op {
                    AssignOp::Assign => {
                        self.compile_expr(value)?;
                        self.emit(Instruction::Dup);
                        self.compile_store(target)?;
                    }
                    _ => {
                        // Compound assignment: load, operate, store
                        self.compile_expr(target)?;
                        self.compile_expr(value)?;
                        match op {
                            AssignOp::AddAssign => {
                                self.emit(Instruction::Add);
                            }
                            AssignOp::SubAssign => {
                                self.emit(Instruction::Sub);
                            }
                            AssignOp::MulAssign => {
                                self.emit(Instruction::Mul);
                            }
                            AssignOp::DivAssign => {
                                self.emit(Instruction::Div);
                            }
                            AssignOp::RemAssign => {
                                self.emit(Instruction::Rem);
                            }
                            AssignOp::PowAssign => {
                                self.emit(Instruction::Pow);
                            }
                            AssignOp::BitAndAssign => {
                                self.emit(Instruction::BitAnd);
                            }
                            AssignOp::BitOrAssign => {
                                self.emit(Instruction::BitOr);
                            }
                            AssignOp::BitXorAssign => {
                                self.emit(Instruction::BitXor);
                            }
                            AssignOp::ShlAssign => {
                                self.emit(Instruction::Shl);
                            }
                            AssignOp::ShrAssign => {
                                self.emit(Instruction::Shr);
                            }
                            AssignOp::UshrAssign => {
                                self.emit(Instruction::Ushr);
                            }
                            _ => {}
                        }
                        self.emit(Instruction::Dup);
                        self.compile_store(target)?;
                    }
                }
            }
            Expr::Sequence(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    self.compile_expr(e)?;
                    if i < exprs.len() - 1 {
                        self.emit(Instruction::Pop);
                    }
                }
            }
            Expr::Member {
                object,
                property,
                optional,
            } => {
                self.compile_expr(object)?;
                if *optional {
                    self.emit(Instruction::Dup);
                    let skip = self.emit(Instruction::JumpIfNullish(0));
                    self.emit(Instruction::GetProperty(property.clone()));
                    let end = self.emit(Instruction::Jump(0));
                    let nullish = self.current_offset();
                    self.patch_jump(skip, nullish);
                    self.emit(Instruction::Pop);
                    self.emit(Instruction::Push(Constant::Undefined));
                    let after = self.current_offset();
                    self.patch_jump(end, after);
                } else {
                    self.emit(Instruction::GetProperty(property.clone()));
                }
            }
            Expr::ComputedMember {
                object,
                property,
                optional,
            } => {
                self.compile_expr(object)?;
                if *optional {
                    self.emit(Instruction::Dup);
                    let skip = self.emit(Instruction::JumpIfNullish(0));
                    self.compile_expr(property)?;
                    self.emit(Instruction::GetIndex);
                    let end = self.emit(Instruction::Jump(0));
                    let nullish = self.current_offset();
                    self.patch_jump(skip, nullish);
                    self.emit(Instruction::Pop);
                    self.emit(Instruction::Push(Constant::Undefined));
                    let after = self.current_offset();
                    self.patch_jump(end, after);
                } else {
                    self.compile_expr(property)?;
                    self.emit(Instruction::GetIndex);
                }
            }
            Expr::Call { callee, args, .. } => {
                // Check if this is a super() call
                if let Expr::Ident(name) = callee.as_ref() {
                    if name == "super" {
                        for arg in args {
                            self.compile_expr(arg)?;
                        }
                        self.emit(Instruction::CallSuper(args.len()));
                        return Ok(());
                    }
                }
                // Check if this is a direct call to an external function
                if let Expr::Ident(name) = callee.as_ref() {
                    if self.external_functions.contains(name) {
                        // Emit args then CallExternal (no callee push needed)
                        for arg in args {
                            self.compile_expr(arg)?;
                        }
                        self.emit(Instruction::CallExternal(name.clone(), args.len()));
                        return Ok(());
                    }
                }
                self.compile_expr(callee)?;
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.emit(Instruction::Call(args.len()));
            }
            Expr::New { callee, args } => {
                self.compile_expr(callee)?;
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.emit(Instruction::Construct(args.len()));
            }
            Expr::ArrowFunction { func_index } | Expr::FunctionExpr { func_index } => {
                self.emit(Instruction::CreateClosure(*func_index));
            }
            Expr::Await(expr) => {
                self.compile_expr(expr)?;
                // Emit Await instruction to unwrap Promise objects.
                // External call suspension is already handled by CallExternal
                // before this point — Await only handles internal promise values.
                self.emit(Instruction::Await);
            }
            Expr::Yield { value, delegate: _ } => {
                // Compile the yielded value (or undefined if none)
                match value {
                    Some(expr) => self.compile_expr(expr)?,
                    None => {
                        self.emit(Instruction::Push(Constant::Undefined));
                    }
                }
                // Yield instruction: suspends the generator, pops value, pushes received value on resume
                self.emit(Instruction::Yield);
            }
            Expr::TypeOf(operand) => {
                self.compile_expr(operand)?;
                self.emit(Instruction::TypeOf);
            }
            Expr::ClassExpr {
                name,
                super_class,
                constructor,
                methods,
                static_methods,
            } => {
                self.compile_class(
                    name.as_deref(),
                    super_class.as_deref(),
                    constructor.as_deref(),
                    methods,
                    static_methods,
                )?;
            }
        }
        Ok(())
    }

    fn compile_class(
        &mut self,
        name: Option<&str>,
        super_class: Option<&str>,
        constructor: Option<&FunctionDef>,
        methods: &[ClassMethod],
        static_methods: &[ClassMethod],
    ) -> Result<()> {
        let class_name = name.unwrap_or("AnonymousClass").to_string();

        // Push super class if present
        if let Some(sc) = super_class {
            if let Some(idx) = self.resolve_local(sc) {
                self.emit(Instruction::LoadLocal(idx));
            } else {
                self.emit(Instruction::LoadGlobal(sc.to_string()));
            }
        }

        // Push static methods: name, closure pairs
        for sm in static_methods {
            self.emit(Instruction::Push(Constant::String(sm.name.clone())));
            let compiled = self.compile_function_def(&sm.func)?;
            let func_idx = self.functions.len();
            self.functions.push(compiled);
            self.emit(Instruction::CreateClosure(func_idx));
        }

        // Push instance methods: name, closure pairs
        for m in methods {
            self.emit(Instruction::Push(Constant::String(m.name.clone())));
            let compiled = self.compile_function_def(&m.func)?;
            let func_idx = self.functions.len();
            self.functions.push(compiled);
            self.emit(Instruction::CreateClosure(func_idx));
        }

        // Push constructor closure (or undefined if none)
        if let Some(ctor) = constructor {
            let compiled = self.compile_function_def(ctor)?;
            let func_idx = self.functions.len();
            self.functions.push(compiled);
            self.emit(Instruction::CreateClosure(func_idx));
        } else {
            self.emit(Instruction::Push(Constant::Undefined));
        }

        self.emit(Instruction::CreateClass {
            name: class_name,
            n_methods: methods.len(),
            n_statics: static_methods.len(),
            has_super: super_class.is_some(),
        });

        Ok(())
    }

    fn compile_store(&mut self, target: &Expr) -> Result<()> {
        match target {
            Expr::Ident(name) if name == "this" => {
                self.emit(Instruction::StoreThis);
            }
            Expr::Ident(name) => {
                if let Some(idx) = self.resolve_local(name) {
                    self.emit(Instruction::StoreLocal(idx));
                } else {
                    self.emit(Instruction::StoreGlobal(name.clone()));
                }
            }
            Expr::Member {
                object, property, ..
            } => {
                self.compile_expr(object)?;
                self.emit(Instruction::SetProperty(property.clone()));
                // SetProperty pushes the modified object back — store it to the parent
                self.compile_store(object)?;
            }
            Expr::ComputedMember {
                object, property, ..
            } => {
                self.compile_expr(object)?;
                self.compile_expr(property)?;
                self.emit(Instruction::SetIndex);
                // SetIndex pushes the modified object back — store it to the parent
                self.compile_store(object)?;
            }
            _ => {
                return Err(ZapcodeError::CompileError(
                    "invalid assignment target".to_string(),
                ));
            }
        }
        Ok(())
    }
}

pub fn compile(program: &Program) -> Result<CompiledProgram> {
    compile_with_externals(program, HashSet::new())
}

pub fn compile_with_externals(
    program: &Program,
    external_functions: HashSet<String>,
) -> Result<CompiledProgram> {
    let mut compiler = Compiler::new(external_functions);
    compiler.compile_program(program)?;

    Ok(CompiledProgram {
        instructions: compiler.instructions,
        functions: compiler.functions,
        local_names: compiler.locals,
    })
}
