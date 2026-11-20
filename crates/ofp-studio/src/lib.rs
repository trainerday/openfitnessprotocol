//! OFP Studio: a local web page for adding a device.
//!
//! [`Studio`] holds everything the page does (and is what the tests drive); [`server`] puts it behind
//! HTTP on 127.0.0.1 and, with the `ble` feature, feeds it from the radio.

pub mod server;
pub mod solve;
mod studio;

pub use studio::{AdvIn, Config, FieldReq, ProposeReq, Studio};
