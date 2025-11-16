use std::ops::{Index, IndexMut};

use psy_ast::{DefId, ExprId, StmtId};
use psy_common::Arena;

use crate::{CheckedDefinitionNode, CheckedExprNode, CheckedStmtNode, Result};

#[derive(Debug)]
pub struct CheckedProgram<F: Clone + From<u32>> {
    pub exprs: Arena<ExprId, CheckedExprNode<F>>,
    pub stmts: Arena<StmtId, CheckedStmtNode>,
    pub defs: Arena<DefId, CheckedDefinitionNode>,
}

impl<F: Clone + From<u32>> CheckedProgram<F> {
    pub fn new() -> Self {
        Self {
            exprs: Arena::new(),
            stmts: Arena::new(),
            defs: Arena::new(),
        }
    }

    pub fn modify_definition(&mut self, def_id: DefId, f: impl FnOnce(&mut CheckedDefinitionNode) -> Result<()>) -> Result<()> {
        f(&mut self[def_id])
    }
}

macro_rules! impl_index {
    ($index_type:ty, $output_type:ty, $field:ident) => {
        impl<F: Clone + From<u32>> Index<$index_type> for CheckedProgram<F> {
            type Output = $output_type;
            fn index(&self, index: $index_type) -> &Self::Output {
                &self.$field[index]
            }
        }

        impl<F: Clone + From<u32>> IndexMut<$index_type> for CheckedProgram<F> {
            fn index_mut(&mut self, index: $index_type) -> &mut Self::Output {
                &mut self.$field[index]
            }
        }
    };
}

impl_index!(ExprId, CheckedExprNode<F>, exprs);
impl_index!(StmtId, CheckedStmtNode, stmts);
impl_index!(DefId, CheckedDefinitionNode, defs);
