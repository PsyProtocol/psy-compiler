#![allow(unused)]

use std::{clone::Clone, collections::HashMap, ops::Deref};

use num_bigint::BigUint;
use plonky2::field::{goldilocks_field::GoldilocksField, types::Field};
use psy_ast::{ModuleId, VisitorContext};
use psy_common::data::qhashout::QHashOut;
use psy_common_circuit::circuits::zk_signature3::manager::SimplePsyZKSignatureManager;
use psy_crypto::signature::zk::wallet::SimplePsyPrivateKey;
use psy_data::{
    config::store_config::{PsyHasher, C, D},
    qblock::cmds::register_user::QBCRegisterUser,
};
use psy_package::Workspace;
use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
use psy_sema::{CheckedFunctionNode, Implementer, TypeChecker, TypeCheckerVisitorContext, TypeId, TypeKey};
use psy_vm::{
    dpn::{
        ops::{exec_context::QExecContext, sym_felt::SymFeltRef},
        vm::def::DPNFunctionCircuitDefinition,
    },
    vm::exec::PsyEvalSessionResult,
};

use crate::cli::{compile_cmd::compile_workspace_full, execute_cmd::ExecuteCommand, test_helpers::prepare_environment_with_real_contract};

pub fn find_contract_method_by_name(
    ctx: &mut TypeCheckerVisitorContext<SymFeltRef, QExecContext>,
    typechecker: &mut TypeChecker<SymFeltRef, QExecContext>,
    method_name: String,
) -> Option<TypeId> {
    let scope_id = ctx.symbols[ModuleId::root()].scope_id;
    let method_ident_id = ctx.intern(method_name);
    ctx.symbols[scope_id].types.get::<TypeKey>(&method_ident_id.into()).cloned()
}

pub fn extract_function_metadata_from_context(
    ctx: &mut TypeCheckerVisitorContext<SymFeltRef, QExecContext>,
    typechecker: &mut TypeChecker<SymFeltRef, QExecContext>,
    compile_results: &[DPNFunctionCircuitDefinition],
) -> HashMap<String, FunctionNode> {
    let mut metadata_map = HashMap::new();

    for result in compile_results {
        if let Some(type_id) = find_contract_method_by_name(ctx, typechecker, result.name.clone()) {
            if let Some(func) = ctx.symbols[type_id].as_function() {
                let fun = FunctionNode(func.clone());
                if fun.is_input_comment() {
                    metadata_map.insert(result.name.clone(), fun);
                }
            }
        }
    }

    metadata_map
}

pub fn convert_param_to_field(param: &CommentParamValue) -> GoldilocksField {
    match param {
        CommentParamValue::Bool(val) => {
            if *val {
                GoldilocksField::ONE
            } else {
                GoldilocksField::ZERO
            }
        }
        CommentParamValue::U32(val) => GoldilocksField::from_noncanonical_u64(*val as u64),
        CommentParamValue::Felt(val) => GoldilocksField::from_noncanonical_biguint(val.clone()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommentParamValue {
    Bool(bool),
    U32(u32),
    Felt(BigUint),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment(psy_ast::Comment);

impl Comment {
    pub fn is_input_comment(&self) -> bool {
        let content = self.content().trim();
        let content = content.strip_prefix("//").unwrap_or(content).trim();
        content.starts_with("input:")
    }

    pub fn is_output_comment(&self) -> bool {
        let content = self.content().trim();
        let content = content.strip_prefix("//").unwrap_or(content).trim();
        content.starts_with("output:")
    }

    pub fn is_metadata_comment(&self, key: &str) -> bool {
        let content = self.content().trim();
        let content = content.strip_prefix("//").unwrap_or(content).trim();
        content.starts_with(&format!("{}:", key))
    }

    pub fn parse_input_values(&self) -> Vec<CommentParamValue> {
        if !self.is_input_comment() {
            return Vec::new();
        }
        let content = self.content().trim();
        let content = content.strip_prefix("//").unwrap_or(content).trim();
        let input_part = content.strip_prefix("input:").unwrap_or("").trim();

        input_part
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| self.parse_value(s))
            .collect()
    }

    pub fn parse_output_values(&self) -> Vec<CommentParamValue> {
        if !self.is_output_comment() {
            return Vec::new();
        }

        let content = self.content().trim();
        let content = content.strip_prefix("//").unwrap_or(content).trim();
        let output_part = content.strip_prefix("output:").unwrap_or("").trim();

        output_part
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| self.parse_value(s))
            .collect()
    }

    fn parse_value(&self, value_str: &str) -> CommentParamValue {
        if value_str.eq_ignore_ascii_case("true") {
            return CommentParamValue::Bool(true);
        } else if value_str.eq_ignore_ascii_case("false") {
            return CommentParamValue::Bool(false);
        }

        if value_str.starts_with("0x") || value_str.starts_with("0X") {
            if let Some(v) = BigUint::parse_bytes(&value_str[2..].as_bytes(), 16) {
                return CommentParamValue::Felt(v);
            }
        }

        if let Ok(val) = value_str.parse::<u32>() {
            return CommentParamValue::U32(val);
        }

        match value_str.parse::<BigUint>() {
            Ok(v) => CommentParamValue::Felt(v),
            Err(_) => CommentParamValue::Felt(BigUint::from(0u32)),
        }
    }

    pub fn parse_metadata_content(&self, key: &str) -> Option<String> {
        let content = self.content().trim();
        let content = content.strip_prefix("//").unwrap_or(content).trim();

        let prefix = format!("{}:", key);
        if !content.starts_with(&prefix) {
            return None;
        }

        Some(content.strip_prefix(&prefix).unwrap_or("").trim().to_string())
    }
}

impl From<psy_ast::Comment> for Comment {
    fn from(comment: psy_ast::Comment) -> Self {
        Self(comment)
    }
}

impl Deref for Comment {
    type Target = psy_ast::Comment;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionNode(CheckedFunctionNode);

impl FunctionNode {
    pub fn is_input_comment(&self) -> bool {
        self.comments
            .clone()
            .into_iter()
            .find(|c| Comment::from(c.clone()).is_input_comment())
            .is_some()
    }

    pub fn get_input_parameters_from_comments(&self) -> Vec<CommentParamValue> {
        self.comments
            .clone()
            .into_iter()
            .filter(|c| Comment::from(c.clone()).is_input_comment())
            .flat_map(|c| Comment::from(c).parse_input_values())
            .collect()
    }

    pub fn get_output_expectations_from_comments(&self) -> Vec<CommentParamValue> {
        self.comments
            .clone()
            .into_iter()
            .filter(|c| Comment::from(c.clone()).is_output_comment())
            .flat_map(|c| Comment::from(c).parse_output_values())
            .collect()
    }

    pub fn get_metadata(&self, key: &str) -> Option<String> {
        self.comments
            .clone()
            .into_iter()
            .filter(|c| Comment::from(c.clone()).is_metadata_comment(key))
            .find_map(|c| Comment::from(c).parse_metadata_content(key))
    }

    pub fn get_all_metadata(&self) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        let known_keys = ["description", "author", "version", "tags", "complexity", "example"];

        for key in known_keys.iter() {
            if let Some(value) = self.get_metadata(key) {
                metadata.insert(key.to_string(), value);
            }
        }
        for comment in &self.comments {
            let comment = Comment::from(comment.clone());
            let content = comment.content().trim();
            let content = content.strip_prefix("//").unwrap_or(content).trim();
            if let Some(colon_idx) = content.find(':') {
                let potential_key = &content[..colon_idx].trim();
                if !known_keys.contains(&potential_key)
                    && !["input", "output"].contains(&potential_key)
                    && potential_key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    if let Some(value) = comment.parse_metadata_content(potential_key) {
                        metadata.insert(potential_key.to_string(), value);
                    }
                }
            }
        }

        metadata
    }
}

impl Deref for FunctionNode {
    type Target = CheckedFunctionNode;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub(crate) async fn run_doc(args: ExecuteCommand, workspace: Workspace) -> crate::errors::Result<()> {
    // Compile the full workspace in order to generate any build artifacts.
    let compilation_result = compile_workspace_full(&workspace, &args.compile_options)?;
    let mut comment_input_parameters = HashMap::with_capacity(compilation_result.circuit_definitions.len());
    let mut comment_output_parameters = HashMap::with_capacity(compilation_result.circuit_definitions.len());
    for result in &compilation_result.circuit_definitions {
        if let Some(fun) = compilation_result.function_metadata.get(&result.name) {
            let input_params = fun.get_input_parameters_from_comments();
            let field_params = input_params.iter().map(|p| convert_param_to_field(p)).collect::<Vec<_>>();

            comment_input_parameters.insert(result.name.clone(), field_params);
            comment_output_parameters.insert(
                result.name.clone(),
                fun.get_output_expectations_from_comments()
                    .iter()
                    .map(|p| convert_param_to_field(p))
                    .collect::<Vec<_>>(),
            );

            if args.compile_options.debug {
                println!("Function: {}", result.name);
                let function_metadata = fun.get_all_metadata();
                if !function_metadata.is_empty() {
                    println!("Metadata:");
                    for (key, value) in function_metadata {
                        println!("  {}: {}", key, value);
                    }
                }
            }
        }
    }
    let priv_key = QHashOut::rand();
    let wallet = SimplePsyZKSignatureManager::<C, D>::new();
    let priv_key_w = SimplePsyPrivateKey::new(priv_key);
    let pub_key_param = priv_key_w.get_public_key_param::<PsyHasher>();
    let contract_state_tree_height = compilation_result.state_tree_height as usize;

    let deployer = QHashOut::rand();
    let (circuits, deploy_cmd) =
        gen_contract_deploy_and_circuits_for_functions::<C, D>(deployer, contract_state_tree_height as u8, &compilation_result.circuit_definitions)?;

    let mut lps = prepare_environment_with_real_contract(
        vec![QBCRegisterUser::new(wallet.get_zksig_circuit_fingerprint(), pub_key_param)],
        vec![deploy_cmd],
        None,
        None,
        None,
    )
    .await?;

    for (def, circuit) in compilation_result.circuit_definitions.into_iter().zip(circuits.into_iter()) {
        if let Some(param) = comment_input_parameters.get(&def.name) {
            let cfc_input = PsyEvalSessionResult::new()
                .exec_contract_call(&mut lps, GoldilocksField::from_canonical_u64(2), &def, param.clone())
                .await?;
            if let Some(output_param) = comment_output_parameters.get(&def.name).cloned() {
                if !output_param.is_empty() {
                    assert_eq!(
                        output_param, cfc_input.outputs,
                        "file:{:?}, expected output: {:?}, actual output: {:?}",
                        args.compile_options.entry_path, output_param, cfc_input.outputs
                    );
                }
            }
            let proof = circuit.prove_base(&cfc_input).unwrap();
            if args.compile_options.debug {
                println!("file:{:?}, input: {:?}", args.compile_options.entry_path, param);
                println!("result_vm input: {:?}", cfc_input.inputs);
                println!("result_vm output: {:?}", cfc_input.outputs);
                println!("public_inputs: {:?}", &proof.public_inputs);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, default::Default, path::PathBuf};

    use num_traits::Num;
    use plonky2::field::fft::ifft;
    use psy_ast::{IdentId, Identifier, Location, ModuleNode, Qualifier, Visibility};
    use psy_package::{CrateName, Package, PackageType};
    use psy_sema::{ScopeId, Type};

    use super::*;
    use crate::cli::compile_cmd::CompileOptions;

    fn line_comment(content: &str) -> psy_ast::Comment {
        psy_ast::Comment::new_line(content.to_string(), Location::default())
    }

    fn function_node_with_comments(comments: Vec<psy_ast::Comment>) -> FunctionNode {
        FunctionNode(CheckedFunctionNode {
            name: Identifier::new(IdentId(0), Location::default()),
            parameters: vec![],
            generic_parameters: vec![],
            body: None,
            qualifier: Qualifier::default(),
            return_type: TypeId(1),
            return_type_path: None,
            scope_id: ScopeId(0),
            visibility: Visibility::Public,
            attrs: vec![],
            type_id: TypeId(1),
            comments,
            location: Location::default(),
        })
    }

    fn circuit_named(name: &str) -> DPNFunctionCircuitDefinition {
        DPNFunctionCircuitDefinition {
            name: name.to_string(),
            method_id: 0,
            circuit_inputs: vec![],
            circuit_outputs: vec![],
            state_commands: vec![],
            state_command_resolution_indices: vec![],
            assertions: vec![],
            definitions: vec![],
            events: vec![],
        }
    }

    #[test]
    fn comment_classification_matches_marker_prefixes_with_and_without_slashes() {
        let with_slashes = Comment::from(line_comment("// input: 1"));
        assert!(with_slashes.is_input_comment());
        assert!(!with_slashes.is_output_comment());

        let without_slashes = Comment::from(line_comment("output: true"));
        assert!(without_slashes.is_output_comment());
        assert!(!without_slashes.is_input_comment());

        let metadata = Comment::from(line_comment("// description: records a transfer"));
        assert!(metadata.is_metadata_comment("description"));
        assert!(!metadata.is_metadata_comment("author"));
        assert!(!Comment::from(line_comment("plain commentary")).is_metadata_comment("description"));
    }

    #[test]
    fn parse_value_covers_bools_hex_decimals_and_the_fallback() {
        let comment = Comment::from(line_comment("input: ignored"));
        assert_eq!(comment.parse_value("TRUE"), CommentParamValue::Bool(true));
        assert_eq!(comment.parse_value("False"), CommentParamValue::Bool(false));
        assert_eq!(comment.parse_value("0x10"), CommentParamValue::Felt(BigUint::from(16u32)));
        // Invalid hex digits fall through every parser to the zero fallback.
        assert_eq!(comment.parse_value("0xzz"), CommentParamValue::Felt(BigUint::from(0u32)));
        assert_eq!(comment.parse_value("42"), CommentParamValue::U32(42));
        let big = BigUint::from_str_radix("99999999999999999999", 10).unwrap();
        assert_eq!(comment.parse_value("99999999999999999999"), CommentParamValue::Felt(big));
        assert_eq!(comment.parse_value("-7"), CommentParamValue::Felt(BigUint::from(0u32)));
        assert_eq!(comment.parse_value("junk"), CommentParamValue::Felt(BigUint::from(0u32)));
    }

    #[test]
    fn input_and_output_parsing_skip_blank_entries_and_require_markers() {
        let comment = Comment::from(line_comment("input: 1, , 2 ,"));
        assert_eq!(
            comment.parse_input_values(),
            vec![CommentParamValue::U32(1), CommentParamValue::U32(2)]
        );

        let comment = Comment::from(line_comment("output: true"));
        assert_eq!(comment.parse_output_values(), vec![CommentParamValue::Bool(true)]);

        assert!(Comment::from(line_comment("1, 2")).parse_input_values().is_empty());
        assert!(Comment::from(line_comment("input: 1")).parse_output_values().is_empty());
        assert!(Comment::from(line_comment("input:")).parse_input_values().is_empty());
    }

    #[test]
    fn metadata_content_returns_the_trimmed_value_for_the_requested_key() {
        let comment = Comment::from(line_comment("// description:  keeps contracts documented  "));
        assert_eq!(comment.parse_metadata_content("description").as_deref(), Some("keeps contracts documented"));
        assert_eq!(comment.parse_metadata_content("author"), None);
    }

    #[test]
    fn function_nodes_aggregate_comment_inputs_outputs_and_metadata() {
        let documented = function_node_with_comments(vec![
            line_comment("input: 1"),
            line_comment("input: true"),
            line_comment("// output: 0x10"),
            line_comment("description: sums the grid"),
            line_comment("author: psy"),
            line_comment("priority: high"),
            line_comment("foo-bar: dropped key"),
            line_comment("plain note"),
        ]);
        assert!(documented.is_input_comment());
        assert_eq!(
            documented.get_input_parameters_from_comments(),
            vec![CommentParamValue::U32(1), CommentParamValue::Bool(true)]
        );
        assert_eq!(
            documented.get_output_expectations_from_comments(),
            vec![CommentParamValue::Felt(BigUint::from(16u32))]
        );
        assert_eq!(documented.get_metadata("description").as_deref(), Some("sums the grid"));
        assert_eq!(documented.get_metadata("missing"), None);

        let metadata = documented.get_all_metadata();
        assert_eq!(metadata.get("description").map(String::as_str), Some("sums the grid"));
        assert_eq!(metadata.get("author").map(String::as_str), Some("psy"));
        assert_eq!(metadata.get("priority").map(String::as_str), Some("high"));
        assert!(!metadata.contains_key("input"));
        assert!(!metadata.contains_key("output"));
        assert!(!metadata.contains_key("foo-bar"));

        let silent = function_node_with_comments(vec![line_comment("no markers here")]);
        assert!(!silent.is_input_comment());
        assert!(silent.get_input_parameters_from_comments().is_empty());
        assert!(silent.get_output_expectations_from_comments().is_empty());
        assert!(silent.get_metadata("description").is_none());
        assert!(silent.get_all_metadata().is_empty());
    }

    #[test]
    fn param_conversion_covers_every_comment_value_kind() {
        assert_eq!(convert_param_to_field(&CommentParamValue::Bool(true)), GoldilocksField::ONE);
        assert_eq!(convert_param_to_field(&CommentParamValue::Bool(false)), GoldilocksField::ZERO);
        assert_eq!(convert_param_to_field(&CommentParamValue::U32(7)), GoldilocksField::from_noncanonical_u64(7));
        let felt = BigUint::from_str_radix("123456789abcdef", 16).unwrap();
        assert_eq!(
            convert_param_to_field(&CommentParamValue::Felt(felt.clone())),
            GoldilocksField::from_noncanonical_biguint(felt)
        );
    }

    #[test]
    fn metadata_extraction_keeps_only_input_commented_methods_present_in_symbols() {
        use psy_common::FileId;
        use psy_common::tree::TreeNode;
        use psy_sema::CheckedProgram;

        let program = psy_ast::Program::<SymFeltRef>::new();
        let mut ctx = TypeCheckerVisitorContext::<SymFeltRef, QExecContext>::new(program);
        let mut typechecker = TypeChecker::new(
            CheckedProgram::new(),
            Box::new(psy_interpreter::Interpreter::<SymFeltRef, QExecContext>::new(QExecContext::new())),
        );

        // Seed the root module so lookups against ModuleId::root() resolve.
        let root = TreeNode::new(
            ModuleId::root(),
            ModuleNode {
                name: Identifier::new(IdentId(0), Location::default()),
                file_id: FileId(0),
                modules: vec![],
                inline_modules: vec![],
                definitions: vec![],
                visibility: Visibility::Public,
                comments: vec![],
                location: Location::default(),
            },
        );
        ctx.symbols.load_modules(std::iter::once(&root));

        let documented = function_node_with_comments(vec![line_comment("input: 1")]).0;
        let priced_ident = ctx.intern("priced");
        ctx.symbols
            .add_type(Some(ScopeId(0)), priced_ident, Type::Function(documented))
            .expect("documented function type registers");
        let silent = function_node_with_comments(vec![]);
        let silent_ident = ctx.intern("silent");
        ctx.symbols
            .add_type(Some(ScopeId(0)), silent_ident, Type::Function(silent.0))
            .expect("silent function type registers");

        let compile_results = vec![circuit_named("priced"), circuit_named("silent"), circuit_named("ghost")];
        let metadata = extract_function_metadata_from_context(&mut ctx, &mut typechecker, &compile_results);
        assert_eq!(metadata.len(), 1, "only the input-commented contract method is kept: {metadata:?}");
        assert!(metadata.contains_key("priced"));

        assert!(find_contract_method_by_name(&mut ctx, &mut typechecker, "priced".to_string()).is_some());
        assert!(find_contract_method_by_name(&mut ctx, &mut typechecker, "ghost".to_string()).is_none());
    }

    #[test]
    fn test_parse_input_values() {
        let comment = Comment::from(psy_ast::Comment::new_line("input: 1, 2, 3".to_string(), Location::default()));
        let input_values = comment.parse_input_values();
        assert_eq!(
            input_values,
            vec![CommentParamValue::U32(1), CommentParamValue::U32(2), CommentParamValue::U32(3)]
        );
        let comment = Comment::from(psy_ast::Comment::new_line("input: true, false, 10".to_string(), Location::default()));
        let input_values = comment.parse_input_values();
        assert_eq!(
            input_values,
            vec![CommentParamValue::Bool(true), CommentParamValue::Bool(false), CommentParamValue::U32(10)]
        );
        let comment = Comment::from(psy_ast::Comment::new_line(
            "input: 0x123456789abcdef, 0xabcdef123456789".to_string(),
            Location::default(),
        ));
        let input_values = comment.parse_input_values();
        assert_eq!(
            input_values,
            vec![
                CommentParamValue::Felt(BigUint::from_str_radix("123456789abcdef", 16).unwrap()),
                CommentParamValue::Felt(BigUint::from_str_radix("abcdef123456789", 16).unwrap()),
            ]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "slow end-to-end circuit generation; run with `make test-slow`"]
    async fn test_doc() {
        insta::glob!("../../../tests", "*_test.psy", |path| {
            let source = std::fs::read_to_string(path).expect("test fixture should be readable");
            if !source.contains("input:") && !source.contains("output:") {
                return;
            }
            let workspace = Workspace {
                root_dir: PathBuf::from("../../../tests"),
                target_dir: PathBuf::from("../../../target"),
                package: Package {
                    version: Some("0.0.1".to_string()),
                    root_dir: PathBuf::from("../../../tests"),
                    entry_path: path.into(),
                    ..Default::default()
                },
            };

            let args = ExecuteCommand {
                compile_options: CompileOptions {
                    entry_path: Some(path.into()),
                    method_names: Some(vec!["main".to_string()]),
                    debug: true,
                    ..Default::default()
                },
                parameters: vec![],
                doc: true,
            };
            if let Err(err) = tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(run_doc(args, workspace))) {
                panic!("file: {:?}, {:?}", path, err);
            }
            #[allow(static_mut_refs)]
            unsafe {
                psy_sema::STD_PRIMITIVE_SCOPE_ID.take()
            };
        });
    }
}
