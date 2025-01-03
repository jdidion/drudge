mod batch;
mod iter;
mod outcome;
mod store;

pub use batch::OutcomeBatch;
pub use iter::OutcomeIteratorExt;
pub use outcome::Outcome;
pub use store::{OutcomeDerefStore, OutcomeStore};

pub(super) use store::sealed::{Outcomes, OutcomesDeref};
