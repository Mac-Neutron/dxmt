//! MacNeutron: the Metal HUD's mtld3d row under the winemetal facade: none. The HUD shows the process that owns the
//! layer (the VM's host, or the loopback test's Wine), and the State Reporter is a macOS framework.

use mtld3d_core::hud_stats::HUD_ROW_COUNT;

use crate::mtl::objc2_quartz_core::CAMetalLayer;

/// Nothing to report.
pub fn report_attached() {}

/// No HUD row to show.
pub fn show_row(_layer: &CAMetalLayer) {}

/// A reporter that is always inert.
pub struct HudReporter;

impl HudReporter {
    pub const fn new(_owner_id: u64) -> Self {
        Self
    }
    pub const fn is_inert(&self) -> bool {
        true
    }
    pub const fn sample_due(&mut self, _now: u64, _hz: u64) -> bool {
        false
    }
    pub const fn report(&mut self, _rows: &[Option<u32>; HUD_ROW_COUNT]) {}
}
