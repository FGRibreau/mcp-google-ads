//! Shared domain models used by tools, safety, and the public MCP surface.

pub mod ad_rotation_mode;
pub mod ad_status;
pub mod next_action_hint;

pub use ad_rotation_mode::AdRotationMode;
pub use ad_status::AdStatus;
pub use next_action_hint::NextActionHint;
