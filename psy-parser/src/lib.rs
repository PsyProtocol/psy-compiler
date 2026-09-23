pub mod error;
pub mod recursive;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub use error::{Error, Result};
use indexmap::IndexMap;
use psy_ast::{Program, *};
use psy_common::Graph;
use psy_vm::dpn::ops::context_trait::{ContextFelt, DPNContext};

#[cfg(target_arch = "wasm32")]
const WASM_STD_ROOT: &str = "/__psy_std__";
#[cfg(target_arch = "wasm32")]
const WASM_STD_FILES: &[(&str, &str)] = &[
    ("std.psy", include_str!("../../psy-std/std.psy")),
    ("default.psy", include_str!("../../psy-std/default.psy")),
    ("event.psy", include_str!("../../psy-std/event.psy")),
    ("prelude.psy", include_str!("../../psy-std/prelude.psy")),
    ("context.psy", include_str!("../../psy-std/context.psy")),
    ("mem.psy", include_str!("../../psy-std/mem.psy")),
    ("storage.psy", include_str!("../../psy-std/storage.psy")),
    ("primitive.psy", include_str!("../../psy-std/primitive.psy")),
];

#[derive(Debug)]
pub struct Parser<'a, 'b, F: Clone + From<u32>, C> {
    program: &'a mut Program<F>,
    ctx: &'b mut C,
    crate_path_graph: Graph<PathBuf>,
}

impl<'a, 'b, F: ContextFelt + From<u32>, C: DPNContext<F>> Parser<'a, 'b, F, C> {
    pub fn new(program: &'a mut Program<F>, ctx: &'b mut C, crate_path_graph: Graph<PathBuf>) -> Self {
        Self {
            program,
            ctx,
            crate_path_graph,
        }
    }

    pub fn parse(&mut self) -> Result<()> {
        preload_embedded_std(self.program);
        let mut crate_id_map = HashMap::new();
        let entry_paths = self.crate_path_graph.clone();
        entry_paths.bfs(&mut |entry_path| {
            let Self { program, ctx, .. } = self;
            let module_id = Self::parse_inner(program, ctx, entry_path.clone())?;
            crate_id_map.insert(entry_path, CrateId::from(module_id));
            Ok::<(), Error>(())
        })?;
        let mut crate_dependency_graph = Graph::new();
        for entry_path in entry_paths.nodes() {
            let node_crate_id = crate_id_map[entry_path];
            crate_dependency_graph.add_node(node_crate_id);
            for dep_path in entry_paths.edges(entry_path).unwrap() {
                let dep_crate_id = crate_id_map[dep_path];
                crate_dependency_graph.add_edge(node_crate_id, dep_crate_id);
            }
        }
        let Self { program, ctx, .. } = self;
        Self::finish_inner(program, ctx, crate_dependency_graph)?;
        Ok(())
    }

    fn parse_module(program: &mut Program<F>, ctx: &mut C, current_path: &PathBuf, location: Location, visibility: Visibility) -> Result<ModuleNode> {
        let module_name = resolve_module_name(program, current_path);
        if !is_valid_module_name(&program.interner[module_name].0) {
            return Err(Error::InvalidModuleName);
        }
        let file_id = program.file_resolver.resolve_file(current_path.clone())?;

        // Keep the source alive independently so parsing can mutably borrow the
        // rest of the program.
        let source = program
            .file_resolver
            .resolve_content_arc(&file_id)
            .ok_or(Error::FileUnresolved)?;
        let module = recursive::parse_module_into(
            recursive::ParseModuleInput {
                source: &source,
                file_id,
                module_name: Identifier::new(
                    module_name,
                    Location::new(file_id, location.start, location.end),
                ),
                visibility,
            },
            program,
            ctx,
        )?;
        Ok(module)
    }

    // std/
    //     prelude
    //
    // modA/
    //     std/
    //         prelude
    // modB/
    //     std/
    //         prelude
    fn parse_inner(program: &mut Program<F>, ctx: &mut C, root_module_path: PathBuf) -> Result<ModuleId> {
        let mut module_stack = vec![(
            false,
            root_module_path.clone(),
            Option::<ModuleId>::None,
            Visibility::Public,
            Location::default(),
        )];
        let mut visited = HashSet::new();
        let mut inline_modules: IndexMap<PathBuf, ModuleNode> = IndexMap::new();

        let mut entry_module_id = None;
        while let Some((is_inline, current_path, parent_module_id, visibility, location)) = module_stack.pop() {
            if visited.contains(&current_path) {
                return Err(Error::FileParsedMultipleTimes(current_path.clone()).into());
            }
            let module_id = program.modules.next_idx();
            entry_module_id.get_or_insert(module_id);
            let module: ModuleNode = if !is_inline {
                Self::parse_module(program, ctx, &current_path, location, visibility)?
            } else {
                inline_modules.remove(&current_path).expect("Inline module not found")
            };

            for (dep_module, visibility, _location) in module.modules.iter().rev() {
                let dep_path = resolve_module_path(program, dep_module.id, &current_path).unwrap();
                module_stack.push((false, dep_path, Some(module_id), *visibility, dep_module.location));
            }

            for inline_module in module.inline_modules.iter().rev() {
                let dep_path = resolve_module_path(program, inline_module.name, &current_path).unwrap();
                module_stack.push((
                    true,
                    dep_path.clone(),
                    Some(module_id),
                    inline_module.visibility,
                    inline_module.name.location,
                ));
                inline_modules.insert(dep_path, inline_module.clone());
            }

            program.modules.add_node(module);
            program.add_module_child(parent_module_id, module_id);

            visited.insert(current_path);
        }

        let entry_module_id = entry_module_id.ok_or(Error::NoEntryModule(root_module_path))?;
        Ok(entry_module_id)
    }

    fn finish_inner(program: &mut Program<F>, ctx: &mut C, mut dependency_graph: Graph<CrateId>) -> Result<()> {
        let std_module_id = Self::parse_inner(program, ctx, std_path())?;
        program.std_module_id = Some(std_module_id);
        let std_crate_id = std_module_id.into();
        dependency_graph.add_node(std_crate_id);
        let crate_ids = dependency_graph.nodes().into_iter().cloned().collect::<Vec<_>>();
        for crate_id in crate_ids {
            dependency_graph.add_edge(crate_id, std_crate_id);
        }
        program.dependency_graph = dependency_graph;
        program.dependency_graph.check_cycle::<Error>()?;

        let module_ids = program.modules.iter().map(|n| n.id()).collect::<Vec<_>>();
        for module_id in module_ids {
            if !program.is_module_std(module_id) {
                let file_id = program.modules[module_id].data().file_id;
                let def_id = program.defs.alloc_item(DefinitionNode::Use(UseNode {
                    visibility: Visibility::Private,
                    kind: Identifier::new(IdentId::STD, Location::new(file_id, 0, 0)),
                    segments: vec![Identifier::new(IdentId::PRELUDE, Location::new(file_id, 0, 0))],
                    target: None,
                    comments: vec![],
                    location: Location::new(file_id, 0, 0),
                }));
                let definitions = &mut program.modules[module_id].data_mut().definitions;
                definitions.insert(0, def_id);
                definitions.sort_by(|&a, &b| {
                    let a = &program.defs[a];
                    let b = &program.defs[b];
                    b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Less)
                });
            }
        }

        // Remove duplicate children.
        program.modules.iter_mut().for_each(|module| {
            let children = module.children_mut();
            children.sort_unstable();
            children.dedup();
        });

        Ok(())
    }
}

pub fn resolve_module_path<F: Clone + From<u32>>(
    program: &mut Program<F>,
    module_name: impl Into<IdentId>,
    current_path: &PathBuf,
) -> Option<PathBuf> {
    let module_name = module_name.into();
    if module_name == IdentId::STD {
        return Some(std_path());
    }
    let mut path = current_path.parent()?.to_path_buf();
    let ext = current_path.extension()?.to_str()?;
    path.push(format!("{}.{}", program.interner[module_name], ext));
    Some(path)
}

fn is_valid_module_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub fn resolve_module_name<F: Clone + From<u32>>(program: &mut Program<F>, file_path: &Path) -> IdentId {
    let interner = &mut program.interner;
    let file_name_without_extension = file_path.file_stem().and_then(|s| s.to_str()).unwrap();
    let module_name = match file_name_without_extension {
        "lib" | "main" => {
            // Get the parent directory name
            file_path
                .parent()
                .and_then(|parent| parent.parent())
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .unwrap()
        }
        s => s,
    };
    interner.intern_ident(module_name)
}

fn std_path() -> PathBuf {
    #[cfg(target_arch = "wasm32")]
    {
        return PathBuf::from(WASM_STD_ROOT).join("std.psy");
    }

    if let Ok(std_path) = std::env::var("DARGO_STD_PATH") {
        return PathBuf::from(std_path);
    }

    let current_file = std::path::Path::new(file!());

    if let Some(parser_dir) = current_file.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
        let std_path = parser_dir.join("psy-std/std.psy");
        if std_path.exists() {
            return std_path;
        }
    }

    if let Ok(cargo_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let candidates = [
            "../psy-std/std.psy",
            "../../psy-std/std.psy",
            "../../../psy-std/std.psy",
            "../psy_compiler/psy-std/std.psy",
            "../../psy_compiler/psy-std/std.psy",
        ];

        for candidate in &candidates {
            let std_path = PathBuf::from(&cargo_dir).join(candidate);
            if std_path.exists() {
                if let Ok(canonical) = std_path.canonicalize() {
                    return canonical;
                }
            }
        }
    }

    panic!("Cannot find psy-std/std.psy. Please set DARGO_STD_PATH environment variable or ensure psy-std is in the expected location relative to psy-parser.");
}

fn preload_embedded_std<F: Clone + From<u32>>(program: &mut Program<F>) {
    #[cfg(target_arch = "wasm32")]
    {
        for (name, content) in WASM_STD_FILES {
            let path = PathBuf::from(WASM_STD_ROOT).join(name);
            if program.file_resolver.resolve_id(&path).is_none() {
                program.file_resolver.add_file(path, *content);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use psy_ast::Program;
    use psy_common::Graph;
    use psy_vm::dpn::ops::exec_context::QExecContext;

    use super::Parser;

    #[test]
    fn module_file_names_use_identifier_syntax() {
        assert!(super::is_valid_module_name("foo"));
        assert!(super::is_valid_module_name("_foo42"));
        for name in ["", "42foo", "foo-bar", "foo.bar", "foo/bar", "foo bar", "包"] {
            assert!(!super::is_valid_module_name(name), "accepted invalid module name {name:?}");
        }
    }

    #[test]
    fn parsing_rejects_invalid_module_file_name() {
        let path = PathBuf::from("foo-bar.psy");
        let mut program = Program::new();
        let file_id = program.file_resolver.add_file(path.clone(), "");
        let mut ctx = QExecContext::new();

        let error = Parser::parse_module(
            &mut program,
            &mut ctx,
            &path,
            psy_ast::Location::new(file_id, 0, 0),
            psy_ast::Visibility::Public,
        )
        .expect_err("a hyphenated module file name must be rejected");
        assert!(matches!(error, super::Error::InvalidModuleName));
    }

    #[test]
    fn test_psy_parser() {
        let mut program = Program::new();
        let mut ctx = QExecContext::new();
        let mut crate_path_graph = Graph::new();
        crate_path_graph.add_node(PathBuf::from("../tests/storage_test.psy"));
        let mut parser = Parser::new(&mut program, &mut ctx, crate_path_graph);
        parser.parse().unwrap();
    }
}

#[cfg(test)]
mod parse_edge_tests {
    use std::path::PathBuf;

    use psy_ast::{Program, Visibility};
    use psy_vm::dpn::ops::exec_context::QExecContext;

    use super::Parser;

    /// Parse one module's worth of source through the real recursive parser.
    fn parse(src: &str) -> Result<(), String> {
        let path = PathBuf::from("edge_case.psy");
        let mut program = Program::new();
        let file_id = program.file_resolver.add_file(path.clone(), src);
        let mut ctx = QExecContext::new();
        Parser::parse_module(
            &mut program,
            &mut ctx,
            &path,
            psy_ast::Location::new(file_id, 0, 0),
            Visibility::Public,
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    fn ok(label: &str, src: &str) {
        if let Err(message) = parse(src) {
            panic!("[{label}] expected parse success, got:\n{message}");
        }
    }

    fn fails(label: &str, src: &str) {
        if let Ok(()) = parse(src) {
            panic!("[{label}] expected a parse rejection, got success");
        }
    }

    fn err(label: &str, src: &str, needle: &str) {
        match parse(src) {
            Ok(()) => panic!("[{label}] expected parse error containing `{needle}`, got success"),
            Err(message) => assert!(
                message.to_lowercase().contains(&needle.to_lowercase()),
                "[{label}] expected error containing `{needle}`, got:\n{message}"
            ),
        }
    }

    #[test]
    fn compound_assignments_and_expression_statements_parse() {
        ok(
            "every compound assignment operator",
            "fn main() { let mut x = 1; x += 2; x -= 3; x *= 4; x /= 5; x %= 6; x &= 7; x |= 8; x ^= 9; x <<= 1; x >>= 1; }",
        );
        ok("bare expression statement", "fn main() { 1 + 2; }");
        ok("plain assignment", "fn main() { let mut x = 1; x = 2; }");
    }

    #[test]
    fn statement_shapes_reject_modules_and_unterminated_bodies() {
        fails("inline module inside a function body", "fn main() { mod inner {} }");
        err("unterminated function body", "fn main() {", "end of file");
        ok("nested struct definition inside a body", "fn main() { struct S {} }");
    }

    #[test]
    fn type_positions_cover_self_tuples_trailing_commas_and_arrays() {
        ok("self type in return position", "fn f() -> Self {}");
        ok("tuple with trailing comma", "fn f(t: (Felt,)) {}");
        ok("empty tuple type", "fn f(t: ()) {}");
        ok("array type annotation", "fn f(a: [Felt; 3]) {}");
        fails("literal inside a tuple type", "fn f(t: (1, Felt)) {}");
        fails("missing colon after parameter name", "fn f(x Felt) {}");
    }

    #[test]
    fn impls_traits_and_type_aliases_guard_their_headers() {
        fails("public impl is rejected", "pub impl P {}");
        fails("attributed trait is rejected", "#[contract] trait T {}");
        fails("attributed type alias is rejected", "#[contract] type A = Felt;");
        ok("private impl parses", "impl P { pub fn m() {} }");
        err("impl method without a body", "impl P { pub fn m(); }", "body");
        ok("trait method without a body", "trait T { pub fn m(); }");
        ok("self parameter parses", "impl P { pub fn m(self) {} }");
    }

    #[test]
    fn module_declarations_guard_comments_and_delimiters() {
        fails("unterminated inline module", "mod m {");
        ok("use roots cover crate and super", "use crate::a::b;\nuse super::c;");
    }

    #[test]
    fn struct_bodies_reject_comments_before_the_closing_brace() {
        err("comment before struct close", "struct S { x: Felt, // trail\n }", "comment");
        ok("empty struct parses", "struct S {}");
    }

    #[test]
    fn parser_std_path_prefers_env_then_falls_back_to_the_checkout() {
        unsafe { std::env::set_var("DARGO_STD_PATH", "/tmp/env_std_marker.psy") };
        assert_eq!(super::std_path(), PathBuf::from("/tmp/env_std_marker.psy"));
        unsafe { std::env::remove_var("DARGO_STD_PATH") };
        let resolved = super::std_path();
        assert!(resolved.is_file(), "expected the workspace psy-std checkout, got {resolved:?}");
    }

    #[test]
    fn function_bodies_and_self_parameters_are_guarded() {
        err("module fn without a body", "fn f();", "body");
        err("top-level fn with a parameter named self", "fn f(self: Felt) {}", "self");
        fails("impl method with self after another parameter", "pub struct P { pub x: Felt }\nimpl P { pub fn m(x: Felt, self: Self) {} }");
        fails("trait method with self after another parameter", "pub trait T { pub fn m(x: Felt, self: Self); }");
    }

    #[test]
    fn impl_and_trait_bodies_guard_ordering_and_eof() {
        err("eof inside an impl body", "pub struct P { pub x: Felt }\nimpl P {", "end of file");
        fails("associated type after methods in an impl", "pub struct P { pub x: Felt }\nimpl P { pub fn m() {}\n    pub type T = Felt; }");
        err("eof inside a trait body", "pub trait T {", "end of file");
        fails("associated type after methods in a trait", "pub trait T { pub fn m();\n    pub type X; }");
        ok("associated type before methods in a trait", "pub trait T { pub type X;\n    pub fn m(); }");
    }

    #[test]
    fn attributes_and_enums_are_guarded_in_item_position() {
        fails("attribute before a module", "#[contract]\nmod m { }");
        fails("attribute before an enum", "#[contract]\nenum E { A }");
        ok("enum with basic tuple and struct variants", "pub enum E { A, B(u32), C { x: Felt } }");
    }

    #[test]
    fn nested_module_declarations_guard_comments_and_delimiters() {
        fails("comment before an external nested module", "mod outer { /* c */ mod inner; }");
        ok("external nested module", "mod outer { mod inner; }");
        fails("eof inside a nested module body", "mod outer { fn");
    }

    #[test]
    fn expression_postfix_and_primary_edges() {
        fails("dangling member access", "fn main() { let x = a.; }");
        fails("empty parentheses expression", "fn main() { let x = (); }");
        fails("missing expression after assign", "fn main() { let x = ; }");
        err("eof after assign", "fn main() { let x =", "end of file");
        fails("eof inside a struct literal", "fn main() { let p = S {");
        ok("empty match expression", "fn main() { let v = match 1 { }; }");
        ok("or-patterns in match arms", "fn main() { let v = match 1 { 1 | 2 => 3, _ => 4 }; }");
        fails("non-literal array repeat count", "fn main() { let a = 1; let b = [0; a]; }");
        ok("comment between binary operands", "fn main() { let x = 1 + // mid\n 2; }");
        ok("empty struct literal", "fn main() { let p = S { }; }");
        fails("untyped closure parameter", "fn main() { let f = |v| v; }");
        ok("typed closure", "fn main() { let f = |v: Felt| -> Felt { return v; }; }");
    }

    #[test]
    fn module_items_reject_stray_tokens() {
        fails("stray literal at module level", "42");
        fails("stray operator at module level", "+");
    }

    #[test]
    fn turbofish_and_comparison_disambiguation_edges() {
        // A turbofish on a bare path with no call and no member target is
        // rejected; the method form (member access) is accepted.
        fails("turbofish on a call result without another call", "fn main() { let x = g()::<Felt>; }");
        ok("bare method turbofish without a call", "fn main() { let r = p.m::<Felt>; }");
        ok("less-than comparison is not generic probing", "fn main() { let x = 1 < 2; }");
        ok("generic arguments on a non-generic struct literal fall back to comparison", "pub struct Q { pub x: Felt }\nfn main() { let q = Q<Felt> { x: 1 }; }");
        ok("mismatched generic arguments on a struct literal fall back to comparison", "pub struct G<T> { pub v: T }\nfn main() { let g = G<Felt, Felt> { v: 1 }; }");
        fails("garbage inside turbofish arguments", "pub mod m { pub fn g<T>(x: T) {} }\nfn main() { m::<Felt(1); }");
        fails("eof inside turbofish arguments", "pub mod m { pub fn g<T>(x: T) {} }\nfn main() { m::<Felt");
    }

    #[test]
    fn lambda_parameter_list_edges() {
        ok("zero-parameter closure", "fn main() { let f = || -> Felt { return 1; }; }");
        ok("two-parameter closure", "fn main() { let f = |a: Felt, b: Felt| -> Felt { return a; }; }");
        ok("closure parameter trailing comma", "fn main() { let f = |a: Felt,| -> Felt { return a; }; }");
        ok("closure with a bare self parameter", "fn main() { let f = |self| -> Felt { return 1; }; }");
        ok("immediately invoked closure", "fn main() { let x = (|v: Felt| -> Felt { return v; })(1); }");
    }

    #[test]
    fn match_struct_literal_and_array_edges() {
        ok("match body holding only comments", "fn main() { let v = match 1 { // only a comment\n }; }");
        ok("comment before a struct literal closing brace", "fn main() { let p = S { x: 1, // c\n }; }");
        err("eof after the array repeat separator", "fn main() { let b = [0;", "end of file");
        err("eof where a semicolon is expected", "fn main() { let x = 1", "end of file");
    }

    #[test]
    fn intrinsic_short_forms_and_message_guards() {
        ok("imt_get single-argument form", "fn main() { let v = __imt_get(1); }");
        ok("imt_set two-argument form", "fn main() { __imt_set(1, 2); }");
        fails("imt_get without arguments", "fn main() { let v = __imt_get(); }");
        fails("assert with a non-string message argument", "fn main() { assert(1, 2); }");
        fails("assert_eq with a non-string message argument", "fn main() { assert_eq(1, 2, 3); }");
    }

    #[test]
    fn comments_around_binary_operators_parse() {
        ok("block comment between operands", "fn main() { let x = 1 /* between */ + 2; }");
        ok("line comment between operands", "fn main() { let x = 1 // note\n + 2; }");
        ok("block comment between comparison operands", "fn main() { let c = 1 >= /* note */ 2; }");
    }

    #[test]
    fn type_position_generic_and_size_edges() {
        fails("generic arguments on an array type", "fn f(x: [Felt; 3]<Felt>) {}");
        fails("non-literal array size", "fn f(x: [Felt; true]) {}");
        ok("generic target on a module path", "pub mod m { pub struct H<T> { pub v: T } }\nfn f(x: m::<H<Felt>>) {}");
        ok("trailing comma in explicit generic arguments", "pub mod m { pub fn g<T>(x: T) {} }\nfn main() { m::<Felt,>(1); }");
    }
}
