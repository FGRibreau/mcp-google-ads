//! Shared domain models used by tools, safety, and the public MCP surface.

pub mod ad_rotation_mode;
pub mod ad_status;
pub mod demographic;
pub mod geo_target_type;
pub mod micros;
pub mod next_action_hint;

pub use ad_rotation_mode::AdRotationMode;
pub use ad_status::AdStatus;
pub use demographic::{AgeRange, Gender, IncomeRange};
pub use geo_target_type::GeoTargetType;
pub use micros::{resolve_micros, to_micros};
pub use next_action_hint::NextActionHint;
