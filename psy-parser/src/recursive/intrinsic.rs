//! Intrinsic parsing for the recursive-descent parser.
//!
//! Handles all IntrinsicExprNode and IntrinsicStmtNode variants.
//! Each intrinsic has a fixed arity and specific argument order.

use psy_ast::{Comment, ExprNode, IntrinsicExprNode, IntrinsicStmtNode, Location, StmtNode, UncheckedType, ValueNode};
use psy_lexer::Token;
use psy_vm::dpn::ops::context_trait::{ContextFelt, DPNContext};

use crate::error::{Error, ExpectedToken, Result};

/// Try to parse an intrinsic expression. Returns None if the current token
/// is not an intrinsic expression token.
pub fn try_parse_intrinsic_expr<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Option<Result<ExprNode<F>>>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let start = parser.cursor.peek_start();
    let token = parser.cursor.peek()?;

    let result = match token {
        // ─── Hash/keccak ──────────────────────────────────────────────────
        Token::IntrinsicHash => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |data, loc| IntrinsicExprNode::Hash { data, location: loc })
        }
        Token::IntrinsicKeccak256 => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |data, loc| IntrinsicExprNode::Keccak256 { data, location: loc })
        }
        Token::IntrinsicHashTwoToOne => {
            parser.cursor.advance();
            parse_binary_intrinsic(parser, start, |left, right, loc| IntrinsicExprNode::HashTwoToOne {
                left,
                right,
                location: loc,
            })
        }

        // ─── Mem ──────────────────────────────────────────────────────────
        Token::IntrinsicMemTransmute => {
            parser.cursor.advance();
            parse_mem_transmute(parser, start)
        }
        Token::IntrinsicMemSizeOf => {
            parser.cursor.advance();
            parse_mem_size_of(parser, start)
        }

        // ─── Storage ──────────────────────────────────────────────────────
        Token::IntrinsicStorageRead => {
            parser.cursor.advance();
            parse_storage_read(parser, start)
        }
        Token::IntrinsicStorageWrite => {
            parser.cursor.advance();
            parse_storage_write(parser, start)
        }
        Token::IntrinsicStorageReadRange => {
            parser.cursor.advance();
            parse_storage_read_range(parser, start)
        }
        Token::IntrinsicStorageWriteRange => {
            parser.cursor.advance();
            parse_storage_write_range(parser, start)
        }

        // ─── Context getters (no args) ────────────────────────────────────
        Token::IntrinsicCtxGetUserId => {
            parser.cursor.advance();
            parse_no_arg_intrinsic(parser, start, |loc| IntrinsicExprNode::GetUserId { location: loc })
        }
        Token::IntrinsicCtxGetContractId => {
            parser.cursor.advance();
            parse_no_arg_intrinsic(parser, start, |loc| IntrinsicExprNode::GetContractId { location: loc })
        }
        Token::IntrinsicCtxGetCallerContractId => {
            parser.cursor.advance();
            parse_no_arg_intrinsic(parser, start, |loc| IntrinsicExprNode::GetCallerContractId { location: loc })
        }
        Token::IntrinsicCtxGetCheckpointId => {
            parser.cursor.advance();
            parse_no_arg_intrinsic(parser, start, |loc| IntrinsicExprNode::GetCheckpointId { location: loc })
        }
        Token::IntrinsicCtxGetLastNonce => {
            parser.cursor.advance();
            parse_no_arg_intrinsic(parser, start, |loc| IntrinsicExprNode::GetLastNonce { location: loc })
        }
        Token::IntrinsicCtxGetUserPublicKeyHash => {
            parser.cursor.advance();
            parse_no_arg_intrinsic(parser, start, |loc| IntrinsicExprNode::GetUserPublicKeyHash { location: loc })
        }
        Token::IntrinsicCtxGetSessionProofTreeRoot => {
            parser.cursor.advance();
            parse_no_arg_intrinsic(parser, start, |loc| IntrinsicExprNode::GetSessionProofTreeRoot { location: loc })
        }

        // ─── Context getters (1 arg: checkpoint_id) ───────────────────────
        Token::IntrinsicCtxGetCheckpointStats => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetCheckpointStats {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetRegisterUsersRoot => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetRegisterUsersRoot {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetGutasRoot => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetGutasRoot {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetCheckpointUserTreeRoot => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetCheckpointUserTreeRoot {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetCheckpointContractTreeRoot => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetCheckpointContractTreeRoot {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetCheckpointDepositTreeRoot => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetCheckpointDepositTreeRoot {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetCheckpointWithdrawalTreeRoot => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetCheckpointWithdrawalTreeRoot {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetCheckpointUserRegistrationTreeRoot => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| {
                IntrinsicExprNode::GetCheckpointUserRegistrationTreeRoot {
                    checkpoint_id,
                    location: loc,
                }
            })
        }
        Token::IntrinsicCtxGetDeployContractsRoot => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetDeployContractsRoot {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetGutaFeesCollected => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetGutaFeesCollected {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetDAFeesCollected => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetDaFeesCollected {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetUserOpsProcessed => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetUserOpsProcessed {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetTotalTransactions => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetTotalTransactions {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetSlotsModified => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetSlotsModified {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetDeployContractsCompleted => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetDeployContractsCompleted {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetRegisterUsersCompleted => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetRegisterUsersCompleted {
                checkpoint_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetGutasCompleted => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |checkpoint_id, loc| IntrinsicExprNode::GetGutasCompleted {
                checkpoint_id,
                location: loc,
            })
        }

        // ─── Context getters (1 arg: contract_id) ─────────────────────────
        Token::IntrinsicCtxGetContractDeployer => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |contract_id, loc| IntrinsicExprNode::GetContractDeployer {
                contract_id,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetContractStateTreeHeight => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |contract_id, loc| IntrinsicExprNode::GetContractStateTreeHeight {
                contract_id,
                location: loc,
            })
        }

        // ─── Context getters (1 arg: slot_index) ──────────────────────────
        Token::IntrinsicCtxGetStateHashAt => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |slot_index, loc| IntrinsicExprNode::GetStateHashAt {
                slot_index,
                location: loc,
            })
        }
        Token::IntrinsicCtxSetStateHashAt => {
            parser.cursor.advance();
            parse_binary_intrinsic(parser, start, |slot_index, new_value, loc| IntrinsicExprNode::CSetStateHashAt {
                slot_index,
                new_value,
                location: loc,
            })
        }

        // ─── Other context getters (3-4 args) ─────────────────────────────
        Token::IntrinsicCtxGetOtherContractStateHashAt => {
            parser.cursor.advance();
            parse_ternary_intrinsic(parser, start, |a, b, c, loc| IntrinsicExprNode::GetOtherContractStateHashAt {
                contract_state_tree_height: a,
                contract_id: b,
                slot_index: c,
                location: loc,
            })
        }
        Token::IntrinsicCtxGetOtherUserContractStateHashAt => {
            parser.cursor.advance();
            parse_quaternary_intrinsic(parser, start, |a, b, c, d, loc| IntrinsicExprNode::GetOtherUserContractStateHashAt {
                contract_state_tree_height: a,
                user_id: b,
                contract_id: c,
                slot_index: d,
                location: loc,
            })
        }

        // ─── IMT ──────────────────────────────────────────────────────────
        Token::IntrinsicImtGet => {
            parser.cursor.advance();
            parse_imt_get(parser, start)
        }
        Token::IntrinsicImtSet => {
            parser.cursor.advance();
            parse_imt_set(parser, start)
        }
        Token::IntrinsicImtContains => {
            parser.cursor.advance();
            parse_imt_contains(parser, start)
        }
        Token::IntrinsicImtGetOtherUser => {
            parser.cursor.advance();
            parse_senary_intrinsic(parser, start, |a, b, c, d, e, f, loc| IntrinsicExprNode::ImtGetOtherUser {
                contract_state_tree_height: a,
                user_id: b,
                contract_id: c,
                key: d,
                base_offset: e,
                capacity: f,
                location: loc,
            })
        }
        Token::IntrinsicImtContainsOtherUser => {
            parser.cursor.advance();
            parse_senary_intrinsic(parser, start, |a, b, c, d, e, f, loc| IntrinsicExprNode::ImtContainsOtherUser {
                contract_state_tree_height: a,
                user_id: b,
                contract_id: c,
                key: d,
                base_offset: e,
                capacity: f,
                location: loc,
            })
        }

        // ─── Invoke ───────────────────────────────────────────────────────
        Token::IntrinsicInvokeSync => {
            parser.cursor.advance();
            parse_invoke_sync(parser, start)
        }
        Token::IntrinsicInvokeDeferred => {
            parser.cursor.advance();
            parse_ternary_intrinsic(parser, start, |contract_id, method_id, inputs, loc| IntrinsicExprNode::InvokeDeferred {
                contract_id,
                method_id,
                inputs,
                location: loc,
            })
        }

        // ─── Crypto ───────────────────────────────────────────────────────
        Token::IntrinsicSecp256k1Verify => {
            parser.cursor.advance();
            parse_ternary_intrinsic(parser, start, |pub_key, msg, sig, loc| IntrinsicExprNode::Secp256k1Verify {
                pub_key,
                msg,
                sig,
                location: loc,
            })
        }

        // ─── Bits ─────────────────────────────────────────────────────────
        Token::IntrinsicSumBits => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |bits, loc| IntrinsicExprNode::SumBits { bits, location: loc })
        }
        Token::IntrinsicSplitBits => {
            parser.cursor.advance();
            parse_split_bits(parser, start)
        }

        // ─── Emit ─────────────────────────────────────────────────────────
        Token::IntrinsicEmit => {
            parser.cursor.advance();
            parse_unary_intrinsic(parser, start, |event_data, loc| IntrinsicExprNode::Emit { event_data, location: loc })
        }

        _ => return None,
    };

    // Wrap in ExprNode::Intrinsic
    Some(match result {
        Ok(node) => {
            let end = parser.cursor.peek_start();
            Ok(ExprNode::Intrinsic(node))
        }
        Err(e) => Err(e),
    })
}

/// Try to parse an intrinsic statement. Returns None if the current token
/// is not an intrinsic statement token.
pub fn try_parse_intrinsic_stmt<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, comments: Vec<Comment>) -> Option<Result<StmtNode>>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let start = parser.cursor.peek_start();
    let token = parser.cursor.peek()?;

    let result = match token {
        Token::IntrinsicAssert => {
            parser.cursor.advance();
            parse_assert(parser, start, comments)
        }
        Token::IntrinsicAssertEq => {
            parser.cursor.advance();
            parse_assert_eq(parser, start, comments)
        }
        Token::IntrinsicCtxClearEntireTree => {
            parser.cursor.advance();
            parse_clear_entire_tree(parser, start, comments)
        }
        _ => return None,
    };

    Some(result)
}

// ─── Helper functions ─────────────────────────────────────────────────────

fn loc(parser: &super::ModuleParser<impl Clone + From<u32>, impl Sized>, start: usize) -> Location {
    let end = parser.cursor.peek_start();
    parser.location(start, end)
}

/// Parse `()` — no-argument intrinsic
fn parse_no_arg_intrinsic<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    start: usize,
    make: impl FnOnce(Location) -> IntrinsicExprNode,
) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    parser.cursor.expect(&Token::RParen)?;
    Ok(make(loc(parser, start)))
}

/// Parse `(expr)` — single-argument intrinsic
fn parse_unary_intrinsic<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    start: usize,
    make: impl FnOnce(psy_ast::ExprId, Location) -> IntrinsicExprNode,
) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let arg = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::RParen)?;
    Ok(make(arg, loc(parser, start)))
}

/// Parse `(expr, expr)` — two-argument intrinsic
fn parse_binary_intrinsic<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    start: usize,
    make: impl FnOnce(psy_ast::ExprId, psy_ast::ExprId, Location) -> IntrinsicExprNode,
) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let a = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let b = parse_arg_expr(parser)?;
    eat_optional_comma(parser);
    parser.cursor.expect(&Token::RParen)?;
    Ok(make(a, b, loc(parser, start)))
}

/// Parse `(expr, expr, expr)` — three-argument intrinsic
fn parse_ternary_intrinsic<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    start: usize,
    make: impl FnOnce(psy_ast::ExprId, psy_ast::ExprId, psy_ast::ExprId, Location) -> IntrinsicExprNode,
) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let a = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let b = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let c = parse_arg_expr(parser)?;
    eat_optional_comma(parser);
    parser.cursor.expect(&Token::RParen)?;
    Ok(make(a, b, c, loc(parser, start)))
}

/// Parse `(expr, expr, expr, expr)` — four-argument intrinsic
fn parse_quaternary_intrinsic<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    start: usize,
    make: impl FnOnce(psy_ast::ExprId, psy_ast::ExprId, psy_ast::ExprId, psy_ast::ExprId, Location) -> IntrinsicExprNode,
) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let a = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let b = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let c = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let d = parse_arg_expr(parser)?;
    eat_optional_comma(parser);
    parser.cursor.expect(&Token::RParen)?;
    Ok(make(a, b, c, d, loc(parser, start)))
}

/// Parse `(a, b, c, d, e, f)` — six-argument intrinsic
fn parse_senary_intrinsic<'src, 'p, F, C>(
    parser: &mut super::ModuleParser<'src, 'p, F, C>,
    start: usize,
    make: impl FnOnce(psy_ast::ExprId, psy_ast::ExprId, psy_ast::ExprId, psy_ast::ExprId, psy_ast::ExprId, psy_ast::ExprId, Location) -> IntrinsicExprNode,
) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let a = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let b = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let c = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let d = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let e = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let f = parse_arg_expr(parser)?;
    eat_optional_comma(parser);
    parser.cursor.expect(&Token::RParen)?;
    Ok(make(a, b, c, d, e, f, loc(parser, start)))
}

/// Parse `::<Type>(data)` — mem_transmute
fn parse_mem_transmute<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::DoubleColon)?;
    parser.cursor.expect(&Token::OperatorLt)?;
    let target_type = parser.parse_path_ty()?;
    parser.cursor.expect_type_gt()?;
    parser.cursor.expect(&Token::LParen)?;
    let data = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::RParen)?;
    Ok(IntrinsicExprNode::MemTransmute {
        data,
        target_type,
        location: loc(parser, start),
    })
}

/// Parse `::<Type>()` — mem_size_of
fn parse_mem_size_of<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::DoubleColon)?;
    parser.cursor.expect(&Token::OperatorLt)?;
    let query_type = parser.parse_path_ty()?;
    parser.cursor.expect_type_gt()?;
    parser.cursor.expect(&Token::LParen)?;
    parser.cursor.expect(&Token::RParen)?;
    Ok(IntrinsicExprNode::MemSizeOf {
        query_type,
        location: loc(parser, start),
    })
}

/// Parse `(height, user_id, contract_id, offset)` — storage_read
fn parse_storage_read<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let h = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let u = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let c = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let o = parse_arg_expr(parser)?;
    eat_optional_comma(parser);
    parser.cursor.expect(&Token::RParen)?;
    Ok(IntrinsicExprNode::StorageRead {
        contract_state_tree_height: h,
        user_id: u,
        contract_id: c,
        offset: o,
        location: loc(parser, start),
    })
}

/// Parse `(offset, value)` — storage_write
fn parse_storage_write<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parse_binary_intrinsic(parser, start, |offset, value, loc| IntrinsicExprNode::StorageWrite {
        offset,
        value,
        location: loc,
    })
}

/// Parse `(height, user_id, contract_id, offset, length)` — storage_read_range
fn parse_storage_read_range<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let h = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let u = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let c = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let o = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let l = parse_arg_expr(parser)?;
    eat_optional_comma(parser);
    parser.cursor.expect(&Token::RParen)?;
    Ok(IntrinsicExprNode::StorageReadRange {
        contract_state_tree_height: h,
        user_id: u,
        contract_id: c,
        offset: o,
        length: l,
        location: loc(parser, start),
    })
}

/// Parse `(offset, values)` — storage_write_range
fn parse_storage_write_range<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parse_binary_intrinsic(parser, start, |offset, values, loc| IntrinsicExprNode::StorageWriteRange {
        offset,
        values,
        location: loc,
    })
}

/// Parse imt_get: `(key)` or `(key, base_offset, capacity)`
fn parse_imt_get<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let key = parse_arg_expr(parser)?;

    if parser.cursor.eat(&Token::Comma).is_some() {
        let base_offset = parse_arg_expr(parser)?;
        parser.cursor.expect(&Token::Comma)?;
        let capacity = parse_arg_expr(parser)?;
        eat_optional_comma(parser);
        parser.cursor.expect(&Token::RParen)?;
        Ok(IntrinsicExprNode::ImtGet {
            key,
            base_offset,
            capacity,
            location: loc(parser, start),
        })
    } else {
        parser.cursor.expect(&Token::RParen)?;
        // Default base_offset and capacity to Felt(0)
        let zero = parser.alloc_expr(ExprNode::Value(ValueNode::Felt(F::from(0), loc(parser, start))));
        Ok(IntrinsicExprNode::ImtGet {
            key,
            base_offset: zero,
            capacity: parser.alloc_expr(ExprNode::Value(ValueNode::Felt(F::from(0), loc(parser, start)))),
            location: loc(parser, start),
        })
    }
}

/// Parse imt_set: `(key, new_value)` or `(key, new_value, base_offset,
/// capacity)`
fn parse_imt_set<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let key = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let new_value = parse_arg_expr(parser)?;

    if parser.cursor.eat(&Token::Comma).is_some() {
        let base_offset = parse_arg_expr(parser)?;
        parser.cursor.expect(&Token::Comma)?;
        let capacity = parse_arg_expr(parser)?;
        eat_optional_comma(parser);
        parser.cursor.expect(&Token::RParen)?;
        Ok(IntrinsicExprNode::ImtSet {
            key,
            new_value,
            base_offset,
            capacity,
            location: loc(parser, start),
        })
    } else {
        parser.cursor.expect(&Token::RParen)?;
        let zero = parser.alloc_expr(ExprNode::Value(ValueNode::Felt(F::from(0), loc(parser, start))));
        Ok(IntrinsicExprNode::ImtSet {
            key,
            new_value,
            base_offset: zero,
            capacity: parser.alloc_expr(ExprNode::Value(ValueNode::Felt(F::from(0), loc(parser, start)))),
            location: loc(parser, start),
        })
    }
}

/// Parse imt_contains: `(key, base_offset, capacity)`
fn parse_imt_contains<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parse_ternary_intrinsic(parser, start, |key, base_offset, capacity, loc| IntrinsicExprNode::ImtContains {
        key,
        base_offset,
        capacity,
        location: loc,
    })
}

/// Parse `::<return_type>(contract_id, method_id, inputs)` — invoke_sync
fn parse_invoke_sync<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::DoubleColon)?;
    parser.cursor.expect(&Token::OperatorLt)?;
    let return_type = parser.parse_path_ty()?;
    parser.cursor.expect_type_gt()?;
    parser.cursor.expect(&Token::LParen)?;
    let contract_id = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let method_id = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let inputs = parse_arg_expr(parser)?;
    eat_optional_comma(parser);
    parser.cursor.expect(&Token::RParen)?;
    Ok(IntrinsicExprNode::InvokeSync {
        contract_id,
        method_id,
        inputs,
        return_type,
        location: loc(parser, start),
    })
}

/// Parse `(target, num_bits: U64)` — split_bits
fn parse_split_bits<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize) -> Result<IntrinsicExprNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let target = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let num_bits = parse_arg_expr(parser)?;
    eat_optional_comma(parser);
    parser.cursor.expect(&Token::RParen)?;
    Ok(IntrinsicExprNode::SplitBits {
        target,
        num_bits,
        location: loc(parser, start),
    })
}

// ─── Intrinsic statements ─────────────────────────────────────────────────

/// Parse `assert(expr, "message"?);`
fn parse_assert<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize, comments: Vec<Comment>) -> Result<StmtNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let left = parse_arg_expr(parser)?;
    let message = if parser.cursor.eat(&Token::Comma).is_some() {
        match parser.cursor.peek() {
            Some(Token::String(s)) => {
                let s = s.to_string();
                parser.cursor.advance();
                Some(s)
            }
            _ => None,
        }
    } else {
        None
    };
    eat_optional_comma(parser);
    parser.cursor.expect(&Token::RParen)?;
    parser.cursor.expect(&Token::Semicolon)?;
    Ok(StmtNode::Intrinsic(IntrinsicStmtNode::Assert {
        left,
        message,
        comments,
        location: loc(parser, start),
    }))
}

/// Parse `assert_eq(expr, expr, "message"?);`
fn parse_assert_eq<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize, comments: Vec<Comment>) -> Result<StmtNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    let left = parse_arg_expr(parser)?;
    parser.cursor.expect(&Token::Comma)?;
    let right = parse_arg_expr(parser)?;
    let message = if parser.cursor.eat(&Token::Comma).is_some() {
        match parser.cursor.peek() {
            Some(Token::String(s)) => {
                let s = s.to_string();
                parser.cursor.advance();
                Some(s)
            }
            _ => None,
        }
    } else {
        None
    };
    eat_optional_comma(parser);
    parser.cursor.expect(&Token::RParen)?;
    parser.cursor.expect(&Token::Semicolon)?;
    Ok(StmtNode::Intrinsic(IntrinsicStmtNode::AssertEq {
        left,
        right,
        message,
        comments,
        location: loc(parser, start),
    }))
}

/// Parse `__ctx_clear_entire_tree();`
fn parse_clear_entire_tree<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>, start: usize, comments: Vec<Comment>) -> Result<StmtNode>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    parser.cursor.expect(&Token::LParen)?;
    parser.cursor.expect(&Token::RParen)?;
    parser.cursor.expect(&Token::Semicolon)?;
    Ok(StmtNode::Intrinsic(IntrinsicStmtNode::ClearEntireTree {
        comments,
        location: loc(parser, start),
    }))
}

// ─── Utilities ────────────────────────────────────────────────────────────

/// Parse a single expression argument and allocate it.
fn parse_arg_expr<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>) -> Result<psy_ast::ExprId>
where
    F: ContextFelt + From<u32>,
    C: DPNContext<F>,
{
    let expr = parser.parse_expression_struct_allowed()?;
    Ok(parser.alloc_expr(expr))
}

/// Eat an optional trailing comma before a closing delimiter.
fn eat_optional_comma<'src, 'p, F, C>(parser: &mut super::ModuleParser<'src, 'p, F, C>)
where
    F: Clone + From<u32>,
{
    parser.cursor.eat(&Token::Comma);
}
