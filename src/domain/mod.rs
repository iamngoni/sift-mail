//! Mail domain types. Untrusted text is parsed into these once, at the edge;
//! everything past that point can rely on the invariants they carry.

pub mod email;
pub mod provider;
pub mod view;

pub use email::{EmailAddress, EmailError};
pub use provider::{Provider, SignIn};
pub use view::TriageView;
