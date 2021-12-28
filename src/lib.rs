#[macro_use]
extern crate lazy_static;

pub mod client;
pub mod github;
pub mod processing;

pub use processing::{Director, DirectorState};
