//! Typed (Tier-2) header forms.
//!
//! Each submodule provides a struct that wraps the raw header value
//! string (kept in [`super::header::Header`]) with a structured view.
//! Parsing is on-demand: callers invoke `<TypedForm>::parse(value)` —
//! either directly, or via a method on `Header` (e.g.
//! [`super::header::Header::typed_from`]).
//!
//! M4 lands `From` and `To`. M5 adds `Via`, `CSeq`, `Contact`.
//! The shape mirrors `rsip 0.4`'s `typed::*` structs so that the
//! M8 cutover in `crate::sip::message` is a near-drop-in replacement.

pub mod contact;
pub mod cseq;
pub mod from;
pub mod to;
pub mod via;

pub use contact::{Contact, ContactAddr};
pub use cseq::CSeq;
pub use from::From;
pub use to::To;
pub use via::Via;
