use std::path::PathBuf;

use clap::Args;
use psy_interpreter::Interpreter;
use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};

use crate::{cli::write_to_file, errors::Result};

#[derive(Debug, Clone, Args)]
pub(crate) struct FmtCommand {
    /// The file to format
    pub file: PathBuf,
}
pub(crate) fn run(args: FmtCommand) -> Result<()> {
    let entry = args.file;
    let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
    let (_, mut ctx) = interpreter.typecheck_single(entry.clone())?;
    let formatted_content = ctx.format_file(&entry)?;
    write_to_file(formatted_content.as_bytes(), &entry)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use psy_interpreter::Interpreter;
    use psy_vm::dpn::ops::{exec_context::QExecContext, sym_felt::SymFeltRef};
    use serial_test::serial;

    use super::{run, FmtCommand};

    #[test]
    fn formatter_reports_missing_input_file() {
        let error = run(FmtCommand {
            file: PathBuf::from("target/does-not-exist-for-format-test.psy"),
        })
        .expect_err("formatting a missing file must fail");
        let rendered = error.to_string();
        assert!(!rendered.is_empty());
    }

    #[test]
    #[serial]
    fn formatter_run_rewrites_the_file_in_place() {
        let entry = std::env::temp_dir().join("fmt_in_place_main.psy");
        std::fs::write(&entry, "use std::prelude::*;\nfn main()->Felt{return 0;}\n").unwrap();

        run(FmtCommand { file: entry.clone() }).expect("fmt must succeed on a valid file");

        let formatted = std::fs::read_to_string(&entry).unwrap();
        let _ = std::fs::remove_file(&entry);
        assert!(formatted.contains("fn main() -> Felt {"), "unexpected output:\n{formatted}");

        // The workspace-relative std discovery walks up from the manifest dir
        // when DARGO_STD_PATH is not set.
        let context = find_std_module_above(env!("CARGO_MANIFEST_DIR"), "context.psy");
        assert!(context.is_file(), "{} must exist", context.display());
    }

    /// Resolve a module of the std root the parser will actually use:
    /// `DARGO_STD_PATH` wins when set (toolchain installs live elsewhere),
    /// otherwise the checked-in `psy-std/` next to this workspace.
    fn std_module_path(name: &str) -> PathBuf {
        if let Ok(std_path) = std::env::var("DARGO_STD_PATH") {
            if let Some(parent) = PathBuf::from(std_path).parent() {
                return parent.join(name);
            }
        }
        find_std_module_above(env!("CARGO_MANIFEST_DIR"), name)
    }

    fn find_std_module_above(start: &str, name: &str) -> PathBuf {
        let mut dir = Some(PathBuf::from(start));
        while let Some(current) = dir {
            let candidate = current.join("psy-std").join(name);
            if candidate.exists() {
                return candidate;
            }
            dir = current.parent().map(std::path::Path::to_path_buf);
        }
        panic!("psy-std/{name} not found above {start}");
    }

    #[test]
    #[serial]
    fn formatter_formats_std_intrinsic_wrapper_modules() {
        // The formatter skips modules *named* std/prelude/primitive, but the
        // context/storage/mem/event modules are only *children* of std —
        // formatting them drives every raw-intrinsic arm of the formatter,
        // which user code can never reach (sema rejects raw intrinsics
        // outside std ancestry).
        let entry = std::env::temp_dir().join("fmt_std_driver_main.psy");
        std::fs::write(&entry, "use std::prelude::*;\n\nfn main() -> Felt {\n    return 0;\n}\n").unwrap();

        let mut interpreter = Interpreter::<SymFeltRef, _>::new(QExecContext::new());
        let typecheck = {
            let result = interpreter.typecheck_single(entry.clone());
            #[allow(static_mut_refs)]
            unsafe {
                let _ = psy_sema::STD_PRIMITIVE_SCOPE_ID.take();
            }
            result
        };
        let _ = std::fs::remove_file(&entry);
        let (_, mut ctx) = typecheck.expect("driver program must typecheck");

        // The std root module itself is skipped entirely.
        let std_root = ctx.format_file(&std_module_path("std.psy")).expect("format std.psy");
        assert_eq!(std_root, "");

        let context = ctx.format_file(&std_module_path("context.psy")).expect("format context.psy");
        assert!(context.contains("fn get_user_id"), "{context}");
        assert!(context.contains("__ctx_get_user_id()"), "{context}");
        assert!(context.contains("__imt_get_other_user("), "{context}");
        assert!(context.contains("__invoke_sync::<T>("), "{context}");
        assert!(context.contains("__secp256k1_verify("), "{context}");

        let storage = ctx.format_file(&std_module_path("storage.psy")).expect("format storage.psy");
        assert!(storage.contains("__storage_read("), "{storage}");
        assert!(storage.contains("__storage_write("), "{storage}");
        assert!(storage.contains("__storage_write_range("), "{storage}");

        let mem = ctx.format_file(&std_module_path("mem.psy")).expect("format mem.psy");
        assert!(mem.contains("__mem_transmute::<U>("), "{mem}");
        assert!(mem.contains("__mem_size_of::<T>()"), "{mem}");

        let event = ctx.format_file(&std_module_path("event.psy")).expect("format event.psy");
        assert!(event.contains("__emit(self)"), "{event}");
    }
}
