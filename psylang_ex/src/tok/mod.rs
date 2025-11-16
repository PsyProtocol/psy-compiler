use std::marker::PhantomData;

use psy_vm::dpn::ops::{
    context_trait::DPNContext,
    sym_felt::SymFeltRef,
    utils::{QStatefulContract, SparseArray, U252},
};

type Felt = SymFeltRef;

/*
const MAX_USERS: usize = 16777216;
const TICKER_FELT_LEN: usize = 2;
#[derive(Debug, QContractState)]
pub struct SimpleTokenContract<C: DPNContext<Felt>> {
    pub balance: U252,
    pub claimed_transfer_amount: SparseArray<U252, MAX_USERS>,
    pub sent_transfer_amount: SparseArray<U252, MAX_USERS>,
    _c: PhantomData<C>,
}

#[qcontract]
impl<C: DPNContext<Felt>> SimpleTokenContract<C> {
    pub fn get_symbol() -> [Felt; TICKER_FELT_LEN] {
        [0x6E656B6F54u64, 0]
    }
    pub fn send_transfer(&mut self, ctx: &mut C, to: Felt, amount: U252) {
        ctx.assert_true(self.balance.gte(amount), "insufficient balance");

        self.balance = self.balance.sub(amount);
        self.sent_transfer_amount[to] = self.sent_transfer_amount[to].add(amount);
    }

    pub fn claim_transfer(&mut self, ctx: &mut C, from: Felt) {
        let sender_state: Self = self.get_contract_state_for_user(from, ctx.get_contract_id());
        let total_sent = sender_state.sent_transfer_amount[ctx.get_user_id()];
        let total_claimed = self.claimed_transfer_amount[from];

        ctx.assert_true(
            total_sent.gte(total_claimed),
            "already claimed incoming tokens from the sender",
        );

        self.claimed_transfer_amount[from] = total_sent;
        self.balance = self.balance.add(total_sent.sub(total_claimed));
    }
}*/
