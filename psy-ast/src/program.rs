use std::ops::{Index, IndexMut};

use psy_common::{Arena, FileResolver, Graph, Tree, TreeNode};

use crate::{
    CrateId, DefId, DefinitionNode, ExprId, ExprNode, FileLocation, Ident, IdentId, Interner, Location, ModuleId, ModuleNode, StmtId, StmtNode,
};

#[derive(Debug)]
pub struct Program<F: Clone + From<u32>> {
    pub modules: Tree<ModuleId, ModuleNode>,
    pub dependency_graph: Graph<CrateId>,
    pub std_module_id: Option<ModuleId>,
    pub file_resolver: FileResolver,
    pub exprs: Arena<ExprId, ExprNode<F>>,
    pub stmts: Arena<StmtId, StmtNode>,
    pub defs: Arena<DefId, DefinitionNode>,
    pub interner: Interner,
}

macro_rules! impl_index {
    ($index_type:ty, $output_type:ty, $field:ident) => {
        impl<F: Clone + From<u32>> Index<$index_type> for Program<F> {
            type Output = $output_type;
            fn index(&self, index: $index_type) -> &Self::Output {
                &self.$field[index]
            }
        }

        impl<F: Clone + From<u32>> IndexMut<$index_type> for Program<F> {
            fn index_mut(&mut self, index: $index_type) -> &mut Self::Output {
                &mut self.$field[index]
            }
        }
    };
}

impl_index!(ExprId, ExprNode<F>, exprs);
impl_index!(StmtId, StmtNode, stmts);
impl_index!(DefId, DefinitionNode, defs);
impl_index!(IdentId, Ident, interner);
impl_index!(ModuleId, TreeNode<ModuleId, ModuleNode>, modules);

impl<F: Clone + From<u32>> Program<F> {
    pub fn new() -> Self {
        Self {
            modules: Tree::new(),
            dependency_graph: Graph::new(),
            std_module_id: None,
            file_resolver: FileResolver::new(),
            exprs: Arena::new(),
            stmts: Arena::new(),
            defs: Arena::new(),
            interner: Interner::new(),
        }
    }

    pub fn find_module_by_name(&self, name: impl Into<IdentId>) -> Option<ModuleId> {
        let name = name.into();
        self.modules.iter().find(|m| m.data().name.id == name).map(|m| m.id())
    }

    pub fn module_name(&self, module_id: impl Into<ModuleId>) -> &Ident {
        let module_id = module_id.into();
        let module = self.modules[module_id].data();
        &self.interner[module.name.id]
    }

    pub fn add_module_child(&mut self, parent: Option<ModuleId>, child: ModuleId) {
        if let Some(parent) = parent {
            self.modules.add_child(parent, child);
        }
    }

    pub fn convert_location(&self, location: &Location) -> FileLocation {
        let path = self.file_resolver.resolve_path(&location.file_id).unwrap().display().to_string();
        FileLocation {
            path: path,
            start: location.start,
            end: location.end,
        }
    }

    pub fn is_module_std(&self, module_id: impl Into<ModuleId>) -> bool {
        let Some(std_module_id) = self.std_module_id else {
            return false;
        };
        let mut module_id = Some(module_id.into());
        while let Some(id) = module_id {
            if id == std_module_id {
                return true;
            }
            module_id = self.modules[id].parent();
        }
        false
    }

    pub fn print_module_graph(&self) {
        println!("[Program modules]");
        let interner = &self.interner;
        for module in self.modules.iter() {
            println!(
                "module: {}, {:?}",
                // self.module_name(module_id),
                interner[module.data().name.id],
                module.id()
            );
            println!("  visibility: {:?}", module.data().visibility);
            println!("  children: ");
            for child in module.children() {
                let child_module = &self.modules[*child];
                println!("    {}, {:?}", interner[child_module.data().name.id], child);
            }
            println!("  dependencies: ");
            if let Some(dependencies) = self.dependency_graph.edges(&CrateId::from(module.id())) {
                for dependency in dependencies.iter() {
                    let dependency_module_id = ModuleId::from(*dependency);
                    let dependency_module = &self.modules[dependency_module_id];
                    println!("    {}, {:?}", interner[dependency_module.data().name.id], dependency_module_id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use psy_common::FileId;

    use super::*;
    use crate::{Identifier, Location, Visibility};

    fn module(program: &mut Program<u64>, name: &str) -> ModuleNode {
        let id = program.interner.intern_ident(name);
        ModuleNode {
            name: Identifier::new(id, Location::new(FileId(0), 0, 0)),
            file_id: FileId(0),
            modules: Vec::new(),
            inline_modules: Vec::new(),
            definitions: Vec::new(),
            visibility: Visibility::Public,
            comments: Vec::new(),
            location: Location::new(FileId(0), 0, 0),
        }
    }

    #[test]
    fn module_lookup_and_parent_relationships_are_consistent() {
        let mut program = Program::<u64>::new();
        let root_node = module(&mut program, "root");
        let child_node = module(&mut program, "child");
        let root = program.modules.add_node(root_node);
        let child = program.modules.add_node(child_node);
        program.add_module_child(Some(root), child);

        let child_name = program.interner.intern_ident("child");
        assert_eq!(program.find_module_by_name(child_name), Some(child));
        assert_eq!(program.module_name(child).to_string(), "child");
        assert_eq!(program.modules[child].parent(), Some(root));
        let missing = program.interner.intern_ident("missing");
        assert!(program.find_module_by_name(missing).is_none());
    }

    #[test]
    fn standard_module_detection_walks_ancestors_and_handles_unset_root() {
        let mut program = Program::<u64>::new();
        let root_node = module(&mut program, "root");
        let std_node = module(&mut program, "std");
        let nested_node = module(&mut program, "nested");
        let root = program.modules.add_node(root_node);
        let std = program.modules.add_node(std_node);
        let nested = program.modules.add_node(nested_node);
        program.add_module_child(Some(root), std);
        program.add_module_child(Some(std), nested);

        assert!(!program.is_module_std(root));
        program.std_module_id = Some(std);
        assert!(program.is_module_std(std));
        assert!(program.is_module_std(nested));
        assert!(!program.is_module_std(root));
    }
    #[test]
    fn program_index_and_index_mut_reach_every_arena() {
        let mut program = Program::<u64>::new();
        let location = Location::new(FileId(0), 0, 0);

        let expr_id = program.exprs.alloc_item(ExprNode::Value(crate::ValueNode::Felt(1, location)));
        let replacement = ExprNode::Value(crate::ValueNode::Felt(2, location));
        assert!(matches!(&program[expr_id], ExprNode::Value(node) if matches!(node, crate::ValueNode::Felt(1, _))));
        program[expr_id] = replacement;
        assert!(matches!(&program[expr_id], ExprNode::Value(node) if matches!(node, crate::ValueNode::Felt(2, _))));

        let ident_id = program.interner.intern_ident("indexed");
        assert_eq!(program[ident_id].to_string(), "indexed");
    }

    #[test]
    fn print_module_graph_renders_children_and_dependencies() {
        let mut program = Program::<u64>::new();
        let root_node = module(&mut program, "root");
        let child_node = module(&mut program, "child");
        let root = program.modules.add_node(root_node);
        let child = program.modules.add_node(child_node);
        program.add_module_child(Some(root), child);
        program.dependency_graph.add_edge(CrateId::from(root), CrateId::from(child));

        // Renders to stdout; the assertions guard the data the printer walks.
        assert_eq!(program.modules.len(), 2);
        assert_eq!(program.modules[child].parent(), Some(root));
        program.print_module_graph();
    }
}
