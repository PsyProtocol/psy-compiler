// Include the generated precompile API - optimized for contract ID lookups
include!(concat!(env!("OUT_DIR"), "/precompile_api.rs"));

// Include the generated precompiled contract constants
include!(concat!(env!("OUT_DIR"), "/precompiled_contracts.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    use psy_vm::dpn::eval::executor::{ExecutionContext, InMemoryStateBackend, StateBackend, VmExecutor};

    const USER_ID: u64 = 524288;
    const MEMBERS: [[u64; 4]; 3] = [[1, 0, 0, 0], [1, 0, 0, 1], [0xffff_ffff_0000_0000, 0, 0, 0]];

    fn policy_state(header: [u64; 4], members: [[u64; 4]; 3]) -> InMemoryStateBackend {
        let mut state = InMemoryStateBackend::new();
        state.set_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, 0, header);
        for (index, member) in members.into_iter().enumerate() {
            state.set_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, index as u64 + 1, member);
        }
        state
    }

    fn set_policy(state: InMemoryStateBackend, expected: [u64; 4], members: [[u64; 4]; 3], next: [[u64; 4]; 3]) -> (bool, InMemoryStateBackend) {
        let context = ExecutionContext {
            user_id: USER_ID, contract_id: MULTISIG_POLICY_CONTRACT_ID as u64, caller_contract_id: MULTISIG_POLICY_CONTRACT_ID as u64,
            checkpoint_id: 1, nonce: 1, user_public_key_hash: [0; 4],
            session_proof_tree_root: [0; 4],
        };
        let inputs: Vec<u64> = expected.into_iter().chain(members.into_iter().flatten()).chain(next.into_iter().flatten()).collect();
        let mut executor = VmExecutor::new(state.clone());
        let function = get_precompiled_contract_function_by_name("multisig_policy", "set_policy").unwrap();
        let result = executor.execute(function, &context, &inputs).unwrap();
        let mut ending = state;
        if result.success {
            ending.apply_overlay(executor.write_overlay());
        }
        (result.success, ending)
    }

    #[test]
    fn policy_bootstrap_rotation_and_stale_cas() {
        assert_eq!(MULTISIG_POLICY_CONTRACT_ID, psy_vm::ups::multisig::MULTISIG_POLICY_CONTRACT_ID);
        let (success, state) = set_policy(InMemoryStateBackend::new(), [0; 4], [[0; 4]; 3], MEMBERS);
        assert!(success);
        assert_eq!(state.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, 0).unwrap(), [1, 2, 3, 0]);
        for (index, member) in MEMBERS.into_iter().enumerate() {
            assert_eq!(state.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, index as u64 + 1).unwrap(), member);
        }
        for slot in 4..16 {
            assert_eq!(state.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, slot).unwrap(), [0; 4]);
        }
        let context = ExecutionContext {
            user_id: USER_ID, contract_id: MULTISIG_POLICY_CONTRACT_ID as u64, caller_contract_id: MULTISIG_POLICY_CONTRACT_ID as u64,
            checkpoint_id: 1, nonce: 1, user_public_key_hash: [0; 4],
            session_proof_tree_root: [0; 4],
        };
        let mut reader = VmExecutor::new(state.clone());
        let result = reader.execute(get_precompiled_contract_function_by_name("multisig_policy", "get_policy").unwrap(), &context, &[]).unwrap();
        assert!(result.success);
        assert_eq!(result.outputs, [1, 2, 3, 0].into_iter().chain(MEMBERS.into_iter().flatten()).collect::<Vec<_>>());
        assert!(result.state_writes.is_empty());
        let mut next = MEMBERS;
        next[1][3] = 2;
        let (success, rotated) = set_policy(state.clone(), [1, 2, 3, 0], MEMBERS, next);
        assert!(success);
        assert_eq!(rotated.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, 0).unwrap(), [2, 2, 3, 0]);
        assert_eq!(rotated.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, 2).unwrap(), next[1]);
        let last_header = [u32::MAX as u64 - 1, 2, 3, 0];
        let (success, last) = set_policy(policy_state(last_header, MEMBERS), last_header, MEMBERS, next);
        assert!(success);
        assert_eq!(last.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, 0).unwrap(), [u32::MAX as u64, 2, 3, 0]);
        assert!(!set_policy(rotated, [1, 2, 3, 0], MEMBERS, MEMBERS).0);
        let mut stale_members = MEMBERS;
        stale_members[0][0] = 2;
        assert!(!set_policy(state, [1, 2, 3, 0], stale_members, next).0);
    }

    #[test]
    fn policy_preserves_all_member_permutations() {
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            let members = order.map(|index| MEMBERS[index]);
            let (success, state) = set_policy(InMemoryStateBackend::new(), [0; 4], [[0; 4]; 3], members);
            assert!(success);
            assert_eq!(state.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, 0).unwrap(), [1, 2, 3, 0]);
            for (index, member) in members.into_iter().enumerate() {
                assert_eq!(state.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, index as u64 + 1).unwrap(), member);
            }
            for slot in 4..16 {
                assert_eq!(state.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, slot).unwrap(), [0; 4]);
            }
            let mut next = members;
            next.swap(0, 2);
            assert!(!set_policy(state.clone(), [1, 2, 3, 0], next, next).0);
            let (success, rotated) = set_policy(state, [1, 2, 3, 0], members, next);
            assert!(success);
            assert_eq!(rotated.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, 0).unwrap(), [2, 2, 3, 0]);
            for (index, member) in next.into_iter().enumerate() {
                assert_eq!(rotated.get_contract_hash(USER_ID, MULTISIG_POLICY_CONTRACT_ID as u64, index as u64 + 1).unwrap(), member);
            }
        }
    }

    #[test]
    fn policy_rejects_invalid_members_and_versions() {
        let header = [1, 2, 3, 0];
        let state = policy_state(header, MEMBERS);
        assert!(!set_policy(state.clone(), header, MEMBERS, MEMBERS).0);
        for index in 0..3 {
            let mut zero = MEMBERS;
            zero[index] = [0; 4];
            assert!(!set_policy(state.clone(), header, MEMBERS, zero).0);
        }
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            let mut duplicate = MEMBERS;
            duplicate[b] = duplicate[a];
            assert!(!set_policy(state.clone(), header, MEMBERS, duplicate).0);
        }
        let mut noncanonical = MEMBERS;
        noncanonical[2][0] = 0xffff_ffff_0000_0001;
        assert!(!set_policy(state.clone(), header, MEMBERS, noncanonical).0);
        let mut next = MEMBERS;
        next[1][3] = 2;
        for invalid in [[0, 2, 3, 0], [1, 0, 3, 0], [1, 1, 3, 0], [1, 3, 3, 0], [1, 2, 4, 0], [1, 2, 3, 1], [u32::MAX as u64, 2, 3, 0], [1u64 << 32, 2, 3, 0], [0; 4]] {
            assert!(!set_policy(policy_state(invalid, MEMBERS), invalid, MEMBERS, next).0);
        }
    }

    #[test]
    fn policy_artifact_and_abi_agree() {
        let artifact: serde_json::Value = serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/multisig_policy.json"))).unwrap();
        let abi: serde_json::Value = serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/multisig_policy.abi.json"))).unwrap();
        assert_eq!(artifact["state_tree_height"], 4);
        assert_eq!(abi["contract"]["state_tree_height"], 4);
        assert_eq!(artifact["abi"], abi);
        let mut methods = list_contract_methods("multisig_policy");
        methods.sort();
        assert_eq!(methods, ["get_policy", "set_policy"]);
    }

    #[test]
    fn test_contract_id_lookup() {
        // Test contract ID lookup by name
        assert_eq!(get_contract_id_by_name("token"), Some(0));
        assert_eq!(get_contract_id_by_name("multisig_policy"), Some(1));
        assert_eq!(get_contract_id_by_name("deposit_tree"), Some(2));
        assert_eq!(get_contract_id_by_name("withdrawal_tree"), Some(3));
        assert_eq!(get_contract_id_by_name("usdt_token"), Some(4));
        assert_eq!(get_contract_id_by_name("faucet"), Some(5));
        assert_eq!(get_contract_id_by_name("nonexistent"), None);

        // Test function lookup by contract ID
        let token_functions = get_precompiled_contract_functions(0);
        let policy_functions = get_precompiled_contract_functions(1);
        let usdt_functions = get_precompiled_contract_functions(4);
        let faucet_functions = get_precompiled_contract_functions(5);

        println!("Policy functions available: {}", policy_functions.is_some());
        println!("USDT functions available: {}", usdt_functions.is_some());
        println!("Token functions available: {}", token_functions.is_some());
        println!("Faucet functions available: {}", faucet_functions.is_some());

        if let Some(functions) = policy_functions {
            println!("Policy contract has {} functions", functions.len());
            for func in functions {
                println!("  - {} (method_id: {})", func.name, func.method_id);
            }
        }

        if let Some(functions) = token_functions {
            println!("Token contract has {} functions", functions.len());
            for func in functions {
                println!("  - {} (method_id: {})", func.name, func.method_id);
            }
        }
    }

    #[test]
    fn test_function_lookup_by_name() {
        // Test looking up specific functions by name
        let mint = get_precompiled_contract_function_by_name("token", "mint");
        let get_policy = get_precompiled_contract_function_by_name("multisig_policy", "get_policy");
        let set_policy = get_precompiled_contract_function_by_name("multisig_policy", "set_policy");
        let faucet = get_precompiled_contract_function_by_name("faucet", "faucet");

        if let Some(mint_func) = mint {
            println!("Found mint function with method_id: {}", mint_func.method_id);
            assert_eq!(mint_func.name, "mint");
        }

        if let Some(get_policy_func) = get_policy {
            println!("Found get_policy function with method_id: {}", get_policy_func.method_id);
            assert_eq!(get_policy_func.name, "get_policy");
        }
        assert!(set_policy.is_some());

        if let Some(faucet_func) = faucet {
            println!("Found faucet function with method_id: {}", faucet_func.method_id);
            assert_eq!(faucet_func.name, "faucet");
        }

        // Test non-existent function
        let nonexistent = get_precompiled_contract_function_by_name("token", "nonexistent_method");
        assert!(nonexistent.is_none());
    }

    #[test]
    fn test_list_operations() {
        let contracts = list_available_contracts();
        println!("Available contracts: {:?}", contracts);
        assert!(contracts.contains(&"multisig_policy".to_string()));
        assert!(contracts.contains(&"usdt_token".to_string()));
        assert!(contracts.contains(&"token".to_string()));
        assert!(contracts.contains(&"faucet".to_string()));

        let policy_methods = list_contract_methods("multisig_policy");
        let usdt_methods = list_contract_methods("usdt_token");
        let token_methods = list_contract_methods("token");
        let faucet_methods = list_contract_methods("faucet");

        println!("Policy methods: {:?}", policy_methods);
        println!("Token methods: {:?}", token_methods);
        println!("USDT methods: {:?}", usdt_methods);
        println!("Faucet methods: {:?}", faucet_methods);

        if !policy_methods.is_empty() {
            assert!(policy_methods.contains(&"get_policy".to_string()));
            assert!(policy_methods.contains(&"set_policy".to_string()));
            assert_eq!(policy_methods.len(), 2);
        }

        if !usdt_methods.is_empty() {
            assert!(usdt_methods.contains(&"mint".to_string()));
        }

        if !token_methods.is_empty() {
            assert!(token_methods.contains(&"mint".to_string()));
            assert!(!token_methods.contains(&"claim_pow_rewards".to_string()));
        }

        if !faucet_methods.is_empty() {
            assert!(faucet_methods.contains(&"faucet".to_string()));
        }
    }

    #[test]
    fn test_api_performance() {
        use std::time::Instant;

        let start = Instant::now();

        // These operations should be very fast since they use pre-computed lookups
        for _ in 0..1000 {
            let _ = get_contract_id_by_name("multisig_policy");
            let _ = get_precompiled_contract_functions(0);
            let _ = get_precompiled_contract_function_by_name("token", "mint");
        }

        let elapsed = start.elapsed();
        println!("1000 API calls took: {:?}", elapsed);

        // These should be very fast - contract IDs use match statements,
        // function lookups use OnceLock for lazy loading
        assert!(elapsed.as_secs() < 1, "API calls should be very fast");
    }
}
