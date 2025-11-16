use psylang_macros::{show_streams, trace_var};

type Felt = u64;

pub struct ExampleContractState {
    pub counter: Felt,
}
pub struct QContract<S> {
    pub state: S,
}

impl QContract<ExampleContractState> {
    pub fn inc_counter_small(&mut self, amount: Felt) {
        if amount < 10 {
            self.state.counter += 1;
        } else {
            self.state.counter += amount;
        }
    }
}
