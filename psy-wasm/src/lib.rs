use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Once},
};

use psy_abi::AbiExtractor;
use psy_common::Graph;
use serde::Serialize;
use wasm_bindgen::prelude::*;

const VIRTUAL_PACKAGE_ROOT: &str = "/virtual_pkg";

#[derive(Serialize)]
struct JsCompileResult {
    success: bool,
    error: Option<String>,
    error_offset: Option<usize>,
    entry_path: Option<String>,
    compile_results: Option<serde_json::Value>,
    abi: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct SerializeFallback<'a> {
    success: bool,
    error: &'a str,
}

static INIT: Once = Once::new();

#[wasm_bindgen(start)]
pub fn main() {
    INIT.call_once(|| {
        console_error_panic_hook::set_once();
        wasm_logger::init(wasm_logger::Config::default());
        wasm_tracing::set_as_global_default();
    });
}

#[wasm_bindgen]
pub fn init_logging() {
    main();
}

#[wasm_bindgen]
pub fn compile_source(source: &str) -> String {
    let entry_path = PathBuf::from(VIRTUAL_PACKAGE_ROOT).join("src/main.psy");
    let files = vec![(entry_path.clone(), source.to_string())];
    compile_virtual_project(files, entry_path)
}

#[wasm_bindgen]
pub fn compile_project(files_json: &str) -> String {
    match parse_project_input(files_json) {
        Ok(project_input) => {
            match build_virtual_files(project_input) {
                Ok((entry_path, method_names, virtual_files)) => {
                    compile_virtual_project_with_methods(virtual_files, entry_path, method_names)
                }
                Err(error) => serialize_result(JsCompileResult {
                    success: false,
                    error: Some(error),
                    error_offset: None,
                    entry_path: None,
                    compile_results: None,
                    abi: None,
                }),
            }
        }
        Err(error) => serialize_result(JsCompileResult {
            success: false,
            error: Some(format!("Invalid files JSON: {error}")),
            error_offset: None,
            entry_path: None,
            compile_results: None,
            abi: None,
        }),
    }
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum ProjectInput {
    Legacy(Vec<(Vec<String>, String)>),
    Explicit {
        entry: Vec<String>,
        #[serde(default)]
        method_names: Option<Vec<String>>,
        files: Vec<(Vec<String>, String)>,
    },
}

struct NormalizedProjectInput {
    entry: Option<Vec<String>>,
    method_names: Option<Vec<String>>,
    files: Vec<(Vec<String>, String)>,
}

fn parse_project_input(files_json: &str) -> Result<NormalizedProjectInput, serde_json::Error> {
    match serde_json::from_str::<ProjectInput>(files_json)? {
        ProjectInput::Legacy(files) => Ok(NormalizedProjectInput {
            entry: None,
            method_names: None,
            files,
        }),
        ProjectInput::Explicit {
            entry,
            method_names,
            files,
        } => Ok(NormalizedProjectInput {
            entry: Some(entry),
            method_names,
            files,
        }),
    }
}

fn compile_virtual_project(files: Vec<(PathBuf, String)>, entry_path: PathBuf) -> String {
    compile_virtual_project_with_methods(files, entry_path, vec!["main".to_string()])
}

fn compile_virtual_project_with_methods(files: Vec<(PathBuf, String)>, entry_path: PathBuf, method_names: Vec<String>) -> String {
    let mut crate_path_graph = Graph::new();
    crate_path_graph.add_node(entry_path.clone());
    let source_index = build_source_index(&files);
    let shared_files = files
        .into_iter()
        .map(|(path, content)| (path, Arc::<str>::from(content)))
        .collect();

    match psy_interpreter::interpret_virtual_files(None, method_names, crate_path_graph, shared_files) {
        Ok(mut result) => {
            let compile_results = match serde_json::to_value(&result.compile_results) {
                Ok(value) => value,
                Err(error) => {
                    return serialize_result(JsCompileResult {
                        success: false,
                        error: Some(format!("Failed to serialize compile results: {error}")),
                        error_offset: None,
                        entry_path: Some(entry_path.display().to_string()),
                        compile_results: None,
                        abi: None,
                    });
                }
            };

            let abi = match extract_abi(&mut result) {
                Ok(value) => Some(value),
                Err(error) => {
                    tracing::warn!("ABI extraction failed: {error}");
                    None
                }
            };

            serialize_result(JsCompileResult {
                success: true,
                error: None,
                error_offset: None,
                entry_path: Some(entry_path.display().to_string()),
                compile_results: Some(compile_results),
                abi,
            })
        }
        Err(error) => {
            let error_text = format!("{error:#}");
            serialize_result(JsCompileResult {
                success: false,
                error: Some(error_text.clone()),
                error_offset: extract_error_offset(&error_text, &source_index),
                entry_path: Some(entry_path.display().to_string()),
                compile_results: None,
                abi: None,
            })
        }
    }
}

fn build_source_index(files: &[(PathBuf, String)]) -> HashMap<String, Arc<str>> {
    files
        .iter()
        .map(|(path, content)| (path.display().to_string(), Arc::<str>::from(content.as_str())))
        .collect()
}

fn build_virtual_files(input: NormalizedProjectInput) -> Result<(PathBuf, Vec<String>, Vec<(PathBuf, String)>), String> {
    if input.files.is_empty() {
        return Err("Project must contain at least one file".to_string());
    }

    let mut first_entry_path = None;
    let mut lib_entry_path = None;
    let mut main_entry_path = None;
    let explicit_entry_path = input.entry.as_ref().map(|parts| module_parts_to_path(parts));
    let mut saw_explicit_entry = explicit_entry_path.is_none();
    let mut virtual_files = Vec::with_capacity(input.files.len());

    for (parts, content) in input.files {
        let path = module_parts_to_path(&parts);
        if explicit_entry_path.as_ref().is_some_and(|entry| *entry == path) {
            saw_explicit_entry = true;
        }
        if first_entry_path.is_none() {
            first_entry_path = Some(path.clone());
        }
        if path.ends_with("lib.psy") && lib_entry_path.is_none() {
            lib_entry_path = Some(path.clone());
        }
        if path.ends_with("main.psy") && main_entry_path.is_none() {
            main_entry_path = Some(path.clone());
        }
        virtual_files.push((path, content));
    }

    if !saw_explicit_entry {
        return Err("Explicit entry file was not found in project files".to_string());
    }

    let entry_path = explicit_entry_path
        .or(main_entry_path)
        .or(lib_entry_path)
        .or(first_entry_path)
        .ok_or_else(|| "Project must contain at least one file".to_string())?;

    let method_names = resolve_method_names(&entry_path, input.method_names)?;

    Ok((entry_path, method_names, virtual_files))
}

fn resolve_method_names(entry_path: &PathBuf, method_names: Option<Vec<String>>) -> Result<Vec<String>, String> {
    if let Some(method_names) = method_names {
        if method_names.is_empty() {
            return Err("method_names must not be empty when provided".to_string());
        }
        return Ok(method_names);
    }

    if entry_path.ends_with("main.psy") {
        return Ok(vec!["main".to_string()]);
    }

    Err("Project entry is not main.psy; provide method_names explicitly".to_string())
}

fn module_parts_to_path(parts: &[String]) -> PathBuf {
    let mut path = PathBuf::from(VIRTUAL_PACKAGE_ROOT).join("src");
    if parts.is_empty() {
        path.push("main.psy");
        return path;
    }

    for (index, part) in parts.iter().enumerate() {
        let is_last = index + 1 == parts.len();
        if is_last {
            if part.ends_with(".psy") {
                path.push(part);
            } else {
                path.push(format!("{part}.psy"));
            }
        } else {
            path.push(part);
        }
    }
    path
}

fn extract_abi(result: &mut psy_interpreter::InterpretResult) -> Result<serde_json::Value, String> {
    let program = &mut result.ctx.program;
    let abi = AbiExtractor::new("contract".to_string())
        .extract_spec_compliant_abi(program)
        .map_err(|error| error.to_string())?;
    serde_json::to_value(abi).map_err(|error| error.to_string())
}

fn extract_error_offset(error_msg: &str, sources: &HashMap<String, Arc<str>>) -> Option<usize> {
    for pattern in ["at offset ", "offset "] {
        if let Some(pos) = error_msg.rfind(pattern) {
            let after = &error_msg[pos + pattern.len()..];
            let number: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(offset) = number.parse::<usize>() {
                return Some(offset);
            }
        }
    }

    extract_error_offset_from_line_col(error_msg, sources)
}

fn extract_error_offset_from_line_col(error_msg: &str, sources: &HashMap<String, Arc<str>>) -> Option<usize> {
    let marker = "[ ";
    let start = error_msg.find(marker)? + marker.len();
    let rest = &error_msg[start..];
    let end = rest.find(" ]")?;
    let location = &rest[..end];
    let (path_text, line, column) = parse_location_triplet(location)?;
    let source = sources.get(&path_text)?;
    line_col_to_offset(source, line, column)
}

fn parse_location_triplet(location: &str) -> Option<(String, usize, usize)> {
    let mut parts = location.rsplitn(3, ':');
    let column = parts.next()?.trim().parse::<usize>().ok()?;
    let line = parts.next()?.trim().parse::<usize>().ok()?;
    let path = parts.next()?.trim().to_string();
    Some((path, line, column))
}

fn line_col_to_offset(source: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }

    let mut current_line = 1usize;
    let mut current_col = 1usize;

    for (offset, ch) in source.char_indices() {
        if current_line == line && current_col == column {
            return Some(offset);
        }

        if ch == '\n' {
            current_line += 1;
            current_col = 1;
        } else {
            current_col += 1;
        }
    }

    if current_line == line && current_col == column {
        return Some(source.len());
    }

    None
}

fn serialize_result(result: JsCompileResult) -> String {
    serde_json::to_string(&result).unwrap_or_else(|error| {
        serde_json::to_string(&SerializeFallback {
            success: false,
            error: &format!("Serialization error: {error}"),
        })
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"Serialization error\"}".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[derive(serde::Deserialize)]
    struct TestCompileResult {
        success: bool,
        error: Option<String>,
        error_offset: Option<usize>,
        entry_path: Option<String>,
        compile_results: Option<Vec<serde_json::Value>>,
    }

    fn parse_result(json: &str) -> TestCompileResult {
        serde_json::from_str(json).unwrap_or_else(|error| panic!("failed to parse result json: {error}\njson: {json}"))
    }

    #[test]
    #[serial]
    fn compile_source_succeeds() {
        let result = parse_result(
            &compile_source(
                r#"
                fn main() {
                    let a = 1;
                    assert_eq(a, 1, "ok");
                }
                "#,
            ),
        );

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert_eq!(result.entry_path.as_deref(), Some("/virtual_pkg/src/main.psy"));
        assert!(result.compile_results.as_ref().is_some_and(|items| !items.is_empty()));
    }

    #[test]
    #[serial]
    fn compile_project_with_module_succeeds() {
        let files = serde_json::json!([
            [["main"], "mod foo;\nfn main() { foo::run(); }"],
            [["foo"], "pub fn run() {}"]
        ]);

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert_eq!(result.entry_path.as_deref(), Some("/virtual_pkg/src/main.psy"));
        assert!(result.compile_results.as_ref().is_some_and(|items| !items.is_empty()));
    }

    #[test]
    #[serial]
    fn compile_project_with_explicit_entry_succeeds() {
        let files = serde_json::json!({
            "entry": ["main"],
            "method_names": ["main"],
            "files": [
                [["main"], "mod foo;\nfn main() { foo::run(); }"],
                [["foo"], "pub fn run() {}"]
            ]
        });

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert_eq!(result.entry_path.as_deref(), Some("/virtual_pkg/src/main.psy"));
        assert!(result.compile_results.as_ref().is_some_and(|items| !items.is_empty()));
    }

    #[test]
    #[serial]
    fn compile_project_rejects_missing_explicit_entry() {
        let files = serde_json::json!({
            "entry": ["missing"],
            "method_names": ["main"],
            "files": [
                [["main"], "fn main() {}"]
            ]
        });

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(!result.success, "expected compile failure");
        assert!(result
            .error
            .as_ref()
            .is_some_and(|msg| msg.contains("Explicit entry file was not found")));
    }

    #[test]
    #[serial]
    fn compile_project_rejects_non_main_entry_without_method_names() {
        let files = serde_json::json!({
            "entry": ["lib"],
            "files": [
                [["lib"], "pub fn run() {}"]
            ]
        });

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(!result.success, "expected compile failure");
        assert!(result
            .error
            .as_ref()
            .is_some_and(|msg| msg.contains("provide method_names explicitly")));
    }

    #[test]
    #[serial]
    fn compile_project_prefers_main_over_lib_in_legacy_mode() {
        let files = serde_json::json!([
            [["lib"], "pub fn helper() {}"],
            [["main"], "fn main() {}"]
        ]);

        let result = parse_result(&compile_project(&files.to_string()));

        assert!(result.success, "expected compile success, got {:?}", result.error);
        assert_eq!(result.entry_path.as_deref(), Some("/virtual_pkg/src/main.psy"));
    }

    #[test]
    #[serial]
    fn compile_source_reports_error_offset() {
        let result = parse_result(
            &compile_source(
                r#"
                fn main( {
                }
                "#,
            ),
        );

        assert!(!result.success, "expected compile failure");
        assert!(result.error.is_some());
        assert!(result.error_offset.is_some(), "expected parse error offset");
    }
}
