//! `opacity` creates a stacking context but no clip: an overflowing descendant
//! of a short `opacity` box must still paint outside it when `overflow` is
//! `visible`. `overflow` (and a filter's own region) is what clips.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const SIZE: usize = 100;

fn pixel(html: &str, x: usize, y: usize) -> [u8; 3] {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(
                SIZE as u32,
                SIZE as u32,
                1.0,
                ColorScheme::Light,
            )),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, SIZE as u32, SIZE as u32, 0, 0),
        SIZE as u32,
        SIZE as u32,
    );
    let idx = (y * SIZE + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

#[track_caller]
fn assert_close(actual: [u8; 3], expected: [u8; 3], message: &str) {
    let close = actual
        .iter()
        .zip(expected.iter())
        .all(|(a, b)| a.abs_diff(*b) <= 2);
    assert!(close, "{message}: expected ~{expected:?}, got {actual:?}");
}

/// The "logo badge overlapping a card" pattern: a 24px-tall container holds a
/// 56px badge that is meant to hang out of it.
fn badge_page(container_style: &str) -> String {
    format!(
        r#"<html><body style="margin:0; background:#ffffff;">
            <div style="{container_style}">
                <div style="margin:0; display:inline-block;">
                    <span style="display:inline-block; width:56px; height:56px; background-color:#ff0000;"></span>
                </div>
            </div>
        </body></html>"#
    )
}

#[test]
fn opacity_with_visible_overflow_does_not_clip_child() {
    let px = pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999;"),
        10,
        40,
    );
    assert_close(
        px,
        [255, 0, 0],
        "a child overflowing an `opacity` box with `overflow: visible` must still paint",
    );
}

#[test]
fn opacity_with_visible_overflow_still_paints_inside() {
    let px = pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999;"),
        10,
        10,
    );
    assert_close(px, [255, 0, 0], "content inside the box must still paint");
}

#[test]
fn opacity_alpha_still_applies_to_overflowing_child() {
    // Half-opaque red over a white page is a single 50% composite: proof the
    // overflowing part is painted *through the opacity layer*, not leaked
    // outside it (which would paint it at full strength).
    let px = pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.5;"),
        10,
        40,
    );
    assert_close(
        px,
        [255, 127, 127],
        "the overflowing part must be composited at the layer's opacity",
    );
}

#[test]
fn opacity_with_hidden_overflow_still_clips_child() {
    let px = pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999; overflow:hidden;"),
        10,
        40,
    );
    assert_close(
        px,
        [255, 255, 255],
        "`overflow: hidden` must keep clipping under an `opacity` layer",
    );
}

#[test]
fn opacity_with_clip_overflow_still_clips_child() {
    let px = pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999; overflow:clip;"),
        10,
        40,
    );
    assert_close(
        px,
        [255, 255, 255],
        "`overflow: clip` must keep clipping under an `opacity` layer",
    );
}

#[test]
fn opacity_with_hidden_overflow_paints_inside() {
    let px = pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999; overflow:hidden;"),
        10,
        10,
    );
    assert_close(px, [255, 0, 0], "clipped box must still paint its content");
}

#[test]
fn opacity_with_axis_hidden_overflow_still_clips_child() {
    // `overflow-y: hidden` forces the computed `overflow-x` to `auto`, so both
    // axes clip. The child must be cut in both directions.
    let px = pixel(
        &badge_page(
            "max-height:24px; width:24px; position:relative; opacity:0.999; overflow-y:hidden;",
        ),
        10,
        40,
    );
    assert_close(
        px,
        [255, 255, 255],
        "`overflow-y: hidden` must keep clipping vertically",
    );
    let px = pixel(
        &badge_page(
            "max-height:24px; width:24px; position:relative; opacity:0.999; overflow-y:hidden;",
        ),
        40,
        10,
    );
    assert_close(
        px,
        [255, 255, 255],
        "`overflow-y: hidden` blockifies `overflow-x`, so it clips horizontally too",
    );
}

#[test]
fn filter_clips_child_to_its_region() {
    // A filter renders into a bounded region (the border box grown by the
    // filter's own expansion), so an overflowing child is clipped by it.
    let px = pixel(
        &badge_page("max-height:24px; position:relative; filter:opacity(0.5);"),
        10,
        40,
    );
    assert_close(
        px,
        [255, 255, 255],
        "a filter must keep clipping to its region",
    );
}

#[test]
fn blurred_filter_clips_child_beyond_its_expansion_area() {
    // `blur` expands the region it renders into, but only by a bounded amount:
    // far outside it, nothing is painted.
    let px = pixel(
        &badge_page("max-height:24px; position:relative; filter:blur(2px);"),
        10,
        90,
    );
    assert_close(
        px,
        [255, 255, 255],
        "a blur's region must stay bounded by its expansion area",
    );
}

#[test]
fn overflowing_child_paints_without_opacity() {
    // Control: the same markup with no opacity already paints correctly.
    let px = pixel(&badge_page("max-height:24px; position:relative;"), 10, 40);
    assert_close(
        px,
        [255, 0, 0],
        "`max-height` with `overflow: visible` must not clip",
    );
}

#[test]
fn opacity_does_not_clip_absolutely_positioned_descendant() {
    // Out-of-flow descendants take a different paint path, but the same rule
    // applies: `opacity` must not clip them.
    let px = pixel(
        r#"<html><body style="margin:0; background:#ffffff;">
            <div style="max-height:24px; position:relative; opacity:0.999;">
                <div style="position:absolute; top:30px; left:0; width:56px; height:56px; background-color:#ff0000;"></div>
            </div>
        </body></html>"#,
        10,
        40,
    );
    assert_close(
        px,
        [255, 0, 0],
        "an abspos descendant of an `opacity` box must not be clipped by it",
    );
}

#[test]
fn opacity_does_not_clip_z_index_hoisted_descendant() {
    // A positioned descendant with a z-index is hoisted to its stacking
    // context, which is the `opacity` box itself.
    let px = pixel(
        r#"<html><body style="margin:0; background:#ffffff;">
            <div style="max-height:24px; position:relative; opacity:0.999;">
                <div style="position:relative; z-index:2; top:30px; width:56px; height:56px; background-color:#ff0000;"></div>
            </div>
        </body></html>"#,
        10,
        40,
    );
    assert_close(
        px,
        [255, 0, 0],
        "a z-index-hoisted descendant of an `opacity` box must not be clipped by it",
    );
}

#[test]
fn scroll_container_with_opacity_still_clips() {
    let px = pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999; overflow:scroll;"),
        10,
        40,
    );
    assert_close(
        px,
        [255, 255, 255],
        "a scroll container must keep clipping under an `opacity` layer",
    );
}

#[test]
fn nested_opacity_layers_do_not_clip() {
    let px = pixel(
        r#"<html><body style="margin:0; background:#ffffff;">
            <div style="max-height:24px; position:relative; opacity:0.999;">
                <div style="max-height:12px; opacity:0.999;">
                    <div style="width:56px; height:56px; background-color:#ff0000;"></div>
                </div>
            </div>
        </body></html>"#,
        10,
        40,
    );
    assert_close(
        px,
        [255, 0, 0],
        "nested `opacity` layers must each stay unclipped",
    );
}
