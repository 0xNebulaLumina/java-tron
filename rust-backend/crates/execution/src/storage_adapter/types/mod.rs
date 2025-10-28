// Type definitions for Tron blockchain data structures

mod witness;
mod freeze;
mod account_aext;
mod vote;
mod state_change;

pub use witness::WitnessInfo;
pub use freeze::FreezeRecord;
pub use account_aext::AccountAext;
pub use vote::{Vote, VotesRecord};
pub use state_change::StateChangeRecord;
