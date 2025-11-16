use std::path::PathBuf;

use crate::package::Package;

#[derive(Clone, Debug, Default)]
pub struct Workspace {
    pub root_dir: PathBuf,
    /// Optional target directory override.
    pub target_dir: PathBuf,
    pub package: Package,
}
