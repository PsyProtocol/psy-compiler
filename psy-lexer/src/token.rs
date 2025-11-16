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
    #[token("__ctx_get_fees_collected")]
    IntrinsicCtxGetFeesCollected,
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

    #[token("new")]
    KeywordNew,
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
    #[token("Array")]
    TypeArray,
    #[token("Self")]
    TypeSelf,

    #[regex("[_a-zA-Z][_0-9a-zA-Z]*", |lex| lex.slice())]
    Ident(&'input str),

    #[regex(r"(?:0|[1-9]\d*)", |lex| lex.slice().parse())]
    U64(u64),
    #[token("false", |_| false)]
    #[token("true", |_| true)]
    Bool(bool),
    #[regex(r"(?:0|[1-9]\d*)u32", |lex| lex.slice().strip_suffix("u32").expect("invalid u32 literal suffix").parse::<u32>())]
    U32(u32),
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
    assert_eq!(lex.next(), Some(Ok(Token::U64(5))));
    assert_eq!(lex.next(), Some(Ok(Token::Semicolon)));
    assert_eq!(lex.next(), None);

    let mut lex = Token::lexer("/* This is a\nblock comment */\nlet y = 10;");
    assert_eq!(lex.next(), Some(Ok(Token::BlockComment("/* This is a\nblock comment */"))));
    assert_eq!(lex.next(), Some(Ok(Token::KeywordLet)));
    assert_eq!(lex.next(), Some(Ok(Token::Ident("y"))));
    assert_eq!(lex.next(), Some(Ok(Token::Assign)));
    assert_eq!(lex.next(), Some(Ok(Token::U64(10))));
    assert_eq!(lex.next(), Some(Ok(Token::Semicolon)));
    assert_eq!(lex.next(), None);
}

#[test]
fn test_lex_integer() {
    let mut lex = Token::lexer("42 0 1234567890");
    assert_eq!(lex.next(), Some(Ok(Token::U64(42))));
    assert_eq!(lex.next(), Some(Ok(Token::U64(0))));
    assert_eq!(lex.next(), Some(Ok(Token::U64(1234567890))));
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
