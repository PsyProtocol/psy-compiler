use std::path::PathBuf;

use psy_ast::{DefaultVisitorContext, ModuleId};
use psy_common::FileId;
use psy_fmt::Formatter;
use psy_vm::dpn::ops::context_trait::{ContextFelt, DPNContext};

use crate::{
    error::{Error, Result},
    TypeCheckerVisitorContext,
};

impl<F: ContextFelt + From<u32> + 'static, C: DPNContext<F>> TypeCheckerVisitorContext<F, C> {
    pub fn format_file(&mut self, file_path: &PathBuf) -> Result<String> {
        let file_path = file_path.canonicalize().map_err(|err| Error::AnyhowError(anyhow::anyhow!("{}", err)))?;
        let file_id = self
            .program
            .file_resolver
            .resolve_id(&file_path)
            .ok_or(Error::AnyhowError(anyhow::anyhow!(
                "cant resolve file `{}`",
                file_path.display().to_string()
            )))?;
        let module_id = self.find_module_id(*file_id).ok_or(Error::AnyhowError(anyhow::anyhow!(
            "cant resolve module file `{}`",
            file_path.display().to_string()
        )))?;
        self.format_module(module_id).map(|ctx| ctx.trim_end().to_owned())
    }

    pub fn format_module(&mut self, module_id: ModuleId) -> Result<String> {
        let mut default_visitor_context: DefaultVisitorContext<'_, F, C> = DefaultVisitorContext::new(&mut self.program);
        let mut formatter = Formatter::new();
        formatter
            .format_module_helper(module_id, true, &mut default_visitor_context)
            .map_err(|err| Error::AnyhowError(anyhow::anyhow!("{}", err)))?;

        Ok(formatter.get_output().to_owned())
    }

    fn find_module_id(&self, file_path: impl Into<FileId>) -> Option<ModuleId> {
        let file_id = file_path.into();
        for node in self.program.modules.iter() {
            let id = node.id();
            let module = node.data();
            let location = module.location;
            // This is just a workaround to find the module id for the file id.
            // As self.program.modules is ordered so it works.
            // TODO: find a correct way to do it
            if location.file_id == file_id {
                return Some(id);
            }
        }
        None
    }
}
