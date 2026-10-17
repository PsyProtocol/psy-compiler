use std::{path::PathBuf, sync::Arc};
use plonky2::{
    field::{goldilocks_field::GoldilocksField as F, types::{Field, PrimeField64}},
    hash::hash_types::HashOutTarget,
    iop::witness::{PartialWitness, WitnessWrite},
    plonk::{circuit_builder::CircuitBuilder, circuit_data::CircuitConfig, config::PoseidonGoldilocksConfig},
};
use psy_common::Graph;
use psy_dpn_circuit::vm::ops::SimpleDPNBuilder;
use psy_interpreter::interpret_virtual_files;
use psy_vm::dpn::vm::{def::DPNFunctionCircuitDefinition, exec::SimpleDPNExecutor};

fn compile_result(source: &str) -> anyhow::Result<DPNFunctionCircuitDefinition> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/uint_contract.psy");
    let mut graph = Graph::new();
    graph.add_node(path.clone());
    let compilation = interpret_virtual_files(None, vec!["main".into()], graph, vec![(path, Arc::from(source))])?;
    assert_eq!(compilation.compile_results.len(), 1);
    Ok(compilation.compile_results.into_iter().next().unwrap())
}

fn compile(source: &str) -> DPNFunctionCircuitDefinition {
    compile_result(source).unwrap()
}

fn prove(function: &DPNFunctionCircuitDefinition, inputs: &[u64], expected: Option<&[u64]>) {
    assert!(function.state_commands.is_empty());
    let mut builder = CircuitBuilder::<F, 2>::new(CircuitConfig::standard_recursion_config());
    let targets = builder.add_virtual_targets(inputs.len());
    let zero = builder.zero();
    let hash = HashOutTarget { elements: [zero; 4] };
    let mut circuit = SimpleDPNBuilder::new_with_contract_ctx(targets.clone(), zero, zero, zero, zero, zero, hash, hash);
    for definition in &function.definitions { circuit.process_var_def(&mut builder, definition); }
    for assertion in &function.assertions {
        builder.connect(circuit.resolve_target(assertion.left), circuit.resolve_target(assertion.right));
    }
    for output in &function.circuit_outputs { builder.register_public_input(circuit.resolve_target(*output)); }
    let gates = builder.num_gates();
    let data = builder.build::<PoseidonGoldilocksConfig>();
    let mut witness = PartialWitness::new();
    for (target, input) in targets.iter().zip(inputs) { witness.set_target(*target, F::from_canonical_u64(*input)).unwrap(); }
    let proof = data.prove(witness);
    let Some(expected) = expected else { assert!(proof.is_err(), "invalid integer input produced a proof"); return; };
    let proof = proof.unwrap();
    assert_eq!(proof.public_inputs.iter().map(|v| v.to_canonical_u64()).collect::<Vec<_>>(), expected);
    data.verify(proof).unwrap();
    let mut native = SimpleDPNExecutor::<F>::new();
    native.inputs = inputs.iter().map(|v| F::from_canonical_u64(*v)).collect();
    for definition in &function.definitions { native.process_var_def(definition); }
    for assertion in &function.assertions { assert_eq!(native.resolve_target(assertion.left), native.resolve_target(assertion.right)); }
    assert_eq!(native.resolve_targets(&function.circuit_outputs).iter().map(|v| v.to_canonical_u64()).collect::<Vec<_>>(), expected);
    println!("uint verified: gates={gates}, degree_bits={}", data.common.degree_bits());
}

#[test]
fn compiled_u256_add_proves_and_rejects_overflow_and_invalid_limb() {
    let function = compile("fn main(a: u256, b: u256) -> u256 { a + b }");
    let mut inputs = [0u64; 16];
    inputs[0] = u32::MAX as u64;
    inputs[8] = 1;
    prove(&function, &inputs, Some(&[0, 1, 0, 0, 0, 0, 0, 0]));
    inputs[..8].fill(u32::MAX as u64);
    prove(&function, &inputs, None);
    inputs[..8].fill(0);
    inputs[7] = 1u64 << 32;
    prove(&function, &inputs, None);
}

#[test]
fn u256_contextual_literals_and_constants_keep_all_limbs() {
    for source in [
        "fn main() -> u256 { 1 }",
        "fn id(value: u256) -> u256 { value } fn main() -> u256 { id(1) }",
        "fn main() -> u256 { let x: u256 = 111111; x }",
        "fn main() -> u256 { let value: u256 = 1 + 2; value }",
        "const VALUE: u256 = 1u256 + 2u256; fn main() -> u256 { VALUE }",
    ] {
        let function = compile(source);
        let expected = if source.contains("111111") { 111111 } else if source.contains("+ 2") { 3 } else { 1 };
        prove(&function, &[], Some(&[expected, 0, 0, 0, 0, 0, 0, 0]));
    }
    let overflow = compile_result("const VALUE: u256 = 0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffu256 + 1u256; fn main() -> u256 { VALUE }");
    let error = overflow.err().expect("overflowing u256 constant compiled successfully");
    assert!(format!("{error:?}").to_lowercase().contains("overflow"), "unexpected constant error: {error:?}");
}

#[test]
fn compiled_u256_multiply_checks_full_product() {
    let function = compile("fn main(a: u256, b: u256) -> u256 { a * b }");
    let mut inputs = [0u64; 16];
    inputs[0] = u32::MAX as u64;
    inputs[8] = u32::MAX as u64;
    prove(&function, &inputs, Some(&[1, u32::MAX as u64 - 1, 0, 0, 0, 0, 0, 0]));
    inputs[..8].fill(u32::MAX as u64);
    inputs[8] = 2;
    prove(&function, &inputs, None);
}

#[test]
fn compiled_u256_division_proves_full_width() {
    std::thread::Builder::new().stack_size(64 * 1024 * 1024).spawn(|| {
        let function = compile("fn main(a: u256, b: u256) -> (u256, u256) { (a / b, a % b) }");
        let mut inputs = [u32::MAX as u64; 16];
        inputs[8..].fill(0);
        inputs[15] = 1u64 << 31;
        let mut expected = [0u64; 16];
        expected[0] = 1;
        expected[8..15].fill(u32::MAX as u64);
        expected[15] = (1u64 << 31) - 1;
        prove(&function, &inputs, Some(&expected));
        inputs[8..].fill(0);
        prove(&function, &inputs, None);
    }).unwrap().join().unwrap();
}

#[test]
fn compiled_u256_shift_and_cast_preserve_high_bits() {
    let function = compile("fn main(a: u256, n: u32) -> u256 { (a << n) >> n }");
    let mut inputs = [0u64; 9];
    inputs[0] = 1;
    inputs[8] = 255;
    prove(&function, &inputs, Some(&[1, 0, 0, 0, 0, 0, 0, 0]));
    inputs[8] = 256;
    prove(&function, &inputs, Some(&[0; 8]));
    let function = compile("fn main(a: Felt) -> u256 { a as u256 }");
    prove(&function, &[0xffff_ffff_0000_0000], Some(&[0, u32::MAX as u64, 0, 0, 0, 0, 0, 0]));
}

#[test]
fn random_u64_graphs_match_independent_oracle() {
    random_uint_graphs_match_independent_oracle::<64, 1>("u64", "PSY_U64_GRAPH_SEED");
}

#[test]
fn random_u128_graphs_match_independent_oracle() {
    random_uint_graphs_match_independent_oracle::<128, 2>("u128", "PSY_U128_GRAPH_SEED");
}

#[test]
fn random_u256_graphs_match_independent_oracle() {
    random_uint_graphs_match_independent_oracle::<256, 4>("u256", "PSY_U256_GRAPH_SEED");
}

fn random_uint_graphs_match_independent_oracle<const BITS: usize, const LIMBS: usize>(ty: &str, seed_var: &str) {
    use ruint::Uint as Oracle;

    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }
    fn words<const BITS: usize, const LIMBS: usize>(value: Oracle<BITS, LIMBS>) -> Vec<u32> {
        (0..BITS / 32).map(|i| (value.as_limbs()[i / 2] >> (32 * (i % 2))) as u32).collect()
    }

    let seeds: Vec<u64> = match std::env::var(seed_var) {
        Ok(value) => vec![value.parse().expect("uint graph seed must be a u64")],
        Err(_) => (0..24).map(|i| 0x7532_3536_2026u64 ^ i).collect(),
    };
    for seed in seeds {
        let mut state = seed.max(1);
        let inputs: [Oracle<BITS, LIMBS>; 3] = std::array::from_fn(|_| match seed % 3 {
            0 => Oracle::<BITS, LIMBS>::from(next(&mut state) & 0xffff_ffff),
            1 => Oracle::<BITS, LIMBS>::from(1u64) << [BITS / 8, BITS * 3 / 8 - 1, BITS * 5 / 8 - 1, BITS * 7 / 8 - 1][(next(&mut state) % 4) as usize],
            _ => Oracle::<BITS, LIMBS>::from_limbs(std::array::from_fn(|_| next(&mut state))),
        });
        let distance = [0u32, 1, 31, 32, 63, (BITS / 2 - 1) as u32, (BITS - 1) as u32, BITS as u32, (BITS + 1) as u32][(seed as usize) % 9];
        let mut values = inputs.to_vec();
        let mut source = format!("fn main(a: {ty}, b: {ty}, c: {ty}, n: u32) -> {ty} {{ let v0 = a; let v1 = b; let v2 = c;");
        for step in 0..4 {
            let left = (next(&mut state) as usize) % values.len();
            let right = (next(&mut state) as usize) % values.len();
            let a = values[left];
            let b = values[right];
            let (value, expression) = match ((seed as usize) + step) % 8 {
                0 => { let (value, overflow) = a.overflowing_add(b); if overflow { (a ^ b, format!("v{left} ^ v{right}")) } else { (value, format!("v{left} + v{right}")) } },
                1 => { let (value, underflow) = a.overflowing_sub(b); if underflow { (a ^ b, format!("v{left} ^ v{right}")) } else { (value, format!("v{left} - v{right}")) } },
                2 => { let (value, overflow) = a.overflowing_mul(b); if overflow { (a ^ b, format!("v{left} ^ v{right}")) } else { (value, format!("v{left} * v{right}")) } },
                3 => (a & b, format!("v{left} & v{right}")),
                4 => (a | b, format!("v{left} | v{right}")),
                5 => (a ^ b, format!("v{left} ^ v{right}")),
                6 => (a.wrapping_shl(distance as usize), format!("v{left} << n")),
                _ => (a.wrapping_shr(distance as usize), format!("v{left} >> n")),
            };
            source.push_str(&format!(" let v{}: {ty} = {expression};", values.len()));
            values.push(value);
        }
        source.push_str(&format!(" v{} }}", values.len() - 1));
        let function = compile(&source);
        let mut input = inputs.into_iter().flat_map(|value| words(value).into_iter().map(u64::from)).collect::<Vec<_>>();
        input.push(distance as u64);
        let mut native = SimpleDPNExecutor::<F>::new();
        native.inputs = input.into_iter().map(F::from_canonical_u64).collect();
        for definition in &function.definitions { native.process_var_def(definition); }
        for assertion in &function.assertions {
            assert_eq!(native.resolve_target(assertion.left), native.resolve_target(assertion.right), "{ty} seed {seed}: {source}");
        }
        let got = native.resolve_targets(&function.circuit_outputs).iter().map(|v| v.to_canonical_u64()).collect::<Vec<_>>();
        let expected = words(*values.last().unwrap()).into_iter().map(u64::from).collect::<Vec<_>>();
        assert_eq!(got, expected, "{ty} seed {seed}: {source}");
        if seed == 0x7532_3536_2026u64 {
            let inputs = native.inputs.iter().map(|value| value.to_canonical_u64()).collect::<Vec<_>>();
            prove(&function, &inputs, Some(&expected));
        }
    }

    let division = compile(&format!("fn main(a: {ty}, b: {ty}) -> ({ty}, {ty}) {{ (a / b, a % b) }}"));
    for seed in 1..=8u64 {
        let mut state = seed ^ 0x6469_7672_656du64;
        let a = Oracle::<BITS, LIMBS>::from_limbs(std::array::from_fn(|_| next(&mut state)));
        let mut b = Oracle::<BITS, LIMBS>::from_limbs(std::array::from_fn(|_| next(&mut state)));
        if b == Oracle::<BITS, LIMBS>::ZERO { b = Oracle::<BITS, LIMBS>::from(1u64); }
        let input = words(a).into_iter().chain(words(b)).map(u64::from).collect::<Vec<_>>();
        let mut native = SimpleDPNExecutor::<F>::new();
        native.inputs = input.into_iter().map(F::from_canonical_u64).collect();
        for definition in &division.definitions { native.process_var_def(definition); }
        for assertion in &division.assertions {
            assert_eq!(native.resolve_target(assertion.left), native.resolve_target(assertion.right), "{ty} division seed {seed}");
        }
        let expected = words(a / b).into_iter().chain(words(a % b)).map(u64::from).collect::<Vec<_>>();
        let got = native.resolve_targets(&division.circuit_outputs).iter().map(|v| v.to_canonical_u64()).collect::<Vec<_>>();
        assert_eq!(got, expected, "{ty} division seed {seed}");
    }
}

// ---------------------------------------------------------------------------
// Width sweep: the same guarantees u256 has, at u64 and u128.
// ---------------------------------------------------------------------------

#[test]
fn compiled_u64_and_u128_add_prove_and_reject_overflow() {
    for (ty, limbs) in [("u64", 2usize), ("u128", 4)] {
        let function = compile(&format!("fn main(a: {ty}, b: {ty}) -> {ty} {{ a + b }}"));
        // a = 2^bits - 2, b = 1 → 2^bits - 1 (all-MAX limbs).
        let mut inputs = vec![u32::MAX as u64; limbs * 2];
        inputs[0] -= 1;
        inputs[limbs..].fill(0);
        inputs[limbs] = 1;
        let mut expected = vec![u32::MAX as u64; limbs];
        prove(&function, &inputs, Some(&expected));
        // a = 2^bits - 1, b = 1 → overflow.
        inputs[0] += 1;
        prove(&function, &inputs, None);
        // An out-of-range limb (2^32) must be rejected.
        inputs[..limbs].fill(0);
        inputs[0] = 1u64 << 32;
        prove(&function, &inputs, None);
    }
}

#[test]
fn compiled_u64_and_u128_literals_and_casts_keep_limbs() {
    for (ty, limbs) in [("u64", 2usize), ("u128", 4)] {
        let mut expected = vec![0u64; limbs];
        expected[0] = 1;
        let function = compile(&format!("fn main() -> {ty} {{ 1 }}"));
        prove(&function, &[], Some(&expected));
        let function = compile(&format!("fn main(a: Felt) -> {ty} {{ a as {ty} }}"));
        prove(&function, &[0xffff_ffff_0000_0000], Some(&[0, u32::MAX as u64].into_iter().chain(std::iter::repeat(0).take(limbs - 2)).collect::<Vec<_>>()));
    }
    let overflow = compile_result("const VALUE: u64 = 0xffffffffffffffffu64 + 1u64; fn main() -> u64 { VALUE }");
    assert!(overflow.err().map(|error| format!("{error:?}").to_lowercase().contains("overflow")).unwrap_or(false));
}

#[test]
fn compiled_u64_and_u128_shifts_drop_bits_past_the_width() {
    for (ty, limbs, edge) in [("u64", 2usize, 63u32), ("u128", 4, 127)] {
        let function = compile(&format!("fn main(a: {ty}, n: u32) -> {ty} {{ (a << n) >> n }}"));
        let mut inputs = vec![0u64; limbs + 1];
        inputs[0] = 1;
        *inputs.last_mut().unwrap() = edge as u64;
        let mut expected = vec![0u64; limbs];
        expected[0] = 1;
        prove(&function, &inputs, Some(&expected));
        *inputs.last_mut().unwrap() = (edge + 1) as u64;
        prove(&function, &inputs, Some(&vec![0u64; limbs]));
    }
}

#[test]
fn compiled_u128_division_proves_full_width() {
    std::thread::Builder::new().stack_size(64 * 1024 * 1024).spawn(|| {
        let function = compile("fn main(a: u128, b: u128) -> (u128, u128) { (a / b, a % b) }");
        // Mirror of the u256 full-width case: max dividend, divisor 2^127.
        let mut inputs = vec![u32::MAX as u64; 8];
        inputs[4..].fill(0);
        inputs[7] = 1u64 << 31;
        let mut expected = vec![0u64; 8];
        expected[0] = 1;
        expected[4..7].fill(u32::MAX as u64);
        expected[7] = (1u64 << 31) - 1;
        prove(&function, &inputs, Some(&expected));
        inputs[4..].fill(0);
        prove(&function, &inputs, None);
    }).unwrap().join().unwrap();
}

#[test]
fn u64_and_u128_division_matches_ruint_oracle() {
    use ruint::aliases::{U128 as Oracle128, U64 as Oracle64};

    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    for seed in 1..=6u64 {
        let mut state = seed ^ 0x7562_3536_7532u64;
        let a64 = Oracle64::from(next(&mut state) | 1);
        let b64 = Oracle64::from(next(&mut state) | 1);
        let function = compile("fn main(a: u64, b: u64) -> (u64, u64) { (a / b, a % b) }");
        // Native u32 inputs are consumed per limb and must each fit 32 bits.
        let word64 = |value: ruint::aliases::U64| [value.as_limbs()[0] as u32, (value.as_limbs()[0] >> 32) as u32].map(u64::from);
        let input = word64(a64).into_iter().chain(word64(b64)).collect::<Vec<_>>();
        let mut native = SimpleDPNExecutor::<F>::new();
        native.inputs = input.iter().map(|value| F::from_canonical_u64(*value)).collect();
        for definition in &function.definitions { native.process_var_def(definition); }
        let expected = word64(a64 / b64).into_iter().chain(word64(a64 % b64)).collect::<Vec<_>>();
        let got = native.resolve_targets(&function.circuit_outputs).iter().map(|v| v.to_canonical_u64()).collect::<Vec<_>>();
        assert_eq!(got, expected, "u64 division seed {seed}");

        let a128 = Oracle128::from_limbs([next(&mut state), next(&mut state) | 1]);
        let b128 = Oracle128::from_limbs([next(&mut state) | 1, next(&mut state)]);
        let b128 = if b128 == Oracle128::ZERO { Oracle128::from(1u64) } else { b128 };
        let function = compile("fn main(a: u128, b: u128) -> (u128, u128) { (a / b, a % b) }");
        let words = |value: Oracle128| [value.as_limbs()[0], value.as_limbs()[1]].iter().flat_map(|limb| [*limb as u32, (*limb >> 32) as u32]).map(u64::from).collect::<Vec<u64>>();
        let input = words(a128).into_iter().chain(words(b128)).collect::<Vec<_>>();
        let mut native = SimpleDPNExecutor::<F>::new();
        native.inputs = input.iter().map(|value| F::from_canonical_u64(*value)).collect();
        for definition in &function.definitions { native.process_var_def(definition); }
        let expected = words(a128 / b128).into_iter().chain(words(a128 % b128)).collect::<Vec<_>>();
        let got = native.resolve_targets(&function.circuit_outputs).iter().map(|v| v.to_canonical_u64()).collect::<Vec<_>>();
        assert_eq!(got, expected, "u128 division seed {seed}");
    }
}
