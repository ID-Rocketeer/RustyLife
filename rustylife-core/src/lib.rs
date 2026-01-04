pub mod cell;
pub mod engine;
pub mod hash;
pub mod space;
pub mod state;
pub mod tree;

use serde::{Deserialize, Serialize};

// The messaging protocol should eventually be updated to
// support viewport-based queries or delta updates, avoiding monolithic state transfers.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Request {
    NextStep,
    Reset,
    GetState,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Response {
    Ok,
    State(Vec<(i128, i128)>),
    Error(String),
}
