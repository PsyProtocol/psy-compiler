use std::fmt;

use logos::Logos;

use crate::error::Error;

#[derive(Logos, Clone, Debug, PartialEq)]
#[logos(skip r"[\s\t\r\n\f]+", error = Error)]
pub enum Token<'input> {
    #[token("const")]
    KeywordConst,
    #[token("let")]
    KeywordLet,

    #[token("mut")]
    KeywordMut,

    #[token("fn")]
    KeywordFn,
    #[token("struct")]
    KeywordStruct,
    #[token("enum")]
    KeywordEnum,
    #[token("impl")]
    KeywordImpl,
    #[token("trait")]
    KeywordTrait,
    #[token("return")]
    KeywordReturn,
    #[token("if")]
    KeywordIf,
    #[token("match")]
    KeywordMatch,
    #[token("else")]
    KeywordElse,
    #[token("while")]
    KeywordWhile,
    #[token("for")]
    KeywordFor,
    #[token("in")]
    KeywordIn,
    #[token("where")]
    KeywordWhere,

    #[token("as")]
    KeywordAs,
    #[token("type")]
    KeywordType,

    #[token("assert")]
    IntrinsicAssert,
    #[token("assert_eq")]
    IntrinsicAssertEq,
    #[token("hash")]
    IntrinsicHash,
    #[token("keccak256")]
    IntrinsicKeccak256,
    #[token("hash_two_to_one")]
    IntrinsicHashTwoToOne,
    #[token("__mem_transmute")]
    IntrinsicMemTransmute,
    #[token("__mem_size_of")]
    IntrinsicMemSizeOf,
    #[token("__storage_read")]
    IntrinsicStorageRead,
    #[token("__storage_write")]
    IntrinsicStorageWrite,
    #[token("__storage_read_range")]
    IntrinsicStorageReadRange,
    #[token("__storage_write_range")]
    IntrinsicStorageWriteRange,
    #[token("__ctx_get_user_id")]
    IntrinsicCtxGetUserId,
    #[token("__ctx_get_contract_id")]
    IntrinsicCtxGetContractId,
    #[token("__ctx_get_contract_deployer")]
    IntrinsicCtxGetContractDeployer,
    #[token("__ctx_get_contract_state_tree_height")]
    IntrinsicCtxGetContractStateTreeHeight,
    #[token("__ctx_get_caller_contract_id")]
    IntrinsicCtxGetCallerContractId,
    #[token("__ctx_get_checkpoint_id")]
    IntrinsicCtxGetCheckpointId,
    #[token("__ctx_get_checkpoint_stats")]
    IntrinsicCtxGetCheckpointStats,
    #[token("__ctx_get_register_users_root")]
    IntrinsicCtxGetRegisterUsersRoot,
    #[token("__ctx_get_gutas_root")]
    IntrinsicCtxGetGutasRoot,
    #[token("__ctx_get_checkpoint_user_tree_root")]
    IntrinsicCtxGetCheckpointUserTreeRoot,
    #[token("__ctx_get_checkpoint_contract_tree_root")]
    IntrinsicCtxGetCheckpointContractTreeRoot,
    #[token("__ctx_get_checkpoint_deposit_tree_root")]
    IntrinsicCtxGetCheckpointDepositTreeRoot,
    #[token("__ctx_get_checkpoint_withdrawal_tree_root")]
    IntrinsicCtxGetCheckpointWithdrawalTreeRoot,
    #[token("__ctx_get_checkpoint_user_registration_tree_root")]
    IntrinsicCtxGetCheckpointUserRegistrationTreeRoot,
    #[token("__ctx_get_deploy_contracts_root")]
    IntrinsicCtxGetDeployContractsRoot,
    #[token("__ctx_get_last_nonce")]
    IntrinsicCtxGetLastNonce,
    #[token("__ctx_get_user_public_key_hash")]
    IntrinsicCtxGetUserPublicKeyHash,
    #[token("__ctx_get_state_hash_at")]
    IntrinsicCtxGetStateHashAt,
    #[token("__ctx_get_other_contract_state_hash_at")]
    IntrinsicCtxGetOtherContractStateHashAt,
    #[token("__ctx_get_other_user_contract_state_hash_at")]
    IntrinsicCtxGetOtherUserContractStateHashAt,
    #[token("__ctx_set_state_hash_at")]
    IntrinsicCtxSetStateHashAt,
    #[token("__imt_get")]
    IntrinsicImtGet,
    #[token("__imt_get_other_user")]
    IntrinsicImtGetOtherUser,
    #[token("__imt_contains_other_user")]
    IntrinsicImtContainsOtherUser,
    #[token("__imt_set")]
    IntrinsicImtSet,
    #[token("__imt_contains")]
    IntrinsicImtContains,
    #[token("__ctx_get_guta_fees_collected")]
    IntrinsicCtxGetGutaFeesCollected,
    #[token("__ctx_get_da_fees_collected")]
    IntrinsicCtxGetDAFeesCollected,
    #[token("__ctx_get_user_ops_processed")]
    IntrinsicCtxGetUserOpsProcessed,
    #[token("__ctx_get_total_transactions")]
    IntrinsicCtxGetTotalTransactions,
    #[token("__ctx_get_slots_modified")]
    IntrinsicCtxGetSlotsModified,
    #[token("__ctx_get_deploy_contracts_completed")]
    IntrinsicCtxGetDeployContractsCompleted,
    #[token("__ctx_get_register_users_completed")]
    IntrinsicCtxGetRegisterUsersCompleted,
    #[token("__ctx_get_gutas_completed")]
    IntrinsicCtxGetGutasCompleted,
    #[token("__ctx_get_session_proof_tree_root")]
    IntrinsicCtxGetSessionProofTreeRoot,
    #[token("__ctx_clear_entire_tree")]
    IntrinsicCtxClearEntireTree,

    #[token("__invoke_sync")]
    IntrinsicInvokeSync,

    #[token("__invoke_deferred")]
    IntrinsicInvokeDeferred,

    #[token("__secp256k1_verify")]
    IntrinsicSecp256k1Verify,

    #[token("__sum_bits")]
    IntrinsicSumBits,

    #[token("__split_bits")]
    IntrinsicSplitBits,

    #[token("__emit")]
    IntrinsicEmit,

    #[token("extern")]
    KeywordExtern,

    #[token("mod")]
    KeywordMod,
    #[token("use")]
    KeywordUse,
    #[token("self")]
    KeywordSelf,
    #[token("crate")]
    KeywordCrate,
    #[token("super")]
    KeywordSuper,

    #[token("pub")]
    KeywordPub,

    #[token("bool")]
    TypeBool,
    #[token("Felt")]
    TypeFelt,
    #[token("u32")]
    TypeU32,
    #[token("u64")]
    TypeU64,
    #[token("u128")]
    TypeU128,
    #[token("u256")]
    TypeU256,
    #[token("Array")]
    TypeArray,
    #[token("Self")]
    TypeSelf,

    #[regex("[_a-zA-Z][_0-9a-zA-Z]*", |lex| lex.slice())]
    Ident(&'input str),

    // Bare decimals and hex are Felt literals; suffix tokens claim their
    // typed spellings by longest match.
    #[regex(r"(?:0|[1-9]\d*|0[xX][0-9a-fA-F]+)", |lex| parse_bare_felt(lex.slice()), priority = 3)]
    FeltLiteral(u64),
    #[token("false", |_| false)]
    #[token("true", |_| true)]
    Bool(bool),
    #[regex(r"(?:0|[1-9]\d*)u32", |lex| lex.slice().strip_suffix("u32").expect("invalid u32 literal suffix").parse::<u32>())]
    U32(u32),
    #[regex(r"(?:0|[1-9]\d*|0[xX][0-9a-fA-F]+)u64", |lex| parse_suffixed_u64(lex.slice()))]
    U64(u64),
    #[regex(r"(?:0|[1-9]\d*|0[xX][0-9a-fA-F]+)u128", |lex| parse_uint::<4>(lex.slice(), "u128"))]
    U128([u32; 4]),
    #[regex(r"(?:0|[1-9]\d*|0[xX][0-9a-fA-F]+)u256", |lex| parse_uint::<8>(lex.slice(), "u256"))]
    U256([u32; 8]),
    #[regex(r#""([^"\\]|\\.)*""#, |lex| lex.slice().strip_prefix('"').unwrap().strip_suffix('"').unwrap())]
    String(&'input str),

    #[token("#")]
    Pound,
    #[token("_", priority = 3)]
    Placeholder,

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("=")]
    Assign,
    #[token(";")]
    Semicolon,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("..")]
    DoubleDot,
    #[token(":")]
    Colon,
    #[token("::")]
    DoubleColon,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,

    #[token("+")]
    OperatorAdd,
    #[token("-")]
    OperatorSub,
    #[token("*")]
    OperatorMul,
    #[token("/")]
    OperatorDiv,
    #[token("**")]
    OperatorPow,
    #[token("%")]
    OperatorMod,

    #[token("==")]
    OperatorEq,
    #[token("!=")]
    OperatorNeq,
    #[token("<")]
    OperatorLt,
    #[token("<=")]
    OperatorLte,
    #[token(">")]
    OperatorGt,
    #[token(">=")]
    OperatorGte,
    #[token("&&")]
    OperatorAnd,
    #[token("||")]
    OperatorOr,

    #[token("&")]
    OperatorBitAnd,
    #[token("|")]
    OperatorBitOr,
    #[token("^")]
    OperatorBitXor,

    #[token("<<")]
    OperatorShl,
    #[token(">>")]
    OperatorShr,

    #[token("!")]
    OperatorNot,
    #[token("+=")]
    OperatorAddAssign,
    #[token("-=")]
    OperatorSubAssign,
    #[token("*=")]
    OperatorMulAssign,
    #[token("/=")]
    OperatorDivAssign,
    #[token("%=")]
    OperatorModAssign,

    #[token("|=")]
    OperatorBitOrAssign,
    #[token("&=")]
    OperatorBitAndAssign,
    #[token("^=")]
    OperatorBitXorAssign,

    #[token("<<=")]
    OperatorBitShlAssign,

    #[token(">>=")]
    OperatorBitShrAssign,

    #[regex(r"//[^\n\r]*", |lex| lex.slice())]
    LineComment(&'input str), // single line comment

    #[regex(r"/\*[^*]*\*+(?:[^/*][^*]*\*+)*/", |lex| lex.slice())]
    BlockComment(&'input str), // block comment
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[test]
fn test_lex_comment() {
    let mut lex = Token::lexer("// This is a line comment\nlet x = 5;");
    assert_eq!(lex.next(), Some(Ok(Token::LineComment("// This is a line comment"))));
    assert_eq!(lex.next(), Some(Ok(Token::KeywordLet)));
    assert_eq!(lex.next(), Some(Ok(Token::Ident("x"))));
    assert_eq!(lex.next(), Some(Ok(Token::Assign)));
    assert_eq!(lex.next(), Some(Ok(Token::FeltLiteral(5))));
    assert_eq!(lex.next(), Some(Ok(Token::Semicolon)));
    assert_eq!(lex.next(), None);

    let mut lex = Token::lexer("/* This is a\nblock comment */\nlet y = 10;");
    assert_eq!(lex.next(), Some(Ok(Token::BlockComment("/* This is a\nblock comment */"))));
    assert_eq!(lex.next(), Some(Ok(Token::KeywordLet)));
    assert_eq!(lex.next(), Some(Ok(Token::Ident("y"))));
    assert_eq!(lex.next(), Some(Ok(Token::Assign)));
    assert_eq!(lex.next(), Some(Ok(Token::FeltLiteral(10))));
    assert_eq!(lex.next(), Some(Ok(Token::Semicolon)));
    assert_eq!(lex.next(), None);
}

#[test]
fn test_lex_integer() {
    let mut lex = Token::lexer("42 0 1234567890");
    assert_eq!(lex.next(), Some(Ok(Token::FeltLiteral(42))));
    assert_eq!(lex.next(), Some(Ok(Token::FeltLiteral(0))));
    assert_eq!(lex.next(), Some(Ok(Token::FeltLiteral(1234567890))));
    assert_eq!(lex.next(), None);
}

#[test]
fn test_lex_suffixed_uint_literals() {
    let mut lex = Token::lexer("1u32 123u64 0xffffffffffffffffu64 1u128 1u256");
    assert_eq!(lex.next(), Some(Ok(Token::U32(1))));
    assert_eq!(lex.next(), Some(Ok(Token::U64(123))));
    assert_eq!(lex.next(), Some(Ok(Token::U64(u64::MAX))));
    assert_eq!(lex.next(), Some(Ok(Token::U128([1, 0, 0, 0]))));
    assert_eq!(lex.next(), Some(Ok(Token::U256([1, 0, 0, 0, 0, 0, 0, 0]))));
    assert_eq!(lex.next(), None);
}

#[test]
fn test_lex_uint_literal_overflow_is_rejected() {
    // 2^64 as a u64 literal must be a lex error, not a silent wrap.
    let mut lex = Token::lexer("18446744073709551616u64");
    assert!(lex.next().unwrap().is_err());
    let mut lex = Token::lexer("340282366920938463463374607431768211456u128");
    assert!(lex.next().unwrap().is_err());
    let mut lex = Token::lexer("18446744073709551616");
    // Bare literals are limited to the historical u64 input range.
    assert!(lex.next().unwrap().is_err());
    assert_eq!(lex.next(), None);
}

#[test]
fn test_lex_uint_type_keywords() {
    let mut lex = Token::lexer("u32 u64 u128 u256");
    assert_eq!(lex.next(), Some(Ok(Token::TypeU32)));
    assert_eq!(lex.next(), Some(Ok(Token::TypeU64)));
    assert_eq!(lex.next(), Some(Ok(Token::TypeU128)));
    assert_eq!(lex.next(), Some(Ok(Token::TypeU256)));
    assert_eq!(lex.next(), None);
}
#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{self, Read},
        path::PathBuf,
    };

    use super::*;
    use crate::Token;

    #[test]
    fn token_display_matches_debug_output() {
        // Format through a trait object so `Display::fmt` executes out-of-line.
        let token: &dyn fmt::Display = &Token::KeywordLet;
        assert_eq!(format!("{token}"), "KeywordLet");
        let token: &dyn fmt::Display = &Token::Ident("counter");
        assert_eq!(format!("{token}"), "Ident(\"counter\")");
    }

    #[test]
    fn test_lex_from_file() -> io::Result<()> {
        // 1. read file content
        let mut file = File::open(PathBuf::from("../tests/struct_fn_call_test.psy"))?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;

        // 2. create Lexer
        let mut lexer = Token::lexer(&content);

        // 3. print Token table header
        println!("{:<10} | {:<20} | {:<10}", "Start", "Token", "End");
        println!("{:-<10}-+-{:-<20}-+-{:-<10}", "-", "-", "-");

        // 4. recursive Lexer output Token
        while let Some(token) = lexer.next() {
            let span = lexer.span();
            let start = span.start;
            let end = span.end;

            println!("{:<10} | {:<20} | {:<10}", start, format!("{:?}", token), end);
        }

        Ok(())
    }
}

// Parse directly into limbs: no intermediate field reduction or u64 truncation.
fn parse_uint<const N: usize>(source: &str, suffix: &str) -> std::result::Result<[u32; N], crate::Error> {
    let source = source.strip_suffix(suffix).unwrap_or(source);
    source
        .parse::<psy_vm::dpn::ops::uint::Uint<N>>()
        .map(|value| value.0)
        .map_err(crate::Error::InvalidWideInteger)
}

fn parse_suffixed_u64(source: &str) -> std::result::Result<u64, crate::Error> {
    let limbs = parse_uint::<2>(source, "u64")?;
    Ok(limbs[0] as u64 | ((limbs[1] as u64) << 32))
}

fn parse_bare_felt(source: &str) -> std::result::Result<u64, crate::Error> {
    let limbs = parse_uint::<2>(source, "")?;
    Ok(limbs[0] as u64 | ((limbs[1] as u64) << 32))
}

#[cfg(test)]
mod u256_tests {
    use super::*;

    #[test]
    fn u256_literal_preserves_full_width_and_rejects_overflow() {
        let maximum = "115792089237316195423570985008687907853269984665640564039457584007913129639935u256";
        assert_eq!(Token::lexer(maximum).next(), Some(Ok(Token::U256([u32::MAX; 8]))));
        assert_eq!(Token::lexer("0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffu256").next(), Some(Ok(Token::U256([u32::MAX; 8]))));
        assert!(Token::lexer("115792089237316195423570985008687907853269984665640564039457584007913129639936u256").next().unwrap().is_err());
        assert_eq!(Token::lexer("4294967296u256").next(), Some(Ok(Token::U256([0, 1, 0, 0, 0, 0, 0, 0]))));
    }
}
