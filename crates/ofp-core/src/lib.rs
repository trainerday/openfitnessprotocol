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

pub use bundle::{Bundle, FieldDef, FieldType, Instance, Layout, LayoutPart, MatchRule, NotifySpec, RateSpec, SampleKind, Table, TableField, Tier, Writes, CONTROL_POINTS, MAX_FRAME_BYTES, MAX_LAYOUT_PARTS, MAX_WRITE_FRAMES, MIN_POLL_MS};
pub use engine::{normalize_uuid, Advertisement, Core, FieldDescription, Message, Settings, Subscription, Write};
pub use expr::{BinOp, Cx, Expr, PartType, MAX_EXPR_DEPTH, MAX_EXPR_NODES};

/// Version of the bundle format this core reads.
pub const BUNDLE_VERSION: u32 = 1;
