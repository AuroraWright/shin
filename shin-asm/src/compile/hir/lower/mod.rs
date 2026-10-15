//! Lower hir into instruction types

mod block;
mod elements;
mod file;
mod instruction;
mod program;

mod from_hir;

#[cfg(test)]
mod test_utils;

pub use block::{LoweredBlock, lower_block};
pub use file::{LoweredFile, lower_file};
pub use from_hir::{
    CodeAddressCollector, FromHirExpr, HirDiagnosticCollector, HirDiagnosticCollectorWithBlock,
    HirDiagnosticCollectorWithFile,
};
pub use program::{LoweredProgram, lower_program};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LowerError;
pub type LowerResult<T> = Result<T, LowerError>;
