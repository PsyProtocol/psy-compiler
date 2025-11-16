use std::marker::PhantomData;

use psy_vm::dpn::ops::{
    context_trait::DPNContext,
    sym_felt::{QStateInitializable, SymFeltRef},
    utils::SparseArray,
};
use psylang_macros::{qcontract, FeltSized, QStateInitializable};

type Felt = SymFeltRef;
use psy_vm::dpn::ops::context_trait::FeltSized;
//use psylang_macros::FeltSized;

#[derive(FeltSized, QStateInitializable)]
pub struct SimpleContractState {
    pub x: Felt,
    pub y: Felt,
    pub z: SparseArray<Felt, 12>,
}
pub struct SimpleContractStateful<C: DPNContext<Felt>> {
    state: SimpleContractState,
    _phantom: PhantomData<C>,
}
impl<C: DPNContext<Felt>> SimpleContractStateful<C> {
    pub fn new_with_ctx(context: &mut C) -> Self {
        let contract_state_tree_height = SimpleContractState::size().next_power_of_two().trailing_zeros() as u16;

        let user_id = context.get_user_id();
        let contract_id = context.get_contract_id();
        Self {
            _phantom: PhantomData,
            state: SimpleContractState::create_stateful_at(context, SymFeltRef::new_constant(0), contract_state_tree_height, contract_id, user_id),
        }
    }
}

#[qcontract]
impl<C: DPNContext<Felt>> SimpleContractStateful<C> {
    pub fn simple_set(&mut self, ctx: &mut C, a: Felt, b: Felt) -> Felt {
        if self.state.x > a {
            self.state.x.set_state(ctx, a * b);
        } else {
            self.state.y.set_state(ctx, a * b);
        }
        self.state.x
    }
}

/*
impl<C: DPNContext<Felt>> SimpleContractStateful<C> {
    pub fn simple_set(&mut self, ctx: &mut C, a: Felt, b: Felt) -> Felt {
        {
            {
                let psy_rwv_cond = {
                    let tmp_left = ({
                        let q_tmp_index_result = a;
                        self.state.z.q_get(ctx, q_tmp_index_result)
                    });
                    let tmp_right = (a);
                    ctx.op_gt(tmp_left, tmp_right)
                };
                ctx.start_if_block(psy_rwv_cond);
                {
                    {
                        let psy_rwv_old_value = ({
                            let q_tmp_index_result = {
                                let tmp_left = (SymFeltRef::cns(2));
                                let tmp_right = (b);
                                ctx.op_mul(tmp_left, tmp_right)
                            };
                            self.state.z.q_get(ctx, q_tmp_index_result)
                        })
                        .clone();
                        let psy_rwv_new_value = ({
                            let tmp_left = (a);
                            let tmp_right = (b);
                            ctx.op_mul(tmp_left, tmp_right)
                        })
                        .clone();
                        ctx.cset_state(psy_rwv_old_value, psy_rwv_new_value);
                    };
                }
            }
            {
                ctx.start_else_block();
                {
                    {
                        let tmp_arg_1 = ({
                            let tmp_left = (a);
                            let tmp_right = (b);
                            ctx.op_mul(tmp_left, tmp_right)
                        });
                        self.state.y.set_state(ctx, tmp_arg_1)
                    };
                }
            }
            ctx.end_if_block();
        }
        {
            let q_tmp_index_result = {
                let tmp_left = (a);
                let tmp_right = (b);
                ctx.op_add(tmp_left, tmp_right)
            };
            self.state.z.q_get(ctx, q_tmp_index_result)
        }
    }
}
    */
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

    use crate::{ex5::SimpleContractStateless, ex7::SimpleContractStateful};

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

    fn run_test_simple_state(v_a: u64, v_b: u64) {
        let mut ctx = QExecContext::new();
        let mut contract = SimpleContractStateful::new_with_ctx(&mut ctx);
        let a = ctx.add_input();
        let b = ctx.add_input();
        let z = contract.simple_set(&mut ctx, a, b);
        let result = exec_eval_simple(vec![v_a, v_b], &ctx, Some(vec![z]));
        println!("{:?}", result);
        /*
        assert_eq!(
            result[0],
            real_simple_math(gl(v_a), gl(v_b)).to_canonical_u64()
        );*/
    }
    #[test]
    fn test_simple_state() {
        run_test_simple_state(2, 3);
        run_test_simple_math(15, 20)
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
