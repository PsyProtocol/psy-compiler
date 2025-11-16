use std::ops::{Index, IndexMut};

use psy_common::{Arena, FileResolver, Graph, Tree, TreeNode};

use crate::{
    CrateId, DefId, DefinitionNode, ExprId, ExprNode, FileLocation, Ident, IdentId, Interner, Location, ModuleId, ModuleNode, StmtId, StmtNode,
};

#[derive(Debug)]
pub struct Program<F: Clone + From<u32>> {
    pub modules: Tree<ModuleId, ModuleNode>,
    pub dependency_graph: Graph<CrateId>,
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
        let mut module_id = Some(module_id.into());
        while let Some(id) = module_id {
            let module = &self.modules[id];
            if module.data().name.id == IdentId::STD {
                return true;
            }
            module_id = module.parent();
        }
        return false;
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
