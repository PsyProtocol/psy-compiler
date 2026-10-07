// Differential test: seeded random computation graphs, psy interpreter vs
// native Rust evaluation.
//
// For each seed the generator builds a DAG of `let` bindings over u32, Felt
// and bool values, prints it as a psy program, and interprets `main` with
// concrete constant arguments. Because every operand is constant-tracked,
// the interpreter either returns a folded value or fails with a clean
// error; the native Rust mirror computes the same outcome and both sides
// must agree exactly — same value, or same error class:
//
//   u32   + - * / % ** & | ^ << >>   checked arithmetic (overflow -> error),
//                                     div/mod by zero -> error, shift
//                                     distances >= 32 fold to 0
//   Felt  + - * / % ** and unary -   Goldilocks arithmetic mod p (p = 2^64
//                                     - 2^32 + 1): wrapping, `/` is field
//                                     division, `%` is integer mod, div/mod
//                                     by zero -> error
//   bool  == != < <= > >= && || ^ !  false < true
//   cast  u32 <-> Felt <-> bool      out-of-range constants -> clean error
//                                     (> 1 for bool, > u32::MAX for u32)
//
// Felt bitwise (& | ^ << >>) is deliberately NOT generated: the VM never
// constant-folds it (felt constants don't take the u32 fold path), so the
// result stays symbolic and has no value to compare.
//
// The generator is boundary-aware: literals and inputs are biased toward
// 0/1/2, powers of two, u32::MAX-adjacent and p-adjacent values so
// overflow, wrap-around and cast-range edges are systematically exercised
// instead of left to luck. Every binary node keeps at least one runtime
// operand so sema's const-item folding never interferes.
//
// Reproduce a failure:
//   PSY_RANDOM_GRAPH_SEED=<seed> cargo test -p psy-interpreter random_computation_graphs_match_native_rust
// Longer fuzzing runs:
//   PSY_RANDOM_GRAPH_ITERS=100000 cargo test -p psy-interpreter random_computation_graphs_match_native_rust
//
// The crypto/bit intrinsic differential (same env knobs, test
// `crypto_intrinsics_match_native_rust`) covers the DSL-reachable
// intrinsics that never appear in scalar graphs:
//
//   hash / hash_two_to_one   Poseidon (Goldilocks) — mirrored with plonky2's
//                            PoseidonHash, the same primitive the circuit uses
//   keccak256                u32-word packing + tiny-keccak, anchored by the
//                            external known-answer vector in
//                            tests/keccak_u32_regression_test.psy
//   __secp256k1_verify       k256 fixtures: valid signatures must verify,
//                            tampered msg/sig/pk must fail
//   split_bits / sum_bits    strict LSB-first bit decomposition (width
//                            <= 64, value must fit the width) and its
//                            weighted binary fold; out-of-domain inputs
//                            are expected clean rejections
//   array element access     bits[i] / digest[i] on intrinsic store nodes
//                            (TargetAt), plus deterministic boundary
//                            suites: all-ones splits, 64-element sums,
//                            felt wrap-around arithmetic, degenerate
//                            secp keys
//
// Intrinsic results are store nodes (HashNoPad/TargetAt/...), not folded
// constants, so `psy_run` resolves them concretely through the VM's
// ContextEval — the eval path the IDE preview uses.

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use plonky2::{
    field::{
        goldilocks_field::GoldilocksField,
        types::{Field, PrimeField64},
    },
    hash::{hash_types::HashOut, poseidon::PoseidonHash},
    plonk::config::{GenericHashOut, Hasher},
};
use psy_vm::dpn::{
    eval::{cache::SimpleEvalCache, simple::DummyContextEvalInput, traits::ContextEval},
    ops::{
        exec_context::QExecContext,
        op_types::DPNOpType,
        sym_felt::SymFeltRef,
        sym_felt_store::SymFeltStore,
    },
};

use super::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

const PARAM_NAMES: [&str; 4] = ["a", "b", "c", "d"];

/// Goldilocks prime, the Felt modulus (verified against the VM fold:
/// `(p - 1) + 2` folds to 1).
const GOLDILOCKS_P: u128 = 0xFFFF_FFFF_0000_0001;

const SHIFT_AMOUNTS: [u32; 14] = [0, 1, 2, 3, 4, 7, 8, 15, 16, 31, 32, 63, 64, 100];
/// 31/32 straddle the u32 pow boundary exactly (2 ** 31 fits, 2 ** 32
/// overflows).
const POW_EXPONENTS: [u32; 7] = [0, 1, 2, 3, 4, 31, 32];
const DIVISORS: [u32; 6] = [0, 0, 1, 1, 2, 3];
const SMALL: [u32; 16] = [0, 1, 2, 3, 4, 5, 7, 8, 15, 16, 31, 32, 63, 64, 255, 256];
const TINY: [u32; 10] = [0, 1, 1, 2, 2, 3, 4, 5, 7, 8];
const EDGES: [u32; 5] = [0x7FFF_FFFF, 0x8000_0000, 0xFFFF_0000, 0xFFFF_FFFE, 0xFFFF_FFFF];

const FELT_TINY: [u64; 10] = [0, 1, 1, 2, 2, 3, 4, 5, 7, 8];
const FELT_DIVISORS: [u64; 8] = [0, 0, 1, 1, 2, 3, 4, 0x1_0000_0000];
const FELT_EDGES: [u64; 10] = [
    0,
    1,
    2,
    0xFFFF_FFFF,
    0x1_0000_0000,
    0x1_0000_0001,
    1 << 63,
    (GOLDILOCKS_P - 2) as u64,
    (GOLDILOCKS_P - 1) as u64,
    (GOLDILOCKS_P - 0x1_0000_0000) as u64,
];

// ---------- deterministic RNG ----------

/// xorshift64* seeded through a SplitMix64 warm-up: neighboring seeds give
/// independent streams and the all-zero fixed point is unreachable.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self((z ^ (z >> 31)).max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn boolean(&mut self) -> bool {
        self.next_u64() & 1 == 0
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

// ---------- graph model ----------

#[derive(Clone, Copy, PartialEq, Eq)]
enum U32Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

impl U32Op {
    fn symbol(self) -> &'static str {
        match self {
            U32Op::Add => "+",
            U32Op::Sub => "-",
            U32Op::Mul => "*",
            U32Op::Div => "/",
            U32Op::Mod => "%",
            U32Op::Pow => "**",
            U32Op::And => "&",
            U32Op::Or => "|",
            U32Op::Xor => "^",
            U32Op::Shl => "<<",
            U32Op::Shr => ">>",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FeltOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

impl FeltOp {
    fn symbol(self) -> &'static str {
        match self {
            FeltOp::Add => "+",
            FeltOp::Sub => "-",
            FeltOp::Mul => "*",
            FeltOp::Div => "/",
            FeltOp::Mod => "%",
            FeltOp::Pow => "**",
        }
    }
}

#[derive(Clone, Copy)]
enum CmpOp {
    Lt,
    Lte,
    Gt,
    Gte,
    Eq,
    Neq,
}

impl CmpOp {
    fn symbol(self) -> &'static str {
        match self {
            CmpOp::Lt => "<",
            CmpOp::Lte => "<=",
            CmpOp::Gt => ">",
            CmpOp::Gte => ">=",
            CmpOp::Eq => "==",
            CmpOp::Neq => "!=",
        }
    }
}

const CMP_OPS: [CmpOp; 6] = [CmpOp::Lt, CmpOp::Lte, CmpOp::Gt, CmpOp::Gte, CmpOp::Eq, CmpOp::Neq];

/// Sema only admits equality and logical ops on bools — ordered
/// comparisons (`< <= > >=`) are Felt/u32-only.
#[derive(Clone, Copy)]
enum BoolOp {
    Eq,
    Neq,
    And,
    Or,
    Xor,
}

impl BoolOp {
    fn symbol(self) -> &'static str {
        match self {
            BoolOp::Eq => "==",
            BoolOp::Neq => "!=",
            BoolOp::And => "&&",
            BoolOp::Or => "||",
            BoolOp::Xor => "^",
        }
    }
}

const BOOL_OPS: [BoolOp; 5] = [BoolOp::Eq, BoolOp::Neq, BoolOp::And, BoolOp::Or, BoolOp::Xor];

#[derive(Clone, Copy)]
enum U32Ref {
    Input(usize),
    Val(usize),
    Lit(u32),
}

#[derive(Clone, Copy)]
enum FeltRef {
    Input(usize),
    Val(usize),
    Lit(u64),
}

#[derive(Clone, Copy)]
enum BoolRef {
    Val(usize),
    Lit(bool),
}

enum U32Expr {
    Ref(U32Ref),
    Bin {
        op: U32Op,
        lhs: U32Ref,
        rhs: U32Ref,
    },
}

enum FeltExpr {
    Ref(FeltRef),
    Bin {
        op: FeltOp,
        lhs: FeltRef,
        rhs: FeltRef,
    },
    Neg(FeltRef),
}

enum U32Node {
    Bin {
        op: U32Op,
        lhs: U32Ref,
        rhs: U32Ref,
    },
    Select {
        cond: BoolRef,
        then: U32Expr,
        els: U32Expr,
    },
}

enum FeltNode {
    Bin {
        op: FeltOp,
        lhs: FeltRef,
        rhs: FeltRef,
    },
    Neg(FeltRef),
    Select {
        cond: BoolRef,
        then: FeltExpr,
        els: FeltExpr,
    },
}

enum BoolNode {
    CmpU32 {
        op: CmpOp,
        lhs: U32Ref,
        rhs: U32Ref,
    },
    CmpFelt {
        op: CmpOp,
        lhs: FeltRef,
        rhs: FeltRef,
    },
    Bin {
        op: BoolOp,
        lhs: BoolRef,
        rhs: BoolRef,
    },
    Not(BoolRef),
}

/// Casts between the three primitive types; out-of-range constants are
/// clean interpreter errors (bool wants 0/1, u32 wants <= 0xffffffff).
#[derive(Clone, Copy)]
enum CastKind {
    U32ToFelt,
    FeltToU32,
    U32ToBool,
    FeltToBool,
    BoolToFelt,
    BoolToU32,
}

const CAST_KINDS: [CastKind; 6] = [
    CastKind::U32ToFelt,
    CastKind::FeltToU32,
    CastKind::U32ToBool,
    CastKind::FeltToBool,
    CastKind::BoolToFelt,
    CastKind::BoolToU32,
];

enum Stmt {
    U32(U32Node),
    Felt(FeltNode),
    Bool(BoolNode),
    /// Cast the referenced value; the statement's own type is the target.
    Cast {
        kind: CastKind,
        src_u32: Option<U32Ref>,
        src_felt: Option<FeltRef>,
        src_bool: Option<BoolRef>,
    },
}

/// A concrete numeric entry argument.
#[derive(Clone, Copy, Debug)]
enum Arg {
    U32(u32),
    Felt(u64),
}

struct Graph {
    seed: u64,
    params: Vec<Arg>,
    /// (name, type) of the numeric binding each statement produces:
    /// "v{i}" for u32, "f{j}" for felt, "p{k}" for bool.
    stmts: Vec<Stmt>,
    u32_count: usize,
    felt_count: usize,
    bool_count: usize,
}

// ---------- generation ----------

fn arith_literal(rng: &mut Rng) -> u32 {
    if rng.chance(55) {
        *rng.pick(&TINY)
    } else if rng.chance(35) {
        *rng.pick(&SMALL)
    } else if rng.chance(20) {
        *rng.pick(&EDGES)
    } else {
        rng.next_u64() as u32
    }
}

fn felt_literal(rng: &mut Rng) -> u64 {
    if rng.chance(55) {
        *rng.pick(&FELT_TINY)
    } else if rng.chance(25) {
        *rng.pick(&FELT_EDGES)
    } else {
        rng.next_u64() % (GOLDILOCKS_P as u64)
    }
}

fn u32_input_value(rng: &mut Rng) -> u32 {
    if rng.chance(40) {
        *rng.pick(&[0u32, 1, 2])
    } else if rng.chance(70) {
        *rng.pick(&SMALL)
    } else if rng.chance(8) {
        *rng.pick(&EDGES)
    } else {
        rng.next_u64() as u32
    }
}

fn felt_input_value(rng: &mut Rng) -> u64 {
    if rng.chance(40) {
        *rng.pick(&[0u64, 1, 2])
    } else if rng.chance(55) {
        *rng.pick(&FELT_TINY)
    } else if rng.chance(10) {
        *rng.pick(&FELT_EDGES)
    } else {
        rng.next_u64() % (GOLDILOCKS_P as u64)
    }
}

fn pick_u32_op(rng: &mut Rng) -> U32Op {
    const WEIGHTED: [(U32Op, u64); 11] = [
        (U32Op::Add, 17),
        (U32Op::Sub, 17),
        (U32Op::Mul, 10),
        (U32Op::Div, 6),
        (U32Op::Mod, 6),
        (U32Op::Pow, 5),
        (U32Op::And, 8),
        (U32Op::Or, 8),
        (U32Op::Xor, 8),
        (U32Op::Shl, 8),
        (U32Op::Shr, 7),
    ];
    const TOTAL: u64 = 17 + 17 + 10 + 6 + 6 + 5 + 8 + 8 + 8 + 8 + 7;
    let mut pick = rng.below(TOTAL);
    for (op, weight) in WEIGHTED {
        if pick < weight {
            return op;
        }
        pick -= weight;
    }
    unreachable!()
}

fn pick_felt_op(rng: &mut Rng) -> FeltOp {
    const WEIGHTED: [(FeltOp, u64); 6] = [
        (FeltOp::Add, 18),
        (FeltOp::Sub, 18),
        (FeltOp::Mul, 16),
        (FeltOp::Div, 8),
        (FeltOp::Mod, 8),
        (FeltOp::Pow, 6),
    ];
    const TOTAL: u64 = 18 + 18 + 16 + 8 + 8 + 6;
    let mut pick = rng.below(TOTAL);
    for (op, weight) in WEIGHTED {
        if pick < weight {
            return op;
        }
        pick -= weight;
    }
    unreachable!()
}

/// A runtime u32 operand: an input or an earlier binding. Bindings are
/// biased toward recent ones so the graph chains instead of degenerating
/// into independent leaves.
fn u32_ref_only(rng: &mut Rng, u32_count: usize, u32_params: usize) -> U32Ref {
    let total = u32_count + u32_params;
    let idx = if total <= 5 || rng.chance(55) {
        rng.below(total as u64) as usize
    } else {
        total - 1 - rng.below(5) as usize
    };
    if idx < u32_params {
        U32Ref::Input(idx)
    } else {
        U32Ref::Val(idx - u32_params)
    }
}

fn felt_ref_only(rng: &mut Rng, felt_count: usize, felt_params: usize) -> FeltRef {
    let total = felt_count + felt_params;
    if total == 0 {
        return FeltRef::Lit(felt_literal(rng));
    }
    let idx = if total <= 5 || rng.chance(55) {
        rng.below(total as u64) as usize
    } else {
        total - 1 - rng.below(5) as usize
    };
    if idx < felt_params {
        FeltRef::Input(idx)
    } else {
        FeltRef::Val(idx - felt_params)
    }
}

fn u32_operand(rng: &mut Rng, u32_count: usize, u32_params: usize) -> U32Ref {
    if u32_count + u32_params == 0 || rng.chance(45) {
        U32Ref::Lit(arith_literal(rng))
    } else {
        u32_ref_only(rng, u32_count, u32_params)
    }
}

fn felt_operand(rng: &mut Rng, felt_count: usize, felt_params: usize) -> FeltRef {
    if felt_count + felt_params == 0 || rng.chance(45) {
        FeltRef::Lit(felt_literal(rng))
    } else {
        felt_ref_only(rng, felt_count, felt_params)
    }
}

fn bool_operand(rng: &mut Rng, bool_count: usize) -> BoolRef {
    if bool_count == 0 || rng.chance(25) {
        BoolRef::Lit(rng.boolean())
    } else {
        BoolRef::Val(rng.below(bool_count as u64) as usize)
    }
}

fn u32_bin_rhs(rng: &mut Rng, op: U32Op, u32_count: usize, u32_params: usize) -> U32Ref {
    match op {
        U32Op::Shl | U32Op::Shr => U32Ref::Lit(*rng.pick(&SHIFT_AMOUNTS)),
        U32Op::Pow => U32Ref::Lit(*rng.pick(&POW_EXPONENTS)),
        U32Op::Div | U32Op::Mod => {
            if rng.chance(70) {
                U32Ref::Lit(*rng.pick(&DIVISORS))
            } else {
                u32_ref_only(rng, u32_count, u32_params)
            }
        }
        _ => u32_operand(rng, u32_count, u32_params),
    }
}

fn felt_bin_rhs(rng: &mut Rng, op: FeltOp, felt_count: usize, felt_params: usize) -> FeltRef {
    match op {
        FeltOp::Div | FeltOp::Mod => {
            if rng.chance(60) {
                FeltRef::Lit(*rng.pick(&FELT_DIVISORS))
            } else {
                felt_ref_only(rng, felt_count, felt_params)
            }
        }
        _ => felt_operand(rng, felt_count, felt_params),
    }
}

/// At least one operand is a runtime value so sema's const-item folding
/// never kicks in; evaluation stays entirely on the interpret path.
fn gen_u32_bin_expr(rng: &mut Rng, u32_count: usize, u32_params: usize) -> U32Expr {
    let op = pick_u32_op(rng);
    let lhs = u32_ref_only(rng, u32_count, u32_params);
    let rhs = u32_bin_rhs(rng, op, u32_count, u32_params);
    U32Expr::Bin { op, lhs, rhs }
}

fn gen_felt_bin_expr(rng: &mut Rng, felt_count: usize, felt_params: usize) -> FeltExpr {
    let op = pick_felt_op(rng);
    let lhs = felt_ref_only(rng, felt_count, felt_params);
    let rhs = felt_bin_rhs(rng, op, felt_count, felt_params);
    FeltExpr::Bin { op, lhs, rhs }
}

struct Counts {
    u32_count: usize,
    felt_count: usize,
    bool_count: usize,
    u32_params: usize,
    felt_params: usize,
}

fn gen_u32_node(rng: &mut Rng, c: &Counts) -> U32Node {
    if c.bool_count > 0 && rng.chance(12) {
        return U32Node::Select {
            cond: bool_operand(rng, c.bool_count),
            then: gen_u32_bin_expr(rng, c.u32_count, c.u32_params),
            els: gen_u32_bin_expr(rng, c.u32_count, c.u32_params),
        };
    }
    let U32Expr::Bin { op, lhs, rhs } = gen_u32_bin_expr(rng, c.u32_count, c.u32_params) else {
        unreachable!()
    };
    U32Node::Bin { op, lhs, rhs }
}

fn gen_felt_node(rng: &mut Rng, c: &Counts) -> FeltNode {
    if c.bool_count > 0 && rng.chance(12) {
        return FeltNode::Select {
            cond: bool_operand(rng, c.bool_count),
            then: gen_felt_bin_expr(rng, c.felt_count, c.felt_params),
            els: gen_felt_bin_expr(rng, c.felt_count, c.felt_params),
        };
    }
    if c.felt_count + c.felt_params > 0 && rng.chance(8) {
        return FeltNode::Neg(felt_ref_only(rng, c.felt_count, c.felt_params));
    }
    let FeltExpr::Bin { op, lhs, rhs } = gen_felt_bin_expr(rng, c.felt_count, c.felt_params) else {
        unreachable!()
    };
    FeltNode::Bin { op, lhs, rhs }
}

fn gen_bool_node(rng: &mut Rng, c: &Counts) -> BoolNode {
    if rng.chance(65) {
        // Comparison over u32 or felt operands.
        let use_felt = (c.felt_count + c.felt_params) > 0 && rng.chance(45);
        let op = *rng.pick(&CMP_OPS);
        if use_felt {
            BoolNode::CmpFelt {
                op,
                lhs: felt_ref_only(rng, c.felt_count, c.felt_params),
                rhs: felt_operand(rng, c.felt_count, c.felt_params),
            }
        } else if c.u32_count + c.u32_params > 0 {
            BoolNode::CmpU32 {
                op,
                lhs: u32_ref_only(rng, c.u32_count, c.u32_params),
                rhs: u32_operand(rng, c.u32_count, c.u32_params),
            }
        } else {
            BoolNode::CmpFelt {
                op,
                lhs: felt_ref_only(rng, c.felt_count, c.felt_params),
                rhs: felt_operand(rng, c.felt_count, c.felt_params),
            }
        }
    } else {
        BoolNode::Bin {
            op: *rng.pick(&BOOL_OPS),
            lhs: bool_operand(rng, c.bool_count),
            rhs: bool_operand(rng, c.bool_count),
        }
    }
}

/// `kind` comes from the caller so availability checks and binding counts
/// stay in sync with what is actually generated.
fn gen_cast(rng: &mut Rng, c: &Counts, kind: CastKind) -> Stmt {
    match kind {
        CastKind::U32ToFelt => Stmt::Cast {
            kind,
            src_u32: Some(u32_ref_only(rng, c.u32_count, c.u32_params)),
            src_felt: None,
            src_bool: None,
        },
        CastKind::FeltToU32 => Stmt::Cast {
            kind,
            src_u32: None,
            src_felt: Some(felt_ref_only(rng, c.felt_count, c.felt_params)),
            src_bool: None,
        },
        CastKind::U32ToBool => Stmt::Cast {
            kind,
            src_u32: Some(u32_ref_only(rng, c.u32_count, c.u32_params)),
            src_felt: None,
            src_bool: None,
        },
        CastKind::FeltToBool => Stmt::Cast {
            kind,
            src_u32: None,
            src_felt: Some(felt_ref_only(rng, c.felt_count, c.felt_params)),
            src_bool: None,
        },
        CastKind::BoolToFelt | CastKind::BoolToU32 => Stmt::Cast {
            kind,
            src_u32: None,
            src_felt: None,
            src_bool: Some(bool_operand(rng, c.bool_count)),
        },
    }
}

fn gen_graph(rng: &mut Rng, seed: u64) -> Graph {
    let n_params = 2 + rng.below(3) as usize;
    let params: Vec<Arg> = (0..n_params)
        .map(|_| {
            if rng.chance(60) {
                Arg::U32(u32_input_value(rng))
            } else {
                Arg::Felt(felt_input_value(rng))
            }
        })
        .collect();
    let u32_params = params.iter().filter(|p| matches!(p, Arg::U32(_))).count();
    let felt_params = params.len() - u32_params;

    let n_numeric = 6 + rng.below(15) as usize;
    let n_bool = 2 + rng.below(7) as usize;
    let mut stmts: Vec<Stmt> = Vec::with_capacity(n_numeric + n_bool);
    let (mut numeric_left, mut bool_left) = (n_numeric, n_bool);
    let mut c = Counts { u32_count: 0, felt_count: 0, bool_count: 0, u32_params, felt_params };
    while numeric_left > 0 || bool_left > 0 {
        let emit_bool = bool_left > 0 && (numeric_left == 0 || rng.chance(35));
        if emit_bool {
            stmts.push(Stmt::Bool(gen_bool_node(rng, &c)));
            bool_left -= 1;
            c.bool_count += 1;
        } else if rng.chance(10) {
            // Casts need a source of the right kind; fall back to a plain
            // binding of the target type when none exists yet.
            let kind = *rng.pick(&CAST_KINDS);
            let has_source = match kind {
                CastKind::U32ToFelt | CastKind::U32ToBool => c.u32_count + u32_params > 0,
                CastKind::FeltToU32 | CastKind::FeltToBool => c.felt_count + felt_params > 0,
                CastKind::BoolToFelt | CastKind::BoolToU32 => true,
            };
            if has_source {
                stmts.push(gen_cast(rng, &c, kind));
                match kind {
                    CastKind::U32ToFelt | CastKind::BoolToFelt => c.felt_count += 1,
                    CastKind::FeltToU32 | CastKind::BoolToU32 => c.u32_count += 1,
                    CastKind::U32ToBool | CastKind::FeltToBool => c.bool_count += 1,
                }
                numeric_left -= 1;
                continue;
            }
            // Fall through to a plain numeric binding.
            let pick_u32 = c.u32_count + u32_params > 0 && rng.chance(55);
            if pick_u32 {
                stmts.push(Stmt::U32(gen_u32_node(rng, &c)));
                c.u32_count += 1;
            } else {
                stmts.push(Stmt::Felt(gen_felt_node(rng, &c)));
                c.felt_count += 1;
            }
            numeric_left -= 1;
        } else if c.u32_count + u32_params > 0 && rng.chance(55) {
            stmts.push(Stmt::U32(gen_u32_node(rng, &c)));
            c.u32_count += 1;
            numeric_left -= 1;
        } else {
            stmts.push(Stmt::Felt(gen_felt_node(rng, &c)));
            c.felt_count += 1;
            numeric_left -= 1;
        }
    }
    Graph { seed, params, stmts, u32_count: c.u32_count, felt_count: c.felt_count, bool_count: c.bool_count }
}

// ---------- psy source emission ----------

fn u32_ref_src(r: U32Ref, u32_params: usize, u32_seen: &[usize]) -> String {
    match r {
        U32Ref::Input(i) => PARAM_NAMES[u32_seen[i]].to_string(),
        U32Ref::Val(i) => format!("v{i}"),
        U32Ref::Lit(x) => format!("{x}u32"),
    }
}

fn felt_ref_src(r: FeltRef, felt_params: usize, felt_seen: &[usize]) -> String {
    match r {
        FeltRef::Input(i) => PARAM_NAMES[felt_seen[i]].to_string(),
        FeltRef::Val(i) => format!("f{i}"),
        FeltRef::Lit(x) => format!("{x}"),
    }
}

fn bool_ref_src(r: BoolRef) -> String {
    match r {
        BoolRef::Val(i) => format!("p{i}"),
        BoolRef::Lit(b) => b.to_string(),
    }
}

fn u32_expr_src(e: &U32Expr, u32_params: usize, u32_seen: &[usize]) -> String {
    match e {
        U32Expr::Ref(r) => u32_ref_src(*r, u32_params, u32_seen),
        U32Expr::Bin { op, lhs, rhs } => {
            format!("({} {} {})", u32_ref_src(*lhs, u32_params, u32_seen), op.symbol(), u32_ref_src(*rhs, u32_params, u32_seen))
        }
    }
}

fn felt_expr_src(e: &FeltExpr, felt_params: usize, felt_seen: &[usize]) -> String {
    match e {
        FeltExpr::Ref(r) => felt_ref_src(*r, felt_params, felt_seen),
        FeltExpr::Bin { op, lhs, rhs } => {
            format!("({} {} {})", felt_ref_src(*lhs, felt_params, felt_seen), op.symbol(), felt_ref_src(*rhs, felt_params, felt_seen))
        }
        FeltExpr::Neg(r) => format!("-{}", felt_ref_src(*r, felt_params, felt_seen)),
    }
}

fn gen_source(g: &Graph) -> String {
    // Parameter positions per kind, for Input refs.
    let u32_seen: Vec<usize> = g
        .params
        .iter()
        .enumerate()
        .filter(|(_, p)| matches!(p, Arg::U32(_)))
        .map(|(i, _)| i)
        .collect();
    let felt_seen: Vec<usize> = g
        .params
        .iter()
        .enumerate()
        .filter(|(_, p)| matches!(p, Arg::Felt(_)))
        .map(|(i, _)| i)
        .collect();
    let u32_params = u32_seen.len();
    let felt_params = felt_seen.len();

    let inputs = g
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| match p {
            Arg::U32(v) => format!("{}={v}u32", PARAM_NAMES[i]),
            Arg::Felt(v) => format!("{}={v}", PARAM_NAMES[i]),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let sig = g
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| match p {
            Arg::U32(_) => format!("{}: u32", PARAM_NAMES[i]),
            Arg::Felt(_) => format!("{}: Felt", PARAM_NAMES[i]),
        })
        .collect::<Vec<_>>()
        .join(", ");
    // Return type = type of the last numeric statement (there is always at
    // least one: n_numeric >= 6). Bool-producing casts count as bool here,
    // matching the emitter's binding names.
    let ret_ty = g
        .stmts
        .iter()
        .rev()
        .map(|s| match s {
            Stmt::U32(_) => "u32",
            Stmt::Felt(_) => "Felt",
            Stmt::Bool(_) => "bool",
            Stmt::Cast { kind, .. } => match kind {
                CastKind::U32ToFelt | CastKind::BoolToFelt => "Felt",
                CastKind::FeltToU32 | CastKind::BoolToU32 => "u32",
                CastKind::U32ToBool | CastKind::FeltToBool => "bool",
            },
        })
        .find(|t| *t != "bool")
        .unwrap_or("u32");

    let mut out = String::new();
    out.push_str("// randomly generated differential-test program (interpreter vs native Rust)\n");
    out.push_str(&format!("// seed: {}\n// inputs: {inputs}\n\n", g.seed));
    out.push_str(&format!("fn main({sig}) -> {ret_ty} {{\n"));
    let (mut v, mut f, mut p) = (0usize, 0usize, 0usize);
    let mut last_numeric = String::new();
    for stmt in &g.stmts {
        match stmt {
            Stmt::U32(node) => {
                let rhs = match node {
                    U32Node::Bin { op, lhs, rhs } => format!(
                        "({} {} {})",
                        u32_ref_src(*lhs, u32_params, &u32_seen),
                        op.symbol(),
                        u32_ref_src(*rhs, u32_params, &u32_seen)
                    ),
                    U32Node::Select { cond, then, els } => format!(
                        "match {} {{ true => {}, _ => {} }}",
                        bool_ref_src(*cond),
                        u32_expr_src(then, u32_params, &u32_seen),
                        u32_expr_src(els, u32_params, &u32_seen)
                    ),
                };
                out.push_str(&format!("    let v{v}: u32 = {rhs};\n"));
                last_numeric = format!("v{v}");
                v += 1;
            }
            Stmt::Felt(node) => {
                let rhs = match node {
                    FeltNode::Bin { op, lhs, rhs } => format!(
                        "({} {} {})",
                        felt_ref_src(*lhs, felt_params, &felt_seen),
                        op.symbol(),
                        felt_ref_src(*rhs, felt_params, &felt_seen)
                    ),
                    FeltNode::Neg(r) => format!("-{}", felt_ref_src(*r, felt_params, &felt_seen)),
                    FeltNode::Select { cond, then, els } => format!(
                        "match {} {{ true => {}, _ => {} }}",
                        bool_ref_src(*cond),
                        felt_expr_src(then, felt_params, &felt_seen),
                        felt_expr_src(els, felt_params, &felt_seen)
                    ),
                };
                out.push_str(&format!("    let f{f}: Felt = {rhs};\n"));
                last_numeric = format!("f{f}");
                f += 1;
            }
            Stmt::Bool(node) => {
                let rhs = match node {
                    BoolNode::CmpU32 { op, lhs, rhs } => format!(
                        "({} {} {})",
                        u32_ref_src(*lhs, u32_params, &u32_seen),
                        op.symbol(),
                        u32_ref_src(*rhs, u32_params, &u32_seen)
                    ),
                    BoolNode::CmpFelt { op, lhs, rhs } => format!(
                        "({} {} {})",
                        felt_ref_src(*lhs, felt_params, &felt_seen),
                        op.symbol(),
                        felt_ref_src(*rhs, felt_params, &felt_seen)
                    ),
                    BoolNode::Bin { op, lhs, rhs } => {
                        format!("({} {} {})", bool_ref_src(*lhs), op.symbol(), bool_ref_src(*rhs))
                    }
                    BoolNode::Not(x) => format!("!{}", bool_ref_src(*x)),
                };
                out.push_str(&format!("    let p{p}: bool = {rhs};\n"));
                p += 1;
            }
            Stmt::Cast { kind, src_u32, src_felt, src_bool } => {
                let (src, target, name) = match kind {
                    CastKind::U32ToFelt => (u32_ref_src(src_u32.unwrap(), u32_params, &u32_seen), "Felt", 'f'),
                    CastKind::FeltToU32 => (felt_ref_src(src_felt.unwrap(), felt_params, &felt_seen), "u32", 'v'),
                    CastKind::U32ToBool => (u32_ref_src(src_u32.unwrap(), u32_params, &u32_seen), "bool", 'p'),
                    CastKind::FeltToBool => (felt_ref_src(src_felt.unwrap(), felt_params, &felt_seen), "bool", 'p'),
                    CastKind::BoolToFelt => (bool_ref_src(src_bool.unwrap()), "Felt", 'f'),
                    CastKind::BoolToU32 => (bool_ref_src(src_bool.unwrap()), "u32", 'v'),
                };
                let idx = match name {
                    'f' => {
                        let i = f;
                        f += 1;
                        i
                    }
                    'v' => {
                        let i = v;
                        v += 1;
                        i
                    }
                    _ => {
                        let i = p;
                        p += 1;
                        i
                    }
                };
                out.push_str(&format!("    let {name}{idx}: {target} = {src} as {target};\n"));
                if name != 'p' {
                    last_numeric = format!("{name}{idx}");
                }
            }
        }
    }
    out.push_str(&format!("    return {last_numeric};\n"));
    out.push_str("}\n");
    out
}

// ---------- native Rust mirror ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MirrorErr {
    Overflow,
    DivZero,
    InvalidCast,
}

/// Goldilocks field arithmetic over u128.
fn f_add(a: u64, b: u64) -> u64 {
    ((a as u128 + b as u128) % GOLDILOCKS_P) as u64
}

fn f_sub(a: u64, b: u64) -> u64 {
    ((GOLDILOCKS_P + a as u128 - b as u128) % GOLDILOCKS_P) as u64
}

fn f_mul(a: u64, b: u64) -> u64 {
    ((a as u128 * b as u128) % GOLDILOCKS_P) as u64
}

fn f_pow(a: u64, e: u64) -> u64 {
    let mut result: u128 = 1;
    let mut base = (a as u128) % GOLDILOCKS_P;
    let mut exp = e as u128;
    while exp > 0 {
        if exp & 1 == 1 {
            result = result * base % GOLDILOCKS_P;
        }
        base = base * base % GOLDILOCKS_P;
        exp >>= 1;
    }
    result as u64
}

fn f_div(a: u64, b: u64) -> u64 {
    // Field division: multiply by the inverse (Fermat: a^(p-2)).
    f_mul(a, f_pow(b, (GOLDILOCKS_P - 2) as u64))
}

fn f_neg(a: u64) -> u64 {
    ((GOLDILOCKS_P - a as u128 % GOLDILOCKS_P) % GOLDILOCKS_P) as u64
}

/// Mirrors the interpreter's constant pre-check exactly (lib.rs
/// `interpret_binary`): u32 add/sub/mul/pow on constants overflow-check in
/// u64 and div/mod by a zero constant is a clean error.
fn apply_op(op: U32Op, l: u32, r: u32) -> Result<u32, MirrorErr> {
    match op {
        U32Op::Add => l.checked_add(r).ok_or(MirrorErr::Overflow),
        U32Op::Sub => l.checked_sub(r).ok_or(MirrorErr::Overflow),
        U32Op::Mul => l.checked_mul(r).ok_or(MirrorErr::Overflow),
        U32Op::Div => {
            if r == 0 {
                Err(MirrorErr::DivZero)
            } else {
                Ok(l / r)
            }
        }
        U32Op::Mod => {
            if r == 0 {
                Err(MirrorErr::DivZero)
            } else {
                Ok(l % r)
            }
        }
        U32Op::Pow => match (l as u64).checked_pow(r as u32) {
            Some(value) if value <= 0xffff_ffff => Ok(value as u32),
            _ => Err(MirrorErr::Overflow),
        },
        U32Op::And => Ok(l & r),
        U32Op::Or => Ok(l | r),
        U32Op::Xor => Ok(l ^ r),
        // Mirrors the fixed VM fold: every bit leaves the 32-bit window once
        // the distance reaches 32, so distances >= 32 fold to 0.
        U32Op::Shl | U32Op::Shr => Ok(if r >= 32 { 0 } else if op == U32Op::Shl { l << r } else { l >> r }),
    }
}

fn apply_felt_op(op: FeltOp, l: u64, r: u64) -> Result<u64, MirrorErr> {
    match op {
        FeltOp::Add => Ok(f_add(l, r)),
        FeltOp::Sub => Ok(f_sub(l, r)),
        FeltOp::Mul => Ok(f_mul(l, r)),
        FeltOp::Div => {
            if r == 0 {
                Err(MirrorErr::DivZero)
            } else {
                Ok(f_div(l, r))
            }
        }
        FeltOp::Mod => {
            if r == 0 {
                Err(MirrorErr::DivZero)
            } else {
                Ok(l % r)
            }
        }
        FeltOp::Pow => Ok(f_pow(l, r)),
    }
}

fn apply_cmp(op: CmpOp, l: u64, r: u64) -> bool {
    match op {
        CmpOp::Lt => l < r,
        CmpOp::Lte => l <= r,
        CmpOp::Gt => l > r,
        CmpOp::Gte => l >= r,
        CmpOp::Eq => l == r,
        CmpOp::Neq => l != r,
    }
}

fn apply_bool_op(op: BoolOp, l: bool, r: bool) -> bool {
    match op {
        BoolOp::Eq => l == r,
        BoolOp::Neq => l != r,
        BoolOp::And => l && r,
        BoolOp::Or => l || r,
        BoolOp::Xor => l ^ r,
    }
}

/// Numeric result of a fully evaluated graph: canonical u64 (u32 values
/// zero-extend).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Num {
    U32(u32),
    Felt(u64),
}

impl Num {
    fn canonical(self) -> u64 {
        match self {
            Num::U32(v) => v as u64,
            Num::Felt(v) => v,
        }
    }
}

struct Mirror<'g> {
    u32_inputs: Vec<u32>,
    felt_inputs: Vec<u64>,
    u32_vals: Vec<u32>,
    felt_vals: Vec<u64>,
    bool_vals: Vec<bool>,
    params: &'g [Arg],
}

impl<'g> Mirror<'g> {
    fn u32_ref(&self, r: U32Ref) -> u32 {
        match r {
            U32Ref::Input(i) => self.u32_inputs[i],
            U32Ref::Val(i) => self.u32_vals[i],
            U32Ref::Lit(x) => x,
        }
    }

    fn felt_ref(&self, r: FeltRef) -> u64 {
        match r {
            FeltRef::Input(i) => self.felt_inputs[i],
            FeltRef::Val(i) => self.felt_vals[i],
            FeltRef::Lit(x) => x,
        }
    }

    fn bool_ref(&self, r: BoolRef) -> bool {
        match r {
            BoolRef::Val(i) => self.bool_vals[i],
            BoolRef::Lit(b) => b,
        }
    }

    fn u32_expr(&self, e: &U32Expr) -> Result<u32, MirrorErr> {
        match e {
            U32Expr::Ref(r) => Ok(self.u32_ref(*r)),
            U32Expr::Bin { op, lhs, rhs } => apply_op(*op, self.u32_ref(*lhs), self.u32_ref(*rhs)),
        }
    }

    fn felt_expr(&self, e: &FeltExpr) -> Result<u64, MirrorErr> {
        match e {
            FeltExpr::Ref(r) => Ok(self.felt_ref(*r)),
            FeltExpr::Bin { op, lhs, rhs } => apply_felt_op(*op, self.felt_ref(*lhs), self.felt_ref(*rhs)),
            FeltExpr::Neg(r) => Ok(f_neg(self.felt_ref(*r))),
        }
    }

    fn u32_node(&self, node: &U32Node) -> Result<u32, MirrorErr> {
        match node {
            U32Node::Bin { op, lhs, rhs } => apply_op(*op, self.u32_ref(*lhs), self.u32_ref(*rhs)),
            // The interpreter evaluates every match arm eagerly (first the
            // pattern arm, then the wildcard), so an error in the untaken
            // arm still aborts — the mirror must do the same.
            U32Node::Select { cond, then, els } => {
                let taken = self.u32_expr(then)?;
                let untaken = self.u32_expr(els)?;
                Ok(if self.bool_ref(*cond) { taken } else { untaken })
            }
        }
    }

    fn felt_node(&self, node: &FeltNode) -> Result<u64, MirrorErr> {
        match node {
            FeltNode::Bin { op, lhs, rhs } => apply_felt_op(*op, self.felt_ref(*lhs), self.felt_ref(*rhs)),
            FeltNode::Neg(r) => Ok(f_neg(self.felt_ref(*r))),
            FeltNode::Select { cond, then, els } => {
                let taken = self.felt_expr(then)?;
                let untaken = self.felt_expr(els)?;
                Ok(if self.bool_ref(*cond) { taken } else { untaken })
            }
        }
    }

    fn bool_node(&self, node: &BoolNode) -> bool {
        match node {
            BoolNode::CmpU32 { op, lhs, rhs } => apply_cmp(*op, self.u32_ref(*lhs) as u64, self.u32_ref(*rhs) as u64),
            BoolNode::CmpFelt { op, lhs, rhs } => apply_cmp(*op, self.felt_ref(*lhs), self.felt_ref(*rhs)),
            BoolNode::Bin { op, lhs, rhs } => apply_bool_op(*op, self.bool_ref(*lhs), self.bool_ref(*rhs)),
            BoolNode::Not(x) => !self.bool_ref(*x),
        }
    }

    fn cast(&self, kind: CastKind, src_u32: Option<U32Ref>, src_felt: Option<FeltRef>, src_bool: Option<BoolRef>) -> Result<CastVal, MirrorErr> {
        // Mirrors interpret_cast's constant range checks.
        match kind {
            CastKind::U32ToFelt => Ok(CastVal::Felt(self.u32_ref(src_u32.unwrap()) as u64)),
            CastKind::FeltToU32 => match self.felt_ref(src_felt.unwrap()) {
                v if v <= 0xffff_ffff => Ok(CastVal::U32(v as u32)),
                _ => Err(MirrorErr::InvalidCast),
            },
            CastKind::U32ToBool => match self.u32_ref(src_u32.unwrap()) {
                0 => Ok(CastVal::Bool(false)),
                1 => Ok(CastVal::Bool(true)),
                _ => Err(MirrorErr::InvalidCast),
            },
            CastKind::FeltToBool => match self.felt_ref(src_felt.unwrap()) {
                0 => Ok(CastVal::Bool(false)),
                1 => Ok(CastVal::Bool(true)),
                _ => Err(MirrorErr::InvalidCast),
            },
            CastKind::BoolToFelt => Ok(CastVal::Felt(self.bool_ref(src_bool.unwrap()) as u64)),
            CastKind::BoolToU32 => Ok(CastVal::U32(self.bool_ref(src_bool.unwrap()) as u32)),
        }
    }
}

/// Result of a cast, tagged by the binding pool it lands in.
#[derive(Clone, Copy)]
enum CastVal {
    U32(u32),
    Felt(u64),
    Bool(bool),
}

fn mirror_eval(g: &Graph) -> Result<Num, MirrorErr> {
    let u32_inputs: Vec<u32> = g
        .params
        .iter()
        .filter_map(|p| match p {
            Arg::U32(v) => Some(*v),
            Arg::Felt(_) => None,
        })
        .collect();
    let felt_inputs: Vec<u64> = g
        .params
        .iter()
        .filter_map(|p| match p {
            Arg::Felt(v) => Some(*v),
            Arg::U32(_) => None,
        })
        .collect();
    let mut m = Mirror {
        u32_inputs,
        felt_inputs,
        u32_vals: Vec::new(),
        felt_vals: Vec::new(),
        bool_vals: Vec::new(),
        params: &g.params,
    };
    let mut result = Ok(Num::U32(0));
    for stmt in &g.stmts {
        match stmt {
            Stmt::U32(node) => {
                let value = m.u32_node(node)?;
                m.u32_vals.push(value);
                result = Ok(Num::U32(value));
            }
            Stmt::Felt(node) => {
                let value = m.felt_node(node)?;
                m.felt_vals.push(value);
                result = Ok(Num::Felt(value));
            }
            Stmt::Bool(node) => {
                let value = m.bool_node(node);
                m.bool_vals.push(value);
            }
            Stmt::Cast { kind, src_u32, src_felt, src_bool } => {
                match m.cast(*kind, *src_u32, *src_felt, *src_bool)? {
                    CastVal::U32(v) => {
                        m.u32_vals.push(v);
                        result = Ok(Num::U32(v));
                    }
                    CastVal::Felt(v) => {
                        m.felt_vals.push(v);
                        result = Ok(Num::Felt(v));
                    }
                    CastVal::Bool(v) => m.bool_vals.push(v),
                }
            }
        }
    }
    result
}

// ---------- psy interpreter harness ----------

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic>".to_string()
    }
}

fn find_function(ctx: &mut TypeCheckerVisitorContext<SymFeltRef, QExecContext>, name: &str) -> Option<TypeId> {
    let name_id = ctx.program.interner.intern_ident(name);
    let key: TypeKey = name_id.into();
    for module in ctx.symbols.modules() {
        if let Some(&tid) = ctx.symbols[module.scope_id].types.get(&key) {
            if ctx.symbols[tid].as_function().is_some() {
                return Some(tid);
            }
        }
    }
    None
}

#[derive(Debug)]
enum PsyOutcome {
    Returned(u64),
    /// Multi-felt return (arrays, hashes): every element concretely
    /// resolved through the VM's eval path. Intrinsic results (hash,
    /// keccak256, split_bits, ...) are store nodes rather than folded
    /// constants, so single-element returns of that shape resolve here too
    /// and come back as `Returned`.
    ReturnedVec(Vec<u64>),
    /// Interpreted cleanly but the result stayed symbolic (non-constant).
    Symbolic,
    Errored(String),
    Panicked(String),
}

/// How `main`'s parameters are bound: concrete constants (everything
/// constant-folds — the differential path) or symbolic inputs materialized
/// from each parameter's declared type (for safety checks on the symbolic
/// path, where the interpreter only builds circuit nodes).
enum Inputs {
    Const(Vec<Arg>),
    Symbolic,
}

fn psy_run(source: &str, inputs: &[Arg]) -> PsyOutcome {
    psy_run_with(source, Inputs::Const(inputs.to_vec()))
}

fn psy_run_with(source: &str, inputs: Inputs) -> PsyOutcome {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_rg_{n}_{unique}.psy"));
    fs::write(&path, source).unwrap();

    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interpreter.typecheck_single(path.clone())
    }));
    let _ = fs::remove_file(&path);
    let (typechecker, mut ctx) = match compiled {
        Err(p) => return PsyOutcome::Panicked(format!("[typecheck panicked] {}", panic_message(&p))),
        Ok(Err(e)) => return PsyOutcome::Errored(format!("[typecheck rejected] {e:#}")),
        Ok(Ok(pair)) => pair,
    };
    let Some(main_tid) = find_function(&mut ctx, "main") else {
        return PsyOutcome::Errored("[no main]".to_string());
    };
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &inputs {
        Inputs::Const(args) => args
            .iter()
            .map(|arg| match arg {
                // u32 constants must be ConstantU32 felts (they route to the
                // u32 fold); Felt constants must be plain Constants.
                Arg::U32(v) => CheckedValueRef::from_u32(SymFeltRef::new_constant_u32(*v)),
                Arg::Felt(v) => CheckedValueRef::new_rc(CheckedValue::Felt(SymFeltRef::new_constant(*v))),
            })
            .collect::<Vec<_>>(),
        Inputs::Symbolic => {
            let function = ctx.symbols[main_tid].as_function().expect("main is a function");
            let location = function.location;
            function
                .parameters
                .iter()
                .map(|parameter| {
                    interpreter
                        .materialize_input(parameter.ty, &ctx.symbols, location)
                        .expect("symbolic input materialization")
                })
                .collect::<Vec<_>>()
        }
    }));
    let args = match built {
        Ok(args) => args,
        Err(p) => return PsyOutcome::Panicked(format!("[input panicked] {}", panic_message(&p))),
    };
    let program = &typechecker.program;
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interpreter.interpret_function(program, main_tid, args, &mut ctx)
    }));
    match run {
        Err(p) => PsyOutcome::Panicked(panic_message(&p)),
        Ok(Err(e)) => PsyOutcome::Errored(format!("{e:#}")),
        Ok(Ok(control)) => {
            let ControlState::Return(value) = control else {
                return PsyOutcome::Errored("[main did not return a value]".to_string());
            };
            let felts = value.to_felts();
            if felts.is_empty() {
                return PsyOutcome::Errored("[main returned no values]".to_string());
            }
            let scalar = matches!(
                &*value.borrow(),
                CheckedValue::U32(_) | CheckedValue::Felt(_) | CheckedValue::Bool(_)
            );
            if scalar && interpreter.is_constant(felts[0].clone()) {
                return PsyOutcome::Returned(felts[0].get_constant_value());
            }
            if matches!(inputs, Inputs::Symbolic) {
                return PsyOutcome::Symbolic;
            }
            // Concrete inputs but the value did not constant-fold (intrinsic
            // results are store nodes). Resolve through the VM's eval path —
            // the same ContextEval the IDE preview executes — so intrinsic
            // outputs can be compared against native mirrors.
            let resolved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let input = DummyContextEvalInput::new(vec![]);
                let mut cache = SimpleEvalCache::new();
                felts
                    .iter()
                    .map(|f| interpreter.context.store.resolve_felt_ref_cached(f.clone(), &input, &mut cache))
                    .collect::<Vec<u64>>()
            }));
            match resolved {
                Err(p) => PsyOutcome::Panicked(format!("[resolve panicked] {}", panic_message(&p))),
                Ok(values) if scalar => PsyOutcome::Returned(values[0]),
                Ok(values) => PsyOutcome::ReturnedVec(values),
            }
        }
    }
}

// ---------- differential loop ----------

enum Outcome {
    Value,
    Overflow,
    DivZero,
    InvalidCast,
}

fn write_artifact(seed: u64, source: &str) -> std::io::Result<PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("random-graph-failures");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("seed_{seed}.psy"));
    fs::write(&path, source)?;
    Ok(path)
}

fn check_seed(seed: u64) -> Outcome {
    let mut rng = Rng::new(seed);
    let graph = gen_graph(&mut rng, seed);
    let source = gen_source(&graph);
    let expected = mirror_eval(&graph);
    let got = psy_run(&source, &graph.params);
    match (&expected, &got) {
        (Ok(v), PsyOutcome::Returned(x)) if *x == v.canonical() => Outcome::Value,
        (Err(MirrorErr::Overflow), PsyOutcome::Errored(msg)) if msg.contains("ArithmeticOverflow") => Outcome::Overflow,
        (Err(MirrorErr::DivZero), PsyOutcome::Errored(msg)) if msg.contains("DivisionByZero") => Outcome::DivZero,
        (Err(MirrorErr::InvalidCast), PsyOutcome::Errored(msg)) if msg.to_lowercase().contains("invalid cast") => Outcome::InvalidCast,
        _ => {
            let artifact = write_artifact(seed, &source)
                .map_or_else(|_| "<artifact write failed; source printed below>".to_string(), |p| p.display().to_string());
            let inputs = graph
                .params
                .iter()
                .enumerate()
                .map(|(i, p)| match p {
                    Arg::U32(v) => format!("{}={v}u32", PARAM_NAMES[i]),
                    Arg::Felt(v) => format!("{}={v}", PARAM_NAMES[i]),
                })
                .collect::<Vec<_>>()
                .join(", ");
            panic!(
                "random computation graph mismatch (seed {seed})\n  \
                 inputs: {inputs}\n  \
                 mirror: {expected:?}\n  \
                 psy:    {got:?}\n  \
                 artifact: {artifact}\n  \
                 reproduce: PSY_RANDOM_GRAPH_SEED={seed} cargo test -p psy-interpreter random_computation_graphs_match_native_rust\n\
                 --- program ---\n{source}\
                 ----------------"
            );
        }
    }
}

#[test]
fn random_computation_graphs_match_native_rust() {
    if let Ok(raw) = std::env::var("PSY_RANDOM_GRAPH_SEED") {
        check_seed(raw.trim().parse().expect("PSY_RANDOM_GRAPH_SEED must be a u64"));
        return;
    }
    let iters: u64 = std::env::var("PSY_RANDOM_GRAPH_ITERS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(10_000);
    let base = 0x5EED_2026_0921_u64;
    let (mut values, mut overflows, mut div_zeros, mut invalid_casts) = (0u64, 0u64, 0u64, 0u64);
    for i in 0..iters {
        match check_seed(base ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15)) {
            Outcome::Value => values += 1,
            Outcome::Overflow => overflows += 1,
            Outcome::DivZero => div_zeros += 1,
            Outcome::InvalidCast => invalid_casts += 1,
        }
    }
    println!(
        "random graph differential: {iters} seeds passed \
         ({values} values, {overflows} expected overflows, {div_zeros} expected div-by-zero errors, {invalid_casts} expected invalid casts)"
    );
}

/// Regressions for the two psy_vm constant-folding bugs this suite uncovered
/// (fixed in psy-node `fix/vm-const-fold`):
///
/// - u32 shift folding shifted the u64 directly: a distance >= 64 panicked
///   in debug and silently wrapped in release (`1u32 << 64u32` folded to 1).
///   Distances >= 32 now fold to 0, matching the existing 32..63 behavior.
/// - `x * 0` with a symbolic x returned `x` instead of 0, silently recording
///   the constraint `x == 0` in the circuit. It now folds to constant 0.
#[test]
fn vm_const_fold_regressions() {
    for (label, source) in [
        ("shl_64", "fn main() -> u32 { return 1u32 << 64u32; }"),
        ("shr_100", "fn main() -> u32 { return 1u32 >> 100u32; }"),
        ("shl_32", "fn main() -> u32 { return 1u32 << 32u32; }"),
        ("shr_33", "fn main() -> u32 { return 4294967295u32 >> 33u32; }"),
    ] {
        match psy_run(source, &[]) {
            PsyOutcome::Returned(0) => {}
            other => panic!("[{label}] expected Returned(0), got {other:?}"),
        }
    }
    match psy_run_with("fn main(x: Felt) -> Felt { return x * 0; }", Inputs::Symbolic) {
        PsyOutcome::Returned(0) => {}
        other => panic!("[mul_zero_symbolic] expected Returned(0), got {other:?}"),
    }
}

/// Non-constant (symbolic) shifts must interpret cleanly: the interpreter
/// only builds the circuit node — no folding, no panic, and the result stays
/// symbolic even for wild distances. Value-level verification of symbolic
/// shifts happens at VM execution/proving time, outside this interpret-layer
/// suite.
#[test]
fn symbolic_shifts_interpret_cleanly() {
    for (label, source) in [
        ("both_symbolic_shl", "fn main(a: u32, b: u32) -> u32 { return a << b; }"),
        ("both_symbolic_shr", "fn main(a: u32, b: u32) -> u32 { return a >> b; }"),
        ("symbolic_base_big_distance", "fn main(a: u32) -> u32 { return a << 70u32; }"),
        ("symbolic_distance", "fn main(b: u32) -> u32 { return 3u32 >> b; }"),
        ("chained", "fn main(a: u32, b: u32) -> u32 { let x = a << b; let y = x >> 2u32; return y << b; }"),
    ] {
        match psy_run_with(source, Inputs::Symbolic) {
            PsyOutcome::Symbolic => {}
            other => panic!("[{label}] expected clean symbolic interpretation, got {other:?}"),
        }
    }
}

/// op_type of `main`'s returned felt, interpreted with symbolic inputs.
/// Pins routing decisions — which op a DSL expression compiles to — that
/// value-level differentials cannot see (e.g. Exp vs ExpConstantPower
/// evaluate identically).
fn psy_result_op_type(source: &str) -> DPNOpType {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("psy_rg_{n}_{unique}.psy"));
    fs::write(&path, source).unwrap();
    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let (typechecker, mut ctx) = interpreter.typecheck_single(path.clone()).expect("typecheck must succeed");
    let _ = fs::remove_file(&path);
    let main_tid = find_function(&mut ctx, "main").expect("main must exist");
    let function = ctx.symbols[main_tid].as_function().expect("main is a function");
    let location = function.location;
    let args: Vec<_> = function
        .parameters
        .iter()
        .map(|p| interpreter.materialize_input(p.ty, &ctx.symbols, location).expect("symbolic input"))
        .collect();
    let control = interpreter
        .interpret_function(&typechecker.program, main_tid, args, &mut ctx)
        .expect("interpretation must succeed");
    let ControlState::Return(value) = control else {
        panic!("main must return a value");
    };
    value.to_felts()[0].get_op_type()
}

/// Felt `**` with a compile-time-constant operand must route to the
/// ExpConstant* ops (fixed in psy-node `fix/vm-const-fold`): the routing
/// checked the u32 lane's `ConstantU32` marker on felt-lane operands
/// (felt constants are `Constant` nodes), so it never fired and every
/// `**` emitted a plain `Exp` — the constant-exponent circuit optimization
/// was unreachable. Both-constant `**` still folds to a plain constant.
#[test]
fn felt_pow_constant_routing() {
    assert_eq!(
        psy_result_op_type("fn main(x: Felt) -> Felt { return x ** 3; }"),
        DPNOpType::ExpConstantPower
    );
    assert_eq!(
        psy_result_op_type("fn main(x: Felt) -> Felt { return 2 ** x; }"),
        DPNOpType::ExpConstantBase
    );
    assert_eq!(
        psy_result_op_type("fn main(x: Felt, y: Felt) -> Felt { return x ** y; }"),
        DPNOpType::Exp
    );
    match psy_run("fn main(x: Felt) -> Felt { return x ** 3; }", &[Arg::Felt(2)]) {
        PsyOutcome::Returned(8) => {}
        other => panic!("[const_pow_fold] expected Returned(8), got {other:?}"),
    }
}

/// Felt `%` and u32 `&`/`|`/`^` with a compile-time-constant operand must
/// route to the Constant* ops (wired in psy-node `fix/vm-const-fold`):
/// ModConstantDividend/Divisor and U32And/Or/XorConstant were previously
/// dead enum values — no producer emitted them, and their consumers read
/// the constant through a phantom const_param slot instead of the operand
/// node. The u32 lane checks the right (mask) operand only; and/or/xor are
/// commutative, so a constant left operand needs no variant of its own.
#[test]
fn mod_and_u32_bitwise_constant_routing() {
    // Felt lane.
    assert_eq!(
        psy_result_op_type("fn main(x: Felt, d: Felt) -> Felt { return x % d; }"),
        DPNOpType::Mod
    );
    assert_eq!(
        psy_result_op_type("fn main(x: Felt) -> Felt { return x % 7; }"),
        DPNOpType::ModConstantDivisor
    );
    assert_eq!(
        psy_result_op_type("fn main(x: Felt) -> Felt { return 1000 % x; }"),
        DPNOpType::ModConstantDividend
    );
    // u32 lane: constant right operand selects the Constant* mask variant.
    assert_eq!(
        psy_result_op_type("fn main(x: u32) -> u32 { return x & 12u32; }"),
        DPNOpType::U32AndConstant
    );
    assert_eq!(
        psy_result_op_type("fn main(x: u32) -> u32 { return x | 12u32; }"),
        DPNOpType::U32OrConstant
    );
    assert_eq!(
        psy_result_op_type("fn main(x: u32) -> u32 { return x ^ 12u32; }"),
        DPNOpType::U32XorConstant
    );
    // Plain variants stay plain with two runtime operands.
    assert_eq!(
        psy_result_op_type("fn main(x: u32, y: u32) -> u32 { return x & y; }"),
        DPNOpType::U32And
    );
    // Shifts: constant bit distance (right operand) / constant value (left).
    assert_eq!(
        psy_result_op_type("fn main(x: u32) -> u32 { return x << 3u32; }"),
        DPNOpType::U32ShiftLeftConstantBitDistance
    );
    assert_eq!(
        psy_result_op_type("fn main(x: u32) -> u32 { return 5u32 << x; }"),
        DPNOpType::U32ShiftLeftConstantValue
    );
    assert_eq!(
        psy_result_op_type("fn main(x: u32) -> u32 { return x >> 3u32; }"),
        DPNOpType::U32ShiftRightConstantBitDistance
    );
    assert_eq!(
        psy_result_op_type("fn main(x: u32) -> u32 { return 5u32 >> x; }"),
        DPNOpType::U32ShiftRightConstantValue
    );
    assert_eq!(
        psy_result_op_type("fn main(x: u32, y: u32) -> u32 { return x << y; }"),
        DPNOpType::U32ShiftLeft
    );
    // Values match native arithmetic on the eval path.
    match psy_run("fn main(x: Felt) -> Felt { return x % 7; }", &[Arg::Felt(30)]) {
        PsyOutcome::Returned(2) => {}
        other => panic!("[mod_const_divisor] expected Returned(2), got {other:?}"),
    }
    match psy_run("fn main(x: Felt) -> Felt { return 1000 % x; }", &[Arg::Felt(7)]) {
        PsyOutcome::Returned(6) => {}
        other => panic!("[mod_const_dividend] expected Returned(6), got {other:?}"),
    }
    match psy_run("fn main(x: u32) -> u32 { return x & 12u32; }", &[Arg::U32(10)]) {
        PsyOutcome::Returned(8) => {}
        other => panic!("[u32_and_const] expected Returned(8), got {other:?}"),
    }
}

// ---------- crypto/bit intrinsic differential ----------
//
// The intrinsics below are DSL-reachable but never appear in scalar
// computation graphs, so they get their own seeded cases. Every case builds
// a `main` from concrete literals, interprets it, resolves the result
// through the VM's eval path, and compares against a native Rust mirror:
// plonky2's Poseidon for hash/hash_two_to_one, tiny-keccak for keccak256,
// k256 for secp256k1_verify fixtures, and plain bit ops for
// split_bits/sum_bits.

/// Split widths cluster on the boundaries: 31/32/33 around the u32 seam,
/// 63/64 at the 64-bit felt ceiling (64 accepts every canonical felt;
/// wider is out of domain — `semantics::split_bits` rejects num_bits > 64,
/// and the circuit could never prove a wider decomposition anyway).
const CRYPTO_SPLIT_WIDTHS: [u64; 8] = [1, 4, 8, 31, 32, 33, 63, 64];
/// Keccak lengths cross the 136-byte rate boundary (34 u32 words): 34 is
/// exactly one block, 35 spills into two, 68 is two full blocks, 69 is
/// just past two, 102 is three full blocks.
const CRYPTO_KECCAC_LENS: [usize; 10] = [1, 8, 16, 17, 24, 34, 35, 68, 69, 102];

/// Mirrors the VM's keccak packing: each felt word contributes its low 32
/// bits as 4 big-endian bytes; the digest is read back as 8 big-endian u32s.
/// The packing itself is the wire convention (shared with the circuit);
/// the Keccak permutation underneath is tiny-keccak.
fn keccak_mirror(words: &[u64]) -> Vec<u64> {
    use tiny_keccak::{Hasher as _, Keccak};
    let mut bytes = Vec::with_capacity(words.len() * 4);
    for word in words {
        bytes.extend_from_slice(&(*word as u32).to_be_bytes());
    }
    let mut digest = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(&bytes);
    keccak.finalize(&mut digest);
    digest
        .chunks_exact(4)
        .take(8)
        .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]) as u64)
        .collect()
}

fn poseidon_hash_mirror(words: &[u64]) -> [u64; 4] {
    let data: Vec<GoldilocksField> = words.iter().map(|w| GoldilocksField::from_noncanonical_u64(*w)).collect();
    PoseidonHash::hash_no_pad(&data).elements.map(|e| e.to_canonical_u64())
}

fn poseidon_two_to_one_mirror(left: &[u64; 4], right: &[u64; 4]) -> [u64; 4] {
    let conv = |v: &[u64; 4]| HashOut { elements: v.map(|w| GoldilocksField::from_noncanonical_u64(w)) };
    PoseidonHash::two_to_one(conv(left), conv(right))
        .elements
        .map(|e| e.to_canonical_u64())
}

/// LSB-first decomposition over the strict domain shared by the VM and the
/// circuit: `num_bits <= 64` and the value must fit the width, otherwise
/// `None` (the expected clean rejection).
fn split_bits_mirror(x: u64, num_bits: u64) -> Option<Vec<u64>> {
    if num_bits > 64 || (num_bits < 64 && x >= 1u64 << num_bits) {
        return None;
    }
    Some((0..num_bits).map(|i| (x >> i) & 1).collect())
}

/// Weighted binary reconstruction `sum(bit[i] * 2^i)` reduced mod p — the
/// strict-boolean SumBits semantics shared by the VM and the circuit (the
/// old unweighted fold diverged from the circuit's mul_add accumulation).
fn sum_bits_mirror(bits: &[u64]) -> u64 {
    bits.iter()
        .enumerate()
        .fold(GoldilocksField::ZERO, |acc, (i, bit)| acc + GoldilocksField::from_noncanonical_u64(bit << i))
        .to_canonical_u64()
}

/// Packs u32-range words exactly like the VM's Secp256k1Verify eval arm:
/// each word's 4 little-endian bytes, then the whole byte sequence
/// reversed — i.e. reversed word order with each word big-endian.
fn secp_pack_u32_words(words: &[u64]) -> Vec<u8> {
    words.iter().flat_map(|w| (*w as u32).to_le_bytes()).rev().collect()
}

/// Message words are full felts: 8 little-endian bytes each, whole
/// sequence reversed.
fn secp_pack_msg_words(words: &[u64; 4]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).rev().collect()
}

/// Inverse of `secp_pack_u32_words`.
fn secp_unpack_words(bytes: &[u8]) -> Vec<u64> {
    let mut words: Vec<u64> = bytes
        .chunks_exact(4)
        .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]) as u64)
        .collect();
    words.reverse();
    words
}

fn secp_mirror(public_key: &[u64; 16], signature: &[u64; 16], msg: &[u64; 4]) -> bool {
    use k256::ecdsa::signature::hazmat::PrehashVerifier;
    let mut sec1 = vec![0x04];
    sec1.extend(secp_pack_u32_words(&public_key[0..8]));
    sec1.extend(secp_pack_u32_words(&public_key[8..16]));
    let Ok(vk) = k256::ecdsa::VerifyingKey::from_sec1_bytes(&sec1) else {
        return false;
    };
    let mut sig_bytes = secp_pack_u32_words(&signature[0..8]);
    sig_bytes.extend(secp_pack_u32_words(&signature[8..16]));
    let Ok(sig) = k256::ecdsa::Signature::from_slice(&sig_bytes) else {
        return false;
    };
    matches!(vk.verify_prehash(&secp_pack_msg_words(msg), &sig), Ok(_))
}

/// Builds a genuinely valid (pk, sig) pair over the packed form of `msg`,
/// so the differential has a positive case: psy must return true.
fn secp_valid_fixture(seed: u64, msg: &[u64; 4]) -> ([u64; 16], [u64; 16]) {
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    let mut rng = Rng::new(seed ^ 0x5EC5_EED0_0000_0000);
    let mut key_bytes = [0u8; 32];
    for chunk in key_bytes.chunks_mut(8) {
        chunk.copy_from_slice(&rng.next_u64().to_le_bytes());
    }
    let sk =
        k256::ecdsa::SigningKey::from_bytes(k256::FieldBytes::from_slice(&key_bytes)).expect("valid signing key");
    let point = sk.verifying_key().to_encoded_point(false);
    let raw = point.as_bytes(); // 0x04 || x[32] || y[32]
    let mut public_key = secp_unpack_words(&raw[1..33]);
    public_key.extend(secp_unpack_words(&raw[33..65]));
    let sig: k256::ecdsa::Signature = sk.sign_prehash(&secp_pack_msg_words(msg)).expect("signing must not fail");
    let sig_bytes = sig.to_bytes();
    let sig_raw: &[u8] = sig_bytes.as_ref();
    let mut signature = secp_unpack_words(&sig_raw[0..32]);
    signature.extend(secp_unpack_words(&sig_raw[32..64]));
    (
        public_key.try_into().expect("16 pk words"),
        signature.try_into().expect("16 sig words"),
    )
}

/// Boundary-biased u32 word for keccak input arrays.
fn keccak_word(rng: &mut Rng) -> u64 {
    match rng.below(8) {
        0 => 0,
        1 => 1,
        2 => u32::MAX as u64,
        3 => (u32::MAX - 1) as u64,
        4 => 0x8000_0000,
        _ => rng.below(1 << 32),
    }
}

fn write_crypto_artifact(seed: u64, kind: &str, source: &str) -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("random-graph-failures");
    if fs::create_dir_all(&dir).is_err() {
        return "<artifact write failed; source printed below>".to_string();
    }
    let path = dir.join(format!("crypto_{kind}_seed_{seed}.psy"));
    if fs::write(&path, source).is_err() {
        return "<artifact write failed; source printed below>".to_string();
    }
    path.display().to_string()
}

fn crypto_expect(seed: u64, kind: &str, source: &str, expected: &[u64]) {
    let outcome = psy_run(source, &[]);
    let got = match &outcome {
        PsyOutcome::Returned(v) => Some(vec![*v]),
        PsyOutcome::ReturnedVec(v) => Some(v.clone()),
        _ => None,
    };
    if got.as_deref() != Some(expected) {
        let artifact = write_crypto_artifact(seed, kind, source);
        panic!(
            "crypto intrinsic mismatch (seed {seed}, case {kind})\n  \
             mirror: {expected:?}\n  \
             psy:    {outcome:?}\n  \
             artifact: {artifact}\n  \
             reproduce: PSY_RANDOM_GRAPH_SEED={seed} cargo test -p psy-interpreter crypto_intrinsics_match_native_rust\n\
             --- program ---\n{source}\
             ----------------"
        );
    }
}

/// Runs `f` with the process panic hook silenced. Expected rejection
/// panics from the VM eval path are caught by `catch_unwind` inside
/// `psy_run`, but the default hook still prints each one — under
/// `make test --nocapture` a green run floods the log and looks like a
/// crash. The mutex keeps concurrent quiet sections from interleaving
/// hook swaps (a stray quiet hook would swallow later panics' messages);
/// genuine mismatches panic from the test thread after the real hook is
/// restored, so they print normally.
fn with_quiet_panics<T>(f: impl FnOnce() -> T) -> T {
    static QUIET: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = QUIET.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = f();
    std::panic::set_hook(previous);
    result
}

/// The out-of-domain cases must fail cleanly (a caught panic from the VM's
/// eval path carrying the semantics error text), never fold to a value.
fn crypto_expect_reject(seed: u64, kind: &str, source: &str, needle: &str) {
    let outcome = with_quiet_panics(|| psy_run(source, &[]));
    if !matches!(&outcome, PsyOutcome::Panicked(msg) if msg.contains(needle)) {
        let artifact = write_crypto_artifact(seed, kind, source);
        panic!(
            "crypto intrinsic should have been rejected (seed {seed}, case {kind})\n  \
             expected panic containing: {needle:?}\n  \
             psy:    {outcome:?}\n  \
             artifact: {artifact}\n  \
             reproduce: PSY_RANDOM_GRAPH_SEED={seed} cargo test -p psy-interpreter crypto_intrinsics_match_native_rust\n\
             --- program ---\n{source}\
             ----------------"
        );
    }
}

fn crypto_case_poseidon(rng: &mut Rng, seed: u64, len: u64) {
    let words: Vec<u64> = (0..len).map(|_| felt_literal(rng)).collect();
    let source = format!(
        "use std::prelude::*;\n\nfn main() -> Hash {{\n    return hash([{}]);\n}}\n",
        words.iter().map(|w| w.to_string()).collect::<Vec<_>>().join(", ")
    );
    crypto_expect(seed, "poseidon", &source, &poseidon_hash_mirror(&words));
}

fn crypto_case_two_to_one(rng: &mut Rng, seed: u64) {
    let left_words: Vec<u64> = (0..rng.below(3) + 1).map(|_| felt_literal(rng)).collect();
    let right_words: Vec<u64> = (0..rng.below(3) + 1).map(|_| felt_literal(rng)).collect();
    let fmt = |w: &[u64]| w.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ");
    let source = format!(
        "use std::prelude::*;\n\nfn main() -> Hash {{\n    \
         let a: Hash = hash([{}]);\n    \
         let b: Hash = hash([{}]);\n    \
         return hash_two_to_one(a, b);\n}}\n",
        fmt(&left_words),
        fmt(&right_words)
    );
    let expected = poseidon_two_to_one_mirror(
        &poseidon_hash_mirror(&left_words),
        &poseidon_hash_mirror(&right_words),
    );
    crypto_expect(seed, "two_to_one", &source, &expected);
}

fn crypto_case_keccak(rng: &mut Rng, seed: u64) {
    let len = *rng.pick(&CRYPTO_KECCAC_LENS);
    let words: Vec<u64> = (0..len).map(|_| keccak_word(rng)).collect();
    let source = format!(
        "use std::prelude::*;\n\nfn main() -> [u32; 8] {{\n    return keccak256([{}]);\n}}\n",
        words.iter().map(|w| format!("{w}u32")).collect::<Vec<_>>().join(", ")
    );
    crypto_expect(seed, "keccak", &source, &keccak_mirror(&words));
}

fn crypto_case_split_bits(rng: &mut Rng, seed: u64) {
    let n = *rng.pick(&CRYPTO_SPLIT_WIDTHS);
    // 64 accepts every canonical felt; below that, half the values stay
    // in width and half are pushed just past 2^n (still canonical, < p)
    // to exercise the strict range check as an expected rejection.
    let x = if n == 64 {
        felt_literal(rng)
    } else if rng.chance(50) {
        rng.below(1u64 << n)
    } else {
        (1u64 << n) + rng.below((GOLDILOCKS_P as u64) - (1u64 << n))
    };
    let bits = split_bits_mirror(x, n);
    let src_bits = format!(
        "use std::prelude::*;\n\nfn main() -> [Felt; {n}] {{\n    \
         let bits: [Felt; {n}] = split_bits({x}, {n});\n    \
         return bits;\n}}\n"
    );
    let src_sum = format!(
        "use std::prelude::*;\n\nfn main() -> Felt {{\n    \
         let bits: [Felt; {n}] = split_bits({x}, {n});\n    \
         return sum_bits(bits);\n}}\n"
    );
    match &bits {
        Some(bits) => {
            crypto_expect(seed, "split_bits", &src_bits, bits);
            crypto_expect(seed, "split_sum_bits", &src_sum, &[sum_bits_mirror(bits)]);
        }
        // Both the array view and its weighted sum reject the same
        // out-of-domain value.
        None => {
            crypto_expect_reject(seed, "split_bits_overflow", &src_bits, "does not fit in");
            crypto_expect_reject(seed, "split_sum_bits_overflow", &src_sum, "does not fit in");
        }
    }
}

fn crypto_case_sum_bits(rng: &mut Rng, seed: u64) {
    // 20% of cases pin the exact 64-element maximum (below(64) + 1 could
    // never reach it); the rest stay in 1..=63.
    let len = if rng.chance(20) { 64 } else { rng.below(63) + 1 };
    // Bits are strict 0/1: the VM booleanizes every input (anything else
    // is a clean "invalid bool value" rejection — pinned by
    // bit_intrinsics_out_of_domain_fail_cleanly), so the differential
    // stays in the valid domain.
    let bits: Vec<u64> = (0..len).map(|_| rng.below(2)).collect();
    let source = format!(
        "use std::prelude::*;\n\nfn main() -> Felt {{\n    \
         let bits: [Felt; {len}] = [{}];\n    \
         return sum_bits(bits);\n}}\n",
        bits.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", ")
    );
    crypto_expect(seed, "sum_bits", &source, &[sum_bits_mirror(&bits)]);
}

fn crypto_case_secp(rng: &mut Rng, seed: u64, tamper: bool) {
    let mut msg: [u64; 4] = [felt_literal(rng), felt_literal(rng), felt_literal(rng), felt_literal(rng)];
    let (mut public_key, mut signature) = secp_valid_fixture(seed, &msg);
    if tamper {
        match rng.below(3) {
            0 => {
                // keep the felt canonical when nudging past p - 1
                let i = rng.below(4) as usize;
                msg[i] = if msg[i] >= (GOLDILOCKS_P - 1) as u64 { msg[i] - 1 } else { msg[i] + 1 };
            }
            1 => {
                let i = rng.below(16) as usize;
                signature[i] ^= 1;
            }
            _ => {
                let i = rng.below(16) as usize;
                public_key[i] ^= 1;
            }
        }
    }
    let expected = secp_mirror(&public_key, &signature, &msg);
    assert_eq!(expected, !tamper, "fixture self-check (seed {seed}): tampering must invalidate");
    let fmt = |w: &[u64]| w.iter().map(|x| format!("{x}u32")).collect::<Vec<_>>().join(", ");
    let source = format!(
        "use std::prelude::*;\n\nfn main() -> bool {{\n    \
         let msg: Hash = [{}, {}, {}, {}];\n    \
         let verified = secp256k1_verify([{}], msg, [{}]);\n    \
         return verified;\n}}\n",
        msg[0],
        msg[1],
        msg[2],
        msg[3],
        fmt(&public_key),
        fmt(&signature)
    );
    crypto_expect(seed, if tamper { "secp_tampered" } else { "secp_valid" }, &source, &[expected as u64]);
}

fn check_crypto_seed(seed: u64) {
    let mut rng = Rng::new(seed);
    match seed % 8 {
        0 => {
            let len = rng.below(3) + 1;
            crypto_case_poseidon(&mut rng, seed, len)
        }
        1 => {
            // 5..=16 crosses the 12-element Poseidon state boundary
            let len = rng.below(12) + 5;
            crypto_case_poseidon(&mut rng, seed, len)
        }
        2 => crypto_case_two_to_one(&mut rng, seed),
        3 => crypto_case_keccak(&mut rng, seed),
        4 => crypto_case_split_bits(&mut rng, seed),
        5 => crypto_case_sum_bits(&mut rng, seed),
        6 => crypto_case_secp(&mut rng, seed, false),
        _ => crypto_case_secp(&mut rng, seed, true),
    }
}

/// Out-of-domain bit intrinsics must fail cleanly, never fold to a value:
/// a decomposition wider than the felt (`num_bits > 64`), a value wider
/// than the requested width, and a non-boolean sum_bits input are all
/// rejections under the strict semantics shared with the circuit —
/// `split_le` / `assert_bool` make those programs unsatisfiable anyway.
/// (The old eval zero-padded wide splits and raw-summed sum_bits, which is
/// exactly where it diverged from the circuit.)
#[test]
fn bit_intrinsics_out_of_domain_fail_cleanly() {
    for n in [65u64, 100, 128] {
        let source = format!(
            "use std::prelude::*;\n\nfn main() -> Felt {{\n    \
             let bits: [Felt; {n}] = split_bits(5, {n});\n    \
             return sum_bits(bits);\n}}\n"
        );
        crypto_expect_reject(0, "split_bits_wide", &source, "at most 64");
    }
    crypto_expect_reject(
        0,
        "split_bits_overflow",
        "use std::prelude::*;\n\nfn main() -> Felt {\n    \
         let bits: [Felt; 8] = split_bits(256, 8);\n    \
         return sum_bits(bits);\n}\n",
        "does not fit in",
    );
    crypto_expect_reject(
        0,
        "sum_bits_non_bool",
        "use std::prelude::*;\n\nfn main() -> Felt {\n    \
         let bits: [Felt; 2] = [1, 2];\n    \
         return sum_bits(bits);\n}\n",
        "invalid bool value",
    );
    // 65 elements is past the 64-bit maximum: a [Felt; 65] literal is
    // type-level fine, the rejection fires at sum_bits evaluation.
    let ones = "1, ".repeat(64) + "1";
    crypto_expect_reject(
        0,
        "sum_bits_too_long",
        &format!(
            "use std::prelude::*;\n\nfn main() -> Felt {{\n    \
             let bits: [Felt; 65] = [{ones}];\n    \
             return sum_bits(bits);\n}}\n"
        ),
        "at most 64",
    );
}

/// Exact in-domain boundaries of the bit intrinsics, deterministically:
/// 2^n - 1 must decompose to all ones and its weighted fold must rebuild
/// the value. Width 64 is covered separately: its all-ones value 2^64 - 1
/// is NOT a canonical felt (p = 2^64 - 2^32 + 1) — the frontend reduces
/// the literal mod p before splitting, so u64::MAX yields the bits of
/// u64::MAX - p, and p - 1 (bits 32..63) is the largest canonical felt.
#[test]
fn bit_intrinsics_exact_boundaries_match_native() {
    for n in [1u64, 4, 8, 31, 32, 63] {
        let value = ((1u128 << n) - 1) as u64;
        let bits = split_bits_mirror(value, n).expect("2^n - 1 always fits");
        assert_eq!(bits, vec![1u64; n as usize]);
        let src_bits = format!(
            "use std::prelude::*;\n\nfn main() -> [Felt; {n}] {{\n    \
             let bits: [Felt; {n}] = split_bits({value}, {n});\n    \
             return bits;\n}}\n"
        );
        crypto_expect(0, "split_all_ones", &src_bits, &bits);
        let src_sum = format!(
            "use std::prelude::*;\n\nfn main() -> Felt {{\n    \
             let bits: [Felt; {n}] = split_bits({value}, {n});\n    \
             return sum_bits(bits);\n}}\n"
        );
        crypto_expect(0, "sum_all_ones", &src_sum, &[sum_bits_mirror(&bits)]);
    }
    let p = GOLDILOCKS_P as u64;
    for (label, value) in [("u64_max_reduced", u64::MAX), ("p_minus_1", p - 1)] {
        let canonical = if value >= p { value - p } else { value };
        let bits = split_bits_mirror(canonical, 64).expect("canonical felt fits 64 bits");
        let src_bits = format!(
            "use std::prelude::*;\n\nfn main() -> [Felt; 64] {{\n    \
             let bits: [Felt; 64] = split_bits({value}, 64);\n    \
             return bits;\n}}\n"
        );
        crypto_expect(0, label, &src_bits, &bits);
        let src_sum = format!(
            "use std::prelude::*;\n\nfn main() -> Felt {{\n    \
             let bits: [Felt; 64] = split_bits({value}, 64);\n    \
             return sum_bits(bits);\n}}\n"
        );
        crypto_expect(0, label, &src_sum, &[sum_bits_mirror(&bits)]);
    }
}

/// Array element access (TargetAt) on intrinsic store nodes: indexing a
/// split_bits array and a keccak digest must read the element the native
/// mirrors hold. Constant out-of-bounds indices are a clean sema-level
/// IndexOutOfBounds error, so only in-range indices appear here.
#[test]
fn array_element_access_matches_native() {
    let x: u64 = 0xABCD;
    let bits = split_bits_mirror(x, 16).expect("in-width value");
    for i in [0usize, 1, 7, 15] {
        let src = format!(
            "use std::prelude::*;\n\nfn main() -> Felt {{\n    \
             let bits: [Felt; 16] = split_bits({x}, 16);\n    \
             return bits[{i}];\n}}\n"
        );
        crypto_expect(0, "target_at_split", &src, &[bits[i]]);
    }
    let words: Vec<u64> = (1..=8u64).map(|w| w * 0x0101_0101).collect();
    let digest = keccak_mirror(&words);
    let fmt = words.iter().map(|w| format!("{w}u32")).collect::<Vec<_>>().join(", ");
    for i in [0usize, 3, 7] {
        let src = format!(
            "use std::prelude::*;\n\nfn main() -> u32 {{\n    \
             return keccak256([{fmt}])[{i}];\n}}\n"
        );
        crypto_expect(0, "target_at_keccak", &src, &[digest[i]]);
    }
}

/// Degenerate secp256k1 inputs (all-zero key/signature words) must verify
/// to false, never abort the evaluation — malformed keys and out-of-range
/// (r, s) map to verification failure in every layer.
#[test]
fn secp_degenerate_inputs_return_false() {
    let msg: [u64; 4] = [1, 2, 3, 4];
    let ones = vec![1u64; 16];
    let zeros = vec![0u64; 16];
    for (label, pk, sig) in [
        ("secp_zero_pk", &zeros, &ones),
        ("secp_zero_sig", &ones, &zeros),
        ("secp_all_zero", &zeros, &zeros),
    ] {
        let expected = secp_mirror(pk.as_slice().try_into().unwrap(), sig.as_slice().try_into().unwrap(), &msg);
        assert!(!expected, "mirror self-check: degenerate input must not verify");
        let fmt = |w: &[u64]| w.iter().map(|x| format!("{x}u32")).collect::<Vec<_>>().join(", ");
        let source = format!(
            "use std::prelude::*;\n\nfn main() -> bool {{\n    \
             let msg: Hash = [1, 2, 3, 4];\n    \
             let verified = secp256k1_verify([{pk}], msg, [{sig}]);\n    \
             return verified;\n}}\n",
            pk = fmt(pk),
            sig = fmt(sig),
        );
        crypto_expect(0, label, &source, &[0]);
    }
}

/// Exact field-wrap boundaries on the felt lane. The random graphs hit
/// these only probabilistically through FELT_EDGES bias; here each one is
/// pinned: (p-1) + 1 = 0, 0 - 1 = p-1, -0 = 0, (-1)^2 = 1, x/x = 1,
/// x % 1 = 0, odd x % 2 = 1. The runtime operand keeps evaluation on the
/// interpret path (the same const-arg shape as vm_const_fold_regressions).
#[test]
fn felt_boundary_arithmetic_matches_native() {
    let p = GOLDILOCKS_P as u64;
    let cases: [(&str, &str, u64, u64); 10] = [
        ("add_wraps_to_zero", "fn main(x: Felt) -> Felt { return x + 1; }", p - 1, f_add(p - 1, 1)),
        ("add_wraps_at_p_minus_2", "fn main(x: Felt) -> Felt { return x + 2; }", p - 2, f_add(p - 2, 2)),
        ("sub_wraps_from_zero", "fn main(x: Felt) -> Felt { return x - 1; }", 0, f_sub(0, 1)),
        ("sub_const_left", "fn main(x: Felt) -> Felt { return 1 - x; }", 2, f_sub(1, 2)),
        ("neg_zero_is_zero", "fn main(x: Felt) -> Felt { return -x; }", 0, f_neg(0)),
        ("neg_p_minus_1_is_one", "fn main(x: Felt) -> Felt { return -x; }", p - 1, f_neg(p - 1)),
        ("neg_one_squared", "fn main(x: Felt) -> Felt { return x * x; }", p - 1, f_mul(p - 1, p - 1)),
        ("div_self_is_one", "fn main(x: Felt) -> Felt { return x / x; }", p - 1, 1),
        ("mod_one_is_zero", "fn main(x: Felt) -> Felt { return x % 1; }", p - 1, 0),
        ("mod_two_odd", "fn main(x: Felt) -> Felt { return x % 2; }", p - 2, 1),
    ];
    for (label, source, input, expected) in cases {
        match psy_run(source, &[Arg::Felt(input)]) {
            PsyOutcome::Returned(v) if v == expected => {}
            other => panic!("[{label}] expected Returned({expected}), got {other:?}"),
        }
    }
}

/// External known-answer vector from tests/keccak_u32_regression_test.psy:
/// keccak256 over sixteen zero u32 words. Anchors both the interpreter and
/// the mirror against a value that was derived outside this suite.
#[test]
fn keccak_intrinsic_known_answer_vector() {
    let words = vec![0u64; 16];
    let expected: [u64; 8] = [
        2905745590, 1995953101, 1115989316, 1058533782, 725017745, 3003793586, 1079527909, 2545573813,
    ];
    assert_eq!(keccak_mirror(&words), expected, "mirror must match the external vector");
    let source = format!(
        "use std::prelude::*;\n\nfn main() -> [u32; 8] {{\n    return keccak256([{}]);\n}}\n",
        "0u32, ".repeat(15) + "0u32"
    );
    crypto_expect(0, "keccak_kat", &source, &expected);
}

#[test]
fn crypto_intrinsics_match_native_rust() {
    if let Ok(raw) = std::env::var("PSY_RANDOM_GRAPH_SEED") {
        check_crypto_seed(raw.trim().parse().expect("PSY_RANDOM_GRAPH_SEED must be a u64"));
        return;
    }
    let iters: u64 = std::env::var("PSY_RANDOM_GRAPH_ITERS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(10_000);
    let base = 0xC0FF_EE20_2609_21_u64;
    for i in 0..iters {
        check_crypto_seed(base ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    println!("crypto intrinsic differential: {iters} seeds passed");
}
