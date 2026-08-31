//! Recursive-descent parser for PSY source files.
//!
//! This module replaces the LALRPOP-generated parser with a hand-written
//! recursive-descent implementation that writes directly into the existing
//! `psy_ast::Program<F>` arenas.
//!
//! Architecture:
//! ```text
//! parse_module_into (entry point)
//!   └── ModuleParser
//!        ├── TokenCursor (navigation, `>` splitting, checkpoints)
//!        ├── `Program` arenas and interner
//!        └── item, statement, expression, and type parsers
//! ```

pub mod cursor;
pub mod expression;
pub mod intrinsic;
pub mod item;
pub mod statement;
pub mod trivia;
pub mod ty;

use psy_ast::{Identifier, Location, ModuleNode, Visibility};
use psy_common::FileId;
use psy_vm::dpn::ops::context_trait::{ContextFelt, DPNContext};

use crate::error::{Error, Result};

/// Input for parsing a single module file.
pub struct ParseModuleInput<'src> {
    pub source: &'src str,
    pub file_id: FileId,
    pub module_name: Identifier,
    pub visibility: Visibility,
}

/// Per-file recursive-descent parser that allocates directly into `Program<F>`.
pub struct ModuleParser<'src, 'p, F: Clone + From<u32>, C> {
    pub cursor: cursor::TokenCursor<'src>,
    pub program: &'p mut psy_ast::Program<F>,
    pub ctx: &'p mut C,
    /// Nesting depth of expression parsing (incremented in parse_precedence).
    /// Deeply nested input previously overflowed the stack (abort, not an
    /// error) — ~150 parens on a 2 MB thread stack, ~400 on wasm32 (M7).
    pub expression_depth: u32,
    /// Whether `Type { ... }` struct literals may be recognized at the current
    /// expression position. Disabled inside control-flow predicates/ranges
    /// (where `{` introduces the construct's body) and re-enabled inside
    /// delimiters and after `=`/`return`/`=>`.
    pub struct_literals_allowed: bool,
}

impl<'src, 'p, F, C> ModuleParser<'src, 'p, F, C>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    /// Create a parser for one source file.
    pub fn new(input: ParseModuleInput<'src>, program: &'p mut psy_ast::Program<F>, ctx: &'p mut C) -> Result<Self> {
        let cursor = cursor::TokenCursor::from_source(input.source, input.file_id).map_err(|error| Error::LexicalError {
            location: psy_ast::Location::new(input.file_id, error.start, error.end),
        })?;

        Ok(Self { cursor, program, ctx, struct_literals_allowed: true, expression_depth: 0 })
    }

    /// Parse a complete module and return its `ModuleNode`.
    pub fn parse(mut self, module_name: Identifier, visibility: Visibility) -> Result<ModuleNode> {
        let start = self.cursor.peek_start();

        let mut module_comments = self.cursor.take_leading_comments();

        let mut definitions = Vec::new();
        let mut modules = Vec::new();
        let mut inline_modules = Vec::new();

        while !self.cursor.at_end() {
            let comments = self.cursor.take_leading_comments();

            // A module may end with trailing comments after the last item;
            // once only comments remain, collect them and stop (the old
            // grammar's `back_comments` slot — feeding them to
            // parse_module_item made an empty cursor report UnexpectedEof).
            if self.cursor.at_end() {
                let mut back = module_comments;
                back.extend(comments);
                module_comments = back;
                break;
            }

            match crate::recursive::item::parse_module_item(&mut self, comments)? {
                ParsedModuleItem::ExternalModule(name, vis, loc) => {
                    modules.push((name, vis, loc));
                }
                ParsedModuleItem::InlineModule(node) => {
                    inline_modules.push(node);
                }
                ParsedModuleItem::Definition(def) => {
                    let def_id = self.program.defs.alloc_item(def);
                    definitions.push(def_id);
                }
            }
        }

        let end = self.cursor.eof_location().end;
        let location = Location::new(self.cursor.file_id(), start, end);
        let file_id = self.cursor.file_id();

        // Items are separated while parsing so definition arena allocation
        // remains in source order.
        Ok(ModuleNode {
            name: module_name,
            file_id,
            modules,
            inline_modules,
            definitions,
            visibility,
            comments: module_comments,
            location,
        })
    }
}

// Arena and source helpers shared by the parser submodules.
impl<'src, 'p, F, C> ModuleParser<'src, 'p, F, C>
where
    F: Clone + From<u32>,
{
    /// Allocate an expression into the arena and return its ID.
    pub fn alloc_expr(&mut self, expr: psy_ast::ExprNode<F>) -> psy_ast::ExprId {
        self.program.exprs.alloc_item(expr)
    }

    /// Allocate a statement into the arena and return its ID.
    pub fn alloc_stmt(&mut self, stmt: psy_ast::StmtNode) -> psy_ast::StmtId {
        self.program.stmts.alloc_item(stmt)
    }

    /// Allocate a definition into the arena and return its ID.
    pub fn alloc_def(&mut self, def: psy_ast::DefinitionNode) -> psy_ast::DefId {
        self.program.defs.alloc_item(def)
    }

    /// Intern an identifier string and return its ID.
    pub fn intern(&mut self, s: &str) -> psy_ast::IdentId {
        self.program.interner.intern_ident(s)
    }

    /// Create an `Identifier` at the current location.
    pub fn ident(&mut self, s: &str, loc: Location) -> psy_ast::Identifier {
        psy_ast::Identifier::new(self.intern(s), loc)
    }

    /// Create a location spanning byte offsets in the current file.
    pub fn location(&self, start: usize, end: usize) -> Location {
        self.cursor.location(start, end)
    }

    /// Build the current-token or EOF parser error without duplicating shape logic.
    pub(crate) fn unexpected(&self, expected: Vec<crate::error::ExpectedToken>) -> Error {
        match self.cursor.peek() {
            Some(found) => Error::UnexpectedToken {
                found: format!("{found:?}"),
                expected,
                location: self.cursor.current_location(),
            },
            None => Error::UnexpectedEof {
                expected,
                location: self.cursor.eof_location(),
            },
        }
    }

    /// Borrow the context (for literal construction).
    pub fn ctx(&mut self) -> &mut C {
        self.ctx
    }
}

/// Internal result of parsing a module-level item.
pub enum ParsedModuleItem {
    ExternalModule(psy_ast::Identifier, Visibility, Location),
    InlineModule(ModuleNode),
    Definition(psy_ast::DefinitionNode),
}

/// Parse a single source file into a `ModuleNode`, writing all expressions,
/// statements, and definitions into the program's arenas.
pub fn parse_module_into<'src, F, C>(input: ParseModuleInput<'src>, program: &mut psy_ast::Program<F>, ctx: &mut C) -> Result<ModuleNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let module_name = input.module_name;
    let visibility = input.visibility;
    let parser = ModuleParser::new(input, program, ctx)?;
    parser.parse(module_name, visibility)
}
