//! Mail domain types. Untrusted text is parsed into these once, at the edge;
//! everything past that point can rely on the invariants they carry.

pub mod choices;
pub mod email;
pub mod message;
pub mod outgoing;
pub mod provider;
pub mod server;
pub mod time;
pub mod triage;
pub mod view;

pub use email::{EmailAddress, EmailError};
pub use message::{
    AccountId, Contact, IncomingMessage, MessageId, Signals, ThreadRef, TriageState,
};
pub use outgoing::{Draft, DraftError, Recipients, ReplyContext};
pub use provider::{Provider, SignIn};
pub use server::{AuthMethod, Host, HostError, ImapSettings, SmtpSettings, SyncCursor};
pub use view::TriageView;
