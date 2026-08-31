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
COUNTER_CONTRACT_PATH := $(PWD)/psy-precompiles/counter

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
	@$(DARGO_CLI_COMPILE) rewards.psy --contract-name=ContractRef --method-names batch_claim_pm_rewards simple_mint simple_burn simple_transfer simple_claim

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
	@$(DARGO_CLI_EXECUTE) array_ref_multiple_fields_offset_test.psy
	@$(DARGO_CLI_TEST) tests/imt_intrinsic_test.psy

	@RUST_LOG=${LOG_LEVEL} cargo test --profile ${PROFILE} \
	       --package psy-ast \
	       --package psy-parser \
	       -- \
	       --nocapture

	@RUST_LOG=${LOG_LEVEL} cargo test --profile ${PROFILE} \
	       --package dargo \
	       artifact_name_tests \
	       -- \
	       --nocapture

	@RUST_LOG=${LOG_LEVEL} cargo test --profile ${PROFILE} \
	       --package psy_compiler_common \
	       --package psy-sema \
	       --package psy-interpreter \
	       --package psy-package \
	       -- \
	       --nocapture

	@RUST_LOG=${LOG_LEVEL} cargo test --profile ${PROFILE} \
	       --package psy-wasm \
	       compile_source_expands_type_aliases_in_abi_layout \
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
	@cd $(USDT_TOKEN_CONTRACT_PATH) && $(DARGO) generate-abi -c USDTTokenContractRef --abi-name usdt_token --method-names withdraw claim_deposit simple_mint simple_transfer simple_claim batch_simple_transfer_2 batch_simple_transfer_5 simple_burn simple_claim_pow_rewards private_transfer private_claim

compile-token-abi:
	@cd $(TOKEN_CONTRACT_PATH) && $(DARGO) generate-abi -c PsyTokenContractRef --abi-name token --method-names withdraw claim_deposit simple_mint simple_transfer simple_claim batch_simple_transfer_2 batch_simple_transfer_5 simple_burn simple_claim_pow_rewards private_transfer private_claim

compile-withdrawal-tree-contract:
	@cd $(WITHDRAWAL_TREE_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyWithdrawalTreeContractRef --method-names set_chain_root get_root get_chain_root append_leaf append_withdrawal batch_append_withdrawals_2 batch_append_withdrawals_5

compile-withdrawal-tree-abi:
	@cd $(WITHDRAWAL_TREE_CONTRACT_PATH) && $(DARGO) generate-abi -c PsyWithdrawalTreeContractRef --abi-name withdrawal_tree --method-names set_chain_root get_root get_chain_root append_leaf append_withdrawal batch_append_withdrawals_2 batch_append_withdrawals_5

compile-deposit-tree-contract:
	@cd $(DEPOSIT_TREE_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyDepositTreeContractRef --method-names get_root get_chain_root is_known_root_hash is_known_root set_chain_root append_leaf append_deposit batch_append_deposits_2 batch_append_deposits_5

compile-deposit-tree-abi:
	@cd $(DEPOSIT_TREE_CONTRACT_PATH) && $(DARGO) generate-abi -c PsyDepositTreeContractRef --abi-name deposit_tree --method-names get_root get_chain_root is_known_root_hash is_known_root set_chain_root append_leaf append_deposit batch_append_deposits_2 batch_append_deposits_5

compile-faucet-contract:
	@cd $(FAUCET_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyFaucetContractRef --method-names faucet

compile-faucet-contract-abi:
	@cd $(FAUCET_CONTRACT_PATH) && $(DARGO) generate-abi -c PsyFaucetContractRef --abi-name faucet --method-names faucet

compile-counter-contract:
	@cd $(COUNTER_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyCounterContractRef --method-names set_value increment

compile-counter-abi:
	@cd $(COUNTER_CONTRACT_PATH) && $(DARGO) generate-abi -c PsyCounterContractRef --abi-name counter --method-names set_value increment

PSY_GENESIS ?= $(PWD)/../psy-node/psy-genesis
GENESIS_CONTRACTS_FILE := $(PSY_GENESIS)/genesis_contracts.json
GENESIS_ABI_DIR := $(PSY_GENESIS)/genesis_abi
ABI_OUTPUT_DIR := $(PWD)/target/genesis_abi

# Compile all precompile contracts and the token update variant, generate the
# psy-genesis contract artifact, then regenerate ABI files into
# psy-genesis/genesis_abi/. psy-genesis owns all token artifacts.
gen-deploy-json: build \
	compile-usdt-token-contract compile-mining-rewards-contract \
	compile-deposit-tree-contract compile-withdrawal-tree-contract \
	compile-faucet-contract
	@cd $(TOKEN_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyTokenContractRef --method-names withdraw claim_deposit simple_transfer simple_claim batch_simple_transfer_2 batch_simple_transfer_5 simple_burn simple_claim_pow_rewards private_transfer private_claim
	@mv $(TOKEN_CONTRACT_PATH)/target/token.json $(TOKEN_CONTRACT_PATH)/target/token.update.json
	@cd $(TOKEN_CONTRACT_PATH) && $(DARGO) compile --contract-name=PsyTokenContractRef --method-names withdraw claim_deposit simple_mint simple_transfer simple_claim batch_simple_transfer_2 batch_simple_transfer_5 simple_burn simple_claim_pow_rewards private_transfer private_claim
	@mkdir -p $(PSY_GENESIS) $(GENESIS_ABI_DIR)
	@test -f $(TOKEN_CONTRACT_PATH)/target/token.json || { echo "error: missing compiled token artifact: $(TOKEN_CONTRACT_PATH)/target/token.json"; exit 1; }
	@test -f $(TOKEN_CONTRACT_PATH)/target/token.update.json || { echo "error: missing compiled token update artifact: $(TOKEN_CONTRACT_PATH)/target/token.update.json"; exit 1; }
	@cp $(TOKEN_CONTRACT_PATH)/target/token.json $(PSY_GENESIS)/token.json
	@cp $(TOKEN_CONTRACT_PATH)/target/token.update.json $(PSY_GENESIS)/token.update.json
	@echo "refreshed $(PSY_GENESIS)/token.json and token.update.json from compiled token artifacts"
	@cargo run --release --package dargo --example gen_deploy_json -- \
		$(GENESIS_CONTRACTS_FILE) \
		$(TOKEN_CONTRACT_PATH)/target/token.json \
		$(MINING_REWARDS_CONTRACT_PATH)/target/mining_rewards.json \
		$(DEPOSIT_TREE_CONTRACT_PATH)/target/deposit_tree.json \
		$(WITHDRAWAL_TREE_CONTRACT_PATH)/target/withdrawal_tree.json \
		$(USDT_TOKEN_CONTRACT_PATH)/target/usdt_token.json \
		$(FAUCET_CONTRACT_PATH)/target/faucet.json
	@# Regenerate ABI files to a separate dir (avoids overwriting compiled .json)
	@mkdir -p $(ABI_OUTPUT_DIR)
	@cd $(TOKEN_CONTRACT_PATH) && $(DARGO) generate-abi -c PsyTokenContractRef --abi-name token --output-dir $(ABI_OUTPUT_DIR) --method-names withdraw claim_deposit simple_mint simple_transfer simple_claim batch_simple_transfer_2 batch_simple_transfer_5 simple_burn simple_claim_pow_rewards private_transfer private_claim
	@cd $(USDT_TOKEN_CONTRACT_PATH) && $(DARGO) generate-abi -c USDTTokenContractRef --abi-name usdt_token --output-dir $(ABI_OUTPUT_DIR) --method-names withdraw claim_deposit simple_mint simple_transfer simple_claim batch_simple_transfer_2 batch_simple_transfer_5 simple_burn simple_claim_pow_rewards private_transfer private_claim
	@cd $(MINING_REWARDS_CONTRACT_PATH) && $(DARGO) generate-abi -c PsyPOWMiningRewardsClaimContractRef --abi-name mining_rewards --output-dir $(ABI_OUTPUT_DIR) --method-names start_session end_session claim_guta_rewards_1 claim_guta_rewards_2 claim_guta_rewards_5
	@cd $(DEPOSIT_TREE_CONTRACT_PATH) && $(DARGO) generate-abi -c PsyDepositTreeContractRef --abi-name deposit_tree --output-dir $(ABI_OUTPUT_DIR) --method-names get_root get_chain_root is_known_root_hash is_known_root set_chain_root append_leaf append_deposit batch_append_deposits_2 batch_append_deposits_5
	@cd $(WITHDRAWAL_TREE_CONTRACT_PATH) && $(DARGO) generate-abi -c PsyWithdrawalTreeContractRef --abi-name withdrawal_tree --output-dir $(ABI_OUTPUT_DIR) --method-names get_root get_chain_root append_leaf append_withdrawal batch_append_withdrawals_2 batch_append_withdrawals_5
	@cd $(FAUCET_CONTRACT_PATH) && $(DARGO) generate-abi -c PsyFaucetContractRef --abi-name faucet --output-dir $(ABI_OUTPUT_DIR) --method-names faucet
	@# `compile` does not emit .abi.json — always source it from the freshly generated $(ABI_OUTPUT_DIR)
	@cargo run --release --package dargo --example gen_deploy_abi_json -- \
		$(GENESIS_ABI_DIR) \
		$(ABI_OUTPUT_DIR)/token.abi.json:token \
		$(ABI_OUTPUT_DIR)/mining_rewards.abi.json:mining_rewards \
		$(ABI_OUTPUT_DIR)/deposit_tree.abi.json:deposit_tree \
		$(ABI_OUTPUT_DIR)/withdrawal_tree.abi.json:withdrawal_tree \
		$(ABI_OUTPUT_DIR)/usdt_token.abi.json:usdt \
		$(ABI_OUTPUT_DIR)/faucet.abi.json:faucet
	@node -e 'const fs=require("fs"),cp=require("child_process"),crypto=require("crypto"),path=require("path"); const compilerRevision=cp.execFileSync("git",["rev-parse","HEAD"],{encoding:"utf8"}).trim(); const isSource=relativePath=>{const normalized=relativePath.replace(/\\/g,"/"); const lower=normalized.toLowerCase(); if(normalized===".compiler-artifact.json")return false; if(["Cargo.toml","Cargo.lock","rust-toolchain.toml","Makefile"].includes(normalized))return true; if(["build.rs","precompiles.json","package.json"].includes(path.posix.basename(normalized)))return true; return [".rs",".psy",".toml",".lock"].some(extension=>lower.endsWith(extension));}; const entries=cp.execFileSync("git",["ls-tree","-r","--full-tree",compilerRevision],{encoding:"utf8"}).trim().split("\n").filter(Boolean).map(line=>{const match=line.match(/^[0-9]+ blob ([0-9a-f]+)\t(.+)$$/); if(!match)throw new Error(`invalid tree entry: $${line}`); return {blob:match[1],path:match[2]};}).filter(entry=>isSource(entry.path)).sort((a,b)=>Buffer.compare(Buffer.from(a.path),Buffer.from(b.path))); const hash=crypto.createHash("sha256"); for(const entry of entries){const blob=cp.execFileSync("git",["cat-file","blob",entry.blob]); hash.update(entry.path); hash.update("\0"); hash.update(blob); hash.update("\0");} const digest=file=>{const bytes=fs.readFileSync(file); return {sha256:crypto.createHash("sha256").update(bytes).digest("hex"),byteSize:bytes.length};}; const genesis=digest("$(GENESIS_CONTRACTS_FILE)"),token=digest("$(PSY_GENESIS)/token.json"),tokenUpdate=digest("$(PSY_GENESIS)/token.update.json"); const sidecar={compilerRevision,compilerSourcesHash:hash.digest("hex"),artifactSha256:genesis.sha256,artifactByteSize:genesis.byteSize,tokenArtifactSha256:token.sha256,tokenArtifactByteSize:token.byteSize,tokenUpdateArtifactSha256:tokenUpdate.sha256,tokenUpdateArtifactByteSize:tokenUpdate.byteSize}; fs.writeFileSync("$(PSY_GENESIS)/.genesis_contracts.compiler-artifact.json",JSON.stringify(sidecar,null,2)+"\n");'
	@echo "psy-genesis contract artifact + genesis_abi/ + compiler provenance regenerated"
