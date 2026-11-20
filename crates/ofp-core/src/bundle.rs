//! The compiled registry: fields and tables, as the core loads them.

use crate::expr::{Expr, PartType};
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Bundle {
    pub version: u32,
    pub fields: Vec<FieldDef>,
    pub tables: Vec<Table>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Int,
    Decimal,
    /// Several numbers from one packet (RR intervals): an array, sent once with its packet, never kept
    /// or repeated as a standing value.
    List,
}

/// At most this many elements in a repeating layout part (a list field).
pub const MAX_LIST: usize = 16;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SampleKind {
    /// The value right now (power, cadence). Reads as zero when the source goes silent.
    Instantaneous,
    /// One thing that happened (a breath, a button press).
    Event,
    /// A running total (energy, distance, elapsed time). Keeps its last value when silent.
    Cumulative,
    /// A setting or state that is neither a rate nor a total (gear).
    State,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Experimental,
    Stable,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FieldDef {
    pub id: u32,
    pub key: String,
    pub name: String,
    pub unit: String,
    #[serde(rename = "type")]
    pub ty: FieldType,
    /// Digits after the decimal point (0 for `int`).
    #[serde(default)]
    pub decimals: u8,
    /// Plausible range. A decoded value outside it is not passed on.
    pub min: f64,
    pub max: f64,
    pub kind: SampleKind,
    pub tier: Tier,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Table {
    pub id: String,
    pub name: String,
    pub reader: String,
    pub version: u32,
    #[serde(rename = "match")]
    pub matches: Vec<MatchRule>,
    /// Packets where this is false are ignored (for example a "averages" packet type).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_when: Option<Expr>,
    /// The value that tells two identical devices apart (a bike number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<Instance>,
    /// `reader: notify`: the characteristic whose notifications this table decodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<NotifySpec>,
    /// After this long without a packet, instantaneous values read as zero.
    pub stale_after_ms: u64,
    /// Values whose position depends on flag bits: each part follows the ones before it that are in
    /// the packet. Fields read a part with `Expr::Part`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    /// `reader: notify`: fixed bytes written once, in order, after subscribing, so the device starts
    /// sending its data. Never computed, never to a control point (checked when the bundle loads).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<Writes>,
    /// Fixed frames written in turn, every `every_ms`, while the device is in use: for devices that
    /// answer only when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll: Option<Writes>,
    pub fields: Vec<TableField>,
}

/// At most this many frames in `start` or `poll`, each at most this long, and polls no closer than this.
pub const MAX_WRITE_FRAMES: usize = 16;
pub const MAX_FRAME_BYTES: usize = 64;
pub const MIN_POLL_MS: u64 = 200;

/// Characteristics a table may never write to: the control points of trainers and power meters.
/// Writing there is control, which OFP does not do.
pub const CONTROL_POINTS: [&str; 6] = [
    "2ad9",                                 // FTMS control point
    "2a66",                                 // Cycling Power control point
    "2a55",                                 // Speed and Cadence control point
    "a026e005-0a7d-4ab3-97fa-f1500f9feb8b", // Wahoo trainer control
    "6e40fec3-b5a3-f393-e0a9-e50e24dcca9e", // Tacx FE-C over Bluetooth, write
    "347b0012-7635-408b-8918-8ff3949ce592", // Elite trainer control
];

/// Fixed bytes to write to one characteristic.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Writes {
    pub service: String,
    pub characteristic: String,
    pub frames: Vec<Vec<u8>>,
    #[serde(default)]
    pub with_response: bool,
    /// `poll` only: how often a frame is written.
    #[serde(default)]
    pub every_ms: u64,
}

/// At most this many parts in a layout.
pub const MAX_LAYOUT_PARTS: usize = 32;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Layout {
    /// Where the first part starts (after the flags).
    pub start: usize,
    pub parts: Vec<LayoutPart>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LayoutPart {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: PartType,
    /// The part is in the packet only when this is true (read from fixed positions: the flags).
    /// Always there when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present_when: Option<Expr>,
    /// The last part may repeat to the end of the packet (at most `MAX_LIST` times): a list field reads it.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub repeat: bool,
}

/// A value that is the speed of a running counter: revolutions, strokes, steps. Kept between packets
/// of the same source: the last count and event time, nothing else.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RateSpec {
    pub count: Expr,
    /// When the last counted event happened, in ticks of `time_unit` seconds.
    pub time: Expr,
    /// The counter and the time start again from zero at these values (65536 for 16 bits).
    pub count_wrap: f64,
    pub time_wrap: f64,
    pub time_unit: f64,
    /// No new event for this long: the rate reads zero (the crank stopped).
    pub stop_ms: u64,
}

/// Where a connected device sends what a `notify` table reads. UUIDs as text, any case, with or
/// without dashes; the 16-bit standard ones may be written short (`2a37`).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NotifySpec {
    pub service: String,
    pub characteristic: String,
    /// Shortest notification this table accepts.
    #[serde(default)]
    pub min_length: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MatchRule {
    /// Bluetooth company id at the start of the manufacturer data. When absent the rule reads a
    /// bare payload (no company id) and must name the device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_prefix: Option<String>,
    /// A service the device advertises (for devices that are connected to).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// Shortest payload (after the company id) this rule accepts.
    #[serde(default)]
    pub min_length: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<Expr>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Instance {
    pub label: String,
    pub value: Expr,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TableField {
    pub field: String,
    pub value: Expr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_when: Option<Expr>,
    /// The field is a rate: `value` reads it as `Expr::Rate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<RateSpec>,
}

impl Bundle {
    pub fn field(&self, key: &str) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.key == key)
    }
}
