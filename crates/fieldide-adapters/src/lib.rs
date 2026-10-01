//! Side-effect adapters. Process, ROS and build adapters arrive in weeks 3-5.

/// Marks the week-one adapter boundary without coupling core state to I/O.
pub trait Adapter {
    fn name(&self) -> &'static str;
}
