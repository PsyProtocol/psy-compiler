use std::marker::PhantomData;

use psy_vm::dpn::ops::{
    context_trait::DPNContext,
    sym_felt::{QStateInitializable, SymFeltRef},
    utils::SparseArray,
};
use psylang_macros::{qcontract, FeltSized};

type Felt = SymFeltRef;
use psy_vm::dpn::ops::context_trait::FeltSized;
//use psylang_macros::FeltSized;

#[derive(FeltSized)]
pub struct SimpleContractState {
    pub x: Felt,
    pub y: Felt,
    pub z: SparseArray<Felt, 12>,
}
impl psy_vm::dpn::ops::sym_felt::QStateInitializable for SimpleContractState {
    fn create_stateful_at<CTXT: DPNContext<SymFeltRef>>(
        context: &mut CTXT,
        state_pointer: SymFeltRef,
        contract_state_tree_height: u16,
        contract_id: SymFeltRef,
        user_id: SymFeltRef,
    ) -> Self {
        let mut cur_offset = 0u64;
        let mut nw_pointer = SymFeltRef::new_constant(cur_offset);
        nw_pointer = context.op_add(state_pointer, SymFeltRef::new_constant(cur_offset));
        let r#f_t_0 = <Felt as QStateInitializable>::create_stateful_at(context, nw_pointer, contract_state_tree_height, contract_id, user_id);
        cur_offset = cur_offset + <Felt as FeltSized>::size();
        nw_pointer = context.op_add(state_pointer, SymFeltRef::new_constant(cur_offset));
        let r#f_t_1 = <Felt as QStateInitializable>::create_stateful_at(context, nw_pointer, contract_state_tree_height, contract_id, user_id);
        cur_offset = cur_offset + <Felt as FeltSized>::size();
        nw_pointer = context.op_add(state_pointer, SymFeltRef::new_constant(cur_offset));
        let r#f_t_2 =
            <SparseArray<Felt, 12> as QStateInitializable>::create_stateful_at(context, nw_pointer, contract_state_tree_height, contract_id, user_id);
        cur_offset = cur_offset + <SparseArray<Felt, 12> as FeltSized>::size();
        Self {
            x: r#f_t_0,
            y: r#f_t_1,
            z: r#f_t_2,
        }
    }
}

pub struct SimpleContractStateful<C: DPNContext<Felt>> {
    _phantom: PhantomData<C>,
}
impl<C: DPNContext<Felt>> SimpleContractStateful<C> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

#[qcontract]
impl<C: DPNContext<Felt>> SimpleContractStateful<C> {
    pub fn simple_math(&mut self, ctx: &mut C, a: Felt, b: Felt) -> Felt {
        let k = (a + 2) * 4 * b - 3 * (a + b);
        let z = k + a;

        ctx.assert_true(z > 3, "z must be gt than 3");
        z
    }
    pub fn if_test_2(&mut self, ctx: &mut C, a: Felt, b: Felt) -> Felt {
        let mut c = a * b;
        let mut k = 123;
        if a < b {
            c = a + b;
        } else if a == b {
            c = a;
        } else if a == 1337 {
            c = b;
        } else {
            k = 456;
        }
        c + k
    }
}

/*
impl<C: DPNContext<Felt>> SimpleContractStateless<C> {
    pub fn simple_math(&mut self, ctx: &mut C, a: Felt, b: Felt) -> Felt {
        let k = (a + 2) * 4 * b - 3 * (a + b);
        let z = k + a;

        ctx.assert_true(z > 3, "z must be gt than 3");
        z
    }
    pub fn if_test(&mut self, ctx: &mut C, a: Felt, b: Felt) -> Felt {
        let mut c = a * b;
        ctx.start_if_block(a < b);
        c = ctx.cset(c, a + b);
        ctx.end_if_block();
        c
    }
    /*
    pub fn if_test_2(&mut self, ctx: &mut C, a: Felt, b: Felt) -> Felt {
        let mut c = a * b;
        if a < b {
            c = a + b;
        }
        c
    }*/
}
*/
#[cfg(test)]
mod test {
    use plonky2::field::{
        goldilocks_field::GoldilocksField,
        types::{Field, PrimeField64},
    };
    use psy_vm::dpn::{
        eval::exec_eval::exec_eval_simple,
        ops::{context_trait::DPNContext, exec_context::QExecContext},
    };

    use crate::ex5::SimpleContractStateless;

    fn gl(a: u64) -> GoldilocksField {
        GoldilocksField::from_canonical_u64(a)
    }

    fn real_simple_math(a: GoldilocksField, b: GoldilocksField) -> GoldilocksField {
        let k = (a + gl(2)) * gl(4) * b - gl(3) * (a + b);
        let z = k + a;

        z
    }

    fn real_if_test(a: GoldilocksField, b: GoldilocksField) -> GoldilocksField {
        let mut c = a * b;
        if a.to_canonical_u64() < b.to_canonical_u64() {
            c = a + b;
        }
        c
    }

    pub fn real_if_test_2(a: GoldilocksField, b: GoldilocksField) -> GoldilocksField {
        let mut c = a * b;
        let mut k = gl(123);
        if a.to_canonical_u64() < b.to_canonical_u64() {
            c = a + b;
        } else if a == b {
            c = a;
        } else if a == gl(1337) {
            c = b;
        } else {
            k = gl(456);
        }
        c + k
    }
    fn run_test_simple_math(v_a: u64, v_b: u64) {
        let mut ctx = QExecContext::new();
        let mut contract = SimpleContractStateless::new();
        let a = ctx.add_input();
        let b = ctx.add_input();
        let z = contract.simple_math(&mut ctx, a, b);
        let result = exec_eval_simple(vec![v_a, v_b], &ctx, Some(vec![z]));

        assert_eq!(result[0], real_simple_math(gl(v_a), gl(v_b)).to_canonical_u64());
    }
    fn run_test_simple_if(v_a: u64, v_b: u64) {
        let mut ctx = QExecContext::new();
        let mut contract = SimpleContractStateless::new();
        let a = ctx.add_input();
        let b = ctx.add_input();
        let z = contract.if_test_2(&mut ctx, a, b);
        let result = exec_eval_simple(vec![v_a, v_b], &ctx, Some(vec![z]));

        assert_eq!(result[0], real_if_test_2(gl(v_a), gl(v_b)).to_canonical_u64());
    }

    #[test]
    fn test_simple_math() {
        run_test_simple_math(2, 3);
        run_test_simple_math(15, 20)
    }
    #[test]
    fn test_simple_if() {
        run_test_simple_if(2, 3);
        run_test_simple_if(25, 20);
    }
}
