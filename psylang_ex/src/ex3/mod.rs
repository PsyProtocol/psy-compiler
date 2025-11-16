use psy_vm::dpn::ops::{context_trait::DPNContext, exec_context::QExecContext, sym_felt::SymFeltRef};
pub mod ex3_generic;
pub mod ex3_mac;
pub struct ExampleContract3 {}

impl ExampleContract3 {
    pub fn new() -> ExampleContract3 {
        ExampleContract3 {}
    }

    pub fn inc_counter_small(&mut self, ctx: &mut QExecContext, a: SymFeltRef, b: SymFeltRef) {
        let mut c = ctx.op_mul(a, b);

        {
            let a = ctx.op_lt(a, b);
            ctx.start_if_block(a);
        }
        {
            let result = ctx.op_add(a, b);
            c = ctx.cset(c, result);
        }
        ctx.end_if_block();
        let x = ctx.op_lt(c, 100.into());
        ctx.assert_true(x, "c must be less than 100");
    }
}

pub fn test_contract() {
    let mut ctx = QExecContext::new();
    let input_a = ctx.add_input();
    let input_b = ctx.add_input();
    ExampleContract3::new().inc_counter_small(&mut ctx, input_a, input_b);
    println!("{:?}", ctx);
}
