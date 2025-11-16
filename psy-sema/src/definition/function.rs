use psy_ast::{AttrNode, Comment, ExprId, Identifier, Location, NodeInfo, NodeType, Qualifier, TypeQualifier, Visibility};

use crate::{ScopeId, TypeId, UNKOWN_TYPE};

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedFunctionNode {
    pub name: Identifier,
    pub parameters: Vec<CheckedFunctionParameter>,
    pub generic_parameters: Vec<TypeId>,
    pub body: Option<ExprId>,
    pub qualifier: Qualifier,
    pub return_type: TypeId,
    pub scope_id: ScopeId,
    pub visibility: Visibility,
    pub attrs: Vec<AttrNode>,
    pub type_id: TypeId,
    pub comments: Vec<Comment>,
    pub location: Location,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedFunctionParameter {
    pub name: Identifier,
    pub qualifier: TypeQualifier,
    pub ty: TypeId,
    pub location: Location,
}

impl CheckedFunctionParameter {
    pub fn new(name: Identifier, qualifier: TypeQualifier, ty: TypeId, location: Location) -> Self {
        Self {
            name,
            qualifier,
            ty,
            location,
        }
    }
}

impl CheckedFunctionNode {
    pub fn signature(&self) -> CheckedFunctionSignature {
        CheckedFunctionSignature {
            parameters: self.parameters.iter().map(|parameter| parameter.ty.clone()).collect(),
            return_type: self.return_type,
        }
    }

    pub fn trait_impl_signature(&self, implementor_type_id: TypeId) -> CheckedFunctionSignature {
        CheckedFunctionSignature {
            parameters: self
                .parameters
                .iter()
                .map(|parameter| {
                    if parameter.ty == UNKOWN_TYPE {
                        implementor_type_id.clone()
                    } else {
                        parameter.ty.clone()
                    }
                })
                .collect(),
            return_type: if self.return_type == UNKOWN_TYPE {
                implementor_type_id
            } else {
                self.return_type.clone()
            },
        }
    }
}

impl NodeInfo for CheckedFunctionNode {
    fn node_type(&self) -> NodeType {
        NodeType::FunctionDef
    }
}

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct CheckedFunctionSignature {
    pub parameters: Vec<TypeId>,
    pub return_type: TypeId,
}
