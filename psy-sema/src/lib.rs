#![feature(if_let_guard)]
#![feature(let_chains)]

mod constraint;
mod context;
mod definition;
mod expr;
mod format;
mod generic;
mod implementer;
mod infer;
mod predecl;
mod program;
mod reference;
mod resolver;
mod rewriter;
mod stmt;
mod symbol_table;
mod traits;
mod r#type;
mod value;
mod variable;

mod error;
mod visualizer;

use std::{
    collections::{HashMap, HashSet},
    result::Result as StdResult,
};

use anyhow::anyhow;
pub use constraint::*;
pub use context::*;
pub use definition::*;
pub use error::*;
pub use expr::*;
pub use generic::*;
pub use implementer::*;
use indexmap::IndexMap;
pub use infer::*;
use itertools::Itertools;
pub use program::*;
use psy_ast::*;
use psy_vm::dpn::ops::context_trait::ContextFelt;
pub use r#type::*;
pub use reference::*;
pub use resolver::*;
pub use stmt::*;
pub use symbol_table::*;
use tracing::instrument;
pub use traits::*;
pub use value::*;
pub use variable::*;
pub use visualizer::*;

use crate::rewriter::Rewriter;

pub struct TypeChecker<F: Clone + From<u32> + ContextFelt, C> {
    pub program: CheckedProgram<F>,
    evaluator: Box<dyn Evaluator<F, C>>,
    pub resolver: ResolverCtxt,
    pub infcx: InferCtxt<F, C>,
    pub implementer: ImplementerCtxt,
    pub unchecked_checked: HashMap<NodeId, NodeId>,

    _marker: std::marker::PhantomData<C>,
}

impl<F: Clone + From<u32> + ContextFelt, C> AstVisitor<F, C> for TypeChecker<F, C> {
    type Expr = ExprNode<F>;

    type Stmt = StmtNode;

    type Definition = DefinitionNode;

    type ExprResult = CheckedExprNode<F>;

    type StmtResult = CheckedStmtNode;

    type DefinitionResult = DefId;

    type Context = TypeCheckerVisitorContext<F, C>;

    type Error = Error;

    #[instrument(level = "debug", skip_all)]
    fn visit_use(&mut self, def_id: DefId, ctx: &mut Self::Context) -> StdResult<Self::DefinitionResult, Self::Error> {
        // TODO: remove clone
        let node = ctx.definition(def_id).as_use().cloned().unwrap();
        self.add_use(&node, ctx)?;
        Ok(self.program.defs.alloc_item(CheckedDefinitionNode::Use(node)))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_path(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let path_node = ctx.expression(node).as_path().cloned().unwrap();
        let checked_path_node = self.resolve_path(&path_node, ctx)?;
        if let Some(var_id) = checked_path_node.variable {
            ctx.add_variable_reference(var_id, checked_path_node.location, false);
        }
        ctx.add_type_reference(checked_path_node.type_id, checked_path_node.location, false);
        return Ok(CheckedExprNode::Path(checked_path_node));
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_index_access(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let index_access_node = ctx.expression(node).as_index_access().cloned().unwrap();
        let checked_expr = self.visit_expr(index_access_node.target, ctx)?;
        let checked_index = self.visit_expr(index_access_node.index, ctx)?;
        let type_id = checked_expr.ty();
        let ty = &ctx.symbols[type_id];

        let inner_ty = ty
            .as_array()
            .ok_or(Error::TypeMismatch {
                location: index_access_node.location,
                expected: vec![ARRAY_TYPE],
                found: type_id,
            })?
            .inner_ty;

        if !self.unify(checked_index.ty(), FELT_TYPE, ctx) {
            return Err(Error::TypeMismatch {
                location: checked_index.location(),
                expected: vec![FELT_TYPE],
                found: checked_index.ty(),
            });
        }

        Ok(CheckedExprNode::IndexAccess(CheckedIndexAccessNode {
            target: self.program.exprs.alloc_item(checked_expr),
            index: self.program.exprs.alloc_item(checked_index),
            type_id: self.substitute_all(inner_ty, ctx)?,
            location: index_access_node.location,
        }))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_member_access(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let member_access_node = ctx.expression(node).as_member_access().cloned().unwrap();
        let checked_expr = self.visit_expr(member_access_node.target, ctx)?;
        let type_id = checked_expr.ty();

        if ctx.ancestor_node_type(1).is_member_call_expr() {
            if let Ok(type_id) = self.find_member(type_id, None, member_access_node.field, ctx) {
                ctx.add_type_reference(type_id, member_access_node.field.location, false);

                let visibility = ctx.symbols[type_id].visibility();
                if !(visibility.is_public() || self.typecheck_member_access(member_access_node.target, ctx)) {
                    return Err(Error::MemberNotPublic {
                        location: member_access_node.location,
                        ty: type_id,
                        field: member_access_node.field.id,
                    });
                }
                return Ok(CheckedExprNode::MemberAccess(CheckedMemberAccessNode {
                    target: self.program.exprs.alloc_item(checked_expr),
                    field: member_access_node.field,
                    type_id,
                    location: member_access_node.location,
                }));
            }
        }

        let fields = ctx.symbols[type_id].as_struct().unwrap().fields.clone();
        let CheckedStructField {
            ty: field_type, visibility, ..
        } = fields.get(&member_access_node.field).ok_or(Error::UnresolvedMember {
            location: member_access_node.location,
            member_name: member_access_node.field.id,
        })?;
        ctx.add_type_reference(*field_type, member_access_node.field.location, false);
        if !(visibility.is_public() || self.typecheck_member_access(member_access_node.target, ctx)) {
            return Err(Error::MemberNotPublic {
                location: member_access_node.location,
                ty: type_id,
                field: member_access_node.field.id,
            });
        }
        return Ok(CheckedExprNode::MemberAccess(CheckedMemberAccessNode {
            target: self.program.exprs.alloc_item(checked_expr),
            field: member_access_node.field.clone(),
            type_id: self.substitute_all(field_type.clone(), ctx)?,
            location: member_access_node.location,
        }));
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_tuple_access(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // get TupleAccessNode
        let tuple_access_node = ctx.expression(node).as_tuple_access().cloned().unwrap();

        let checked_expr = self.visit_expr(tuple_access_node.target, ctx)?;
        let type_id = checked_expr.ty();
        let ty = &ctx.symbols[type_id];
        let element_types = ty.as_tuple().ok_or(anyhow!("Expected tuple type"))?;

        if tuple_access_node.index >= element_types.len() {
            return Err(Error::IndexOutOfBounds {
                location: tuple_access_node.location,
                index: tuple_access_node.index,
                length: element_types.len(),
            });
        }

        let field_type = element_types
            .get(tuple_access_node.index)
            .ok_or(Error::IndexOutOfBounds {
                location: tuple_access_node.location,
                index: tuple_access_node.index,
                length: element_types.len(),
            })?
            .clone();
        Ok(CheckedExprNode::TupleAccess(CheckedTupleAccessNode {
            target: self.program.exprs.alloc_item(checked_expr),
            index: tuple_access_node.index,
            type_id: self.substitute_all(field_type, ctx)?,
            location: tuple_access_node.location,
        }))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_intrinsic_expr(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let intrinsic_node = ctx.expression(node).as_intrinsic().cloned().unwrap();
        match intrinsic_node {
            IntrinsicExprNode::GetUserId { location } => {
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetUserId {
                    type_id: FELT_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetContractId { location } => {
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetContractId {
                    type_id: FELT_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetContractDeployer { contract_id, location } => {
                let contract_id = self.visit_expr(contract_id, ctx)?;
                if !self.unify(contract_id.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: contract_id.location(),
                        expected: vec![FELT_TYPE],
                        found: contract_id.ty(),
                    });
                }
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetContractDeployer {
                    contract_id: self.program.exprs.alloc_item(contract_id),
                    type_id: HASH_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetCallerContractId { location } => {
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetCallerContractId {
                    type_id: FELT_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetCheckpointId { location } => {
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetCheckpointId {
                    type_id: FELT_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetLastNonce { location } => {
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetLastNonce {
                    type_id: FELT_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetUserPublicKeyHash { location } => {
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetUserPublicKeyHash {
                    type_id: HASH_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetStateHashAt { slot_index, location } => {
                let slot_index = self.visit_expr(slot_index, ctx)?;
                if !self.unify(slot_index.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: slot_index.location(),
                        expected: vec![FELT_TYPE],
                        found: slot_index.ty(),
                    });
                }
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetStateHashAt {
                    slot_index: self.program.exprs.alloc_item(slot_index),
                    type_id: HASH_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetOtherContractStateHashAt {
                contract_state_tree_height,
                contract_id,
                slot_index,
                location,
            } => {
                let contract_state_tree_height = self.visit_expr(contract_state_tree_height, ctx)?;
                let contract_id = self.visit_expr(contract_id, ctx)?;
                let slot_index = self.visit_expr(slot_index, ctx)?;
                if !self.unify(contract_state_tree_height.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: contract_state_tree_height.location(),
                        expected: vec![FELT_TYPE],
                        found: contract_state_tree_height.ty(),
                    });
                }
                if !self.unify(contract_id.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: contract_id.location(),
                        expected: vec![FELT_TYPE],
                        found: contract_id.ty(),
                    });
                }
                if !self.unify(slot_index.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: slot_index.location(),
                        expected: vec![FELT_TYPE],
                        found: slot_index.ty(),
                    });
                }

                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetOtherContractStateHashAt {
                    contract_state_tree_height: self.program.exprs.alloc_item(contract_state_tree_height),
                    contract_id: self.program.exprs.alloc_item(contract_id),
                    slot_index: self.program.exprs.alloc_item(slot_index),
                    type_id: HASH_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetOtherUserContractStateHashAt {
                contract_state_tree_height,
                user_id,
                contract_id,
                slot_index,
                location,
            } => {
                let contract_state_tree_height = self.visit_expr(contract_state_tree_height, ctx)?;
                let user_id = self.visit_expr(user_id, ctx)?;
                let contract_id = self.visit_expr(contract_id, ctx)?;
                let slot_index = self.visit_expr(slot_index, ctx)?;
                if !self.unify(contract_state_tree_height.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: contract_state_tree_height.location(),
                        expected: vec![FELT_TYPE],
                        found: contract_state_tree_height.ty(),
                    });
                }
                if !self.unify(user_id.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: user_id.location(),
                        expected: vec![FELT_TYPE],
                        found: user_id.ty(),
                    });
                }
                if !self.unify(contract_id.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: contract_id.location(),
                        expected: vec![FELT_TYPE],
                        found: contract_id.ty(),
                    });
                }
                if !self.unify(slot_index.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: slot_index.location(),
                        expected: vec![FELT_TYPE],
                        found: slot_index.ty(),
                    });
                }

                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetOtherUserContractStateHashAt {
                    contract_state_tree_height: self.program.exprs.alloc_item(contract_state_tree_height),
                    user_id: self.program.exprs.alloc_item(user_id),
                    contract_id: self.program.exprs.alloc_item(contract_id),
                    slot_index: self.program.exprs.alloc_item(slot_index),
                    type_id: HASH_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::CSetStateHashAt {
                slot_index,
                new_value,
                location,
            } => {
                let slot_index = self.visit_expr(slot_index, ctx)?;
                let new_value = self.visit_expr(new_value, ctx)?;

                if !self.unify(slot_index.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: slot_index.location(),
                        expected: vec![FELT_TYPE],
                        found: slot_index.ty(),
                    });
                }
                if !self.unify(new_value.ty(), HASH_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: new_value.location(),
                        expected: vec![HASH_TYPE],
                        found: new_value.ty(),
                    });
                }

                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::CSetStateHashAt {
                    slot_index: self.program.exprs.alloc_item(slot_index),
                    new_value: self.program.exprs.alloc_item(new_value),
                    type_id: HASH_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::MemTransmute { data, target_type, location } => {
                let data = self.visit_expr(data, ctx)?;
                let target_type = self.typecheck(&target_type, ctx)?;

                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::MemTransmute {
                    data: self.program.exprs.alloc_item(data),
                    target_type: self.substitute_all(target_type, ctx)?,
                    location,
                }));
            }
            IntrinsicExprNode::MemSizeOf { query_type: ty, location } => {
                let ty = self.typecheck(&ty, ctx)?;

                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::MemSizeOf {
                    query_type: ty,
                    type_id: FELT_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::StorageRead {
                contract_state_tree_height,
                user_id,
                contract_id,
                offset,
                location,
            } => {
                let contract_state_tree_height = self.visit_expr(contract_state_tree_height, ctx)?;
                let user_id = self.visit_expr(user_id, ctx)?;
                let contract_id = self.visit_expr(contract_id, ctx)?;
                let offset = self.visit_expr(offset, ctx)?;
                if !self.unify(contract_state_tree_height.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: offset.location(),
                        expected: vec![FELT_TYPE],
                        found: contract_state_tree_height.ty(),
                    });
                }
                if !self.unify(user_id.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: offset.location(),
                        expected: vec![FELT_TYPE],
                        found: user_id.ty(),
                    });
                }
                if !self.unify(contract_id.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: offset.location(),
                        expected: vec![FELT_TYPE],
                        found: contract_id.ty(),
                    });
                }
                if !self.unify(offset.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: offset.location(),
                        expected: vec![FELT_TYPE],
                        found: offset.ty(),
                    });
                }
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::StorageRead {
                    contract_state_tree_height: self.program.exprs.alloc_item(contract_state_tree_height),
                    user_id: self.program.exprs.alloc_item(user_id),
                    contract_id: self.program.exprs.alloc_item(contract_id),
                    offset: self.program.exprs.alloc_item(offset),
                    type_id: FELT_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::StorageReadRange {
                contract_state_tree_height,
                user_id,
                contract_id,
                offset,
                length,
                location,
            } => {
                let contract_state_tree_height = self.visit_expr(contract_state_tree_height, ctx)?;
                let user_id = self.visit_expr(user_id, ctx)?;
                let contract_id = self.visit_expr(contract_id, ctx)?;
                let offset = self.visit_expr(offset, ctx)?;
                let length = self.visit_expr(length, ctx)?;
                if !self.unify(contract_state_tree_height.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: offset.location(),
                        expected: vec![FELT_TYPE],
                        found: contract_state_tree_height.ty(),
                    });
                }
                if !self.unify(user_id.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: offset.location(),
                        expected: vec![FELT_TYPE],
                        found: user_id.ty(),
                    });
                }
                if !self.unify(contract_id.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: offset.location(),
                        expected: vec![FELT_TYPE],
                        found: contract_id.ty(),
                    });
                }
                if !self.unify(offset.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: offset.location(),
                        expected: vec![FELT_TYPE],
                        found: offset.ty(),
                    });
                }
                if !self.unify(length.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: length.location(),
                        expected: vec![FELT_TYPE],
                        found: length.ty(),
                    });
                }
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::StorageReadRange {
                    contract_state_tree_height: self.program.exprs.alloc_item(contract_state_tree_height),
                    user_id: self.program.exprs.alloc_item(user_id),
                    contract_id: self.program.exprs.alloc_item(contract_id),
                    offset: self.program.exprs.alloc_item(offset),
                    length: self.program.exprs.alloc_item(length),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::StorageWrite { offset, value, location } => {
                let offset = self.visit_expr(offset, ctx)?;
                let value = self.visit_expr(value, ctx)?;
                if !self.unify(offset.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: offset.location(),
                        expected: vec![FELT_TYPE],
                        found: offset.ty(),
                    });
                }
                if !self.unify(value.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: value.location(),
                        expected: vec![FELT_TYPE],
                        found: value.ty(),
                    });
                }
                Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::StorageWrite {
                    offset: self.program.exprs.alloc_item(offset),
                    value: self.program.exprs.alloc_item(value),
                    type_id: FELT_TYPE,
                    location,
                }))
            }
            IntrinsicExprNode::StorageWriteRange { offset, values, location } => {
                let offset = self.visit_expr(offset, ctx)?;
                let values = self.visit_expr(values, ctx)?;
                if !self.unify(offset.ty(), FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: offset.location(),
                        expected: vec![FELT_TYPE],
                        found: offset.ty(),
                    });
                }
                Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::StorageWriteRange {
                    offset: self.program.exprs.alloc_item(offset),
                    values: self.program.exprs.alloc_item(values),
                    type_id: VOID_TYPE,
                    location,
                }))
            }
            IntrinsicExprNode::Hash { data, location } => {
                let data = self.visit_expr(data, ctx)?;

                Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::Hash {
                    data: self.program.exprs.alloc_item(data),
                    type_id: HASH_TYPE,
                    location,
                }))
            }
            IntrinsicExprNode::HashTwoToOne { left, right, location } => {
                let left = self.visit_expr(left, ctx)?;
                let right = self.visit_expr(right, ctx)?;

                Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::HashTwoToOne {
                    left: self.program.exprs.alloc_item(left),
                    right: self.program.exprs.alloc_item(right),
                    type_id: HASH_TYPE,
                    location,
                }))
            }
            IntrinsicExprNode::InvokeSync {
                contract_id,
                method_id,
                inputs,
                return_type,
                location,
            } => {
                let contract_id = self.visit_expr(contract_id, ctx)?;
                let method_id = self.visit_expr(method_id, ctx)?;
                let inputs = self.visit_expr(inputs, ctx)?;
                let return_type = self.typecheck(&return_type, ctx)?;

                Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::InvokeSync {
                    contract_id: self.program.exprs.alloc_item(contract_id),
                    method_id: self.program.exprs.alloc_item(method_id),
                    inputs: self.program.exprs.alloc_item(inputs),
                    type_id: return_type,
                    location,
                }))
            }
            IntrinsicExprNode::InvokeDeferred {
                contract_id,
                method_id,
                inputs,
                location,
            } => {
                let contract_id = self.visit_expr(contract_id, ctx)?;
                let method_id = self.visit_expr(method_id, ctx)?;
                let inputs = self.visit_expr(inputs, ctx)?;

                Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::InvokeDeferred {
                    contract_id: self.program.exprs.alloc_item(contract_id),
                    method_id: self.program.exprs.alloc_item(method_id),
                    inputs: self.program.exprs.alloc_item(inputs),
                    type_id: VOID_TYPE,
                    location,
                }))
            }
            IntrinsicExprNode::Secp256k1Verify { pub_key, msg, sig, location } => {
                let pub_key = self.visit_expr(pub_key, ctx)?;

                let msg = self.visit_expr(msg, ctx)?;
                let sig = self.visit_expr(sig, ctx)?;

                Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::Secp256k1Verify {
                    pub_key: self.program.exprs.alloc_item(pub_key),
                    msg: self.program.exprs.alloc_item(msg),
                    sig: self.program.exprs.alloc_item(sig),
                    type_id: BOOL_TYPE,
                    location,
                }))
            }
            IntrinsicExprNode::GetCheckpointStats { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetCheckpointStats {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetRegisterUsersRoot { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetRegisterUsersRoot {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetGutasRoot { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetGutasRoot {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetDeployContractsRoot { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetDeployContractsRoot {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetFeesCollected { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetFeesCollected {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetUserOpsProcessed { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetUserOpsProcessed {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetTotalTransactions { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetTotalTransactions {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetSlotsModified { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetSlotsModified {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetRegisterUsersCompleted { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetRegisterUsersCompleted {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetGutasCompleted { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetGutasCompleted {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::GetDeployContractsCompleted { checkpoint_id, location } => {
                let checkpoint_id = self.visit_expr(checkpoint_id, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::GetDeployContractsCompleted {
                    checkpoint_id: self.program.exprs.alloc_item(checkpoint_id),
                    type_id: UNKOWN_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::SumBits { bits, location } => {
                let bits = self.visit_expr(bits, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::SumBits {
                    bits: self.program.exprs.alloc_item(bits),
                    type_id: FELT_TYPE,
                    location,
                }));
            }
            IntrinsicExprNode::SplitBits { target, num_bits, location } => {
                let target = self.visit_expr(target, ctx)?;
                return Ok(CheckedExprNode::Intrinsic(CheckedIntrinsicExprNode::SplitBits {
                    target: self.program.exprs.alloc_item(target),
                    num_bits,
                    type_id: ARRAY_TYPE,
                    location,
                }));
            }
        }
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_value(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let value_node = ctx.expression(node).as_value().cloned().unwrap();
        match value_node {
            ValueNode::Felt(f, location) => Ok(CheckedExprNode::Value(CheckedValueNode::Felt(f.clone(), location))),
            ValueNode::Bool(b, location) => Ok(CheckedExprNode::Value(CheckedValueNode::Bool(b.clone(), location))),
            ValueNode::U32(u, location) => Ok(CheckedExprNode::Value(CheckedValueNode::U32(u.clone(), location))),
            ValueNode::Array(size, arr, location) => {
                let mut inner_ty = UNKOWN_TYPE;
                let mut elements = Vec::with_capacity(arr.len());
                for e in arr {
                    let checked_expr = self.visit_expr(e, ctx)?;
                    if !self.unify(checked_expr.ty(), inner_ty, ctx) {
                        return Err(Error::TypeMismatch {
                            location: checked_expr.location(),
                            expected: vec![inner_ty],
                            found: checked_expr.ty(),
                        });
                    }
                    inner_ty = checked_expr.ty();
                    elements.push(self.program.exprs.alloc_item(checked_expr));
                }

                let underlying_type_id = ctx.symbols.get_type_id(Some(ScopeId::primitive()), IdentId::TYPE_ARRAY).unwrap();

                let size_ty = self.populate_constant(size.into(), ctx)?;

                let &CheckedArrayNode {
                    inner_ty: generic_inner_ty,
                    size_ty: generic_size_ty,
                    ..
                } = ctx.symbols[underlying_type_id].as_array().unwrap();

                if !self.unify(generic_inner_ty, inner_ty, ctx) {
                    return Err(Error::TypeMismatch {
                        location: location,
                        expected: vec![generic_inner_ty],
                        found: inner_ty,
                    });
                }
                if !self.unify(generic_size_ty, size_ty, ctx) {
                    return Err(Error::TypeMismatch {
                        location: location,
                        expected: vec![generic_size_ty],
                        found: size_ty,
                    });
                }

                let type_id = self.substitute_all(underlying_type_id, ctx)?;

                Ok(CheckedExprNode::Value(CheckedValueNode::Array(type_id, elements, location)))
            }
            ValueNode::Struct(path, generic_args, data, location) => Ok({
                let checked_path_node = self.visit_expr(path, ctx)?;
                let underlying_type_id = self.poly_of(checked_path_node.ty(), ctx).unwrap();
                let fields = ctx.symbols[underlying_type_id].as_struct().unwrap().fields.clone();
                let generic_parameters = ctx.symbols[underlying_type_id].generic_parameters();
                if fields.len() != data.len() {
                    return Err(anyhow!(format!(
                        "Expected {} fields for Struct {} but found {} fields",
                        fields.len(),
                        ctx.ident(ctx.symbols[underlying_type_id].name()),
                        data.len()
                    ))
                    .into());
                }

                let mut new_data = IndexMap::new();
                for (field_name, CheckedStructField { ty: field_type, .. }) in fields {
                    let field_value = self.visit_expr(data.get(&field_name).unwrap().clone(), ctx)?;
                    if !self.unify(field_type, field_value.ty(), ctx) {
                        return Err(Error::TypeMismatch {
                            location: field_value.location(),
                            expected: vec![field_type],
                            found: field_value.ty(),
                        });
                    }
                    new_data.insert(field_name, self.program.exprs.alloc_item(field_value));
                }

                for (generic_arg, generic_param) in generic_args.clone().iter().zip(generic_parameters.clone().into_iter()) {
                    let generic_arg = self.typecheck(generic_arg, ctx)?;
                    if !self.unify(generic_param, generic_arg, ctx) {
                        return Err(Error::TypeMismatch {
                            location: location,
                            expected: vec![generic_arg],
                            found: generic_param,
                        });
                    }
                }

                let type_id = self.substitute_all(underlying_type_id, ctx)?;
                ctx.add_type_reference(underlying_type_id, checked_path_node.location(), false);

                CheckedExprNode::Value(CheckedValueNode::Struct(type_id, new_data, location))
            }),
        }
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_binary(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let binary_node = ctx.expression(node).as_binary().cloned().unwrap();
        let checked_lhs = self.visit_expr(binary_node.lhs, ctx)?;
        let checked_rhs = self.visit_expr(binary_node.rhs, ctx)?;

        let lhs_ty = checked_lhs.ty();
        if !self.unify(lhs_ty, checked_rhs.ty(), ctx) {
            return Err(Error::TypeMismatch {
                location: checked_rhs.location(),
                expected: vec![lhs_ty],
                found: checked_rhs.ty(),
            });
        }

        let type_id = match binary_node.operator {
            BinaryOperator::Add
            | BinaryOperator::Sub
            | BinaryOperator::Mul
            | BinaryOperator::Div
            | BinaryOperator::Pow
            | BinaryOperator::Mod
            | BinaryOperator::BitShr
            | BinaryOperator::BitShl
            | BinaryOperator::BitAnd
            | BinaryOperator::BitOr => {
                if self.unify(lhs_ty, FELT_TYPE, ctx) {
                    FELT_TYPE
                } else if self.unify(lhs_ty, U32_TYPE, ctx) {
                    U32_TYPE
                } else {
                    return Err(Error::TypeMismatch {
                        location: binary_node.location,
                        expected: vec![FELT_TYPE, U32_TYPE],
                        found: lhs_ty,
                    });
                }
            }
            BinaryOperator::BitXor => {
                if self.unify(lhs_ty, FELT_TYPE, ctx) {
                    FELT_TYPE
                } else if self.unify(lhs_ty, U32_TYPE, ctx) {
                    U32_TYPE
                } else if self.unify(lhs_ty, BOOL_TYPE, ctx) {
                    BOOL_TYPE
                } else {
                    return Err(Error::TypeMismatch {
                        location: binary_node.location,
                        expected: vec![FELT_TYPE, U32_TYPE, BOOL_TYPE],
                        found: lhs_ty,
                    });
                }
            }
            BinaryOperator::And | BinaryOperator::Or => {
                if !self.unify(lhs_ty, BOOL_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: binary_node.location,
                        expected: vec![BOOL_TYPE],
                        found: lhs_ty,
                    });
                }
                BOOL_TYPE
            }
            BinaryOperator::Eq | BinaryOperator::Neq => {
                if !self.unify(lhs_ty, BOOL_TYPE, ctx) && !self.unify(lhs_ty, FELT_TYPE, ctx) && !self.unify(lhs_ty, U32_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: binary_node.location,
                        expected: vec![BOOL_TYPE, FELT_TYPE, U32_TYPE],
                        found: lhs_ty,
                    });
                }
                BOOL_TYPE
            }
            BinaryOperator::Lt | BinaryOperator::Lte | BinaryOperator::Gt | BinaryOperator::Gte => {
                if !self.unify(lhs_ty, FELT_TYPE, ctx) && !self.unify(lhs_ty, U32_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: binary_node.location,
                        expected: vec![FELT_TYPE, U32_TYPE],
                        found: lhs_ty,
                    });
                }
                BOOL_TYPE
            }
        };

        Ok(CheckedExprNode::Binary(CheckedBinaryNode {
            lhs: self.program.exprs.alloc_item(checked_lhs),
            operator: binary_node.operator,
            rhs: self.program.exprs.alloc_item(checked_rhs),
            type_id: self.substitute_all(type_id, ctx)?,
            location: binary_node.location,
        }))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_unary(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let unary_node = ctx.expression(node).as_unary().cloned().unwrap();
        let checked_expr = self.visit_expr(unary_node.rhs, ctx)?;
        let type_id = checked_expr.ty();

        match unary_node.operator {
            UnaryOperator::Neg => {
                if !self.unify(type_id, FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: unary_node.location,
                        expected: vec![FELT_TYPE],
                        found: type_id,
                    });
                }
            }
            UnaryOperator::Not => {
                if !self.unify(type_id, BOOL_TYPE, ctx) && !self.unify(type_id, FELT_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: unary_node.location,
                        expected: vec![FELT_TYPE, BOOL_TYPE],
                        found: type_id,
                    });
                }
            }
        }
        if !self.unify(type_id, FELT_TYPE, ctx) && !self.unify(type_id, BOOL_TYPE, ctx) {
            return Err(Error::TypeMismatch {
                location: unary_node.location,
                expected: vec![FELT_TYPE, BOOL_TYPE],
                found: type_id,
            });
        }

        Ok(CheckedExprNode::Unary(CheckedUnaryNode {
            operator: unary_node.operator,
            rhs: self.program.exprs.alloc_item(checked_expr),
            type_id: self.substitute_all(type_id, ctx)?,
            location: unary_node.location,
        }))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_call(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let call_node = ctx.expression(node).as_call().cloned().unwrap();
        let callee = self.visit_expr(call_node.callee, ctx)?;
        let ty = callee.ty();
        let generic_parameters = ctx.symbols[ty].generic_parameters();
        for (generic_param, generic_arg) in generic_parameters.iter().zip(call_node.generic_parameters.iter()) {
            let generic_arg = self.typecheck(generic_arg, ctx)?;
            if !self.unify(generic_param.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: call_node.location,
                    expected: vec![generic_param.clone()],
                    found: generic_arg,
                });
            }
        }

        let signature = ctx.symbols[ty].signature();

        if call_node.args.len() != signature.parameters.len() {
            return Err(Error::InvalidFunctionArguments {
                location: call_node.location,
                method_name: ty,
                expected: format!("{} parameters", signature.parameters.len()),
                found: format!("{}", call_node.args.len()),
            });
        }
        let mut args = Vec::new();
        for (i, arg) in call_node.args.iter().enumerate() {
            let type_arg = self.visit_expr(arg.clone(), ctx)?;
            if !self.unify(signature.parameters[i], type_arg.ty(), ctx) {
                return Err(Error::TypeMismatch {
                    location: call_node.location,
                    expected: vec![signature.parameters[i]],
                    found: type_arg.ty(),
                });
            }
            args.push(type_arg);
        }

        let callee_expr_id = self.program.exprs.alloc_item(callee);
        let checked_expr = CheckedExprNode::Call(CheckedCallNode {
            callee: self.rewrite_expr(callee_expr_id, ctx)?,
            generic_parameters: generic_parameters
                .into_iter()
                .map(|generic_param| self.substitute_all(generic_param, ctx))
                .collect::<Result<Vec<TypeId>>>()?,
            args: self.program.exprs.alloc_items(args),
            type_id: self.substitute_all(signature.return_type, ctx)?,
            location: call_node.location,
        });

        return Ok(checked_expr);
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_member_call(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let call_node = ctx.expression(node).as_member_call().cloned().unwrap();
        let callee = self.visit_expr(call_node.callee, ctx)?;
        let ty = callee.ty();
        let generic_parameters = ctx.symbols[ty].generic_parameters();

        for (generic_param, generic_arg) in generic_parameters.iter().zip(call_node.generic_parameters.iter()) {
            let generic_arg = self.typecheck(generic_arg, ctx)?;
            if !self.unify(generic_param.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: call_node.location,
                    expected: vec![generic_param.clone()],
                    found: generic_arg,
                });
            }
        }

        let signature = ctx.symbols[ty].signature();

        let mut args = Vec::new();
        let receiver = {
            let receiver = self.visit_expr(call_node.receiver, ctx)?;
            if !self.unify(signature.parameters[0], receiver.ty(), ctx) {
                return Err(Error::TypeMismatch {
                    location: call_node.location,
                    expected: vec![signature.parameters[0]],
                    found: receiver.ty(),
                });
            }

            self.program.exprs.alloc_item(receiver)
        };

        for (i, arg) in call_node.args.iter().enumerate() {
            let type_arg = self.visit_expr(arg.clone(), ctx)?;
            if !self.unify(signature.parameters[i + 1], type_arg.ty(), ctx) {
                return Err(Error::TypeMismatch {
                    location: call_node.location,
                    expected: vec![signature.parameters[i + 1]],
                    found: type_arg.ty(),
                });
            }
            args.push(type_arg);
        }

        let checked_expr = CheckedExprNode::MemberCall(CheckedMemberCallNode {
            callee: self.program.exprs.alloc_item(callee),
            receiver,
            generic_parameters: generic_parameters
                .into_iter()
                .map(|generic_param| self.substitute_all(generic_param, ctx))
                .collect::<Result<Vec<TypeId>>>()?,
            args: self.program.exprs.alloc_items(args),
            type_id: self.substitute_all(signature.return_type, ctx)?,
            location: call_node.location,
        });

        return Ok(checked_expr);
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_tuple(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        let tuple_node = ctx.expression(node).as_tuple().cloned().unwrap();

        let checked_elements: Result<Vec<CheckedExprNode<F>>> = tuple_node.elements.iter().map(|expr_id| self.visit_expr(*expr_id, ctx)).collect();

        let checked_elements = checked_elements?;
        let element_types: Vec<TypeId> = checked_elements.iter().map(|e| e.ty()).collect();

        let tuple_type = Type::Tuple(element_types.clone());
        let scope_id = ScopeId::primitive();
        let type_id = ctx.symbols.get_or_add_type(Some(scope_id), tuple_type.key(), tuple_type)?;

        let elements_with_types = checked_elements.into_iter().map(|e| (e.ty(), self.program.exprs.alloc_item(e))).collect();

        let checked_expr = CheckedExprNode::Value(CheckedValueNode::Tuple(type_id, elements_with_types, tuple_node.location));

        Ok(checked_expr)
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_cast(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let cast_node = ctx.expression(node).as_cast().cloned().unwrap();
        let src_expr = self.visit_expr(cast_node.value, ctx)?;
        let src_type = src_expr.ty();
        let target_type = self.typecheck(&cast_node.target_type, ctx)?;

        if (self.unify(src_type, FELT_TYPE, ctx) || self.unify(src_type, BOOL_TYPE, ctx) || self.unify(src_type, U32_TYPE, ctx))
            && (self.unify(target_type, FELT_TYPE, ctx) || self.unify(target_type, BOOL_TYPE, ctx) || self.unify(target_type, U32_TYPE, ctx))
        {
            return Ok(CheckedExprNode::Cast(CheckedCastNode {
                value: self.program.exprs.alloc_item(src_expr),
                target_type,
                location: cast_node.location,
            }));
        } else {
            return Err(Error::TypeMismatch {
                location: cast_node.location,
                expected: vec![target_type],
                found: src_type,
            });
        };
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_if_expr(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let if_expr_node = ctx.expression(node).as_if_expr().cloned().unwrap();
        let checked_expr = self.visit_expr(if_expr_node.if_branch.predicate, ctx)?;
        if !self.unify(checked_expr.ty(), BOOL_TYPE, ctx) {
            return Err(Error::TypeMismatch {
                location: if_expr_node.location,
                expected: vec![BOOL_TYPE],
                found: checked_expr.ty(),
            });
        }

        let checked_block = self.visit_expr(if_expr_node.if_branch.body, ctx)?;
        let if_type = checked_block.as_block_expr().unwrap().type_id;
        let if_branch = CheckedCase {
            predicate: self.program.exprs.alloc_item(checked_expr),
            type_id: BOOL_TYPE,
            body: self.program.exprs.alloc_item(checked_block),
        };

        let mut elseif_branches = Vec::with_capacity(if_expr_node.elseif_branches.len());
        for branch in &if_expr_node.elseif_branches {
            let checked_expr = self.visit_expr(branch.predicate, ctx)?;
            if !self.unify(checked_expr.ty(), BOOL_TYPE, ctx) {
                return Err(Error::TypeMismatch {
                    location: branch.location,
                    expected: vec![BOOL_TYPE],
                    found: checked_expr.ty(),
                });
            }
            let checked_block = self.visit_expr(branch.body, ctx)?;
            let else_if_type = checked_block.as_block_expr().unwrap().type_id;

            if !self.unify(else_if_type, if_type, ctx) {
                return Err(Error::TypeMismatch {
                    location: branch.location,
                    expected: vec![else_if_type],
                    found: if_type,
                });
            }

            elseif_branches.push(CheckedCase {
                predicate: self.program.exprs.alloc_item(checked_expr).clone(),
                type_id: BOOL_TYPE,
                body: self.program.exprs.alloc_item(checked_block),
            });
        }

        let else_branch = if let Some(else_branch) = if_expr_node.else_branch {
            let checked_block = self.visit_expr(else_branch, ctx)?;
            let else_type = checked_block.as_block_expr().unwrap().type_id;

            if !self.unify(else_type, if_type, ctx) {
                return Err(Error::TypeMismatch {
                    location: if_expr_node.location,
                    expected: vec![if_type],
                    found: else_type,
                });
            }

            Some(self.program.exprs.alloc_item(checked_block))
        } else {
            None
        };

        Ok(CheckedExprNode::IfExpr(CheckedIfExprNode {
            if_branch,
            elseif_branches,
            else_branch,
            type_id: self.substitute_all(if_type, ctx)?,
            location: if_expr_node.location,
        }))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_while(&mut self, node: StmtId, ctx: &mut Self::Context) -> StdResult<Self::StmtResult, Self::Error> {
        // TODO: remove clone
        let while_node = ctx.statement(node).as_while().cloned().unwrap();
        let predicate = self.visit_expr(while_node.predicate, ctx)?;
        if !self.unify(predicate.ty(), BOOL_TYPE, ctx) {
            return Err(Error::TypeMismatch {
                location: while_node.location,
                expected: vec![BOOL_TYPE],
                found: predicate.ty(),
            });
        }
        let checked_block = self.visit_expr(while_node.body, ctx)?;
        Ok(CheckedStmtNode::While(CheckedWhileNode {
            predicate: self.program.exprs.alloc_item(predicate),
            type_id: BOOL_TYPE,
            body: self.program.exprs.alloc_item(checked_block),
            comments: while_node.comments,
            location: while_node.location,
        }))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_assignment(&mut self, node: StmtId, ctx: &mut Self::Context) -> StdResult<Self::StmtResult, Self::Error> {
        // TODO: remove clone
        let assignment_node = ctx.statement(node).as_assignment().cloned().unwrap();
        let checked_rhs = self.visit_expr(assignment_node.value, ctx)?;
        let checked_lhs = self.visit_expr(assignment_node.target, ctx)?;

        let lhs_ty = checked_lhs.ty();

        if !self.unify(lhs_ty, checked_rhs.ty(), ctx) {
            return Err(Error::TypeMismatch {
                location: assignment_node.location,
                expected: vec![lhs_ty],
                found: checked_rhs.ty(),
            });
        }

        Ok(CheckedStmtNode::Assignment(CheckedAssignmentNode {
            target: self.program.exprs.alloc_item(checked_lhs),
            operator: assignment_node.operator,
            value: self.program.exprs.alloc_item(checked_rhs),
            type_id: self.substitute_all(lhs_ty, ctx)?,
            comments: assignment_node.comments,
            location: assignment_node.location,
        }))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_variable(&mut self, node: StmtId, ctx: &mut Self::Context) -> StdResult<Self::StmtResult, Self::Error> {
        // TODO: remove clone
        let variable_node = ctx.statement(node).as_variable().cloned().unwrap();
        let lhs_ty = self.typecheck(&variable_node.ty, ctx)?;
        let checked_expr = self.visit_expr(variable_node.value, ctx)?;
        let rhs_ty = checked_expr.ty();
        if !self.unify(rhs_ty, lhs_ty, ctx) {
            return Err(Error::TypeMismatch {
                location: variable_node.location,
                expected: vec![lhs_ty],
                found: rhs_ty,
            });
        }
        let current_scope_id = ctx.symbols.current_scope_id().unwrap();
        let var_id = ctx
            .symbols
            .declare_variable(CheckedVariable::new(
                variable_node.name,
                rhs_ty,
                variable_node.qualifier,
                current_scope_id,
                variable_node.location,
            ))
            .ok_or(error::Error::VariableAlreadyDefined {
                location: variable_node.location,
                variable: variable_node.name.id,
            })?;
        ctx.add_variable_reference(var_id, variable_node.name.location, false);

        let checked_variable = CheckedVariableNode {
            name: variable_node.name,
            ty: self.substitute_all(rhs_ty, ctx)?,
            qualifier: variable_node.qualifier,
            value: self.program.exprs.alloc_item(checked_expr),
            scope_id: current_scope_id,
            comments: variable_node.comments,
            location: variable_node.location,
        };
        Ok(CheckedStmtNode::Variable(checked_variable))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_return(&mut self, node: StmtId, ctx: &mut Self::Context) -> StdResult<Self::StmtResult, Self::Error> {
        // TODO: remove clone
        let return_node = ctx.statement(node).as_return().cloned().unwrap();
        if !ctx.ancestor_node_type(1).is_block_expr() || !ctx.ancestor_node_type(2).is_function() {
            return Err(Error::InvalidReturn {
                location: return_node.location,
                message: format!("Cannot return from {:?} node", ctx.ancestor_node_type(2)),
            });
        }

        let ret = if let Some(expr) = return_node.expr_id {
            let expr = self.visit_expr(expr, ctx)?;
            Some(self.program.exprs.alloc_item(expr))
        } else {
            None
        };

        Ok(CheckedStmtNode::Return(CheckedReturnNode {
            ret,
            comments: return_node.comments,
            location: return_node.location,
        }))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_intrinsic_stmt(&mut self, node: StmtId, ctx: &mut Self::Context) -> StdResult<Self::StmtResult, Self::Error> {
        let node = ctx.statement(node).as_intrinsic().cloned().unwrap();
        match node {
            IntrinsicStmtNode::Assert {
                left,
                message,
                comments,
                location,
            } => {
                let checked_lhs = self.visit_expr(left, ctx)?;

                if !self.unify(checked_lhs.ty(), BOOL_TYPE, ctx) {
                    return Err(Error::TypeMismatch {
                        location: location,
                        expected: vec![BOOL_TYPE],
                        found: checked_lhs.ty(),
                    });
                }

                Ok(CheckedStmtNode::Intrinsic(CheckedIntrinsicStmtNode::Assert {
                    left: self.program.exprs.alloc_item(checked_lhs),
                    message: message,
                    comments: comments,
                    location: location,
                }))
            }
            IntrinsicStmtNode::AssertEq {
                left,
                right,
                message,
                comments,
                location,
            } => {
                let checked_lhs = self.visit_expr(left, ctx)?;
                let checked_rhs = self.visit_expr(right, ctx)?;

                if !self.unify(checked_lhs.ty(), checked_rhs.ty(), ctx) {
                    return Err(Error::TypeMismatch {
                        location: location,
                        expected: vec![checked_lhs.ty()],
                        found: checked_rhs.ty(),
                    });
                }

                Ok(CheckedStmtNode::Intrinsic(CheckedIntrinsicStmtNode::AssertEq {
                    left: self.program.exprs.alloc_item(checked_lhs),
                    right: self.program.exprs.alloc_item(checked_rhs),
                    message: message,
                    comments: comments,
                    location: location,
                }))
            }
            IntrinsicStmtNode::ClearEntireTree { comments, location } => Ok(CheckedStmtNode::Intrinsic(CheckedIntrinsicStmtNode::ClearEntireTree {
                comments: comments,
                location: location,
            })),
        }
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_impl(&mut self, node: DefId, ctx: &mut Self::Context) -> StdResult<Self::DefinitionResult, Self::Error> {
        // TODO: remove clone
        let impl_node = ctx.definition(node).as_impl().cloned().unwrap();

        ctx.symbols.start_scope(ScopeKind::Impl);
        self.infcx.enter_context();

        let mut checked_generic_parameters = Vec::new();
        for generic_parameter in &impl_node.generic_parameters {
            let checked_generic_parameter = self.typecheck_generic_parameter(generic_parameter, ctx)?;
            let type_id = ctx.symbols.add_type_variable(ScopeKind::Impl, checked_generic_parameter)?;
            checked_generic_parameters.push(type_id);
        }

        let implementor_type_id = self.typecheck(&impl_node.ty, ctx)?;
        let implementor_poly_type_id = self.poly_of(implementor_type_id, ctx).unwrap();
        if ctx.symbols[implementor_type_id]
            .generic_parameters()
            .iter()
            .any(|&p| !ctx.symbols[p].is_type_variable())
        {
            return Err(Error::SpecializationNotAllowed {
                location: impl_node.location,
            });
        }

        ctx.symbols.add_type_id(None, IdentId::TYPE_SELF, implementor_type_id)?;

        let mut methods = Vec::new();

        for (generic_parameter, generic_arg) in ctx.symbols[implementor_poly_type_id]
            .generic_parameters()
            .iter()
            .zip_eq(ctx.symbols[implementor_type_id].generic_parameters())
        {
            if !self.unify(generic_parameter.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: impl_node.location,
                    expected: vec![generic_parameter.clone()],
                    found: generic_arg,
                });
            }
        }

        for &function_id in &impl_node.body {
            ctx.push_node_id(NodeId::from(function_id));
            self.infcx.enter_scope();

            let checked_def_id = self.typecheck_function_predecl(function_id, ScopeKind::ImplMethod, ScopeKind::Impl, ctx)?;

            self.infcx.exit_scope();
            ctx.pop_node_id();

            methods.push(checked_def_id);
        }

        let mut associated_types = IndexMap::new();
        for (name, associated_ty) in &impl_node.associated_types {
            let (root, target, type_id) = if let UncheckedType::Path(path) = &associated_ty.ty {
                let checked_path = self.resolve_path(path, ctx)?;
                (checked_path.root, checked_path.target, checked_path.type_id)
            } else {
                (None, None, self.typecheck(&associated_ty.ty, ctx)?)
            };

            associated_types.insert(
                name.clone(),
                CheckedAssociatedTypeValue {
                    root,
                    target,
                    type_id,
                    visibility: associated_ty.visibility,
                    comments: associated_ty.comments.clone(),
                    location: associated_ty.location,
                },
            );
        }

        let checked_impl = CheckedImplNode {
            associated_types,
            generic_parameters: checked_generic_parameters,
            ty: implementor_type_id,
            body: methods,
            scope_id: ctx.symbols.current_scope_id().unwrap(),
            comments: impl_node.comments,
            location: impl_node.location,
        };

        self.infcx.exit_context();
        ctx.symbols.end_scope();

        let impl_id = self.program.defs.alloc_item(CheckedDefinitionNode::Impl(checked_impl));
        self.register_impl(impl_id, ctx)?;

        self.unchecked_checked.insert(node.into(), impl_id.into());

        for &function_id in &impl_node.body {
            ctx.push_node_id(NodeId::from(function_id));
            self.infcx.enter_scope();

            self.typecheck_function_signature(function_id, ScopeKind::ImplMethod, ScopeKind::Impl, ctx)?;

            self.infcx.exit_scope();
            ctx.pop_node_id();
        }

        Ok(impl_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_trait(&mut self, node: DefId, ctx: &mut Self::Context) -> StdResult<Self::DefinitionResult, Self::Error> {
        let trait_node = ctx.definition(node).as_trait().cloned().unwrap();

        let checked_def_id = self.typecheck_trait_predecl(node, ctx)?;
        let type_id = self.program[checked_def_id].as_trait().unwrap().type_id;

        ctx.symbols.enter_scope(ctx.symbols[type_id].scope_id());
        self.infcx.enter_context();

        let mut checked_generic_parameters = Vec::with_capacity(trait_node.generic_parameters.len());
        for generic_parameter in &trait_node.generic_parameters {
            let checked_generic_parameter = self.typecheck_generic_parameter(generic_parameter, ctx)?;
            let generic_type_id = ctx.symbols.add_type_variable(ScopeKind::Trait, checked_generic_parameter)?;
            checked_generic_parameters.push(generic_type_id);
        }

        let mut associated_types = IndexMap::new();
        for (name, associated_ty) in &trait_node.associated_types {
            let checked_generic_parameter = self.typecheck_generic_parameter(
                &GenericParameter::new(name.id, associated_ty.constraints.clone(), associated_ty.location),
                ctx,
            )?;
            let type_id = ctx.symbols.add_type_variable(ScopeKind::Trait, checked_generic_parameter.clone())?;
            associated_types.insert(
                name.clone(),
                CheckedAssociatedType {
                    type_id,
                    constraints: checked_generic_parameter.constraints,
                    visibility: associated_ty.visibility,
                    comments: associated_ty.comments.clone(),
                    location: associated_ty.location,
                },
            );
        }

        ctx.symbols.modify_type(type_id, &mut |ty: &mut Type| {
            let ty_mut = ty.as_trait_mut().unwrap();
            ty_mut.generic_parameters = checked_generic_parameters.clone();
            ty_mut.associated_types = associated_types.clone();
            Ok(())
        })?;
        self.program.modify_definition(checked_def_id, |def: &mut CheckedDefinitionNode| {
            let def_mut = def.as_trait_mut().unwrap();
            def_mut.generic_parameters = checked_generic_parameters;
            def_mut.associated_types = associated_types;
            Ok(())
        })?;

        for &function_id in &trait_node.body {
            ctx.push_node_id(NodeId::from(function_id));
            self.infcx.enter_scope();

            self.typecheck_function_signature(function_id, ScopeKind::TraitMethod, ScopeKind::Trait, ctx)?;

            self.infcx.exit_scope();
            ctx.pop_node_id();
        }

        self.register_instance(type_id, type_id, ctx)?;

        self.infcx.exit_context();
        ctx.symbols.end_scope();

        Ok(checked_def_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_function(&mut self, node: DefId, ctx: &mut Self::Context) -> StdResult<Self::DefinitionResult, Self::Error> {
        self.infcx.enter_context();
        let checked_def_id = self.typecheck_function_predecl(node, ScopeKind::Function, ScopeKind::Function, ctx)?;
        self.typecheck_function_signature(node, ScopeKind::Function, ScopeKind::Function, ctx)?;
        self.typecheck_function_body(node, ctx)?;
        self.infcx.exit_context();
        Ok(checked_def_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_struct(&mut self, node: DefId, ctx: &mut Self::Context) -> StdResult<Self::DefinitionResult, Self::Error> {
        let struct_node = ctx.definition(node).as_struct().cloned().unwrap();

        let checked_def_id = self.typecheck_struct_predecl(node, ctx)?;
        let checked_struct_node = self.program[checked_def_id].as_struct().cloned().unwrap();
        let type_id = checked_struct_node.type_id;

        ctx.symbols.enter_scope(ctx.symbols[type_id].scope_id());
        self.infcx.enter_context();

        let mut checked_generic_parameters = Vec::with_capacity(struct_node.generic_parameters.len());
        for generic_parameter in struct_node.generic_parameters {
            let checked_generic_parameter = self.typecheck_generic_parameter(&generic_parameter, ctx)?;
            let generic_type_id = ctx.symbols.add_type_variable(ScopeKind::Struct, checked_generic_parameter)?;
            checked_generic_parameters.push(generic_type_id);
        }

        let mut fields = IndexMap::new();
        for (
            field_name,
            StructField {
                ty: field_type,
                attrs,
                visibility,
                comments,
                location,
            },
        ) in &struct_node.fields
        {
            let field_type = self.typecheck(&field_type, ctx)?;
            fields.insert(
                field_name.clone(),
                CheckedStructField::new(field_type, attrs.clone(), visibility.clone(), comments.clone(), location.clone()),
            );
        }

        ctx.symbols.modify_type(type_id, &mut |ty: &mut Type| {
            let ty_mut = ty.as_struct_mut().unwrap();
            ty_mut.generic_parameters = checked_generic_parameters.clone();
            ty_mut.fields = fields.clone();
            Ok(())
        })?;
        self.program.modify_definition(checked_def_id, |def: &mut CheckedDefinitionNode| {
            let def_mut = def.as_struct_mut().unwrap();
            def_mut.generic_parameters = checked_generic_parameters;
            def_mut.fields = fields;
            Ok(())
        })?;

        self.register_instance(type_id, type_id, ctx)?;

        self.infcx.exit_context();
        ctx.symbols.exit_scope();

        Ok(checked_def_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_enum(&mut self, node: DefId, ctx: &mut Self::Context) -> StdResult<Self::DefinitionResult, Self::Error> {
        ctx.symbols.start_scope(ScopeKind::Enum);
        self.infcx.enter_context();
        // TODO: remove clone
        let enum_node = ctx.definition(node).as_enum().cloned().unwrap();
        let current_scope_id = ctx.symbols.current_scope_id().unwrap();

        let mut generic_parameters = Vec::new();

        for generic_parameter in enum_node.generic_parameters {
            let checked_generic_parameter = self.typecheck_generic_parameter(&generic_parameter, ctx)?;
            let type_id = ctx.symbols.add_type_variable(ScopeKind::Enum, checked_generic_parameter)?;
            generic_parameters.push(type_id);
        }

        let checked_enum = CheckedEnumNode {
            generic_parameters,
            name: enum_node.name,
            variants: Vec::new(),
            scope_id: current_scope_id,
            visibility: enum_node.visibility,
            comments: enum_node.comments,
            location: enum_node.location,
        };
        let ty = Type::Enum(checked_enum.clone());
        let type_id = ctx.symbols.add_type(ctx.symbols.parent_scope_id(), checked_enum.name, ty)?;

        ctx.add_type_reference(type_id, enum_node.location, false);

        self.infcx.exit_context();
        ctx.symbols.end_scope();
        for variant in &enum_node.variants {
            match variant {
                EnumVariant::Basic(_name) => todo!(),
                EnumVariant::Tuple(_name, _members) => todo!(),
                EnumVariant::Struct(_name, _fields) => todo!(),
            }
        }
        todo!();
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_expr(&mut self, expr_id: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        ctx.push_node_id(NodeId::from(expr_id));
        self.infcx.enter_scope();
        let res = match ctx.expression(expr_id).node_type() {
            NodeType::PathExpr => self.visit_path(expr_id, ctx)?,
            NodeType::ValueExpr => self.visit_value(expr_id, ctx)?,
            NodeType::BinaryExpr => self.visit_binary(expr_id, ctx)?,
            NodeType::UnaryExpr => self.visit_unary(expr_id, ctx)?,
            NodeType::CallExpr => self.visit_call(expr_id, ctx)?,
            NodeType::MemberCallExpr => self.visit_member_call(expr_id, ctx)?,
            NodeType::CastExpr => self.visit_cast(expr_id, ctx)?,
            NodeType::IndexAccessExpr => self.visit_index_access(expr_id, ctx)?,
            NodeType::MemberAccessExpr => self.visit_member_access(expr_id, ctx)?,
            NodeType::IntrinsicExpr => self.visit_intrinsic_expr(expr_id, ctx)?,
            NodeType::LambdaFunctionExpr => self.visit_lambda_function(expr_id, ctx)?,
            NodeType::BlockExpr => self.visit_block_expr(expr_id, ctx)?,
            NodeType::IfExpr => self.visit_if_expr(expr_id, ctx)?,
            NodeType::TupleExpr => self.visit_tuple(expr_id, ctx)?,
            NodeType::TupleAccessExpr => self.visit_tuple_access(expr_id, ctx)?,
            NodeType::MatchExpr => self.visit_match(expr_id, ctx)?,
            NodeType::ParenthesesExpr => self.visit_parentheses(expr_id, ctx)?,
            _ => std::unreachable!(),
        };
        self.infcx.exit_scope();
        ctx.pop_node_id();
        Ok(res)
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_definition(&mut self, def_id: DefId, ctx: &mut Self::Context) -> StdResult<Self::DefinitionResult, Self::Error> {
        ctx.push_node_id(NodeId::from(def_id));
        let res = match ctx.definition(def_id).node_type() {
            NodeType::FunctionDef => self.visit_function(def_id, ctx)?,
            NodeType::StructDef => self.visit_struct(def_id, ctx)?,
            NodeType::EnumDef => self.visit_enum(def_id, ctx)?,
            NodeType::ImplDef => self.visit_impl(def_id, ctx)?,
            NodeType::TraitImplDef => self.visit_trait_impl(def_id, ctx)?,
            NodeType::TraitDef => self.visit_trait(def_id, ctx)?,
            NodeType::TypeAliasDef => self.visit_type_alias(def_id, ctx)?,
            NodeType::ConstDef => self.visit_const(def_id, ctx)?,
            NodeType::UseDef => self.visit_use(def_id, ctx)?,
            _ => std::unreachable!(),
        };
        ctx.pop_node_id();
        Ok(res)
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_stmt(&mut self, stmt_id: StmtId, ctx: &mut Self::Context) -> StdResult<Self::StmtResult, Self::Error> {
        ctx.push_node_id(NodeId::from(stmt_id));
        let res = match ctx.statement(stmt_id).node_type() {
            NodeType::WhileStmt => self.visit_while(stmt_id, ctx)?,
            NodeType::ForStmt => self.visit_for(stmt_id, ctx)?,
            NodeType::AssignmentStmt => self.visit_assignment(stmt_id, ctx)?,
            NodeType::VariableStmt => self.visit_variable(stmt_id, ctx)?,
            NodeType::ReturnStmt => self.visit_return(stmt_id, ctx)?,
            NodeType::DefinitionStmt => Self::StmtResult::from({
                let definition = self.visit_definition(ctx.statement(stmt_id).as_definition().unwrap().clone(), ctx)?;
                definition
            }),
            NodeType::ExpressionStmt => Self::StmtResult::from({
                let expr = self.visit_expr(ctx.statement(stmt_id).as_expression().unwrap().clone(), ctx)?;
                self.program.exprs.alloc_item(expr)
            }),
            NodeType::IntrinsicStmt => self.visit_intrinsic_stmt(stmt_id, ctx)?,
            _ => unreachable!(),
        };
        ctx.pop_node_id();
        Ok(res)
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_module(&mut self, _module_id: ModuleId, _ctx: &mut Self::Context) -> StdResult<(), Self::Error> {
        unreachable!()
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_program(&mut self, ctx: &mut Self::Context) -> StdResult<(), Self::Error> {
        // TODO: remove clone
        ctx.symbols.load_modules(ctx.program().modules.clone().iter());

        let mut std_primitive_processed = false;
        self.traverse_module_tree(ctx, &mut |type_checker, module_id, module, ctx| {
            if !std_primitive_processed {
                if module.is_std() && module.is_self_primitive() {
                    ctx.add_module_reference(module_id, module.name.location, false);
                    type_checker.typecheck_std_primitive_module(ctx)?;
                    std_primitive_processed = true;
                }
            }
            for &def_id in &module.definitions {
                type_checker.typecheck_definition_predecl(def_id, ctx)?;
            }
            Ok(())
        })?;
        self.traverse_module_tree(ctx, &mut |type_checker, _, module, ctx| {
            for &def_id in &module.definitions {
                type_checker.typecheck_definition_header(def_id, ctx)?;
            }
            Ok(())
        })?;

        self.traverse_module_tree(ctx, &mut |type_checker, _, module, ctx| {
            for &def_id in &module.definitions {
                type_checker.typecheck_definition_body(def_id, ctx)?;
            }
            Ok(())
        })?;

        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_block_expr(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        let BlockExprNode { stmts, expr, location, .. } = ctx.expression(node).as_block_expr().unwrap().clone();
        ctx.symbols.start_scope(ScopeKind::Block);

        let current_scope_id = ctx.symbols.current_scope_id().unwrap();

        let mut checked_stmts = Vec::with_capacity(stmts.len());

        for (i, stmt) in stmts.iter().enumerate() {
            let checked_stmt = self.visit_stmt(stmt.clone(), ctx)?;
            if checked_stmt.is_return() && (i != stmts.len() - 1 || expr.is_some()) {
                return Err(Error::InvalidReturn {
                    location: checked_stmt.as_return().unwrap().location,
                    message: format!("Quick return"),
                });
            }
            checked_stmts.push(checked_stmt);
        }

        let (return_type_id, checked_return_expr) = match expr {
            Some(expr) => {
                let checked_expr = self.visit_expr(expr, ctx)?;
                (checked_expr.ty(), Some(checked_expr))
            }
            None => {
                if let Some(CheckedReturnNode {
                    ret: Some(ret),
                    comments: _comments,
                    location: _location,
                }) = checked_stmts.last().and_then(|x| x.as_return())
                {
                    (self.program.exprs[ret.clone()].ty(), None)
                } else {
                    (VOID_TYPE, None)
                }
            }
        };

        ctx.symbols.end_scope();

        let checked_block_expr = CheckedBlockExprNode {
            stmts: self.program.stmts.alloc_items(checked_stmts),
            expr: checked_return_expr.map(|e| self.program.exprs.alloc_item(e)),
            type_id: return_type_id,
            scope_id: current_scope_id,
            location: location,
        };

        Ok(CheckedExprNode::BlockExpr(checked_block_expr))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_type_alias(&mut self, node: DefId, ctx: &mut Self::Context) -> StdResult<Self::DefinitionResult, Self::Error> {
        // TODO: remove clone
        let node = ctx.definition(node).as_type_alias().cloned().unwrap();

        let type_id = self.typecheck(&node.ty, ctx)?;
        let mut key: TypeKey = node.name.id.into();
        key.visibility = node.visibility;
        ctx.symbols.add_type_id(None, key, type_id)?;

        Ok(self.program.defs.alloc_item(CheckedDefinitionNode::TypeAlias(CheckedTypeAliasNode {
            name: node.name,
            ty: type_id,
            comments: node.comments,
            visibility: node.visibility,
        })))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_const(&mut self, node: DefId, ctx: &mut Self::Context) -> StdResult<Self::DefinitionResult, Self::Error> {
        // TODO: remove clone
        let node = ctx.definition(node).as_const().cloned().unwrap();
        let name = node.name;
        let lhs_ty = self.typecheck(&node.ty, ctx)?;
        let value = self.visit_expr(node.value, ctx)?;
        let rhs_ty = value.ty();
        if !self.unify(lhs_ty, rhs_ty, ctx) {
            return Err(Error::TypeMismatch {
                location: node.location,
                expected: vec![rhs_ty],
                found: lhs_ty,
            });
        }

        let value = self.evaluator.evaluate_expr(&self.program, &value, ctx);

        let node = CheckedConstNode {
            name: None,
            ty: rhs_ty,
            value: ctx.symbols.get_or_add_constant(value),
            scope_id: ScopeId::primitive(),
            visibility: node.visibility,
        };

        let type_id = ctx
            .symbols
            .get_or_add_type(Some(ScopeId::primitive()), TypeKey::from(node.value), Type::Const(node.clone()))?;

        ctx.symbols.add_type_id(None, name, type_id)?;

        Ok(self.program.defs.alloc_item(CheckedDefinitionNode::Const(node)))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_for(&mut self, node: StmtId, ctx: &mut Self::Context) -> StdResult<Self::StmtResult, Self::Error> {
        // TODO: remove clone
        let for_node = ctx.statement(node).as_for().cloned().unwrap();
        ctx.symbols.start_scope(ScopeKind::Block);
        let start = self.visit_expr(for_node.start, ctx)?;
        let end = self.visit_expr(for_node.end, ctx)?;
        if !(self.unify(start.ty(), FELT_TYPE, ctx) && self.unify(end.ty(), FELT_TYPE, ctx)
            || self.unify(start.ty(), U32_TYPE, ctx) && self.unify(end.ty(), U32_TYPE, ctx))
        {
            return Err(Error::TypeMismatch {
                location: for_node.location,
                expected: vec![FELT_TYPE, U32_TYPE],
                found: start.ty(),
            });
        }

        let current_scope_id = ctx.symbols.current_scope_id().unwrap();
        let variable = CheckedVariable::new(
            for_node.variable,
            start.ty(),
            TypeQualifier::new(true, for_node.location),
            current_scope_id,
            for_node.location,
        );
        let var_id = ctx.symbols.declare_variable(variable).ok_or(error::Error::VariableAlreadyDefined {
            location: for_node.location,
            variable: for_node.variable.id,
        })?;
        ctx.add_variable_reference(var_id, for_node.variable.location, false);

        ctx.symbols.start_scope(ScopeKind::Block);
        let checked_block = self.visit_expr(for_node.body, ctx)?;
        let node = CheckedStmtNode::For(CheckedForNode {
            variable: for_node.variable,
            start: self.program.exprs.alloc_item(start),
            end: self.program.exprs.alloc_item(end),
            body: self.program.exprs.alloc_item(checked_block),
            scope_id: current_scope_id,
            comments: for_node.comments,
            location: for_node.location,
        });
        ctx.symbols.end_scope();
        ctx.symbols.end_scope();
        Ok(node)
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_match(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        //get match node
        let match_node = ctx.expression(node).as_match().cloned().unwrap();
        let checked_scrutinee = self.visit_expr(match_node.scrutinee, ctx)?;

        //There are two type constraints here, one is that the scrutinee_type must be
        // consistent with the type of the value of the pattern The other is
        // that the return types of all cases must be consistent
        let scrutinee_type = checked_scrutinee.ty();
        let white_list = vec![FELT_TYPE, BOOL_TYPE, U32_TYPE];
        if !white_list.contains(&scrutinee_type) {
            return Err(Error::TypeMismatch {
                location: match_node.location,
                expected: white_list,
                found: scrutinee_type,
            });
        }

        if scrutinee_type == BOOL_TYPE {
            if !match_node.arms.len() == 2 {
                return Err(Error::IncompleteMatch {
                    location: match_node.location,
                    message: "Boolean match must have 2 arms".to_string(),
                });
            }
        }
        let mut checked_arms = Vec::new();
        let mut match_expr_type: Option<TypeId> = None;
        let mut wildcard_case: Option<CheckedMatchArm> = None;

        for (_idx, arm) in match_node.arms.iter().enumerate() {
            let checked_pattern = match &arm.pattern {
                MatchPattern::Value(pattern_expr, _pattern_location) => {
                    let checked_pattern_expr = self.visit_expr(*pattern_expr, ctx)?;
                    let pattern_type = checked_pattern_expr.ty();

                    if !self.unify(scrutinee_type, pattern_type, ctx) {
                        //todo!: When the location of value is implemented, you need to refactor here
                        let location = checked_pattern_expr.location();
                        return Err(Error::TypeMismatch {
                            location: location,
                            expected: vec![scrutinee_type],
                            found: pattern_type,
                        });
                    }
                    Some(self.program.exprs.alloc_item(checked_pattern_expr))
                }
                MatchPattern::PlaceHolder(location) => {
                    if wildcard_case.is_some() {
                        return Err(Error::DuplicateWildcard { location: location.clone() });
                    }
                    None
                }
            };

            let checked_body = self.visit_expr(arm.body, ctx)?;
            let arm_body_type = checked_body.ty();
            match_expr_type.get_or_insert(arm_body_type);
            if !self.unify(match_expr_type.unwrap(), arm_body_type, ctx) {
                let location = checked_body.location();
                return Err(Error::TypeMismatch {
                    location: location,
                    expected: vec![match_expr_type.unwrap()],
                    found: arm_body_type,
                });
            }

            let checked_arm = CheckedMatchArm {
                pattern: checked_pattern,
                body: self.program.exprs.alloc_item(checked_body),
                location: arm.location,
            };

            //note: move the wildcard case to the end
            if checked_arm.pattern.is_none() {
                wildcard_case = Some(checked_arm);
            } else {
                checked_arms.push(checked_arm);
            }
        }

        if let Some(placeholder) = wildcard_case {
            checked_arms.push(placeholder);
        }

        Ok(CheckedExprNode::Match(CheckedMatchNode {
            value: self.program.exprs.alloc_item(checked_scrutinee),
            cases: checked_arms,
            type_id: match_expr_type.unwrap_or(VOID_TYPE),
            scope_id: ctx.symbols.current_scope_id().unwrap(),
            location: match_node.location,
        }))
    }

    fn visit_parentheses(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        let expr_id = ctx.expression(node).as_parentheses().unwrap().clone();
        self.visit_expr(expr_id, ctx)
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_lambda_function(&mut self, node: ExprId, ctx: &mut Self::Context) -> StdResult<Self::ExprResult, Self::Error> {
        // TODO: remove clone
        ctx.symbols.start_scope(ScopeKind::LambdaFunction);
        let function = ctx.expression(node).as_lambda_function().cloned().unwrap();

        let current_scope_id = ctx.symbols.current_scope_id().unwrap();

        let mut parameters = Vec::with_capacity(function.parameters.len());

        for parameter in &function.parameters {
            let parameter_type = self.typecheck(&parameter.ty, ctx)?;
            let variable = CheckedVariable::new(parameter.name, parameter_type, parameter.qualifier, current_scope_id, parameter.location);
            let var_id = ctx.symbols.declare_variable(variable).ok_or(error::Error::VariableAlreadyDefined {
                location: function.location,
                variable: parameter.name.id,
            })?;
            ctx.add_variable_reference(var_id, parameter.name.location, false);
            parameters.push(CheckedFunctionParameter::new(
                parameter.name,
                parameter.qualifier,
                parameter_type,
                parameter.location,
            ));
        }

        let expected_return_type = if let Some(ref ret) = function.return_type {
            self.typecheck(ret, ctx)?
        } else {
            VOID_TYPE
        };

        let checked_body = {
            let checked_body = self.visit_expr(function.body.clone(), ctx)?;
            let actual_return_type = checked_body.ty();
            if !self.unify(expected_return_type, actual_return_type, ctx) {
                return Err(Error::TypeMismatch {
                    location: function.location,
                    expected: vec![expected_return_type],
                    found: actual_return_type,
                });
            }
            checked_body
        };

        let checked_function = CheckedLambdaFunctionNode {
            name: Identifier::new(ctx.intern_lambda(), function.location),
            parameters,
            body: self.program.exprs.alloc_item(checked_body),
            return_type: expected_return_type,
            scope_id: current_scope_id,
            type_id: ctx.symbols.next_type_id(0),
            location: function.location,
        };

        let ty = Type::LambdaFunction(checked_function.clone());
        ctx.symbols.add_type(ctx.symbols[current_scope_id].parent, ty.key(), ty)?;

        ctx.symbols.end_scope();
        Ok(CheckedExprNode::LambdaFunction(checked_function))
    }

    #[instrument(level = "debug", skip_all)]
    fn visit_trait_impl(&mut self, node: DefId, ctx: &mut Self::Context) -> StdResult<Self::DefinitionResult, Self::Error> {
        // TODO: remove clone
        let trait_impl_node = ctx.definition(node).as_trait_impl().cloned().unwrap();

        ctx.symbols.start_scope(ScopeKind::Impl);
        self.infcx.enter_context();

        let mut checked_generic_parameters = Vec::new();
        for generic_parameter in &trait_impl_node.generic_parameters {
            let checked_generic_parameter = self.typecheck_generic_parameter(generic_parameter, ctx)?;
            let type_id = ctx.symbols.add_type_variable(ScopeKind::Impl, checked_generic_parameter)?;
            checked_generic_parameters.push(type_id);
        }

        let trait_type_id = self.typecheck(&trait_impl_node.trait_ty, ctx)?;
        let trait_poly_type_id = self.poly_of(trait_type_id, ctx).unwrap();
        let implementor_type_id = self.typecheck(&trait_impl_node.ty, ctx)?;
        let implementor_poly_type_id = self.poly_of(implementor_type_id, ctx).unwrap();

        if ctx.symbols[implementor_type_id]
            .generic_parameters()
            .iter()
            .any(|&p| !ctx.symbols[p].is_type_variable())
            || ctx.symbols[trait_type_id]
                .generic_parameters()
                .iter()
                .any(|&p| !ctx.symbols[p].is_type_variable())
        {
            return Err(Error::SpecializationNotAllowed {
                location: trait_impl_node.location,
            });
        }

        for (generic_parameter, generic_arg) in ctx.symbols[implementor_poly_type_id]
            .generic_parameters()
            .iter()
            .zip_eq(ctx.symbols[implementor_type_id].generic_parameters())
        {
            if !self.unify(generic_parameter.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: trait_impl_node.location,
                    expected: vec![generic_parameter.clone()],
                    found: generic_arg,
                });
            }
        }

        for (generic_parameter, generic_arg) in ctx.symbols[trait_poly_type_id]
            .generic_parameters()
            .iter()
            .zip_eq(ctx.symbols[trait_type_id].generic_parameters())
        {
            if !self.unify(generic_parameter.clone(), generic_arg, ctx) {
                return Err(Error::TypeMismatch {
                    location: trait_impl_node.location,
                    expected: vec![generic_parameter.clone()],
                    found: generic_arg,
                });
            }
        }

        ctx.symbols.add_type_id(None, IdentId::TYPE_SELF, implementor_type_id)?;

        let trait_node = ctx.symbols[trait_poly_type_id].clone().into_trait().unwrap();
        let mut associated_types = IndexMap::new();
        for (name, associated_ty) in &trait_node.associated_types {
            let impl_type = trait_impl_node.associated_types.get(name).ok_or(Error::MissingAssociatedType {
                location: trait_impl_node.location,
                trait_name: trait_node.name.id,
                type_name: name.id,
            })?;

            let (root, target, type_id) = if let UncheckedType::Path(path) = &impl_type.ty {
                let checked_path = self.resolve_path(path, ctx)?;
                (checked_path.root, checked_path.target, checked_path.type_id)
            } else {
                (None, None, self.typecheck(&impl_type.ty, ctx)?)
            };
            if !self.unify(associated_ty.type_id, type_id, ctx) {
                return Err(Error::TypeMismatch {
                    location: trait_impl_node.location,
                    expected: vec![associated_ty.type_id.clone()],
                    found: type_id,
                });
            }
            associated_types.insert(
                name.clone(),
                CheckedAssociatedTypeValue {
                    root,
                    target,
                    type_id,
                    visibility: associated_ty.visibility,
                    comments: associated_ty.comments.clone(),
                    location: associated_ty.location,
                },
            );
        }

        let mut unimplemented_methods: HashSet<DefId> = trait_node.unchecked_body.iter().cloned().collect();
        let mut checked_methods = Vec::with_capacity(trait_node.body.len());

        for &function_id in &trait_impl_node.body {
            ctx.push_node_id(NodeId::from(function_id));
            self.infcx.enter_scope();

            let method_id = self.typecheck_function_predecl(function_id, ScopeKind::TraitMethod, ScopeKind::Trait, ctx)?;

            self.infcx.exit_scope();
            ctx.pop_node_id();

            let method = self.program[method_id].as_function().unwrap();
            let i = trait_node
                .body
                .iter()
                .position(|&trait_def_id| {
                    let trait_function = self.program.defs[trait_def_id].as_function().unwrap();
                    // trait_function.trait_impl_signature(implementor_type_id) ==
                    // method.signature()
                    trait_function.name == method.name
                })
                .ok_or(Error::UnresolvedTraitMethod {
                    method_location: method.location,
                    trait_name: trait_node.name.id,
                    method_name: method.name.id,
                })?;
            unimplemented_methods.remove(&trait_node.unchecked_body[i]);
            checked_methods.push(method_id);
        }

        for unimplemented_method in unimplemented_methods {
            let new_def_id = ctx.alloc_definition(ctx.definition(unimplemented_method).clone());
            ctx.program.defs[node].as_trait_impl_mut().unwrap().body.push(new_def_id);
            ctx.push_node_id(NodeId::from(new_def_id));
            self.infcx.enter_scope();

            let method_id = self.typecheck_function_predecl(new_def_id, ScopeKind::TraitMethod, ScopeKind::Trait, ctx)?;

            self.infcx.exit_scope();
            ctx.pop_node_id();

            checked_methods.push(method_id);
        }

        let checked_impl = CheckedTraitImplNode {
            associated_types,
            generic_parameters: checked_generic_parameters,
            trait_ty: trait_type_id,
            ty: implementor_type_id,
            body: checked_methods,
            scope_id: ctx.symbols.current_scope_id().unwrap(),
            comments: trait_impl_node.comments,
            location: trait_impl_node.location,
        };

        self.infcx.exit_context();
        ctx.symbols.end_scope();

        let trait_impl_id = self.program.defs.alloc_item(CheckedDefinitionNode::TraitImpl(checked_impl));
        self.register_trait_impl(trait_impl_id, ctx)?;

        self.unchecked_checked.insert(node.into(), trait_impl_id.into());

        for &function_id in &trait_impl_node.body {
            ctx.push_node_id(NodeId::from(function_id));
            self.infcx.enter_scope();

            self.typecheck_function_signature(function_id, ScopeKind::TraitMethod, ScopeKind::Trait, ctx)?;

            self.infcx.exit_scope();
            ctx.pop_node_id();
        }

        Ok(trait_impl_id)
    }
}

impl<F: Clone + From<u32> + ContextFelt, C> TypeChecker<F, C> {
    pub fn new(program: CheckedProgram<F>, evaluator: Box<dyn Evaluator<F, C>>) -> Self {
        Self {
            program,
            evaluator,
            resolver: ResolverCtxt::new(),
            infcx: InferCtxt::new(),
            implementer: ImplementerCtxt::new(),
            unchecked_checked: HashMap::new(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Generic module tree traversal with customizable processing logic
    fn traverse_module_tree_inner(
        &mut self,
        module_id: ModuleId,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
        visited_modules: &mut std::collections::HashSet<ModuleId>,
        process_module: &mut dyn FnMut(&mut Self, ModuleId, &ModuleNode, &mut TypeCheckerVisitorContext<F, C>) -> Result<()>,
    ) -> Result<()> {
        if visited_modules.contains(&module_id) {
            return Ok(());
        }
        visited_modules.insert(module_id);

        ctx.symbols.enter_module(module_id);
        // Recursively process child modules
        let modules_tree = &ctx.program().modules;
        let children = modules_tree[module_id].children().to_vec();
        for &child_module_id in &children {
            self.traverse_module_tree_inner(child_module_id, ctx, visited_modules, process_module)?;
        }
        let module = ctx.module(module_id).clone();
        process_module(self, module_id, &module, ctx)?;
        ctx.symbols.exit_module();

        Ok(())
    }

    fn traverse_module_tree(
        &mut self,
        ctx: &mut TypeCheckerVisitorContext<F, C>,
        process_module: &mut dyn FnMut(&mut Self, ModuleId, &ModuleNode, &mut TypeCheckerVisitorContext<F, C>) -> Result<()>,
    ) -> Result<()> {
        let mut visited_modules = std::collections::HashSet::new();
        let dependency_graph = ctx.program().dependency_graph.clone();
        dependency_graph.ts::<Error>(&mut |&crate_id| {
            let crate_root_module_id = ModuleId::from(crate_id);
            self.traverse_module_tree_inner(crate_root_module_id, ctx, &mut visited_modules, process_module)?;
            Ok(())
        })?;
        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    fn typecheck_std_primitive_module(&mut self, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<()> {
        #[allow(static_mut_refs)]
        unsafe {
            let scope_id = ctx.symbols.current_scope_id().unwrap();
            if let Err(id) = STD_PRIMITIVE_SCOPE_ID.set(scope_id)
                && id != scope_id
            {
                STD_PRIMITIVE_SCOPE_ID.take().unwrap();
                STD_PRIMITIVE_SCOPE_ID.set(scope_id).unwrap();
            }
            // //Warning: Not safe to run in a multithreaded environment
            // *STD_PRIMITIVE_SCOPE_ID.get_or_init(|| {
            //     ctx.symbols
            //         .current_scope_id()
            //         .expect("cannot get current scope id")
            // })
        };
        for ty in &*PRIMITIVE_TYPES {
            ctx.symbols.add_type(None, ty.key(), ty.clone())?;
        }
        self.typecheck_array(ctx)?;

        let felt_type = UncheckedType::Basic(Identifier::new(IdentId::TYPE_FELT, Location::default()));
        let hash_ty_node = UncheckedType::Array(Box::new(felt_type), 4, Location::default());

        let checked_hash_ty = self.typecheck(&hash_ty_node, ctx)?;

        let mut key: TypeKey = ctx.program.interner.intern_ident("Hash").into();
        key.visibility = Visibility::Public;
        ctx.symbols.add_type_id(None, key, checked_hash_ty)?;

        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    fn typecheck_member_access(&mut self, receiver: ExprId, ctx: &TypeCheckerVisitorContext<F, C>) -> bool {
        ctx.symbols
            .find(None, vec![ScopeKind::Impl], |s| s.kind.eq(&ScopeKind::ImplMethod).then_some(true))
            .is_some()
            && ctx.expression(receiver).as_path().map(|x| x.is_receiver()).unwrap_or(false)
    }

    #[instrument(level = "debug", skip_all)]
    fn populate_constant(&mut self, value: ConstValue, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<TypeId> {
        let value_f = self.evaluator.from_constant(value);
        let ty = match value {
            ConstValue::U32(_) => U32_TYPE,
            ConstValue::Felt(_) => FELT_TYPE,
            ConstValue::Bool(_) => BOOL_TYPE,
            _ => unreachable!(),
        };
        let node = CheckedConstNode {
            name: None,
            ty,
            value: ctx.symbols.get_or_add_constant(CheckedValueRef::from_value(value_f, ty)),
            scope_id: ScopeId::primitive(),
            visibility: Visibility::Public,
        };

        ctx.symbols
            .get_or_add_type(Some(ScopeId::primitive()), TypeKey::from(node.value), Type::Const(node))
    }

    #[instrument(level = "debug", skip_all)]
    fn typecheck_array(&mut self, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<TypeId> {
        ctx.symbols.start_scope(ScopeKind::Array);
        self.infcx.enter_context();

        let inner_ty = ctx.symbols.add_type_variable(
            ScopeKind::Module,
            CheckedGenericParameter::new(IdentId::T, vec![], ScopeId::primitive(), Location::default()),
        )?;
        let size = ctx.symbols.add_type_variable(
            ScopeKind::Module,
            CheckedGenericParameter::new(IdentId::N, vec![U32_TYPE], ScopeId::primitive(), Location::default()),
        )?;

        let checked_array = CheckedArrayNode {
            inner_ty,
            size_ty: size,
            scope_id: ctx.symbols.current_scope_id().unwrap(),
        };

        let ty = Type::Array(checked_array.clone());
        let type_id = ctx.symbols.add_type(Some(ScopeId::primitive()), ty.name(), ty)?;

        self.infcx.exit_context();
        ctx.symbols.end_scope();
        Ok(type_id)
    }

    #[instrument(level = "debug", skip_all)]
    fn typecheck(&mut self, ty: &UncheckedType, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<TypeId> {
        self.infcx.enter_scope();
        let type_id = match ty {
            UncheckedType::Const(value, _) => self.populate_constant(*value, ctx)?,
            UncheckedType::Basic(name) => match name.id {
                IdentId::TYPE_BOOL => BOOL_TYPE,
                IdentId::TYPE_FELT => FELT_TYPE,
                IdentId::TYPE_U32 => U32_TYPE,
                _ => ctx.symbols.get_type_id(None, name).ok_or(Error::UnresolvedType {
                    location: name.location,
                    resolved_type: name.id,
                })?,
            },
            UncheckedType::Path(path) => self.resolve_path(path.as_ref(), ctx)?.type_id,
            UncheckedType::Generic(name, generic_parameters, location) => {
                let underlying_type_id = ctx.symbols.get_type_id(None, name).ok_or(Error::UnresolvedType {
                    location: location.clone(),
                    resolved_type: name.id,
                })?;

                let mut checked_generic_args = Vec::new();
                for generic_parameter in generic_parameters {
                    checked_generic_args.push(self.typecheck(generic_parameter, ctx)?);
                }

                match ctx.symbols[underlying_type_id].clone() {
                    Type::Struct(checked_struct) => {
                        if checked_struct.generic_parameters.len() != checked_generic_args.len() {
                            return Err(Error::InvalidGenericArguments {
                                location: checked_struct.location,
                                expected: format!("{} generic parameters", checked_struct.generic_parameters.len()),
                                found: format!("{}", checked_generic_args.len()),
                            });
                        }

                        for (generic_param, generic_arg) in checked_struct.generic_parameters.iter().zip(checked_generic_args.iter()) {
                            if !self.unify(*generic_param, *generic_arg, ctx) {
                                return Err(Error::TypeMismatch {
                                    location: location.clone(),
                                    expected: vec![*generic_param],
                                    found: *generic_arg,
                                });
                            }
                        }

                        self.substitute_all(underlying_type_id, ctx)?
                    }

                    Type::Array(checked_array) => {
                        if checked_generic_args.len() != 2 {
                            return Err(Error::InvalidGenericArguments {
                                location: location.clone(),
                                expected: format!("2 generic parameters",),
                                found: format!("{}", checked_generic_args.len()),
                            });
                        }
                        if !self.unify(checked_array.inner_ty, checked_generic_args[0], ctx) {
                            return Err(Error::TypeMismatch {
                                location: location.clone(),
                                expected: vec![checked_array.inner_ty],
                                found: checked_generic_args[0],
                            });
                        }
                        if !self.unify(checked_array.size_ty, checked_generic_args[1], ctx) {
                            return Err(Error::TypeMismatch {
                                location: location.clone(),
                                expected: vec![checked_array.size_ty],
                                found: checked_generic_args[1],
                            });
                        }
                        self.substitute_all(underlying_type_id, ctx)?
                    }

                    Type::Trait(checked_trait) => {
                        for (generic_param, generic_arg) in checked_trait.generic_parameters.iter().zip(checked_generic_args.iter()) {
                            if !self.unify(*generic_param, *generic_arg, ctx) {
                                return Err(Error::TypeMismatch {
                                    location: location.clone(),
                                    expected: vec![*generic_param],
                                    found: *generic_arg,
                                });
                            }
                        }

                        self.substitute_all(underlying_type_id, ctx)?
                    }

                    _ => unreachable!(),
                }
            }
            UncheckedType::Array(inner_ty, size, location) => {
                let underlying_type_id = ctx.symbols.get_type_id(Some(ScopeId::primitive()), IdentId::TYPE_ARRAY).unwrap();

                let &CheckedArrayNode {
                    inner_ty: generic_inner_ty,
                    size_ty: generic_size_ty,
                    ..
                } = ctx.symbols[underlying_type_id].as_array().unwrap();

                let inner_ty = self.typecheck(inner_ty.as_ref(), ctx)?;

                let size_ty = self.populate_constant(size.clone().into(), ctx)?;

                if !self.unify(generic_inner_ty, inner_ty, ctx) {
                    return Err(Error::TypeMismatch {
                        location: location.clone(),
                        expected: vec![generic_inner_ty],
                        found: inner_ty,
                    });
                }
                if !self.unify(generic_size_ty, size_ty, ctx) {
                    return Err(Error::TypeMismatch {
                        location: location.clone(),
                        expected: vec![generic_size_ty],
                        found: size_ty,
                    });
                }

                self.substitute_all(underlying_type_id, ctx)?
            }
            UncheckedType::Tuple(elements, _) => {
                // check each element and collect results into a Result<Vec<TypeId>>
                let checked_elements = elements.iter().map(|elem_ty| self.typecheck(elem_ty, ctx)).collect::<Result<_>>()?;

                let checked_tuple = Type::Tuple(checked_elements);

                let scope_id = ScopeId::primitive();

                ctx.symbols.get_or_add_type(Some(scope_id), checked_tuple.key(), checked_tuple)?
            }
            UncheckedType::Unknown => UNKOWN_TYPE,
            UncheckedType::FunctionSignature(function_signature, _) => {
                let mut parameters = Vec::with_capacity(function_signature.parameters.len());
                for parameter_ty in &function_signature.parameters {
                    let ty = self.typecheck(parameter_ty, ctx)?;
                    parameters.push(ty);
                }
                let return_type = if let Some(ref ty) = function_signature.return_type {
                    Some(self.typecheck(ty, ctx)?)
                } else {
                    None
                };

                let ty = Type::FunctionSignature(CheckedFunctionSignature {
                    parameters,
                    return_type: return_type.unwrap_or(VOID_TYPE),
                });

                ctx.symbols.get_or_add_type(None, ty.key(), ty)?
            }
            UncheckedType::TraitCast(ty, _trait_ty, _) => self.typecheck(ty, ctx)?,
        };

        let is_self_type = ty.is_basic() && ty.as_basic().unwrap().id == IdentId::TYPE_SELF;
        ctx.add_type_reference(type_id, ty.location(), is_self_type);
        self.infcx.exit_scope();
        Ok(type_id)
    }

    fn typecheck_generic_parameter(&mut self, ty: &GenericParameter, ctx: &mut TypeCheckerVisitorContext<F, C>) -> Result<CheckedGenericParameter> {
        let mut constraints = Vec::with_capacity(ty.constraints.len());
        for constraint in &ty.constraints {
            let constraint = self.typecheck(constraint, ctx)?;
            constraints.push(constraint);
        }

        if !(constraints.iter().all(|&c| ctx.symbols[c].is_trait())
            || (constraints.len() == 1 && matches!(constraints[0], FELT_TYPE | BOOL_TYPE | U32_TYPE)))
        {
            return Err(Error::InvalidGenericConstraint { location: ty.location });
        }

        Ok(CheckedGenericParameter {
            name: ty.name,
            constraints,
            scope_id: ctx.symbols.current_scope_id().unwrap(),
            location: ty.location,
        })
    }

    pub fn add_use(&mut self, use_path: &UseNode, ctx: &mut TypeCheckerVisitorContext<F, C>) -> StdResult<(), Error> {
        let type_ids = self.resolve_use(&use_path, ctx)?;
        let type_ids = type_ids.into_iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>();
        for (mut key, type_id) in type_ids {
            key.visibility = use_path.visibility;
            let _ = ctx.symbols.add_type_id(None, key.clone(), type_id);
        }
        Ok(())
    }
}
