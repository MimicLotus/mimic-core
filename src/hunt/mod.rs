pub mod deb;
pub mod git;
pub mod mirror;

pub use deb::{DebHunter, DebPackageInfo};
pub use git::GitHunter;
pub use mirror::{MirrorResolver, UpstreamGround};
