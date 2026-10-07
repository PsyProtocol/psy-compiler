// Execution-layer tests for the interpreter.
//
// `const_eval_tests.rs` stops at typechecking; this suite drives the full
// `interpret` / `test` pipeline (symbolic execution of the function body with
// a stub compile function) so the statement/expression arms of
// `__interpret__`, `interpret_binary`, `interpret_unary`, assignment
// operators, loops, matches, intrinsics, and the `#[test]` runner are all
// exercised without paying for DPN proving.
//
// Contract entry-point discovery (auto-discovery, explicit `-c`/`-m`
// resolution, view-method enforcement) is covered with the real
// `PsyCompileResult::compile_exec` backend so state commands are visible.
//
// Each case owns an independent interpreter, symbol table, and uniquely named temporary file.

use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use psy_vm::dpn::{
    ops::{exec_context::QExecContext, sym_felt::SymFeltRef},
    vm::{compile::PsyCompileResult, def::DPNFunctionCircuitDefinition},
};

use super::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Compile-function stub: records the method name/id and folds the outputs to
/// constants. Keeps the suite fast — only the interpret layer is under test.
fn stub_compile_fn(
    _context: &QExecContext,
    (name, method_id, outputs): (String, u32, Vec<SymFeltRef>),
) -> DPNFunctionCircuitDefinition {
    DPNFunctionCircuitDefinition {
        name,
        method_id,
        circuit_inputs: vec![],
        circuit_outputs: outputs.iter().map(|felt| felt.get_constant_value()).collect(),
        state_commands: vec![],
        state_command_resolution_indices: vec![],
        assertions: vec![],
        definitions: vec![],
        events: vec![],
    }
}

/// The real compile backend (used for contract cases where the view-method
/// check must observe generated state commands).
fn real_compile_fn(
    context: &QExecContext,
    (name, method_id, outputs): (String, u32, Vec<SymFeltRef>),
) -> DPNFunctionCircuitDefinition {
    PsyCompileResult::compile_exec(name, method_id, &context.store, context, &outputs)
}

/// Outcome of typechecking + interpreting a source snippet.
#[derive(Debug)]
enum Outcome {
    /// Both typecheck and interpretation succeeded; holds the compiled defs.
    Executed(Vec<DPNFunctionCircuitDefinition>),
    /// Typecheck or interpretation returned a structured error.
    Failed(String),
    /// The pipeline panicked instead of yielding a clean error.
    Panicked(String),
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic>".to_string()
    }
}

fn temp_psy_path(label: &str) -> std::path::PathBuf {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("psy_exec_{label}_{unique}_{n}.psy"))
}

fn run_with(
    source: &str,
    label: &str,
    contract_name: Option<&str>,
    methods: &[&str],
    real_backend: bool,
) -> Outcome {
    let path = temp_psy_path(label);
    fs::write(&path, source).unwrap();
    let path_arg = path.clone();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (mut typechecker, mut ctx) = interpreter
            .typecheck_single(path_arg.clone())
            .map_err(|e| format!("typecheck: {e:#}"))?;
        let method_strings: Vec<String> = methods.iter().map(|s| s.to_string()).collect();
        if real_backend {
            interpreter.interpret(&mut typechecker, &mut ctx, contract_name.map(String::from), method_strings, real_compile_fn)
        } else {
            interpreter.interpret(&mut typechecker, &mut ctx, contract_name.map(String::from), method_strings, stub_compile_fn)
        }
        .map_err(|e| format!("interpret: {e:#}"))
    }));

    let _ = fs::remove_file(&path);

    match result {
        Ok(Ok(defs)) => Outcome::Executed(defs),
        Ok(Err(msg)) => Outcome::Failed(msg),
        Err(p) => Outcome::Panicked(panic_message(&p)),
    }
}

fn run(source: &str, label: &str) -> Outcome {
    run_with(source, label, None, &[], false)
}

/// Run the `#[test]`-attributed functions of a source through
/// `Interpreter::test`.
fn run_psy_tests(source: &str, label: &str) -> Outcome {
    let path = temp_psy_path(label);
    fs::write(&path, source).unwrap();
    let path_arg = path.clone();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (mut typechecker, mut ctx) = interpreter
            .typecheck_single(path_arg.clone())
            .map_err(|e| format!("typecheck: {e:#}"))?;
        interpreter.test(&mut typechecker, &mut ctx, stub_compile_fn).map_err(|e| format!("test: {e:#}"))
    }));

    let _ = fs::remove_file(&path);

    match result {
        Ok(Ok(defs)) => Outcome::Executed(defs),
        Ok(Err(msg)) => Outcome::Failed(msg),
        Err(p) => Outcome::Panicked(panic_message(&p)),
    }
}

fn expect_exec(label: &str, source: &str) {
    match run(source, label) {
        Outcome::Executed(_) => {}
        Outcome::Failed(msg) => panic!("[{label}] expected execution, got FAILURE:\n{msg}"),
        Outcome::Panicked(msg) => panic!("[{label}] expected execution, got PANIC:\n{msg}"),
    }
}

fn expect_failure(label: &str, source: &str, needle: &str) {
    expect_failure_with(label, source, None, &[], needle);
}

fn expect_failure_with(label: &str, source: &str, contract: Option<&str>, methods: &[&str], needle: &str) {
    match run_with(source, label, contract, methods, false) {
        Outcome::Executed(_) => panic!("[{label}] expected failure mentioning `{needle}`, got success"),
        Outcome::Failed(msg) => assert!(
            msg.to_lowercase().contains(&needle.to_lowercase()),
            "[{label}] expected failure mentioning `{needle}`, got:\n{msg}"
        ),
        Outcome::Panicked(msg) => panic!("[{label}] expected failure mentioning `{needle}`, got PANIC:\n{msg}"),
    }
}

fn expect_test_panic(label: &str, source: &str, needle: &str) {
    match run_psy_tests(source, label) {
        Outcome::Panicked(msg) => assert!(
            msg.contains(needle),
            "[{label}] expected panic mentioning `{needle}`, got:\n{msg}"
        ),
        other => panic!("[{label}] expected panic mentioning `{needle}`, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Binary operators
// ---------------------------------------------------------------------------

#[test]
fn felt_binary_operators_execute() {
    expect_exec(
        "felt_ops",
        r#"
        fn main() -> Felt {
            let a: Felt = 17;
            let b: Felt = 5;
            assert_eq(a + b, 22, "add");
            assert_eq(a - b, 12, "sub");
            assert_eq(a * b, 85, "mul");
            assert_eq(a / 1, 17, "div by one");
            assert_eq(a ** 2, 289, "pow");
            assert(a == 17, "eq");
            assert(a != b, "neq");
            assert(a > b, "gt");
            assert(a >= 17, "gte");
            assert(b < a, "lt");
            assert(b <= 5, "lte");
            assert_eq(1 << 4, 16, "shl");
            assert_eq(32 >> 2, 8, "shr");
            assert_eq(12 & 10, 8, "bitand");
            assert_eq(12 | 10, 14, "bitor");
            assert_eq(12 ^ 10, 6, "bitxor");
            return a + b;
        }
        "#,
    );
}

#[test]
fn u32_binary_operators_execute() {
    expect_exec(
        "u32_ops",
        r#"
        fn main() -> u32 {
            let a: u32 = 17u32;
            let b: u32 = 5u32;
            assert_eq(a + b, 22u32, "add");
            assert_eq(a - b, 12u32, "sub");
            assert_eq(a * b, 85u32, "mul");
            assert_eq(a / b, 3u32, "div");
            assert_eq(a % b, 2u32, "mod");
            assert_eq(2u32 ** 3u32, 8u32, "pow");
            assert(a < b == false, "lt");
            assert(a <= 17u32, "lte");
            assert(a > b, "gt");
            assert(a >= b, "gte");
            assert_eq(1u32 << 4u32, 16u32, "shl");
            assert_eq(32u32 >> 2u32, 8u32, "shr");
            assert_eq(12u32 & 10u32, 8u32, "bitand");
            assert_eq(12u32 | 10u32, 14u32, "bitor");
            assert_eq(12u32 ^ 10u32, 6u32, "bitxor");
            assert_eq(12u32 ^ 10u32, 6u32, "xor twice");
            return a % b;
        }
        "#,
    );
}

#[test]
fn bool_logic_operators_execute() {
    expect_exec(
        "bool_ops",
        r#"
        fn main() -> bool {
            let t: bool = true;
            let f: bool = false;
            assert(t && t, "and tt");
            assert(!(t && f), "and tf");
            assert(t || f, "or tf");
            assert(!(f || f), "or ff");
            assert(t == !f, "eq via not");
            assert(t != f, "neq");
            assert(t ^ f, "xor tf");
            assert(!(t ^ t), "xor tt");
            return t && !f;
        }
        "#,
    );
}

#[test]
fn constant_division_and_overflow_are_clean_errors() {
    expect_failure(
        "div_by_zero",
        r#"
        fn main() -> Felt {
            return 1 / 0;
        }
        "#,
        "DivisionByZero",
    );
    expect_failure(
        "mod_by_zero",
        r#"
        fn main() -> u32 {
            return 5u32 % 0u32;
        }
        "#,
        "DivisionByZero",
    );
    expect_failure(
        "u32_overflow",
        r#"
        fn main() -> u32 {
            return 4294967295u32 + 1u32;
        }
        "#,
        "ArithmeticOverflow",
    );
    expect_failure(
        "u32_pow_overflow",
        r#"
        fn main() -> u32 {
            return 2u32 ** 32u32;
        }
        "#,
        "ArithmeticOverflow",
    );
    expect_failure(
        "u32_sub_underflow",
        r#"
        fn main() -> u32 {
            return 3u32 - 4u32;
        }
        "#,
        "ArithmeticOverflow",
    );
    expect_failure(
        "assign_div_by_zero",
        r#"
        fn main() {
            let mut x: Felt = 8;
            x /= 0;
        }
        "#,
        "DivisionByZero",
    );
}

// ---------------------------------------------------------------------------
// Unary operators, casts, assignment operators
// ---------------------------------------------------------------------------

#[test]
fn unary_operators_execute() {
    expect_exec(
        "unary_ops",
        r#"
        fn main() -> Felt {
            let x: Felt = 5;
            let neg: Felt = -x;
            assert_eq(neg + x, 0, "felt negation");
            let not_true: bool = !true;
            assert(not_true == false, "bool not");
            let not_false: bool = !false;
            assert(not_false == true, "bool not false");
            return -x;
        }
        "#,
    );
}

#[test]
fn casts_execute_and_reject_out_of_range() {
    expect_exec(
        "casts_ok",
        r#"
        fn main(a: Felt) -> Felt {
            let u = a as u32;
            let b = u as bool;
            assert(b == true || b == false, "bool cast");
            let back = (u as Felt) + 1;
            return back;
        }
        "#,
    );
    expect_failure(
        "cast_to_bool_invalid",
        r#"
        fn main() -> bool {
            return 2 as bool;
        }
        "#,
        "invalid cast",
    );
    expect_failure(
        "cast_to_u32_invalid",
        r#"
        fn main() -> u32 {
            return 4294967296 as u32;
        }
        "#,
        "invalid cast",
    );
}

#[test]
fn compound_assignment_operators_execute() {
    expect_exec(
        "compound_assign",
        r#"
        fn main() -> Felt {
            let mut x: Felt = 10;
            x += 5;
            assert_eq(x, 15, "add assign");
            x -= 3;
            assert_eq(x, 12, "sub assign");
            x *= 2;
            assert_eq(x, 24, "mul assign");
            x /= 4;
            assert_eq(x, 6, "div assign");
            x %= 4;
            assert_eq(x, 2, "mod assign");
            x <<= 3;
            assert_eq(x, 16, "shl assign");
            x >>= 2;
            assert_eq(x, 4, "shr assign");
            x |= 3;
            assert_eq(x, 7, "or assign");
            x &= 5;
            assert_eq(x, 5, "and assign");
            x ^= 1;
            assert_eq(x, 4, "xor assign");

            let mut u: u32 = 20u32;
            u += 3u32;
            u -= 1u32;
            u *= 2u32;
            u /= 4u32;
            u %= 5u32;
            u <<= 1u32;
            u >>= 1u32;
            u |= 8u32;
            u &= 12u32;
            u ^= 4u32;
            assert_eq(u, 8u32, "u32 compound assigns");

            let mut flag: bool = true;
            flag ^= true;
            assert(flag == false, "bool xor assign");
            return x;
        }
        "#,
    );
}

// ---------------------------------------------------------------------------
// Control flow
// ---------------------------------------------------------------------------

#[test]
fn while_and_for_loops_execute() {
    expect_exec(
        "loops",
        r#"
        fn main() -> Felt {
            let mut total: Felt = 0;
            let mut i: Felt = 0;
            while i <= 10 {
                total += i;
                i += 1;
            }
            assert_eq(total, 55, "while sum");

            let mut u_total: u32 = 0u32;
            for n in 0u32..10u32 {
                u_total += n;
            }
            assert_eq(u_total, 45u32, "for sum u32");

            let mut nested: Felt = 0;
            for a in 0u32..3u32 {
                for b in 0u32..3u32 {
                    nested += 1;
                }
            }
            assert_eq(nested, 9, "nested for");

            let mut early: u32 = 0u32;
            for n in 0u32..100u32 {
                if n == 5u32 {
                    early = n;
                }
            }
            assert_eq(early, 5u32, "conditional inside for");
            return total;
        }
        "#,
    );
}

#[test]
fn uncertain_loop_conditions_are_rejected() {
    expect_failure(
        "uncertain_while",
        r#"
        fn main(a: Felt) {
            let mut x: Felt = a;
            while x < 10 {
                x += 1;
            }
        }
        "#,
        "uncertain loop condition",
    );
    expect_failure(
        "uncertain_for_bound",
        r#"
        fn main(n: u32) {
            let mut total: u32 = 0u32;
            for i in 0u32..n {
                total += i;
            }
        }
        "#,
        "uncertain loop condition",
    );
}

#[test]
fn match_statements_and_expressions_execute() {
    expect_exec(
        "match_all",
        r#"
        fn match_case(input: Felt) -> Felt {
            let mut result: Felt = 0;
            match input {
                0 => { result += 10; },
                1 => { result += 20; },
                _ => { result += 50; },
            };
            let extra: Felt = match input {
                0 => 100,
                _ => 400,
            };
            result + extra
        }

        fn match_bool(input: bool) -> Felt {
            match input {
                true => 1,
                false => 2,
            }
        }

        fn match_u32(input: u32) -> Felt {
            let mut result: Felt = 0;
            match input {
                0u32 => { result += 5; },
                1u32 => { result += 15; },
                _ => { result += 55; },
            };
            result
        }

        fn main() -> Felt {
            assert_eq(match_case(0), 110, "match case 0");
            assert_eq(match_case(1), 420, "match case 1");
            assert_eq(match_case(9), 450, "match wildcard");
            assert_eq(match_bool(true), 1, "match bool true");
            assert_eq(match_bool(false), 2, "match bool false");
            assert_eq(match_u32(0u32), 5, "match u32 0");
            assert_eq(match_u32(9u32), 55, "match u32 wildcard");
            return match_case(2);
        }
        "#,
    );
}

#[test]
fn if_else_and_else_if_chains_execute() {
    expect_exec(
        "if_else",
        r#"
        fn main() -> Felt {
            let a: Felt = 3;
            let mut grade: Felt = 0;
            if a == 1 {
                grade = 10;
            }
            else if a == 2 {
                grade = 20;
            }
            else if a == 3 {
                grade = 30;
            }
            else {
                grade = 40;
            };
            assert_eq(grade, 30, "else-if chain");

            let max: Felt = if a > 2 { a } else { 2 };
            assert_eq(max, 3, "if expression");
            return grade;
        }
        "#,
    );
}

// ---------------------------------------------------------------------------
// Functions, closures, recursion
// ---------------------------------------------------------------------------

#[test]
fn closures_and_helper_calls_execute() {
    expect_exec(
        "closures",
        r#"
        fn double(x: Felt) -> Felt {
            return x * 2;
        }

        fn main() -> Felt {
            let max = |a: Felt, b: Felt| -> Felt {
                if a > b { a } else { b }
            };
            assert_eq(max(1, 2), 2, "closure call");
            assert_eq(double(max(3, 4)), 8, "fn of closure");
            let add_one = |x: Felt| -> Felt { x + 1 };
            assert_eq(add_one(41), 42, "single arg closure");
            return max(10, 20) + double(1);
        }
        "#,
    );
}

#[test]
fn recursion_is_rejected() {
    expect_failure(
        "recursion",
        r#"
        fn fact(n: Felt) -> Felt {
            let result: Felt = if n <= 1 { 1 } else { n * fact(n - 1) };
            return result;
        }

        fn main() -> Felt {
            return fact(5);
        }
        "#,
        "Recursion",
    );
}

// ---------------------------------------------------------------------------
// Arrays and structs (values, params, mutation)
// ---------------------------------------------------------------------------

#[test]
fn array_literals_repeat_and_mutation_execute() {
    expect_exec(
        "arrays",
        r#"
        fn main() -> Felt {
            let repeated: [Felt; 4] = [7; 4];
            assert_eq(repeated[0], 7, "repeat first");
            assert_eq(repeated[3], 7, "repeat last");
            let mut nested: [[Felt; 3]; 2] = [[11; 3]; 2];
            nested[0][0] = 99;
            assert_eq(nested[0][0], 99, "nested mutation");
            let mut variable_repeat: [Felt; 2] = [9; 2];
            variable_repeat[0] += 1;
            assert_eq(variable_repeat[0], 10, "mutated copy");
            assert_eq(variable_repeat[1], 9, "independent copies");
            let empty: [Felt; 0] = [123; 0];
            let mut literal: [Felt; 3] = [1, 2, 3];
            literal[1] = 20;
            assert_eq(literal[1], 20, "literal mutation");
            let mut compound: [u32; 2] = [4u32, 6u32];
            compound[0] %= 3u32;
            assert_eq(compound[0], 1u32, "u32 array compound assign");
            return repeated[0] + literal[2];
        }
        "#,
    );
}

#[test]
fn oversized_repeat_arrays_are_rejected() {
    expect_failure(
        "repeat_too_large",
        r#"
        fn main() {
            let huge: [Felt; 2000000] = [1; 2000000];
            assert_eq(huge[0], 1, "never reached");
        }
        "#,
        "ArrayTooLarge",
    );
    expect_failure(
        "total_materialization_exceeded",
        r#"
        fn main() {
            let a: [Felt; 1000000] = [1; 1000000];
            let b: [Felt; 1000000] = [1; 1000000];
            let c: [Felt; 1000000] = [1; 1000000];
            let d: [Felt; 1000000] = [1; 1000000];
            let e: [Felt; 1000000] = [1; 1000000];
            assert_eq(e[0], 1, "never reached");
        }
        "#,
        "ArrayTooLarge",
    );
}

#[test]
fn array_and_struct_parameters_are_materialized() {
    expect_exec(
        "param_materialize",
        r#"
        struct Point {
            pub x: Felt,
            pub y: Felt,
        }

        fn sum(arr: [Felt; 3]) -> Felt {
            return arr[0] + arr[1] + arr[2];
        }

        fn main(arr: [Felt; 4], p: Point, m: [[u32; 2]; 2], flag: bool) -> Felt {
            assert_eq(arr[0] + arr[1] + arr[2] + arr[3], sum_of_inputs(), "unused helper");
            return p.x + p.y + (m[1][1] as Felt);
        }

        fn sum_of_inputs() -> Felt {
            return 0;
        }
        "#,
    );
}

#[test]
fn struct_values_methods_and_mutation_execute() {
    expect_exec(
        "structs",
        r#"
        struct Item {
            pub id: Felt,
            pub value: Felt,
            pub data: [Felt; 2],
        }

        impl Item {
            pub fn total(self) -> Felt {
                return self.id + self.value;
            }

            pub fn with_offset(self, offset: Felt) -> Felt {
                return self.total() + offset;
            }
        }

        fn main() -> Felt {
            let mut item: Item = Item { id: 1, value: 10, data: [100, 200] };
            assert_eq(item.id, 1, "field read");
            assert_eq(item.data[1], 200, "array field read");
            item.value = 99;
            item.data[1] = 999;
            assert_eq(item.value, 99, "field write");
            assert_eq(item.data[1], 999, "array field write");
            assert_eq(item.total(), 100, "method call");
            assert_eq(item.with_offset(5), 105, "chained method call");

            let mut arr: [Item; 2] = [
                Item { id: 1, value: 10, data: [1, 2] },
                Item { id: 2, value: 20, data: [3, 4] },
            ];
            arr[1].value = 777;
            assert_eq(arr[1].value, 777, "struct in array write");
            assert_eq(arr[0].total(), 11, "struct in array method");

            let mut items: [Item; 3] = [Item { id: 0, value: 0, data: [0, 0] }; 3];
            for i in 0u32..3u32 {
                let index = i as Felt;
                items[index].value = index * 10;
            }
            assert_eq(items[2].value, 20, "loop struct writes");
            return item.total() + arr[1].value + items[1].value;
        }
        "#,
    );
}

// ---------------------------------------------------------------------------
// Intrinsics: assert / assert_eq / clear_entire_tree
// ---------------------------------------------------------------------------

#[test]
fn assert_intrinsics_execute_and_fail_cleanly() {
    expect_exec(
        "assert_ok",
        r#"
        fn main() {
            assert(1 == 1, "trivially true");
            assert(true);
            assert_eq(2 + 3, 5, "sum");
            assert_eq(true, true, "bool eq");
            assert_eq(2u32 * 3u32, 6u32, "u32 eq");
        }
        "#,
    );
    expect_failure(
        "assert_false",
        r#"
        fn main() {
            assert(false, "boom");
        }
        "#,
        "boom",
    );
    expect_failure(
        "assert_eq_mismatch",
        r#"
        fn main() {
            assert_eq(1, 2, "mismatch");
        }
        "#,
        "mismatch",
    );
    expect_failure(
        "assert_eq_mismatch_no_message",
        r#"
        fn main() {
            assert_eq(5u32, 6u32);
        }
        "#,
        "assertion failure",
    );
}

#[test]
fn clear_entire_tree_intrinsic_executes() {
    // Storage teardown intrinsic inside a contract write method.
    expect_exec(
        "clear_tree",
        r#"
        #[contract]
        #[derive(Storage)]
        pub struct Box {
            pub value: Felt,
        }

        #[contract::write_method]
        pub fn reset(value: Felt) {
            let c = BoxRef::new(ContractMetadata::current());
            c.value = value;
            clear_entire_tree();
        }
        "#,
    );
}

// ---------------------------------------------------------------------------
// Contract entry points
// ---------------------------------------------------------------------------

const LIFECYCLE_CONTRACT: &str = r#"
    #[contract]
    #[derive(Storage)]
    pub struct LifecycleContract {
        pub value: Felt,
    }

    #[contract::write_method]
    pub fn set_value(value: Felt) {
        let c = LifecycleContractRef::new(ContractMetadata::current());
        c.value = value;
    }

    #[contract::view_method]
    pub fn get_value() -> Felt {
        let c = LifecycleContractRef::new(ContractMetadata::current());
        return c.value.get();
    }
"#;

#[test]
fn contract_auto_discovery_executes_all_methods() {
    match run_with(LIFECYCLE_CONTRACT, "contract_auto", None, &[], true) {
        Outcome::Executed(defs) => {
            let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
            assert!(names.contains(&"set_value"), "write method compiled: {names:?}");
            assert!(names.contains(&"get_value"), "view method compiled: {names:?}");
            for def in &defs {
                if def.name == "get_value" {
                    assert!(def.is_view_function(), "get_value must stay read-only");
                }
            }
        }
        Outcome::Failed(msg) => panic!("[contract_auto] expected execution, got FAILURE:\n{msg}"),
        Outcome::Panicked(msg) => panic!("[contract_auto] expected execution, got PANIC:\n{msg}"),
    }
}

#[test]
fn contract_explicit_name_and_single_method_execute() {
    match run_with(LIFECYCLE_CONTRACT, "contract_explicit", Some("LifecycleContract"), &["set_value"], true) {
        Outcome::Executed(defs) => {
            assert_eq!(defs.len(), 1, "only the requested method compiles");
            assert_eq!(defs[0].name, "set_value");
        }
        Outcome::Failed(msg) => panic!("[contract_explicit] expected execution, got FAILURE:\n{msg}"),
        Outcome::Panicked(msg) => panic!("[contract_explicit] expected execution, got PANIC:\n{msg}"),
    }
}

#[test]
fn contract_entry_point_errors_are_reported() {
    // Unknown contract name.
    expect_failure_with("contract_missing", LIFECYCLE_CONTRACT, Some("Nope"), &[], "undefined function");
    // Unknown method on a known contract.
    expect_failure_with("method_missing", LIFECYCLE_CONTRACT, Some("LifecycleContract"), &["nope"], "undefined function");
    // Free-fn `-m` target that exists but is not a function.
    expect_failure_with(
        "method_not_a_function",
        r#"
        struct Point {
            pub x: Felt,
        }

        fn main() -> Felt {
            return 0;
        }
        "#,
        None,
        &["Point"],
        "non-function type",
    );
}

#[test]
fn view_method_that_writes_state_is_rejected() {
    let source = r#"
        #[contract]
        #[derive(Storage)]
        pub struct Bad {
            pub value: Felt,
        }

        #[contract::view_method]
        pub fn sneaky(value: Felt) {
            let c = BadRef::new(ContractMetadata::current());
            c.value = value;
        }
    "#;
    match run_with(source, "view_writes", None, &[], true) {
        Outcome::Failed(msg) => assert!(
            msg.contains("view_method") && msg.contains("writes state"),
            "[view_writes] unexpected message:\n{msg}"
        ),
        other => panic!("[view_writes] expected view violation failure, got {other:?}"),
    }
}

#[test]
fn overloaded_contract_methods_are_rejected() {
    let source = r#"
        #[contract]
        #[derive(Storage)]
        pub struct Dup {
            pub value: Felt,
        }

        impl Dup {
            #[contract::write_method]
            pub fn set_value(value: Felt) {
                let c = DupRef::new(ContractMetadata::current());
                c.value = value;
            }
        }

        impl DupRef {
            #[contract::write_method]
            pub fn set_value(value: Felt) {
                let c = DupRef::new(ContractMetadata::current());
                c.value = value;
            }
        }
    "#;
    expect_failure_with("overloaded_methods", source, Some("Dup"), &[], "overloaded contract method");
}

// ---------------------------------------------------------------------------
// Preprocess: storage layout / maps / nested refs
// ---------------------------------------------------------------------------

#[test]
fn contract_storage_with_array_and_map_fields_typechecks() {
    expect_exec(
        "storage_layout",
        r#"
        #[contract]
        #[derive(Storage)]
        pub struct Vault {
            pub note: Felt,
            pub slots: [Felt; 4],
            pub balances: Map<Hash, Hash, 8u32>,
        }

        #[contract::write_method]
        pub fn seed(value: Felt) {
            let c = VaultRef::new(ContractMetadata::current());
            c.note = value;
            c.balances.insert([4001, 0, 0, 0], [1, 2, 3, 4]);
        }

        #[contract::view_method]
        pub fn peek() -> Felt {
            let c = VaultRef::new(ContractMetadata::current());
            return c.note.get();
        }
        "#,
    );
}

#[test]
fn nested_storage_ref_fields_typecheck() {
    expect_exec(
        "nested_refs",
        r#"
        #[derive(Storage)]
        pub struct Profile {
            pub age: Felt,
            pub level: Felt,
        }

        #[contract]
        #[derive(Storage)]
        pub struct Player {
            pub id: Felt,
            #[ref]
            pub profile: Profile,
            pub tags: [Felt; 2],
        }

        fn main() -> Felt {
            assert_eq(Profile::size(), 2, "nested struct size");
            assert_eq(Player::size(), 5, "array counts every element");
            return Player::size();
        }
        "#,
    );
}

#[test]
fn second_map_in_contract_is_rejected() {
    expect_failure(
        "two_maps",
        r#"
        #[contract]
        #[derive(Storage)]
        pub struct TwoMaps {
            pub a: Map<Felt, Felt, 4u32>,
            pub b: Map<Felt, Felt, 4u32>,
        }

        fn main() -> Felt {
            return 0;
        }
        "#,
        "Only one Map",
    );
}

#[test]
fn map_nested_in_array_field_typechecks() {
    // A single Map nested inside an array field counts as one map and is
    // accepted.
    expect_exec(
        "map_in_array",
        r#"
        #[contract]
        #[derive(Storage)]
        pub struct MapArray {
            pub grids: [Map<Felt, Felt, 2u32>; 2],
        }

        fn main() -> Felt {
            return 0;
        }
        "#,
    );
}

#[test]
fn recursive_storage_struct_cycle_is_handled() {
    let outcome = run(
        r#"
        struct Inner {
            pub outer: Outer,
        }

        #[contract]
        #[derive(Storage)]
        pub struct Outer {
            pub inner: Inner,
            pub note: Felt,
        }

        fn main() -> Felt {
            return 0;
        }
        "#,
        "storage_cycle",
    );
    match outcome {
        // Rejection at preprocess/sema is fine, and so is success — what
        // matters is that the cycle neither hangs nor panics the counter.
        Outcome::Failed(_) => {}
        Outcome::Executed(_) => {}
        Outcome::Panicked(msg) => panic!("[storage_cycle] cyclic layout must not panic:\n{msg}"),
    }
}

// ---------------------------------------------------------------------------
// `#[test]` runner
// ---------------------------------------------------------------------------

#[test]
fn psy_test_runner_executes_passing_and_expected_panic_tests() {
    match run_psy_tests(
        r#"
        #[test]
        fn simple_pass() {
            assert_eq(1 + 1, 2, "arithmetic");
        }

        #[test]
        #[should_panic]
        fn expected_failure() {
            assert(false, "deliberate");
        }

        #[test]
        fn helper_calls() {
            assert(max(1, 2) == 2, "max works");
        }

        fn max(a: Felt, b: Felt) -> Felt {
            if a > b { a } else { b }
        }
        "#,
        "tests_ok",
    ) {
        Outcome::Executed(defs) => {
            // `simple_pass` and `helper_calls` compile; the should_panic case
            // only prints.
            assert_eq!(defs.len(), 2, "passing tests produce compiled defs");
        }
        Outcome::Failed(msg) => panic!("[tests_ok] expected runner success, got FAILURE:\n{msg}"),
        Outcome::Panicked(msg) => panic!("[tests_ok] expected runner success, got PANIC:\n{msg}"),
    }
}

#[test]
fn psy_test_runner_panics_when_a_test_fails() {
    expect_test_panic(
        "test_fails",
        r#"
        #[test]
        fn bad() {
            assert(false, "boom in test");
        }
        "#,
        "Test bad failed",
    );
}

#[test]
fn psy_test_runner_panics_when_should_panic_test_passes() {
    expect_test_panic(
        "unexpected_pass",
        r#"
        #[test]
        #[should_panic]
        fn fine() {
            assert(true);
        }
        "#,
        "Expected panic",
    );
}

#[test]
fn psy_test_runner_runs_contract_storage_tests() {
    match run_psy_tests(
        r#"
        #[contract]
        #[derive(Storage)]
        pub struct Adjacent {
            pub before: Felt,
            pub balances: Map<Hash, Hash, 32u32>,
            pub after: Felt,
        }

        #[test]
        fn map_ops_preserve_adjacent_fields() {
            let c = AdjacentRef::new(ContractMetadata::current());
            c.before = 111;
            c.after = 222;
            c.balances.insert([4001, 0, 0, 0], [42, 0, 0, 0]);
            assert_eq(c.before, 111, "before unchanged");
            assert_eq(c.after, 222, "after unchanged");
        }
        "#,
        "tests_storage",
    ) {
        Outcome::Executed(_) => {}
        Outcome::Failed(msg) => panic!("[tests_storage] expected success, got FAILURE:\n{msg}"),
        Outcome::Panicked(msg) => panic!("[tests_storage] expected success, got PANIC:\n{msg}"),
    }
}

// ---------------------------------------------------------------------------
// Void main / block expressions
// ---------------------------------------------------------------------------

#[test]
fn void_main_and_block_expressions_execute() {
    expect_exec(
        "void_main",
        r#"
        fn main() {
            let x: Felt = 2;
            assert_eq(x, 2, "void main body");
        }
        "#,
    );
}

// ---------------------------------------------------------------------------
// Formatter grammar coverage: enums, traits, impls, type aliases, consts.
//
// `typecheck_*` pipelines format only the user module, so definition kinds
// that appear nowhere else in test fixtures (enum struct/tuple variants,
// trait associated types with constraints, impl blocks) never reach
// psy-fmt. This suite parses and formats such a program directly.
// ---------------------------------------------------------------------------

/// Parses `source` (virtual single-file workspace) and returns the formatter
/// output without running sema, so formatting of purely syntactic constructs
/// can be asserted even where the typechecker has no support yet.
fn format_source(source: &str) -> String {
    super::with_primitive_scope_reset(|| -> anyhow::Result<String> {
        let path = PathBuf::from("/virtual/src/main.psy");
        let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
        let mut program = Program::new();
        program.file_resolver.add_file(path.clone(), std::sync::Arc::from(source));
        let mut graph = Graph::new();
        graph.add_node(path);

        let mut parser = Parser::new(&mut program, &mut interpreter.context, graph);
        parser.parse().map_err(|error| anyhow::anyhow!("fixture must parse: {error:#}"))?;

        let mut default_visitor_context: DefaultVisitorContext<'_, SymFeltRef, QExecContext> = DefaultVisitorContext::new(&mut program);
        let mut formatter = Formatter::new();
        formatter
            .visit_program(&mut default_visitor_context)
            .map_err(|error| anyhow::anyhow!("fixture must format: {error}"))?;
        Ok(formatter.get_output().to_owned())
    })
    .expect("parse + format within primitive scope reset")
}

#[test]
fn formatter_renders_enums_traits_impls_and_aliases() {
    let output = format_source(
        r#"
        pub enum Shape {
            Point,
            Rect(Felt, u32),
            Circle {
                pub radius: Felt,
            },
        }

        pub trait Describable<T> {
            pub type Output: Storage;
            pub type Plain;
            pub fn describe(self: Self, other: T) -> Felt;
        }

        impl Describable<Felt> for Shape {
            pub type Output = [Felt; 2];
            pub type Plain = Felt;
            pub fn describe(self: Self, other: Felt) -> Felt {
                return other;
            }
        }

        impl Shape {
            pub fn area(self: Self) -> Felt {
                return 1;
            }
        }

        type Alias = [Felt; 4];
        pub const MAX: Felt = 10;

        pub fn main() -> Felt {
            let s: Felt = MAX;
            let arr: Alias = [1, 2, 3, 4];
            return s + arr[0 as Felt];
        }
        "#,
    );

    // Enum with all three variant kinds.
    assert!(output.contains("enum Shape"), "missing enum:\n{output}");
    assert!(output.contains("Point,"), "missing basic variant:\n{output}");
    assert!(output.contains("Rect(Felt, u32)"), "missing tuple variant:\n{output}");
    assert!(output.contains("Circle {"), "missing struct variant:\n{output}");
    assert!(output.contains("radius: Felt"), "missing struct variant field:\n{output}");

    // Trait with associated types (constrained and plain) and a signature.
    assert!(output.contains("trait Describable<T>"), "missing trait:\n{output}");
    assert!(output.contains("type Output: Storage;"), "missing constrained assoc type:\n{output}");
    assert!(output.contains("type Plain;"), "missing assoc type:\n{output}");
    assert!(output.contains("fn describe(self: Self, other: T) -> Felt"), "missing trait method:\n{output}");

    // Trait impl with associated type values and an inherent impl.
    assert!(output.contains("impl Describable<Felt> for Shape"), "missing trait impl:\n{output}");
    assert!(output.contains("type Output = [Felt; 2]"), "missing assoc type value:\n{output}");
    assert!(output.contains("impl Shape"), "missing inherent impl:\n{output}");
    assert!(output.contains("fn area(self: Self) -> Felt"), "missing impl method:\n{output}");

    // Type alias, const, and a body that uses them.
    assert!(output.contains("type Alias = [Felt; 4]"), "missing type alias:\n{output}");
    assert!(output.contains("const MAX:Felt = 10"), "missing const:\n{output}");
    assert!(output.contains("fn main() -> Felt"), "missing main:\n{output}");
}

// ---------------------------------------------------------------------------
// Rewriter coverage: intrinsics that user code may call directly (they lex
// as their own tokens, not `__`-gated builtins) inside a GENERIC function
// body, so instantiating the function from main rewrites those intrinsic
// nodes in psy-sema/src/rewriter.rs. Event emission through the derived
// `Event` impl exercises the Emit arm the same way.
// ---------------------------------------------------------------------------

#[test]
fn generic_bodies_with_hash_mem_and_event_intrinsics_instantiate() {
    expect_exec(
        "generic_intrinsics",
        r#"
        use std::prelude::*;

        #[derive(Event)]
        pub struct Ping {
            pub v: Felt,
        }

        fn probe_hashes<T>(x: T, a: Hash, b: Hash) -> Felt {
            let h1: Hash = hash(a);
            let h2: [u32; 8] = keccak256(b);
            let h3: Hash = hash_two_to_one(a, b);
            let n: Felt = size_of::<Hash>();
            let t: Felt = transmute::<Felt, u32>(7u32);
            let ping = Ping { v: 1 };
            ping.emit();
            let h2_ok: bool = h2[0u32 as Felt] == h2[1u32 as Felt];
            return n + t + h1[0 as Felt] + h3[0 as Felt] + h2_ok as Felt + (x == x) as Felt;
        }

        fn main(q: Felt) -> Felt {
            let r: Felt = probe_hashes(q, [1, 2, 3, 4], [5, 6, 7, 8]);
            return r;
        }
        "#,
    );
}

#[test]
fn tuple_parameters_and_returns_flow_through_interpretation() {
    expect_exec(
        "tuple_param_return",
        r#"
        fn make_pair(x: Felt) -> (Felt, Felt) {
            return (x + 1, x + 2);
        }

        fn main(q: Felt) -> Felt {
            let pair: (Felt, Felt) = make_pair(q);
            return pair.0 + pair.1;
        }
        "#,
    );
}

#[test]
fn calculate_type_size_sums_tuple_element_sizes() {
    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let mut ctx = TypeCheckerVisitorContext::<SymFeltRef, QExecContext>::new(Program::new());
    let felt = ctx.symbols.create_type(psy_sema::Type::Felt).unwrap();
    let tuple = ctx.symbols.create_type(psy_sema::Type::Tuple(vec![felt, felt, felt])).unwrap();
    assert_eq!(interpreter.calculate_type_size(tuple, &ctx), 3);
}

#[test]
fn if_else_if_expression_chains_execute() {
    expect_exec(
        "else_if_chain",
        r#"
        fn main(x: Felt) -> Felt {
            let y: Felt = if x == 0 {
                1
            } else if x == 1 {
                2
            } else {
                3
            };
            return y;
        }
        "#,
    );
}

#[test]
fn first_class_function_arguments_match_expected_signatures() {
    expect_exec(
        "first_class_fn_arg",
        r#"
        fn apply(f: fn(Felt, Felt) -> Felt, x: Felt, y: Felt) -> Felt {
            return f(x, y);
        }

        fn add(a: Felt, b: Felt) -> Felt {
            return a + b;
        }

        fn main(q: Felt) -> Felt {
            return apply(add, q, 1);
        }
        "#,
    );
}

#[test]
fn type_checker_visit_module_is_unreachable_by_design() {
    let path = temp_psy_path("visit_module");
    fs::write(&path, "fn main() {}\n").unwrap();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let (mut typechecker, mut ctx) = interpreter.typecheck_single(path.clone()).expect("typecheck a trivial module");
    let _ = fs::remove_file(&path);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = psy_ast::AstVisitor::visit_module(&mut typechecker, psy_ast::ModuleId(0), &mut ctx);
    }));
    assert!(result.is_err(), "TypeChecker::visit_module must hit unreachable!()");
}

#[test]
fn equality_uses_an_inherent_generic_eq_method_when_present() {
    expect_exec(
        "generic_eq_method",
        r#"
        pub struct P {
            pub v: Felt,
        }

        impl P {
            pub fn eq<T>(self: Self, rhs: P) -> bool {
                return self.v == rhs.v;
            }
        }

        fn main(q: Felt) -> Felt {
            let a = P { v: q };
            let b = P { v: q + 1 };
            return (a == b) as Felt;
        }
        "#,
    );
}

#[test]
fn indexing_uses_an_inherent_generic_index_method_when_present() {
    expect_exec(
        "generic_index_method",
        r#"
        pub struct I {
            pub v: [Felt; 4],
        }

        impl I {
            pub fn index<T>(self: Self, idx: Felt) -> Felt {
                return self.v[idx];
            }
        }

        fn main(q: Felt) -> Felt {
            let i = I { v: [1, 2, 3, 4] };
            return i[q];
        }
        "#,
    );
}

#[test]
fn compound_assignment_uses_an_inherent_generic_add_assign_method_when_present() {
    expect_exec(
        "generic_add_assign_method",
        r#"
        pub struct A {
            pub v: Felt,
        }

        impl A {
            pub fn add_assign<T>(mut self: Self, rhs: Felt) {
                self.v = self.v + rhs;
            }
        }

        fn main(q: Felt) -> Felt {
            let mut a = A { v: q };
            a += 1;
            return a.v;
        }
        "#,
    );
}

#[test]
fn assert_eq_uses_an_inherent_generic_eq_method_when_present() {
    expect_exec(
        "generic_eq_method_assert",
        r#"
        pub struct Q {
            pub v: Felt,
        }

        impl Q {
            pub fn eq<T>(self: Self, rhs: Q) -> bool {
                return self.v == rhs.v;
            }
        }

        fn main(q: Felt) {
            let a = Q { v: q };
            let b = Q { v: q };
            assert_eq(a, b, "custom eq");
        }
        "#,
    );
}

#[test]
fn passing_an_unknown_function_value_is_a_clean_error() {
    expect_failure(
        "unknown_function_value",
        r#"
        fn apply(f: fn(Felt, Felt) -> Felt, x: Felt, y: Felt) -> Felt {
            return f(x, y);
        }

        fn main(q: Felt) -> Felt {
            return apply(nope, q, 1);
        }
        "#,
        "unresolved type",
    );
}

#[test]
fn passing_a_function_with_a_mismatched_signature_is_a_clean_error() {
    expect_failure(
        "wrong_arity_function_value",
        r#"
        fn apply(f: fn(Felt, Felt) -> Felt, x: Felt, y: Felt) -> Felt {
            return f(x, y);
        }

        fn add3(a: Felt, b: Felt, c: Felt) -> Felt {
            return a + b + c;
        }

        fn main(q: Felt) -> Felt {
            return apply(add3, q, 1);
        }
        "#,
        "type mismatch",
    );
}

#[test]
fn lambdas_with_named_return_types_execute() {
    expect_exec(
        "lambda_named_return_type",
        r#"
        pub struct P {
            pub v: Felt,
        }

        fn main(q: Felt) -> Felt {
            let make = |x: Felt| -> P {
                return P { v: x };
            };
            return make(q).v;
        }
        "#,
    );
}

#[test]
fn associated_functions_on_types_execute() {
    expect_exec(
        "associated_fn_call",
        r#"
        pub struct P {
            pub v: Felt,
        }

        impl P {
            pub fn create(x: Felt) -> P {
                return P { v: x };
            }
        }

        fn main(q: Felt) -> Felt {
            return P::create(q).v;
        }
        "#,
    );
}
