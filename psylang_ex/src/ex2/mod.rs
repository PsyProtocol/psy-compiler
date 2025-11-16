use psy_vm::dpn::ops::exec_context::QExecContext;
use psylang_macros::trace_var;

pub struct ExampleContract2 {}

impl ExampleContract2 {
    pub fn new() -> ExampleContract2 {
        ExampleContract2 {}
    }

    #[trace_var]
    pub fn inc_counter_small(&mut self, ctx: &mut QExecContext, a: u64, b: u64) {
        let mut c = a * b;
        if a < b {
            c = a + b;
        }
        //ctx.assert_eq(c,100, "c must be less than 100");
        assert!(c < 100, "c must be less than 100");
    }
}
