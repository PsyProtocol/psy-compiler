PROFILE := release
LOG_LEVEL := dargo=info

export DARGO_STD_PATH := $(PWD)/psy-std/std.psy

check:
	@cargo check --workspace --all-targets --tests --benches --examples --bins

fix:
	# @cargo machete --fix
	@cargo fix --all-targets --allow-dirty --allow-staged

install:
	@cargo install --path psy-dargo-cli --locked
	@cargo install --path psy-lsp-server --locked

build:
	@RUSTFLAGS="-A warnings" cargo build --profile ${PROFILE} -p psy-precompiles
	@RUSTFLAGS="-A warnings" cargo build --profile ${PROFILE} --bin dargo --bin psy-lsp-server

clean:
	@rm -r target

DARGO_CLI_COMPILE = RUST_LOG=$(LOG_LEVEL) ./target/${PROFILE}/dargo compile --program-dir tests --debug --entry-path
DARGO_CLI_EXECUTE = RUST_LOG=${LOG_LEVEL} ./target/${PROFILE}/dargo execute --program-dir tests --debug --entry-path
DARGO_CLI_TEST    = RUST_LOG=${LOG_LEVEL} ./target/${PROFILE}/dargo test --file

DARGO = RUST_LOG=$(LOG_LEVEL) $(PWD)/target/${PROFILE}/dargo
USDT_TOKEN_CONTRACT_PATH := $(PWD)/psy-precompiles/usdt_token
TOKEN_CONTRACT_PATH := $(PWD)/psy-precompiles/token
MINING_REWARDS_CONTRACT_PATH := $(PWD)/psy-precompiles/mining_rewards
WITHDRAWAL_TREE_CONTRACT_PATH := $(PWD)/psy-precompiles/withdrawal_tree
DEPOSIT_TREE_CONTRACT_PATH := $(PWD)/psy-precompiles/deposit_tree
FAUCET_CONTRACT_PATH := $(PWD)/psy-precompiles/faucet

ci:
	# @$(DARGO_CLI_TEST) tests/in_mod_attr_test.psy
	# @$(DARGO_CLI_TEST) tests/should_panic_test.psy
	@$(DARGO_CLI_TEST) tests/for_if_test.psy
	@$(DARGO_CLI_TEST) tests/array_struct_modification_test.psy
	@$(DARGO_CLI_TEST) tests/conditional_assert_test.psy
	@$(DARGO_CLI_TEST) tests/guta_nullifier_calculation_test.psy
	@$(DARGO_CLI_TEST) tests/root_calculation_test.psy
	@$(DARGO_CLI_TEST) tests/imt_single_map_prove_test.psy
	@$(DARGO_CLI_TEST) tests/imt_ordering_prove_test.psy
	@$(DARGO_CLI_TEST) tests/imt_post_map_field_prove_test.psy

	@$(DARGO_CLI_COMPILE) ctx_test.psy
	@$(DARGO_CLI_COMPILE) storage_test.psy --contract-name=SimpleContract --method-names set_a set_b set_c set_d get_a get_b get_c get_d
	@$(DARGO_CLI_COMPILE) basic_ups.psy --contract-name=Contract --method-names simple_mint simple_transfer simple_claim
	@$(DARGO_CLI_COMPILE) token.psy --contract-name=ContractRef --method-names simple_mint simple_transfer simple_claim
	@$(DARGO_CLI_COMPILE) two_user_ups.psy --contract-name=Contract --method-names simple_mint simple_transfer simple_claim

	@$(DARGO_CLI_EXECUTE) assert_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) keccak256_test.psy
	@$(DARGO_CLI_EXECUTE) ctx_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) inline_module_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) opcode_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) parameter_passing_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) pub_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) return_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) self_test.psy
	@$(DARGO_CLI_EXECUTE) storage_test.psy
	@$(DARGO_CLI_EXECUTE) storage_ref_test.psy
	@$(DARGO_CLI_EXECUTE) ref_type_attr_test.psy
	@$(DARGO_CLI_EXECUTE) ref_type_generic_test.psy
	@$(DARGO_CLI_EXECUTE) ref_type_nested_eq_assign_test.psy
	@$(DARGO_CLI_EXECUTE) ref_type_deep_chain_update_test.psy
	@$(DARGO_CLI_EXECUTE) associated_type_path_test.psy
	@$(DARGO_CLI_EXECUTE) trait_default_associated_type_test.psy
	@$(DARGO_CLI_EXECUTE) storage_ref_type_annotation_test.psy
	@$(DARGO_CLI_EXECUTE) ref_struct_eq_assign_test.psy
	@$(DARGO_CLI_EXECUTE) storage_u32_assign_ops_test.psy
	@$(DARGO_CLI_EXECUTE) storage_ref_index_write_sugar_test.psy
	@$(DARGO_CLI_EXECUTE) storage_ref_bracket_sugar_test.psy
	@$(DARGO_CLI_EXECUTE) eq_trait_storage_ref_test.psy
	@$(DARGO_CLI_EXECUTE) array_ref_struct_index_test.psy
	@$(DARGO_CLI_EXECUTE) array_ref_struct_bulk_assign_test.psy
	@$(DARGO_CLI_EXECUTE) storage_ref_alias_consistency_test.psy
	@$(DARGO_CLI_EXECUTE) storage_derive_only_ref_generation_test.psy
	@$(DARGO_CLI_EXECUTE) trait_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) member_call_expected_signature_test.psy
	@$(DARGO_CLI_EXECUTE) member_call_expected_signature_array_arg_test.psy
	@$(DARGO_CLI_EXECUTE) member_call_expected_signature_arity_test.psy
	@$(DARGO_CLI_EXECUTE) member_call_expected_signature_generic_receiver_test.psy
	@$(DARGO_CLI_EXECUTE) member_call_expected_signature_function_arg_test.psy
	@$(DARGO_CLI_EXECUTE) member_access_vs_member_call_test.psy
	@$(DARGO_CLI_EXECUTE) nested_array_trait_impl_test.psy
	@$(DARGO_CLI_EXECUTE) hash_test.psy
	@$(DARGO_CLI_EXECUTE) hash_two_to_one_test.psy
	@$(DARGO_CLI_EXECUTE) verify_proof_test.psy
	@$(DARGO_CLI_EXECUTE) first_class_function_test.psy
	@$(DARGO_CLI_EXECUTE) type_alias_test.psy
	@$(DARGO_CLI_EXECUTE) const_test.psy --parameters 1
	@$(DARGO_CLI_EXECUTE) while_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) for_test.psy
	@$(DARGO_CLI_EXECUTE) lambda_test.psy
	@$(DARGO_CLI_EXECUTE) generics_test.psy
	@$(DARGO_CLI_EXECUTE) polymorphism.psy
	@$(DARGO_CLI_EXECUTE) type_hint_test.psy
	@$(DARGO_CLI_EXECUTE) exp_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) array_test.psy --parameters 1,1
	@$(DARGO_CLI_EXECUTE) u32_test.psy --parameters 2,3
	# @$(DARGO_CLI_EXECUTE) enum_test.psy
	@$(DARGO_CLI_EXECUTE) tuple_test.psy
	@$(DARGO_CLI_EXECUTE) ambiguity_test.psy
	@$(DARGO_CLI_EXECUTE) match_test.psy --parameters 100
	@$(DARGO_CLI_EXECUTE) if_test.psy
	@$(DARGO_CLI_EXECUTE) block_test.psy
	@$(DARGO_CLI_EXECUTE) path_test.psy
	@$(DARGO_CLI_EXECUTE) should_panic_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) basic_ups.psy --contract-name=Contract --method-names=simple_mint --method-names=simple_transfer --parameters 133700 --parameters 2,1000
	@$(DARGO_CLI_EXECUTE) basic_ups.psy --contract-name=Contract --method-names=simple_mint --method-names=simple_transfer --parameters 1000 --parameters=2,100
	@$(DARGO_CLI_EXECUTE) token.psy --contract-name=ContractRef --method-names=simple_mint --method-names=simple_transfer --parameters 1000 --parameters 2,100
	@$(DARGO_CLI_EXECUTE) two_user_ups.psy --contract-name=Contract --method-names=simple_mint --method-names=simple_transfer --parameters 1000 --parameters 2,100
	@$(DARGO_CLI_EXECUTE) check_secp_sign_test.psy
	@$(DARGO_CLI_EXECUTE) clear_entire_tree_test.psy
	@$(DARGO_CLI_TEST) tests/imt_intrinsic_test.psy

	@RUST_LOG=${LOG_LEVEL} cargo test --profile ${PROFILE} \
	       --package psy-ast \
	       --package psy-parser \
	       -- \
	       --nocapture

	@RUST_LOG=${LOG_LEVEL} cargo test --profile ${PROFILE} \
	       --package psy-sema \
	       --package psy-interpreter \
	       -- \
	       --nocapture

update-snapshots:
	@cargo insta review

wasm-node:
	@wasm-pack build psy-wasm --target nodejs --${PROFILE} --out-dir pkg-node

wasm-web:
	@wasm-pack build psy-wasm --target web --${PROFILE} --out-dir pkg-web

wasm: wasm-node wasm-web

.PHONY: check fix build format update-snapshots wasm wasm-node wasm-web

FILE                     := $(PWD)/tests/opcode_test.psy
PARAMETERS               := 1,2

interpret:
	@RUST_LOG=${LOG_LEVEL} ./target/${PROFILE}/dargo execute --program-dir $(dir ${FILE}) --debug --entry-path $(notdir ${FILE}) --parameters ${PARAMETERS}

compile-token-contract:
	@cd $(TOKEN_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyTokenContractRef --method-names withdraw claim_deposit simple_mint simple_transfer simple_claim batch_simple_transfer_2 batch_simple_transfer_5 simple_burn simple_claim_pow_rewards private_transfer private_claim

compile-mining-rewards-contract:
	@cd $(MINING_REWARDS_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyPOWMiningRewardsClaimContractRef --method-names start_session end_session claim_guta_rewards_1 claim_guta_rewards_2 claim_guta_rewards_5

compile-usdt-token-contract:
	@cd $(USDT_TOKEN_CONTRACT_PATH) && $(DARGO) compile --contract-name=USDTTokenContractRef --method-names withdraw claim_deposit simple_mint simple_transfer simple_claim batch_simple_transfer_2 batch_simple_transfer_5 simple_burn simple_claim_pow_rewards private_transfer private_claim

compile-usdt-token-abi:
	@cd $(USDT_TOKEN_CONTRACT_PATH) && $(DARGO) generate-abi -c usdt_token.abi

compile-token-abi:
	@cd $(TOKEN_CONTRACT_PATH) && $(DARGO) generate-abi -c token.abi

compile-withdrawal-tree-contract:
	@cd $(WITHDRAWAL_TREE_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyWithdrawalTreeContractRef --method-names get_root get_chain_root append_leaf append_withdrawal batch_append_withdrawals_2 batch_append_withdrawals_5

compile-withdrawal-tree-abi:
	@cd $(WITHDRAWAL_TREE_CONTRACT_PATH) && $(DARGO) generate-abi -c withdrawal_tree.abi

compile-deposit-tree-contract:
	@cd $(DEPOSIT_TREE_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyDepositTreeContractRef --method-names get_root get_chain_root append_leaf append_deposit batch_append_deposits_2 batch_append_deposits_5 is_known_root is_known_root_hash

compile-deposit-tree-abi:
	@cd $(DEPOSIT_TREE_CONTRACT_PATH) && $(DARGO) generate-abi -c deposit_tree.abi

compile-faucet-contract:
	@cd $(FAUCET_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyFaucetContractRef --method-names faucet

compile-faucet-contract-abi:
	@cd $(FAUCET_CONTRACT_PATH) && $(DARGO) generate-abi -c faucet.abi

PARTH_GENERIC_V1 ?= $(PWD)/../parth-generic-v1

gen-deploy-json: build
	@cargo run --release --package dargo --example gen_deploy_json -- \
		$(PARTH_GENERIC_V1)/genesis_contracts.json \
		$(TOKEN_CONTRACT_PATH)/target/token.json \
		$(MINING_REWARDS_CONTRACT_PATH)/target/mining_rewards.json \
		$(DEPOSIT_TREE_CONTRACT_PATH)/target/deposit_tree.json \
		$(WITHDRAWAL_TREE_CONTRACT_PATH)/target/withdrawal_tree.json \
		$(USDT_TOKEN_CONTRACT_PATH)/target/usdt_token.json \
		$(FAUCET_CONTRACT_PATH)/target/faucet.json
