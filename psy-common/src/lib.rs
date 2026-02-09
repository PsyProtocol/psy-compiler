// Re-export all public modules from upstream psy_common_core
pub use psy_common_core::args;
pub use psy_common_core::data;
pub use psy_common_core::error;
pub use psy_common_core::job;
pub use psy_common_core::macros;
pub use psy_common_core::traits;
pub use psy_common_core::ups;
pub use psy_common_core::utils;

// Conditionally re-export platform-specific modules
#[cfg(not(target_arch = "wasm32"))]
pub use psy_common_core::health;
#[cfg(not(target_arch = "wasm32"))]
pub use psy_common_core::jwt;
#[cfg(not(target_arch = "wasm32"))]
pub use psy_common_core::logging;

// Re-export error types to crate root (needed by graph.rs which uses `crate::Error`)
pub use psy_common_core::error::*;

// Re-export logging to crate root (matches upstream)
#[cfg(not(target_arch = "wasm32"))]
pub use psy_common_core::logging::*;

// Local modules (missing from upstream)
pub mod arena;
pub mod file_resolver;
pub mod graph;
pub mod tree;

// Re-export key types to crate root (matches upstream)
pub use arena::Arena;
pub use file_resolver::{FileId, FileResolver};
pub use graph::Graph;
pub use tree::{Tree, TreeNode};
