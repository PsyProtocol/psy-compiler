// Execution-layer edge coverage for arms that typechecking alone never
// reaches: the binary/unary operator dispatch matrix, symbolic (non-constant)
// index reads and writes through arrays/tuples/structs, array-repeat
// materialization limits, failing assertion reporting, and the
// `interpret_vfs_files` / `typecheck_lsp` entry points.
//
// Every case drives `main` through the interpreter with a stub compile
// function (no DPN proving) and resets the shared STD_PRIMITIVE_SCOPE_ID
// singleton so the suite stays hermetic. All cases are `#[serial]`.

use std::sync::atomic::{AtomicU64, Ordering};

use psy_vm::dpn::{
    ops::{exec_context::QExecContext, sym_felt::SymFeltRef},
    vm::{compile::PsyCompileResult, def::DPNFunctionCircuitDefinition},
};
use serial_test::serial;

use super::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

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

/// Unused today but kept symmetric with the other exec suites: the real
/// backend, for cases that must observe generated state commands.
#[allow(dead_code)]
fn real_compile_fn(
    context: &QExecContext,
    (name, method_id, outputs): (String, u32, Vec<SymFeltRef>),
) -> DPNFunctionCircuitDefinition {
    PsyCompileResult::compile_exec(name, method_id, &context.store, context, &outputs)
}

fn reset_primitive_scope() {
    #[allow(static_mut_refs)]
    unsafe {
        let _ = STD_PRIMITIVE_SCOPE_ID.take();
    }
}

fn exec_source(label: &str, source: &str) -> Result<(), String> {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_exec_{n}.psy"));
    std::fs::write(&path, source).unwrap();
    let path_arg = path.clone();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = {
        let (mut typechecker, mut ctx) = interpreter
            .typecheck_single(path_arg)
            .map_err(|e| format!("typecheck: {e:#}"))?;
        interpreter
            .interpret(&mut typechecker, &mut ctx, None::<Ident>, vec![], stub_compile_fn)
            .map(|_| ())
            .map_err(|e| format!("interpret: {e:#}"))
    };
    let _ = std::fs::remove_file(&path);
    reset_primitive_scope();
    result
}

fn exec_accepts(label: &str, source: &str) {
    if let Err(message) = exec_source(label, source) {
        panic!("[{label}] expected execution success, got:\n{message}");
    }
}

fn exec_rejects(label: &str, source: &str, needle: &str) {
    match exec_source(label, source) {
        Ok(()) => panic!("[{label}] expected execution failure containing `{needle}`, got success"),
        Err(message) => assert!(
            message.to_lowercase().contains(&needle.to_lowercase()),
            "[{label}] expected failure containing `{needle}`, got:\n{message}"
        ),
    }
}

/// Sweep every binary-operator dispatch arm on all three operand families.
/// `a` is a symbolic input so nothing constant-folds away before dispatch.
#[test]
#[serial]
fn operator_matrix_executes_felt_u32_and_bool_binops() {
    exec_accepts(
        "felt operator matrix",
        r#"
use std::prelude::*;

fn main(a: Felt) {
    let f01 = a + 1;
    let f02 = a - 1;
    let f03 = a * 2;
    let f04 = a / 2;
    let f05 = a % 3;
    let f06 = a ** 2;
    let f07 = a >> 1;
    let f08 = a << 1;
    let f09 = a & 3;
    let f10 = a | 8;
    let f11 = a ^ 1;
    let b01 = a == 1;
    let b02 = a != 1;
    let b03 = a < 1;
    let b04 = a <= 1;
    let b05 = a > 1;
    let b06 = a >= 1;
    assert_eq(f01, f01, "felt stable");
    assert(b01 || b02, "felt compare");
}
"#,
    );
    exec_accepts(
        "u32 operator matrix",
        r#"
use std::prelude::*;

fn main(a: Felt) {
    let u = a as u32;
    let n01 = u + 1u32;
    let n02 = u - 1u32;
    let n03 = u * 2u32;
    let n04 = u / 2u32;
    let n05 = u % 3u32;
    let n06 = u ** 2u32;
    let n07 = u >> 1u32;
    let n08 = u << 1u32;
    let n09 = u & 3u32;
    let n10 = u | 8u32;
    let n11 = u ^ 1u32;
    let c01 = u == 1u32;
    let c02 = u != 1u32;
    let c03 = u < 1u32;
    let c04 = u <= 1u32;
    let c05 = u > 1u32;
    let c06 = u >= 1u32;
    assert_eq(n01, n01, "u32 stable");
    assert(c01 || c02, "u32 compare");
}
"#,
    );
    exec_accepts(
        "bool operator matrix",
        r#"
use std::prelude::*;

fn main(a: Felt) {
    let p = a == 1;
    let q = a == 2;
    let d01 = p && q;
    let d02 = p || q;
    let d03 = p ^ q;
    let d04 = p == q;
    let d05 = p != q;
    assert(d01 || d02 || d03 || d04 || d05, "bool ops");
}
"#,
    );
    exec_accepts(
        "unary operators execute",
        r#"
use std::prelude::*;

fn main(a: Felt) {
    let neg = -a;
    let flag = a == 1;
    let not = !flag;
    assert(neg == neg && !not || not, "unary stable");
}
"#,
    );
}

/// Symbolic (non-constant) felt indices take the lane-select paths in
/// `CheckedValueRef::get_path`/`set_path`; tuple and struct targets take the
/// positional/name-based arms.
#[test]
#[serial]
fn symbolic_index_paths_read_and_write_composites() {
    exec_accepts(
        "symbolic array index read and write",
        r#"
use std::prelude::*;

fn main(a: Felt) {
    let mut arr = [10, 20, 30];
    arr[a] = 5;
    let v = arr[a];
    assert_eq(v, v, "symbolic readback");
}
"#,
    );
    exec_accepts(
        "tuple element write and read",
        r#"
use std::prelude::*;

fn main(a: Felt) {
    let mut t = (1, 2);
    t.1 = a;
    assert_eq(t.1, a, "tuple field");
}
"#,
    );
    exec_accepts(
        "struct field write and read",
        r#"
use std::prelude::*;

pub struct Point { pub x: Felt, pub y: Felt }

fn main(a: Felt) {
    let mut p = Point { x: 1, y: 2 };
    p.x = a;
    assert_eq(p.x, a, "struct field");
}
"#,
    );
    exec_rejects(
        "out-of-range constant index",
        r#"
use std::prelude::*;

fn main() {
    let arr = [10, 20, 30];
    assert_eq(arr[5], 1, "unreachable");
}
"#,
        "index",
    );
}

/// Array repeats materialize at execution; the interpreter caps the element
/// count and rejects oversized repeats before allocating.
#[test]
#[serial]
fn array_repeats_materialize_and_oversized_repeats_reject() {
    exec_accepts(
        "small array repeat",
        r#"
use std::prelude::*;

fn main() {
    let a = [7; 4];
    assert_eq(a[3], 7, "repeat value");
}
"#,
    );
    exec_rejects(
        "oversized array repeat",
        r#"
use std::prelude::*;

fn main() {
    let a = [0; 1048577];
    assert_eq(a[0], 0, "unreachable");
}
"#,
        "toolarge",
    );
}

/// Failing constant assertions surface the user message at execution.
#[test]
#[serial]
fn failing_assertions_report_their_messages() {
    exec_rejects(
        "assert_eq with differing constants",
        r#"
use std::prelude::*;

fn main() {
    assert_eq(2, 3, "boom");
}
"#,
        "boom",
    );
    exec_rejects(
        "assert with a false constant",
        r#"
use std::prelude::*;

fn main() {
    assert(false, "nope");
}
"#,
        "nope",
    );
}

/// The VFS entry point runs an in-memory program without touching the disk.
#[test]
#[serial]
fn vfs_files_interpret_a_virtual_program() {
    let source = "use std::prelude::*;\nfn main() { let a = split_bits(3, 2); assert_eq(a[0], 1, \"vfs\"); }";
    let mut graph = Graph::new();
    graph.add_node(std::path::PathBuf::from("/virtual/src/main.psy"));
    let result = super::interpret_vfs_files(
        None,
        vec![],
        graph,
        vec![(std::path::PathBuf::from("/virtual/src/main.psy"), std::sync::Arc::from(source))],
    );
    reset_primitive_scope();
    match result {
        Ok(res) => assert_eq!(res.compile_results.len(), 1, "vfs main compiled"),
        Err(err) => panic!("[vfs_files] expected success, got:\n{err:#}"),
    }
}

/// The LSP typecheck entry point shares the pipeline but lowers errors to
/// diagnostics instead of anyhow chains.
#[test]
#[serial]
fn typecheck_lsp_typechecks_the_module_graph() {
    let entry: PathBuf = "../tests/module_test/foo/src/main.psy".into();
    let dependency_entry: PathBuf = "../tests/module_test/bar/src/lib.psy".into();

    let mut crate_path_graph = Graph::new();
    crate_path_graph.add_node(entry.clone());
    crate_path_graph.add_edge(entry.clone(), dependency_entry);

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let result = interpreter.typecheck_lsp(crate_path_graph);
    reset_primitive_scope();
    match result {
        Ok((_typechecker, _ctx)) => {}
        Err(err) => panic!("[typecheck_lsp] expected success, got:\n{err:?}"),
    }
}
