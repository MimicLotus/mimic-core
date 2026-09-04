pub mod deb;
pub mod git;

pub use deb::{DebHunter, DebPackageInfo};
pub use git::GitHunter;
