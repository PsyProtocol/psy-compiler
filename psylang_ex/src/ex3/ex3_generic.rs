use std::marker::PhantomData;

use psy_vm::dpn::{
    eval::exec_eval::exec_eval_simple,
    ops::{
        context_trait::{ContextFelt, DPNContext},
        exec_context::QExecContext,
        sym_felt::SymFeltRef,
    },
};
use psylang_macros::qcontract;

pub struct ExampleContract2<F: ContextFelt, C: DPNContext<F>> {
    _phantom: PhantomData<(F, C)>,
}

impl<F: ContextFelt, C: DPNContext<F>> ExampleContract2<F, C> {
    pub fn new() -> ExampleContract2<F, C> {
        ExampleContract2 { _phantom: PhantomData }
    }
    pub fn inc_counter_small(&mut self, ctx: &mut C, a: F, b: F) -> F {
        let k = {
            let tmp_left = ({
                let tmp_left = ({
                    let tmp_left = (a);
                    let tmp_right = (b);
                    ctx.op_mul(tmp_left, tmp_right)
                });
                let tmp_right = (a);
                ctx.op_mul(tmp_left, tmp_right)
            });
            let tmp_right = (b);
            ctx.op_add(tmp_left, tmp_right)
        };
        let z = {
            let tmp_left = (k);
            let tmp_right = (a);
            ctx.op_add(tmp_left, tmp_right)
        };
        let check_z = {
            let tmp_arg_0 = (z);
            let tmp_arg_1 = (F::cns(100));
            ctx.op_lt(tmp_arg_0, tmp_arg_1)
        };
        {
            let tmp_arg_0 = (check_z);
            let tmp_arg_1 = ("c must be less than 100");
            ctx.assert_true(tmp_arg_0, tmp_arg_1)
        };
        z
    }
}

type F = SymFeltRef;
pub struct ExampleContract3C<C: DPNContext<F>> {
    _phantom: PhantomData<C>,
}

impl<C: DPNContext<F>> ExampleContract3C<C> {
    pub fn new() -> ExampleContract3C<C> {
        ExampleContract3C { _phantom: PhantomData }
    }
    pub fn inc_counter_small(&mut self, ctx: &mut C, a: F, b: F) -> F {
        let k = (a * 2) + 4 * b - 1 * 2;
        let z = k + a;
        ctx.assert_true((z > 3).into(), "z must be gt than 3");
        z
    }
}
pub struct ExampleContract3D<C: DPNContext<F>> {
    _phantom: PhantomData<C>,
}
impl<C: DPNContext<F>> ExampleContract3D<C> {
    pub fn new() -> ExampleContract3D<C> {
        ExampleContract3D { _phantom: PhantomData }
    }
}
#[qcontract]
impl<C: DPNContext<F>> ExampleContract3D<C> {
    pub fn inc_counter_small(&mut self, ctx: &mut C, a: F, b: F) -> F {
        let k = (a * 2) + 4 * b - 1 * 2;
        let z = k + a;

        ctx.assert_true(z > 3, "z must be gt than 3");
        z
    }
}

pub fn test_it_3() {
    let mut ctx = QExecContext::new();
    let mut contract = ExampleContract3D::new();
    let a = SymFeltRef::cns(2);
    let b = SymFeltRef::cns(3);
    let z = contract.inc_counter_small(&mut ctx, a, b);
    assert_eq!(z.get_u64(), 16);
}
pub fn test_it_3v2() {
    let mut ctx = QExecContext::new();
    let mut contract = ExampleContract3D::new();
    let a = ctx.add_input();
    let b = ctx.add_input();
    let z = contract.inc_counter_small(&mut ctx, a, b);
    let result = exec_eval_simple(vec![2, 3], &ctx, Some(vec![z]));

    assert_eq!(result[0], 16);
}
#[cfg(test)]
mod test {
    use psy_vm::dpn::ops::exec_context::QExecContext;

    use super::*;

    #[test]
    fn test_example_contract3() {
        let mut ctx = QExecContext::new();
        let mut contract = ExampleContract3C::new();
        let a = F::cns(2);
        let b = F::cns(3);
        let z = contract.inc_counter_small(&mut ctx, a, b);
        println!("{:?}", ctx);
        assert_eq!(z.get_u64(), 16);
    }
    #[test]
    fn test_example_contract3_sym() {
        let mut ctx = QExecContext::new();
        let mut contract = ExampleContract3C::new();
        let a = F::cns(2);
        let b = F::cns(3);
        let z = contract.inc_counter_small(&mut ctx, a, b);
        assert_eq!(z.get_u64(), 16);
    }
    #[test]
    fn test_sym_1() {
        test_it_3v2();
    }
}
