use psy_common::{define_arena_id, Arena, FileId};

use crate::{Comment, DefId, DefinitionNode, IdentId, Identifier, Location, NodeInfo, NodeType, Visibility};
#[derive(Clone, Copy, Eq, Hash, PartialEq, Debug)]
pub struct CrateId(pub usize);

impl CrateId {
    pub fn from_module_id(module_id: ModuleId) -> Self {
        Self(module_id.0)
    }
}

impl From<ModuleId> for CrateId {
    fn from(module_id: ModuleId) -> Self {
        Self(module_id.0)
    }
}

impl From<&ModuleId> for CrateId {
    fn from(module_id: &ModuleId) -> Self {
        Self(module_id.0)
    }
}

impl From<CrateId> for ModuleId {
    fn from(crate_id: CrateId) -> Self {
        Self(crate_id.0)
    }
}

define_arena_id!(ModuleId);

impl ModuleId {
    pub const fn root() -> Self {
        Self(0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModuleKind {
    File { file_id: FileId },
}

#[derive(Debug, Clone, PartialEq)]
pub struct UseNode {
    pub visibility: Visibility,
    pub kind: Identifier,
    pub segments: Vec<Identifier>,
    pub target: Option<Identifier>,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl NodeInfo for UseNode {
    fn node_type(&self) -> NodeType {
        NodeType::UseDef
    }
}

#[derive(Clone, Debug)]
pub struct ModuleNode {
    pub name: Identifier,
    pub file_id: FileId,
    pub modules: Vec<(Identifier, Visibility, Location)>,
    pub inline_modules: Vec<ModuleNode>,
    pub definitions: Vec<DefId>,
    pub visibility: Visibility,
    pub comments: Vec<Comment>,
    pub location: Location,
}

impl ModuleNode {
    pub fn new(
        name: Identifier,
        file_id: FileId,
        visibility: Visibility,
        module_items: Vec<ModuleItemNode>,
        def_nodes: &mut Arena<DefId, DefinitionNode>,
        comments: Vec<Comment>,
        location: Location,
    ) -> Self {
        let mut inline_modules = vec![];
        let mut modules = vec![];
        let mut definitions = vec![];
        for item in module_items.into_iter() {
            match item {
                ModuleItemNode::InlineModule(m) => inline_modules.push(m),
                ModuleItemNode::ModuleDecl(m) => modules.push(m),
                ModuleItemNode::Definition(d) => definitions.push(d),
                ModuleItemNode::Comment(_c) => todo!(),
            }
        }
        let module = Self {
            name,
            file_id,
            modules,
            inline_modules,
            definitions,
            visibility,
            comments,
            location,
        };
        module
    }

    // FIXME: this is a workaround to get the std module
    pub fn is_std(&self) -> bool {
        matches!(self.name.id, IdentId::STD | IdentId::PRELUDE | IdentId::PRIMITIVE)
    }

    pub fn is_self_primitive(&self) -> bool {
        self.name == IdentId::PRIMITIVE
    }
}

#[derive(Clone, Debug)]
pub enum ModuleItemNode {
    ModuleDecl((Identifier, Visibility, Location)),
    InlineModule(ModuleNode),
    Definition(DefId),
    Comment(Comment),
}

#[cfg(test)]
mod tests {
    use psy_common::FileId;

    use super::*;

    fn identifier(name: &str) -> Identifier {
        let id = match name {
            "std" => IdentId::STD,
            "prelude" => IdentId::PRELUDE,
            "primitive" => IdentId::PRIMITIVE,
            _ => IdentId::from(999usize),
        };
        Identifier::new(id, Location::new(FileId(0), 0, 0))
    }

    fn empty_module(name: &str) -> ModuleNode {
        ModuleNode {
            name: identifier(name),
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
    fn module_constructor_separates_children_and_definitions() {
        let mut defs = Arena::new();
        let external = (identifier("external"), Visibility::Private, Location::default());
        let inline = empty_module("inline");
        let module = ModuleNode::new(
            identifier("root"),
            FileId(0),
            Visibility::Public,
            vec![
                ModuleItemNode::ModuleDecl(external),
                ModuleItemNode::InlineModule(inline),
                ModuleItemNode::Definition(DefId::from(0usize)),
            ],
            &mut defs,
            Vec::new(),
            Location::default(),
        );

        assert_eq!(module.modules.len(), 1);
        assert_eq!(module.modules[0].0.id, identifier("external").id);
        assert_eq!(module.inline_modules.len(), 1);
        assert_eq!(module.inline_modules[0].name.id, identifier("inline").id);
        assert_eq!(module.definitions, vec![DefId::from(0usize)]);
    }

    #[test]
    fn standard_module_helpers_match_only_known_standard_names() {
        for name in ["std", "prelude", "primitive"] {
            let module = empty_module(name);
            assert!(module.is_std(), "{name} should be a standard module");
        }
        let primitive = empty_module("primitive");
        assert!(primitive.is_self_primitive());
        assert!(!empty_module("user").is_std());
        assert!(!empty_module("user").is_self_primitive());
    }
}
