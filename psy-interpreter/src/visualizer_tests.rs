// Tests for the sema AST visualizer (psy-sema/src/visualizer.rs). The debug_*
// entry points must render every node shape reachable from a normal program
// without panicking, and the output must carry stable markers per node kind,
// so the renderer stays usable as a debugging aid.

use serial_test::serial;

use super::*;

const SOURCE: &str = r#"
use std::prelude::*;

const LIMIT: Felt = 10;

pub struct Point {
    pub x: Felt,
    y: Felt,
}

pub trait Bounded {
    pub fn ceiling() -> Self;
}

impl Bounded for Felt {
    pub fn ceiling() -> Self {
        return LIMIT;
    }
}

fn weighted(value: Felt, mut weight: u32) -> Felt {
    let mut total: Felt = 0;
    let grid: [Felt; 3] = [1, 2, 3];
    for i in 0u32..3u32 {
        total += grid[i as Felt] * value;
    }
    while weight > 0u32 {
        weight -= 1u32;
    }
    let flag: bool = (weight == 0u32);
    let shifted: Felt = (3u32 + weight) as Felt;
    let bump = |v: Felt| -> Felt {
        return v + v;
    };
    if flag {
        total = bump(total) + shifted;
    }
    else {
        total = value;
    };
    return total;
}

fn main(q: Felt) -> Felt {
    let p = Point { x: q, y: 2 };
    let result = weighted(p.x, 3u32);
    assert(result > 0);
    return result;
}
"#;

struct Compiled {
    ctx: TypeCheckerVisitorContext<SymFeltRef, QExecContext>,
}

fn typecheck_virtual(source: &str) -> Compiled {
    let compiled = super::with_primitive_scope_reset(|| {
        let path = PathBuf::from("/virtual/src/main.psy");
        let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
        let program = Program::new();
        program.file_resolver.add_file(path.clone(), Arc::from(source));
        let mut graph = Graph::new();
        graph.add_node(path);
        let (_typechecker, ctx) = interpreter
            .typecheck_with_program(graph, program)
            .map_err(|error| anyhow::anyhow!("visualizer fixture must typecheck: {error:#}"))?;
        Ok(Compiled { ctx })
    })
    .expect("typecheck within primitive scope reset");
    compiled
}

fn find_function_type(ctx: &mut TypeCheckerVisitorContext<SymFeltRef, QExecContext>, name: &str) -> Option<TypeId> {
    let name_id = ctx.program.interner.intern_ident(name);
    let key: TypeKey = name_id.into();
    for module in ctx.symbols.modules().clone() {
        if let Some(&tid) = ctx.symbols[module.scope_id].types.get(&key) {
            if ctx.symbols[tid].as_function().is_some() {
                return Some(tid);
            }
        }
    }
    None
}

#[test]
#[serial]
fn debug_type_renders_declared_types_with_their_shapes() {
    let mut c = typecheck_virtual(SOURCE);

    let weighted = find_function_type(&mut c.ctx, "weighted").expect("weighted not found");
    let rendered = c.ctx.debug_type(weighted);
    assert!(rendered.contains("fn weighted"), "function header missing:\n{rendered}");
    assert!(rendered.contains("Parameters:"), "parameters missing:\n{rendered}");
    assert!(rendered.contains("mut "), "mutable parameter missing:\n{rendered}");
    assert!(rendered.contains("Return Type: Felt"), "return type missing:\n{rendered}");
    assert!(rendered.contains("Body:"), "body missing:\n{rendered}");

    let point = find_function_type(&mut c.ctx, "Point");
    let point_tid = point.or_else(|| {
        // Point is a struct, not a function: search the type table directly.
        let name_id = c.ctx.program.interner.intern_ident("Point");
        let key: TypeKey = name_id.into();
        c.ctx.symbols
            .modules()
            .iter()
            .find_map(|module| c.ctx.symbols[module.scope_id].types.get(&key).copied())
    });
    let tid = point_tid.expect("Point type not found");
    let rendered = c.ctx.debug_type(tid);
    assert!(rendered.contains("struct Point"), "struct header missing:\n{rendered}");
    assert!(rendered.contains("Fields:"), "fields missing:\n{rendered}");
    assert!(rendered.contains("pub "), "public field marker missing:\n{rendered}");

    // Rendering every type in the table (std included) must not panic and
    // must reach the array and trait arms.
    let mut all = String::new();
    for index in 0..c.ctx.symbols.types.len() {
        all.push_str(&c.ctx.debug_type(TypeId::from(index)));
        all.push('\n');
    }
    assert!(all.contains("Array"), "array type missing");
    assert!(all.contains("trait "), "trait type missing");
}

#[test]
#[serial]
fn debug_scope_renders_constants_variables_types_and_children() {
    let c = typecheck_virtual(SOURCE);

    let mut all = String::new();
    for module in c.ctx.symbols.modules() {
        all.push_str(&c.ctx.debug_scope(module.scope_id));
        all.push('\n');
    }
    assert!(all.contains("Variables:"), "variables section missing");
    assert!(all.contains("Types:"), "types section missing");
    assert!(all.contains("mut "), "mutable variable missing");
    assert!(all.contains("Felt"), "type name missing");
}

#[test]
#[serial]
fn debug_variable_renders_qualifier_and_type_name() {
    let mut c = typecheck_virtual(SOURCE);
    let weighted = find_function_type(&mut c.ctx, "weighted").expect("weighted not found");
    let scope_id = c.ctx.symbols[weighted].as_function().expect("weighted is a function").scope_id;

    let variables: Vec<(IdentId, VarId)> = c.ctx.symbols[scope_id]
        .variables
        .iter()
        .map(|(ident, var)| (*ident, *var))
        .collect();
    assert!(!variables.is_empty(), "weighted has no local variables");

    let mut saw_mutable = false;
    for (ident_id, var_id) in variables {
        let rendered = c.ctx.debug_variable(ident_id, var_id);
        assert!(!rendered.is_empty(), "variable render was empty");
        assert!(rendered.contains(':'), "variable render lacks type separator: {rendered}");
        saw_mutable |= rendered.contains("mut ");
    }
    assert!(saw_mutable, "no mutable variable was rendered");
}

#[test]
#[serial]
fn debug_expr_renders_every_expression_kind_in_the_program() {
    let c = typecheck_virtual(SOURCE);

    let mut all = String::new();
    for index in 0..c.ctx.program.exprs.len() {
        all.push_str(&c.ctx.debug_expr(ExprId(index)));
        all.push('\n');
    }
    for marker in [
        "Binary:",
        "Call",
        "Path",
        "Index Access",
        "Member Access",
        "If Expr",
        "Block Expr",
        "Lambda Function",
        "Cast",
        "Parentheses",
    ] {
        assert!(all.contains(marker), "expression marker {marker:?} missing");
    }
}

#[test]
#[serial]
fn debug_stmt_renders_every_statement_kind_in_the_program() {
    let c = typecheck_virtual(SOURCE);

    let mut all = String::new();
    for index in 0..c.ctx.program.stmts.len() {
        all.push_str(&c.ctx.debug_stmt(StmtId(index)));
        all.push('\n');
    }
    for marker in ["While", "For", "Assignment", "Variable", "Return"] {
        assert!(all.contains(marker), "statement marker {marker:?} missing");
    }
}

#[test]
#[serial]
fn debug_definition_renders_every_definition_kind_in_the_program() {
    let c = typecheck_virtual(SOURCE);

    let mut all = String::new();
    for index in 0..c.ctx.program.defs.len() {
        all.push_str(&c.ctx.debug_definition(DefId(index)));
        all.push('\n');
    }
    for marker in ["Function", "Struct", "Trait", "Impl", "Const", "Use", "pub "] {
        assert!(all.contains(marker), "definition marker {marker:?} missing");
    }
}

/// A second fixture exercising the grammar shapes SOURCE lacks: type
/// aliases, associated types, match, tuples, tuple access, unary not, and
/// else-if chains. (Enums are excluded: `visit_enum` is a stub that always
/// rejects, so no checked enum nodes can exist.)
const RICH_SOURCE: &str = r#"
use std::prelude::*;

// A commented definition so the comments section renders.
pub trait Carrier {
    pub type Load: Storage;
    pub fn load(self: Self) -> Felt;
}

pub struct Holder {
    pub shape: Felt,
}

type Alias = [Felt; 4];

impl Carrier for Holder {
    pub type Load = [Felt; 2];
    pub fn load(self: Self) -> Felt {
        return 1;
    }
}

fn classify(input: Felt, pair: (Felt, bool)) -> Felt {
    let kind: Felt = match input {
        0 => 100,
        1 => 200,
        _ => 300,
    };
    let first = pair.0;
    let second = pair.1;
    let flipped = !second;
    let triple = (first, kind, 3);
    let middle = triple.1;
    let tier: Felt = if first > 5 {
        1
    } else if first > 2 {
        2
    } else {
        3
    };
    return kind + first + middle + tier + (flipped == second) as Felt;
}

fn main() -> Felt {
    return classify(2, (7, true));
}
"#;

#[test]
#[serial]
fn debug_renderers_cover_aliases_associated_types_and_match_shapes() {
    let mut c = typecheck_virtual(RICH_SOURCE);

    // Every definition renders, including type aliases and trait/impl
    // associated types.
    let mut defs = String::new();
    for index in 0..c.ctx.program.defs.len() {
        defs.push_str(&c.ctx.debug_definition(DefId(index)));
        defs.push('\n');
    }
    for marker in ["Type Alias", "Load"] {
        assert!(defs.contains(marker), "definition marker {marker:?} missing:\n{defs}");
    }
    assert!(defs.contains("Comments:"), "trait doc comment missing:\n{defs}");

    // Every expression renders, including match arms, tuples, tuple access,
    // unary not, and else-if chains.
    let mut exprs = String::new();
    for index in 0..c.ctx.program.exprs.len() {
        exprs.push_str(&c.ctx.debug_expr(ExprId(index)));
        exprs.push('\n');
    }
    for marker in ["Match", "Arms:", "Tuple:", "Tuple Access", "Unary:", "Else If Branches"] {
        assert!(exprs.contains(marker), "expression marker {marker:?} missing:\n{exprs}");
    }

    // Rendering every type reaches the alias and const arms without
    // panicking.
    let mut types = String::new();
    for index in 0..c.ctx.symbols.types.len() {
        types.push_str(&c.ctx.debug_type(TypeId::from(index)));
        types.push('\n');
    }
    assert!(!types.is_empty());
}

/// A third fixture for shapes the other two lack: `const fn`, attributes,
/// bare expression statements, and trailing comments on block expressions.
const ATTR_SOURCE: &str = r#"
use std::prelude::*;

pub const fn zero() -> Felt {
    return 0;
}

#[contract]
pub struct C {}

#[contract::write_method]
fn tagged(v: Felt) -> Felt {
    let picked = if v > 0 {
        zero()
    } else {
        zero()
    };
    zero();
    return picked;
}

fn main() -> Felt {
    return tagged(1);
}
"#;

#[test]
#[serial]
fn debug_renderers_cover_const_fns_attrs_and_expression_statements() {
    let mut c = typecheck_virtual(ATTR_SOURCE);

    let mut types = String::new();
    for index in 0..c.ctx.symbols.types.len() {
        types.push_str(&c.ctx.debug_type(TypeId::from(index)));
        types.push('\n');
    }
    assert!(types.contains("const "), "const qualifier missing:\n{types}");

    let mut defs = String::new();
    for index in 0..c.ctx.program.defs.len() {
        defs.push_str(&c.ctx.debug_definition(DefId(index)));
        defs.push('\n');
    }
    assert!(defs.contains("Attrs:"), "fn attrs missing:\n{defs}");

    // Bare `zero();` statements and the commented if-expression render
    // without panicking.
    let mut stmts = String::new();
    for index in 0..c.ctx.program.stmts.len() {
        stmts.push_str(&c.ctx.debug_stmt(StmtId(index)));
        stmts.push('\n');
    }
    // Block-expr comments do not survive checking (the rewriter rebuilds
    // the node without them), so no expr-comments assertion here.
}

/// A fourth fixture for renderer shapes the others lack: inherent-impl
/// associated types, function-signature parameters, and definitions nested
/// inside function bodies.
const IMPL_SOURCE: &str = r#"
use std::prelude::*;

// A commented struct so the definition's comments section renders.
pub struct Holder {
    pub shape: Felt,
}

impl Holder {
    pub type Native = Felt;
    pub fn nat(self: Self) -> Felt {
        return 1;
    }
}

// A commented function so the function definition's comments render.
fn trans(x: Felt) -> Felt {
    return x;
}

fn apply(op: fn(Felt) -> Felt) -> Felt {
    return op(2);
}

fn main() -> Felt {
    let h = Holder { shape: 1 };
    return h.nat() + apply(trans);
}
"#;

#[test]
#[serial]
fn debug_renderers_cover_impl_assoc_types_and_signature_params() {
    let c = typecheck_virtual(IMPL_SOURCE);

    // The inherent impl renders its associated types block.
    let mut defs = String::new();
    for index in 0..c.ctx.program.defs.len() {
        defs.push_str(&c.ctx.debug_definition(DefId(index)));
        defs.push('\n');
    }
    assert!(defs.contains("Associated Types"), "impl assoc types missing:\n{defs}");
    assert!(defs.contains("Native"), "assoc type name missing:\n{defs}");
    assert!(defs.contains("commented struct"), "struct doc comment missing:\n{defs}");
    assert!(defs.contains("commented function"), "function doc comment missing:\n{defs}");

    // A fn-signature parameter renders through get_type_name's
    // FunctionSignature arm.
    let mut types = String::new();
    for index in 0..c.ctx.symbols.types.len() {
        types.push_str(&c.ctx.debug_type(TypeId::from(index)));
        types.push('\n');
    }
    assert!(types.contains("FunctionSignature"), "fn-signature type name missing:\n{types}");
}
