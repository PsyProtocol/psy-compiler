use std::{fs, marker::PhantomData};

use plonky2::{
    field::{goldilocks_field::GoldilocksField, types::Field},
    hash::{hash_types::HashOut, poseidon::PoseidonHash},
    plonk::config::{AlgebraicHasher, GenericConfig, PoseidonGoldilocksConfig},
};
use psy_common::{data::qhashout::QHashOut, ups::circuits::LocalCircuitId, utils::debug_timer::DebugTimer};
use psy_common_circuit::circuits::traits::qstandard::QStandardCircuit;
use psy_config::network_constants::{GLOBAL_USER_TREE_HEIGHT, UPS_SESSION_PROOF_TREE_HEIGHT};
use psy_crypto::hash::{traits::hasher::MerkleZeroHasher, utils::gen_dapen_contract_function_method_id};
use psy_data::{
    qblock::cmds::deploy_contract::QBCDeployContract,
    qdata::contract::ContractCodeDefinition,
    qstore::{controllers::session_info::SessionCircuitInfoStore, imm::cmd_processor::PsyReadCommandProcessorSync},
};
use psy_dpn_circuit::circuits::cfc::DapenContractFunctionCircuit;
use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
use psy_ups_circuit::{
    circuit_manager::core::{PsyUPSStepCircuitManager, QCircuitManager},
    session::UserProvingSessionManager,
};
use psy_vm::dpn::{
    ops::{context_trait::DPNContext, exec_context::QExecContext, sym_felt::SymFeltRef},
    vm::{compile::PsyCompileResult, def::DPNFunctionCircuitDefinition},
};
use psylang_macros::qcontract;

type Felt = SymFeltRef;

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
    pub fn simple_mint_debug(&mut self, ctx: &mut C, amount: Felt) -> Felt {
        let self_user_leaf = ctx.get_state_hash_at(ctx.get_user_id());
        //[balance,alt1,alt2,alt3]
        let current_balance = self_user_leaf[0];

        let new_balance = current_balance + amount;
        ctx.cset_state_hash_at(ctx.get_user_id(), [new_balance, self_user_leaf[1], self_user_leaf[2], self_user_leaf[3]]);

        new_balance
    }

    pub fn simple_transfer(&mut self, ctx: &mut C, recipient: Felt, amount: Felt) -> Felt {
        let self_user_id = ctx.get_user_id();
        let self_user_leaf = ctx.get_state_hash_at(self_user_id);

        let current_balance = self_user_leaf[0];

        ctx.assert_true((amount <= current_balance).into(), "insufficient balance");

        let new_balance = current_balance - amount;
        ctx.assert_true((new_balance < current_balance).into(), "user balance overflow");

        ctx.cset_state_hash_at(self_user_id, [new_balance, self_user_leaf[1], self_user_leaf[2], self_user_leaf[3]]);

        let p2p_leaf = ctx.get_state_hash_at(recipient);
        let previous_total_sent_to_recipient = p2p_leaf[2];

        let new_total_sent_to_recipient = previous_total_sent_to_recipient + amount;
        ctx.assert_true(
            (new_total_sent_to_recipient > previous_total_sent_to_recipient).into(),
            "sent amount overflow",
        );

        ctx.cset_state_hash_at(recipient, [p2p_leaf[0], p2p_leaf[1], new_total_sent_to_recipient, p2p_leaf[3]]);
        current_balance
    }

    pub fn simple_claim(&mut self, ctx: &mut C, sender: Felt) -> Felt {
        let self_user_id = ctx.get_user_id();
        ctx.assert_true((sender != self_user_id).into(), "you cannot claim from your self");

        let self_leaf = ctx.get_state_hash_at(self_user_id);
        let current_balance = self_leaf[0];

        let loc_transfer_info_for_sender = ctx.get_state_hash_at(sender);
        let loc_previous_total_recieved_from_sender = loc_transfer_info_for_sender[0];

        let sender_transfer_info_leaf_for_me = ctx.get_other_user_contract_state_hash_at(0, sender, ctx.get_contract_id(), self_user_id);

        let sender_total_sent_to_me = sender_transfer_info_leaf_for_me[2];

        ctx.assert_true(
            sender_total_sent_to_me > loc_previous_total_recieved_from_sender,
            "no tokens to claim from this sender",
        );

        let tokens_to_claim = sender_total_sent_to_me - loc_previous_total_recieved_from_sender;

        let loc_new_total_recieved_from_sender = sender_total_sent_to_me;

        ctx.cset_state_hash_at(
            sender,
            [
                loc_new_total_recieved_from_sender,
                loc_transfer_info_for_sender[1],
                loc_transfer_info_for_sender[2],
                loc_transfer_info_for_sender[3],
            ],
        );

        let new_balance = tokens_to_claim + current_balance;
        ctx.assert_true((current_balance < new_balance).into(), "balance overflow");

        ctx.cset_state_hash_at(self_user_id, [new_balance, self_leaf[1], self_leaf[2], self_leaf[3]]);

        new_balance
    }
}

// Rest of the helper functions and structs for test contracts...
fn compile_simple_mint_debug() -> anyhow::Result<DPNFunctionCircuitDefinition> {
    let mut ctx = QExecContext::new();
    let mut contract = SimpleContractStateful::new();
    let amount = ctx.add_input();
    let z = contract.simple_mint_debug(&mut ctx, amount);
    let outputs = vec![z];
    let method_args = [("amount".to_string(), 1usize)];
    let method_name = "simple_mint_debug".to_string();
    let method_id = gen_dapen_contract_function_method_id(method_name.clone(), &method_args);
    let fn_circuit_def = PsyCompileResult::compile_exec("simple_mint_debug".to_string(), method_id, &ctx.store, &ctx, &outputs);

    Ok(fn_circuit_def)
}

fn compile_simple_transfer() -> anyhow::Result<DPNFunctionCircuitDefinition> {
    let mut ctx = QExecContext::new();
    let mut contract = SimpleContractStateful::new();
    let recipient = ctx.add_input();
    let amount = ctx.add_input();
    let z = contract.simple_transfer(&mut ctx, recipient, amount);
    let outputs = vec![z];
    let method_args = [("recipient".to_string(), 1usize), ("amount".to_string(), 1usize)];
    let method_name = "simple_transfer".to_string();
    let method_id = gen_dapen_contract_function_method_id(method_name.clone(), &method_args);
    let fn_circuit_def = PsyCompileResult::compile_exec("simple_transfer".to_string(), method_id, &ctx.store, &ctx, &outputs);

    Ok(fn_circuit_def)
}

fn compile_simple_claim() -> anyhow::Result<DPNFunctionCircuitDefinition> {
    let mut ctx = QExecContext::new();
    let mut contract = SimpleContractStateful::new();
    let sender = ctx.add_input();
    let z = contract.simple_claim(&mut ctx, sender);
    let outputs = vec![z];
    let method_args = [("sender".to_string(), 1usize)];
    let method_name = "simple_claim".to_string();
    let method_id = gen_dapen_contract_function_method_id(method_name.clone(), &method_args);
    let fn_circuit_def = PsyCompileResult::compile_exec("simple_claim".to_string(), method_id, &ctx.store, &ctx, &outputs);

    Ok(fn_circuit_def)
}

#[derive(Debug)]
pub struct SimpleTestContractItem<C: GenericConfig<D>, const D: usize>
where
    C::Hasher: AlgebraicHasher<C::F>,
{
    pub circuit: DapenContractFunctionCircuit<C, D>,
    pub def: DPNFunctionCircuitDefinition,
}

pub struct SimpleTestContract<C: GenericConfig<D>, const D: usize>
where
    C::Hasher: AlgebraicHasher<C::F>,
{
    pub funcs: Vec<SimpleTestContractItem<C, D>>,
}

impl<C: GenericConfig<D>, const D: usize> SimpleTestContract<C, D>
where
    C::Hasher: AlgebraicHasher<C::F>,
{
    pub fn new_empty() -> Self {
        Self { funcs: Vec::new() }
    }
    pub fn new_with_items(funcs: Vec<SimpleTestContractItem<C, D>>) -> Self {
        Self { funcs }
    }
    pub fn add_func(&mut self, func: SimpleTestContractItem<C, D>) {
        self.funcs.push(func);
    }
    pub fn add_func_def(&mut self, circuit: DapenContractFunctionCircuit<C, D>, def: DPNFunctionCircuitDefinition) {
        self.funcs.push(SimpleTestContractItem { circuit, def });
    }
    pub fn register_funcs(&self, contract_id: u32, scs: &mut SessionCircuitInfoStore<C::F>) {
        for func in self.funcs.iter() {
            scs.register_circuit(
                LocalCircuitId::new_cfc(contract_id, func.def.method_id),
                func.circuit.get_fingerprint(),
                func.circuit.get_verifier_config_ref().into(),
            );
        }
    }
}

type C = PoseidonGoldilocksConfig;
const D: usize = 2;
type F = GoldilocksField;

impl SimpleTestContract<C, D> {
    pub async fn prove_func<
        R: PsyReadCommandProcessorSync<F>
            + psy_data::traits::qdatastore::qtreedata::PsyComboDataStoreReaderSync<F>
            + kvq::traits::KVQBinaryStore
            + Send
            + Sync,
    >(
        &self,
        circuit_mgr: &PsyUPSStepCircuitManager<C, D>,
        mgr: &mut UserProvingSessionManager<F, PoseidonHash, R, C, D>,
        contract_id: u32,
        fn_name: &str,
        inputs: Vec<F>,
    ) -> anyhow::Result<()> {
        for (i, f) in self.funcs.iter().enumerate() {
            if f.def.name.eq(fn_name) {
                mgr.prove_contract_call(
                    circuit_mgr,
                    F::from_canonical_u32(contract_id),
                    i as u32,
                    &f.def,
                    inputs,
                )
                .await?;
                return Ok(());
            }
        }
        anyhow::bail!("unable to find function {}", fn_name);
    }
}

pub fn gen_test_contract<C: GenericConfig<D>, const D: usize>(
    deployer: QHashOut<C::F>,
) -> anyhow::Result<(SimpleTestContract<C, D>, QBCDeployContract<C::F>)>
where
    C::Hasher: AlgebraicHasher<C::F> + MerkleZeroHasher<HashOut<C::F>>,
{
    let mut timer = DebugTimer::new("demo_user_proving_session");

    timer.lap("start");

    let simple_mint_debug_def = compile_simple_mint_debug()?;
    timer.lap("compiled simple_mint_debug");
    let simple_transfer_def = compile_simple_transfer()?;
    timer.lap("compiled simple_transfer");
    let simple_claim_def = compile_simple_claim()?;
    timer.lap("compiled simple_claim");

    let defs_array = [simple_mint_debug_def, simple_transfer_def, simple_claim_def];

    let contract_state_tree_height = GLOBAL_USER_TREE_HEIGHT as usize;

    let (result_circuits, deploy_cmd) =
        gen_contract_deploy_and_circuits_for_functions::<C, D>(deployer, contract_state_tree_height as u8, &defs_array)?;
    let mut result_circuits = result_circuits;
    timer.lap("finished building fn circuits");
    let [simple_mint_debug_def, simple_transfer_def, simple_claim_def] = defs_array;

    timer.lap("start: setup circuits");

    let simple_claim_circuit = result_circuits.pop().unwrap();
    let simple_transfer_circuit = result_circuits.pop().unwrap();
    let simple_mint_debug_circuit = result_circuits.pop().unwrap();
    timer.lap("end: setup circuits");

    let funcs = vec![
        SimpleTestContractItem {
            circuit: simple_mint_debug_circuit,
            def: simple_mint_debug_def,
        },
        SimpleTestContractItem {
            circuit: simple_transfer_circuit,
            def: simple_transfer_def,
        },
        SimpleTestContractItem {
            circuit: simple_claim_circuit,
            def: simple_claim_def,
        },
    ];
    let stc = SimpleTestContract::<C, D>::new_with_items(funcs);

    Ok((stc, deploy_cmd))
}

pub fn gen_test_contract_2<C: GenericConfig<D>, const D: usize>(
    deployer: QHashOut<C::F>,
) -> anyhow::Result<(SimpleTestContract<C, D>, QBCDeployContract<C::F>)>
where
    C::Hasher: AlgebraicHasher<C::F> + MerkleZeroHasher<HashOut<C::F>>,
{
    let mut timer = DebugTimer::new("demo_user_proving_session");

    timer.lap("start");

    let mut artifact: Vec<DPNFunctionCircuitDefinition> =
        serde_json::from_str(&fs::read_to_string("./psy_dev_cli/basic_ups/target/basic_ups.json")?)?;
    let simple_claim_def = artifact.pop().unwrap();
    timer.lap("compiled simple_claim");
    let simple_transfer_def = artifact.pop().unwrap();
    timer.lap("compiled simple_transfer");
    let simple_mint_debug_def = artifact.pop().unwrap();
    timer.lap("compiled simple_mint_debug");

    let defs_array = [simple_mint_debug_def, simple_transfer_def, simple_claim_def];

    let contract_state_tree_height = GLOBAL_USER_TREE_HEIGHT as usize;

    let (result_circuits, deploy_cmd) = gen_contract_deploy_and_circuits_for_functions(deployer, contract_state_tree_height as u8, &defs_array)?;
    let mut result_circuits = result_circuits;
    timer.lap("finished building fn circuits");
    let [simple_mint_debug_def, simple_transfer_def, simple_claim_def] = defs_array;

    timer.lap("start: setup circuits");

    let simple_claim_circuit = result_circuits.pop().unwrap();
    let simple_transfer_circuit = result_circuits.pop().unwrap();
    let simple_mint_debug_circuit = result_circuits.pop().unwrap();
    timer.lap("end: setup circuits");

    let funcs = vec![
        SimpleTestContractItem {
            circuit: simple_mint_debug_circuit,
            def: simple_mint_debug_def,
        },
        SimpleTestContractItem {
            circuit: simple_transfer_circuit,
            def: simple_transfer_def,
        },
        SimpleTestContractItem {
            circuit: simple_claim_circuit,
            def: simple_claim_def,
        },
    ];
    let stc = SimpleTestContract::<C, D>::new_with_items(funcs);

    Ok((stc, deploy_cmd))
}