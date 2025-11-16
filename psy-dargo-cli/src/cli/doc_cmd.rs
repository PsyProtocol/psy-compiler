#![allow(unused)]

use std::{clone::Clone, collections::HashMap, ops::Deref};

use num_bigint::BigUint;
use plonky2::field::{goldilocks_field::GoldilocksField, types::Field};
use psy_ast::{ModuleId, VisitorContext};
use psy_common::data::qhashout::QHashOut;
use psy_common_circuit::circuits::zk_signature3::manager::SimplePsyZKSignatureManager;
use psy_config::network_constants::GLOBAL_USER_TREE_HEIGHT;
use psy_crypto::signature::zk::wallet::SimplePsyPrivateKey;
use psy_data::{
    config::store_config::{PsyHasher, C, D},
    qblock::cmds::register_user::QBCRegisterUser,
};
use psy_package::Workspace;
use psy_prover::session::gen_contract_deploy_and_circuits_for_functions;
use psy_sema::{CheckedFunctionNode, Implementer, TypeChecker, TypeCheckerVisitorContext, TypeId, TypeKey};
use psy_store::prepare_environment_with_real_contract;
use psy_vm::{
    dpn::{
        ops::{exec_context::QExecContext, sym_felt::SymFeltRef},
        vm::def::DPNFunctionCircuitDefinition,
    },
    vm::exec::PsyEvalSessionResult,
};

use crate::cli::{compile_cmd::compile_workspace_full, execute_cmd::ExecuteCommand};

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
    let contract_state_tree_height = GLOBAL_USER_TREE_HEIGHT as usize;

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
    use psy_ast::Location;
    use psy_package::{CrateName, Package, PackageType};

    use super::*;
    use crate::cli::compile_cmd::CompileOptions;

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

    #[tokio::test]
    async fn test_doc() {
        insta::glob!("../../../tests", "*_test.psy", |path| {
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
                    method_names: vec!["main".to_string()],
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
