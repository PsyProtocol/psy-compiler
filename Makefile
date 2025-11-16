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

ci:
	@$(DARGO_CLI_TEST) tests/in_mod_attr_test.psy
	# @$(DARGO_CLI_TEST) tests/should_panic_test.psy
	@$(DARGO_CLI_TEST) tests/for_if_test.psy
	@$(DARGO_CLI_TEST) tests/array_struct_modification_test.psy
	@$(DARGO_CLI_TEST) tests/conditional_assert_test.psy
	@$(DARGO_CLI_TEST) tests/guta_nullifier_calculation_test.psy
	@$(DARGO_CLI_TEST) tests/root_calculation_test.psy

	@$(DARGO_CLI_COMPILE) ctx_test.psy
	@$(DARGO_CLI_COMPILE) storage_test.psy --contract-name=SimpleContract --method-names set_a set_b set_c set_d get_a get_b get_c get_d
	@$(DARGO_CLI_COMPILE) basic_ups.psy --contract-name=Contract --method-names simple_mint simple_transfer simple_claim
	@$(DARGO_CLI_COMPILE) token.psy --contract-name=ContractRef --method-names simple_mint simple_transfer simple_claim
	@$(DARGO_CLI_COMPILE) two_user_ups.psy --contract-name=Contract --method-names simple_mint simple_transfer simple_claim

	@$(DARGO_CLI_EXECUTE) assert_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) ctx_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) inline_module_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) opcode_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) parameter_passing_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) pub_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) return_test.psy --parameters 2,3
	@$(DARGO_CLI_EXECUTE) self_test.psy
	@$(DARGO_CLI_EXECUTE) storage_test.psy
	@$(DARGO_CLI_EXECUTE) trait_test.psy --parameters 2,3
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

.PHONY: check fix build format update-snapshots

FILE                     := $(PWD)/tests/opcode_test.psy
PARAMETERS               := 1,2

interpret:
	@RUST_LOG=${LOG_LEVEL} ./target/${PROFILE}/dargo execute --program-dir $(dir ${FILE}) --debug --entry-path $(notdir ${FILE}) --parameters ${PARAMETERS}
