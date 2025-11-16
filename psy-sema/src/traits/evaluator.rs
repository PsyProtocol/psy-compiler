use psy_ast::ConstValue;
use psy_vm::dpn::ops::context_trait::ContextFelt;

use crate::{CheckedExprNode, CheckedProgram, CheckedValueRef, TypeCheckerVisitorContext};

pub trait Evaluator<F: Clone + From<u32> + ContextFelt, C> {
    fn evaluate_expr(
        &mut self,
        program: &CheckedProgram<F>,
        expr: &CheckedExprNode<F>,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
    ) -> CheckedValueRef<F>;
    fn to_constant(&mut self, value: F) -> u64;
    fn from_constant(&mut self, value: ConstValue) -> F;
}
