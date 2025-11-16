use psy_vm::dpn::ops::{context_trait::DPNContext, exec_context::QExecContext, sym_felt::SymFeltRef};

pub struct ExampleContract3B {}
impl ExampleContract3B {
    pub fn new() -> ExampleContract3B {
        ExampleContract3B {}
    }

    pub fn inc_counter_small(&mut self, ctx: &mut QExecContext, a: SymFeltRef, b: SymFeltRef) {
        let k = a * b * a + b;
        let z = k + a;
        /*
        let mut c = a*b;
        {
            let i_ctx_condition = ctx.op_lt(a, b);
            ctx.start_if_block(i_ctx_condition);
        }
        {
            let result = ctx.op_add(a, b);
            c = ctx.cset(c, result);
        }
        ctx.end_if_block();
        let x = ctx.op_lt(c, 100.into());*/
        let check_z = ctx.op_lt(z, 100.into());
        ctx.assert_true(check_z, "c must be less than 100");
    }
}

pub struct ExampleContract2 {}

impl ExampleContract2 {
    pub fn new() -> ExampleContract2 {
        ExampleContract2 {}
    }
    pub fn inc_counter_small(&mut self, ctx: &mut QExecContext, a: SymFeltRef, b: SymFeltRef) -> SymFeltRef {
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
            let tmp_arg_1 = (SymFeltRef::cns(100));
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

#[cfg(test)]
mod tests {
    use psy_vm::dpn::{
        eval::exec_eval::exec_eval_simple,
        ops::{context_trait::DPNContext, exec_context::QExecContext},
    };

    use super::ExampleContract2;

    #[test]
    fn test_pass_1() {
        let mut ctx = QExecContext::new();
        let input_a = ctx.add_input();
        let input_b = ctx.add_input();
        let z = ExampleContract2::new().inc_counter_small(&mut ctx, input_a, input_b);
        let result = exec_eval_simple(vec![4, 5], &ctx, Some(vec![z]));
        println!("{:?}", result);
    }
}
