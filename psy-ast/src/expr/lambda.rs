use crate::{ExprId, FunctionParameter, Location, NodeInfo, NodeType, UncheckedType};

#[derive(Debug, Clone, PartialEq)]
pub struct LambdaFunctionNode {
    pub parameters: Vec<FunctionParameter>,
    pub body: ExprId,
    pub return_type: Option<UncheckedType>,
    pub location: Location,
}

impl NodeInfo for LambdaFunctionNode {
    fn node_type(&self) -> NodeType {
        NodeType::LambdaFunctionExpr
    }
}
