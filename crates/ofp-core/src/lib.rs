//! Open Fitness Protocol core.
//!
//! The core has no Bluetooth and no I/O. The host passes in what its own Bluetooth layer hears
//! ([`Core::on_advertisement`]) and the time ([`Core::tick`]); the core answers with messages:
//! a source appeared, values, a source went silent.
//!
//! `no_std` + `alloc`, so the same code runs on desktop, phones, WebAssembly and microcontrollers.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod bundle;
mod engine;
mod expr;

pub use bundle::{Bundle, FieldDef, FieldType, Instance, MatchRule, SampleKind, Table, TableField, Tier};
pub use engine::{Advertisement, Core, FieldDescription, Message};
pub use expr::{BinOp, Expr, MAX_EXPR_DEPTH, MAX_EXPR_NODES};

/// Version of the bundle format this core reads.
pub const BUNDLE_VERSION: u32 = 1;
