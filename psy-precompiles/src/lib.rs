// Include the generated precompile API - optimized for contract ID lookups
include!(concat!(env!("OUT_DIR"), "/precompile_api.rs"));

// Include the generated precompiled contract constants
include!(concat!(env!("OUT_DIR"), "/precompiled_contracts.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contract_id_lookup() {
        // Test contract ID lookup by name
        assert_eq!(get_contract_id_by_name("token"), Some(0));
        assert_eq!(get_contract_id_by_name("mining_rewards"), Some(1));
        assert_eq!(get_contract_id_by_name("deposit_tree"), Some(2));
        assert_eq!(get_contract_id_by_name("withdrawal_tree"), Some(3));
        assert_eq!(get_contract_id_by_name("usdt_token"), Some(4));
        assert_eq!(get_contract_id_by_name("faucet"), Some(5));
        assert_eq!(get_contract_id_by_name("nonexistent"), None);

        // Test function lookup by contract ID
        let token_functions = get_precompiled_contract_functions(0);
        let rewards_functions = get_precompiled_contract_functions(1);
        let usdt_functions = get_precompiled_contract_functions(4);
        let faucet_functions = get_precompiled_contract_functions(5);

        println!("Rewards functions available: {}", rewards_functions.is_some());
        println!("USDT functions available: {}", usdt_functions.is_some());
        println!("Token functions available: {}", token_functions.is_some());
        println!("Faucet functions available: {}", faucet_functions.is_some());

        if let Some(functions) = rewards_functions {
            println!("Rewards contract has {} functions", functions.len());
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
        let claim_pow_rewards = get_precompiled_contract_function_by_name("token", "claim_pow_rewards");
        let claim_reward = get_precompiled_contract_function_by_name("mining_rewards", "claim_guta_rewards_1");
        let claim_rewards_2 = get_precompiled_contract_function_by_name("mining_rewards", "claim_guta_rewards_2");
        let claim_rewards_5 = get_precompiled_contract_function_by_name("mining_rewards", "claim_guta_rewards_5");
        let claim_rewards_10 = get_precompiled_contract_function_by_name("mining_rewards", "claim_guta_rewards_10");
        let legacy_session = get_precompiled_contract_function_by_name("mining_rewards", "start_session");
        let faucet = get_precompiled_contract_function_by_name("faucet", "faucet");

        if let Some(mint_func) = mint {
            println!("Found mint function with method_id: {}", mint_func.method_id);
            assert_eq!(mint_func.name, "mint");
        }

        if let Some(claim_func) = claim_pow_rewards {
            println!("Found claim_pow_rewards function with method_id: {}", claim_func.method_id);
            assert_eq!(claim_func.name, "claim_pow_rewards");
        }

        if let Some(claim_reward_func) = claim_reward {
            println!("Found claim_guta_rewards_1 function with method_id: {}", claim_reward_func.method_id);
            assert_eq!(claim_reward_func.name, "claim_guta_rewards_1");
        }
        assert!(claim_rewards_2.is_some());
        assert!(claim_rewards_5.is_some());
        assert!(claim_rewards_10.is_some());
        assert!(legacy_session.is_none());

        for method_name in [
            "claim_guta_rewards_1",
            "claim_guta_rewards_2",
            "claim_guta_rewards_5",
            "claim_guta_rewards_10",
        ] {
            let function = get_precompiled_contract_function_by_name("mining_rewards", method_name)
                .unwrap_or_else(|| panic!("missing mining rewards method {method_name}"));
            let encoded = serde_json::to_string(function).expect("serialize compiled mining rewards method");
            assert!(
                encoded.contains("GetSelfUserCurrentContractStateSlotSingle"),
                "{method_name} must read the claimed_guta_nullifiers slot"
            );
            assert!(
                encoded.contains("SetContractStateSlotSingle"),
                "{method_name} must mark the claimed_guta_nullifier slot as used"
            );
            assert!(
                encoded.contains("reward already claimed"),
                "{method_name} must reject duplicate claims"
            );
            assert!(
                !encoded.contains("ContainsSelfUserCurrentIMTContractStateValue"),
                "{method_name} must not use the obsolete claimed_jobs Map"
            );
        }

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
        assert!(contracts.contains(&"mining_rewards".to_string()));
        assert!(contracts.contains(&"usdt_token".to_string()));
        assert!(contracts.contains(&"token".to_string()));
        assert!(contracts.contains(&"faucet".to_string()));

        let rewards_methods = list_contract_methods("mining_rewards");
        let usdt_methods = list_contract_methods("usdt_token");
        let token_methods = list_contract_methods("token");
        let faucet_methods = list_contract_methods("faucet");

        println!("Rewards methods: {:?}", rewards_methods);
        println!("Token methods: {:?}", token_methods);
        println!("USDT methods: {:?}", usdt_methods);
        println!("Faucet methods: {:?}", faucet_methods);

        // These should contain the methods defined in config.json
        if !rewards_methods.is_empty() {
            assert!(rewards_methods.contains(&"claim_guta_rewards_1".to_string()));
            assert!(rewards_methods.contains(&"claim_guta_rewards_2".to_string()));
            assert!(rewards_methods.contains(&"claim_guta_rewards_5".to_string()));
            assert!(rewards_methods.contains(&"claim_guta_rewards_10".to_string()));
            assert_eq!(rewards_methods.len(), 4);
        }

        if !usdt_methods.is_empty() {
            assert!(usdt_methods.contains(&"mint".to_string()));
        }

        if !token_methods.is_empty() {
            assert!(token_methods.contains(&"mint".to_string()));
            assert!(token_methods.contains(&"claim_pow_rewards".to_string()));
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
            let _ = get_contract_id_by_name("mining_rewards");
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
