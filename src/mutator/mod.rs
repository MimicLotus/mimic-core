pub mod soname;
pub mod elf;

pub use soname::{DynamicDependencyAnalysis, SonameScanner};
pub use elf::ElfMutator;
