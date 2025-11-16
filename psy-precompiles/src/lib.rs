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
        assert_eq!(get_contract_id_by_name("rewards"), Some(1));
        assert_eq!(get_contract_id_by_name("nonexistent"), None);

        // Test function lookup by contract ID
        let token_functions = get_precompiled_contract_functions(0);
        let rewards_functions = get_precompiled_contract_functions(1);

        println!("Rewards functions available: {}", rewards_functions.is_some());
        println!("Token functions available: {}", token_functions.is_some());

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
        let simple_mint = get_precompiled_contract_function_by_name("token", "simple_mint");
        let batch_claim = get_precompiled_contract_function_by_name("rewards", "batch_claim_pm_rewards");

        if let Some(mint_func) = simple_mint {
            println!("Found simple_mint function with method_id: {}", mint_func.method_id);
            assert_eq!(mint_func.name, "simple_mint");
        }

        if let Some(claim_func) = batch_claim {
            println!("Found batch_claim_pm_rewards function with method_id: {}", claim_func.method_id);
            assert_eq!(claim_func.name, "batch_claim_pm_rewards");
        }

        // Test non-existent function
        let nonexistent = get_precompiled_contract_function_by_name("token", "nonexistent_method");
        assert!(nonexistent.is_none());
    }

    #[test]
    fn test_list_operations() {
        let contracts = list_available_contracts();
        println!("Available contracts: {:?}", contracts);
        assert!(contracts.contains(&"rewards".to_string()));
        assert!(contracts.contains(&"token".to_string()));

        let rewards_methods = list_contract_methods("rewards");
        let token_methods = list_contract_methods("token");

        println!("Rewards methods: {:?}", rewards_methods);
        println!("Token methods: {:?}", token_methods);

        // These should contain the methods defined in config.json
        if !rewards_methods.is_empty() {
            assert!(rewards_methods.contains(&"simple_mint".to_string()));
        }

        if !token_methods.is_empty() {
            assert!(token_methods.contains(&"simple_mint".to_string()));
        }
    }

    #[test]
    fn test_api_performance() {
        use std::time::Instant;

        let start = Instant::now();

        // These operations should be very fast since they use pre-computed lookups
        for _ in 0..1000 {
            let _ = get_contract_id_by_name("rewards");
            let _ = get_precompiled_contract_functions(0);
            let _ = get_precompiled_contract_function_by_name("token", "simple_mint");
        }

        let elapsed = start.elapsed();
        println!("1000 API calls took: {:?}", elapsed);

        // These should be very fast - contract IDs use match statements,
        // function lookups use OnceLock for lazy loading
        assert!(elapsed.as_secs() < 1, "API calls should be very fast");
    }
}
