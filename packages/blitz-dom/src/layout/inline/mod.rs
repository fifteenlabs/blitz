mod build;
mod layout;

pub(crate) use build::build_inline_layout_into;

use style::values::{computed::Display, specified::box_::DisplayOutside};

/// Blitz generates no anonymous table boxes, so an internal table box (a cell, a row, ...)
/// inside an inline formatting context is laid out as an atomic inline in place of the
/// anonymous inline-table CSS wraps it in (CSS 2.1 §17.2.1). It takes that wrapper's
/// place on the line: baseline-aligned, by the wrapper's baseline. The box's own
/// `vertical-align` only places its content inside it.
fn stands_in_for_inline_table(display: Display) -> bool {
    display.outside() == DisplayOutside::InternalTable
}
