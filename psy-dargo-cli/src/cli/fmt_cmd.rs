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
