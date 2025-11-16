use crate::{
    AttrNode, Comment, ExprId, GenericParameter, Identifier, Location, NodeInfo, NodeType, Qualifier, TypeQualifier, UncheckedType, Visibility,
};

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionNode {
    pub name: Identifier,
    pub parameters: Vec<FunctionParameter>,
    pub generic_parameters: Vec<GenericParameter>,
    pub body: Option<ExprId>,
    pub return_type: Option<UncheckedType>,
    pub qualifier: Qualifier,
    pub visibility: Visibility,
    pub attrs: Vec<AttrNode>,
    pub comments: Vec<Comment>,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionParameter {
    pub name: Identifier,
    pub qualifier: TypeQualifier,
    pub ty: UncheckedType,
    pub location: Location,
}

impl FunctionParameter {
    pub fn new(name: Identifier, qualifier: TypeQualifier, ty: UncheckedType, location: Location) -> Self {
        Self {
            name,
            qualifier,
            ty,
            location,
        }
    }
}

impl NodeInfo for FunctionNode {
    fn node_type(&self) -> NodeType {
        NodeType::FunctionDef
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionSignature {
    pub parameters: Vec<UncheckedType>,
    pub return_type: Option<UncheckedType>,
}
