//! Sharing workflows, split by concern:
//!
//! - [`lifecycle`]     — create / accept / revoke / reject shares + `cleanup_incoming_share`.
//! - [`registration`]  — recipient-side received-picture registration / unregistration.
//! - [`shareback`]     — ShareBack auto-accept (mapping wiring).
//! - [`delivery`]      — in-process task delivery of announce / unannounce.
//!
//! Picture announcement is driven exclusively by the tagging pipeline (`infra::pipeline`): share
//! acceptance moves the sender's `OutgoingShare` to `pending_first_announcement`, and the pipeline
//! announces its coverage and flips it to `active`. These services only manage share state and
//! hand work to the pipeline + task queue.

pub mod delivery;
pub mod lifecycle;
pub mod registration;
pub mod shareback;

pub use delivery::{deliver_announce_task, deliver_unannounce_task};
pub use lifecycle::{
    accept_incoming_share, cleanup_incoming_share, create_outgoing_share, reject_incoming_share,
    revoke_outgoing_share,
};
pub use registration::{register_received_pictures, unregister_announced_pictures};
pub use shareback::auto_accept_shareback_local;
