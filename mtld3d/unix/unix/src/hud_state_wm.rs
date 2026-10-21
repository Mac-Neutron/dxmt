//! The Metal HUD's mtld3d row under the winemetal facade: none.
//!
//! The HUD shows the process that owns the layer (the VM's host, or the loopback test's Wine), and
//! the State Reporter is a macOS framework.

use mtld3d_core::hud_stats::HUD_ROW_COUNT;

/// Nothing to report.
pub const fn report_attached() {}

/// A reporter that is always inert.
pub struct HudReporter;

impl HudReporter {
    pub const fn new(_owner_id: u64) -> Self {
        Self
    }
    pub const fn is_inert(&self) -> bool {
        let Self = self;
        true
    }
    pub const fn sample_due(&self, _now: u64, _hz: u64) -> bool {
        let Self = self;
        false
    }
    pub const fn report(&self, _rows: &[Option<u32>; HUD_ROW_COUNT]) {
        let Self = self;
    }
}
