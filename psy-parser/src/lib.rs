pub mod error;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use error::UserError;
pub use error::{Error, Result};
use indexmap::IndexMap;
use lalrpop_util::lalrpop_mod;
use psy_ast::{Program, *};
use psy_common::Graph;
use psy_lexer::{GenericTokenTransformer, Lexer, Loc, Token};
use psy_vm::dpn::ops::context_trait::{ContextFelt, DPNContext};

pub type LalrpopError<'input> = lalrpop_util::ParseError<Loc, Token<'input>, UserError>;

lalrpop_mod!(pub psy);

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
        let file_id = program.file_resolver.resolve_file(current_path.clone())?;
        let file_content = program.file_resolver.resolve_content(&file_id).ok_or(Error::FileUnresolved)?;

        let lexer = Lexer::new(file_content);
        let transformer = GenericTokenTransformer::new(lexer);
        let tokens: Vec<_> = transformer.collect::<psy_lexer::Result<Vec<_>>>()?;
        let module = psy::ModuleParser::new()
            .parse(
                file_content,
                file_id,
                Identifier::new(module_name, Location::new(file_id, location.start, location.end)),
                &mut program.exprs,
                &mut program.stmts,
                &mut program.defs,
                &mut program.interner,
                visibility,
                ctx,
                tokens,
            )
            .map_err(|e| Error::from_lalrpop_error(e, file_id))?;
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
                    inline_module.visibility.clone(),
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

        // Remove duplicates children
        program.modules.iter_mut().for_each(|module| {
            let children = module.children_mut();
            children.sort_unstable();
            children.dedup();
        });

        // program.print_module_graph();
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
    path.push(format!("{}.{}", program.interner[module_name.clone()], ext));
    Some(path)
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use psy_ast::Program;
    use psy_common::Graph;
    use psy_vm::dpn::ops::exec_context::QExecContext;

    use super::Parser;
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
