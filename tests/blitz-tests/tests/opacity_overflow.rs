//! `opacity` creates a stacking context but no clip: an overflowing descendant
//! of a short `opacity` box must still paint outside it when `overflow` is
//! `visible`. `overflow` (and a filter's own region) is what clips.
//!
//! Every case is swept over the device scale. The layer's region is built from
//! `border_box_path()`, which `create_css_rect` has already multiplied by the
//! scale, and grown to `scrollable_overflow`, which `resolve_transforms` stores
//! in the same device pixels — a sweep is what says the two spaces still agree.
//! The CSS layout is identical at every scale because the viewport is sized in
//! device pixels from the same CSS box, so one set of CSS-pixel sample points
//! serves them all.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

/// The page's size in CSS pixels. The buffer is this many device pixels at
/// scale one, twice as many at scale two.
const SIZE: f32 = 100.0;

const SCALES: [f32; 2] = [1.0, 2.0];

fn pixel(html: &str, x: f32, y: f32, scale: f32) -> [u8; 3] {
    let side = (SIZE * scale) as u32;
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(side, side, scale, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, scale as f64, side, side, 0, 0),
        side,
        side,
    );
    let column = (x * scale) as usize;
    let row = (y * scale) as usize;
    let idx = (row * side as usize + column) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

/// Assert the colour at one CSS-pixel point, at every device scale.
#[track_caller]
fn assert_pixel(html: &str, x: f32, y: f32, expected: [u8; 3], message: &str) {
    for scale in SCALES {
        let actual = pixel(html, x, y, scale);
        let close = actual
            .iter()
            .zip(expected.iter())
            .all(|(a, b)| a.abs_diff(*b) <= 2);
        assert!(
            close,
            "{message} at ({x}, {y}) css px, scale {scale}: expected ~{expected:?}, got {actual:?}"
        );
    }
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
    assert_pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999;"),
        10.0,
        40.0,
        [255, 0, 0],
        "a child overflowing an `opacity` box with `overflow: visible` must still paint",
    );
}

#[test]
fn opacity_with_visible_overflow_still_paints_inside() {
    assert_pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999;"),
        10.0,
        10.0,
        [255, 0, 0],
        "content inside the box must still paint",
    );
}

#[test]
fn opacity_alpha_still_applies_to_overflowing_child() {
    // Half-opaque red over a white page is a single 50% composite: proof the
    // overflowing part is painted *through the opacity layer*, not leaked
    // outside it (which would paint it at full strength).
    assert_pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.5;"),
        10.0,
        40.0,
        [255, 127, 127],
        "the overflowing part must be composited at the layer's opacity",
    );
}

#[test]
fn opacity_with_hidden_overflow_still_clips_child() {
    assert_pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999; overflow:hidden;"),
        10.0,
        40.0,
        [255, 255, 255],
        "`overflow: hidden` must keep clipping under an `opacity` layer",
    );
}

#[test]
fn opacity_with_clip_overflow_still_clips_child() {
    assert_pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999; overflow:clip;"),
        10.0,
        40.0,
        [255, 255, 255],
        "`overflow: clip` must keep clipping under an `opacity` layer",
    );
}

#[test]
fn opacity_with_hidden_overflow_paints_inside() {
    assert_pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999; overflow:hidden;"),
        10.0,
        10.0,
        [255, 0, 0],
        "clipped box must still paint its content",
    );
}

#[test]
fn opacity_with_axis_hidden_overflow_still_clips_child() {
    // `overflow-y: hidden` forces the computed `overflow-x` to `auto`, so both
    // axes clip. The child must be cut in both directions.
    assert_pixel(
        &badge_page(
            "max-height:24px; width:24px; position:relative; opacity:0.999; overflow-y:hidden;",
        ),
        10.0,
        40.0,
        [255, 255, 255],
        "`overflow-y: hidden` must keep clipping vertically",
    );
    assert_pixel(
        &badge_page(
            "max-height:24px; width:24px; position:relative; opacity:0.999; overflow-y:hidden;",
        ),
        40.0,
        10.0,
        [255, 255, 255],
        "`overflow-y: hidden` blockifies `overflow-x`, so it clips horizontally too",
    );
}

#[test]
fn filter_clips_child_to_its_region() {
    // A filter renders into a bounded region (the border box grown by the
    // filter's own expansion), so an overflowing child is clipped by it.
    assert_pixel(
        &badge_page("max-height:24px; position:relative; filter:opacity(0.5);"),
        10.0,
        40.0,
        [255, 255, 255],
        "a filter must keep clipping to its region",
    );
}

/// A 24px-tall box with `filter: blur(2px)` on it. CSS gives `blur()` the
/// standard deviation directly, and a gaussian's expansion area is three of
/// them, so the region this renders into ends 24 + 6 = 30 CSS pixels down —
/// at every device scale, since both halves of that sum are device lengths.
const BLURRED_BADGE: &str = "max-height:24px; position:relative; filter:blur(2px);";

/// How far below the border box a `blur(2px)` region reaches: three deviations.
const BLUR_REACH: f32 = 6.0;

#[test]
fn blurred_filter_clips_child_beyond_its_expansion_area() {
    // `blur` expands the region it renders into, but only by a bounded amount:
    // far outside it, nothing is painted.
    assert_pixel(
        &badge_page(BLURRED_BADGE),
        10.0,
        90.0,
        [255, 255, 255],
        "a blur's region must stay bounded by its expansion area",
    );
}

#[test]
fn a_blurs_region_ends_three_deviations_past_the_border_box() {
    // The bound the test above only samples from far away, pinned tightly
    // enough to tell three deviations from six: the badge is painted a pixel
    // inside the region and gone a pixel outside it. Anything that grew the
    // region by a second expansion area - a backend inflating the clip it was
    // handed, say - would still be painting red at the second point.
    //
    // `anyrender_vello_cpu` drops the filter itself when built with
    // `multithreading`, as it is here, so what is measured is purely the
    // region: no gaussian softens either sample.
    assert_pixel(
        &badge_page(BLURRED_BADGE),
        10.0,
        24.0 + BLUR_REACH - 2.0,
        [255, 0, 0],
        "a blur's region has to reach three deviations past the border box",
    );
    assert_pixel(
        &badge_page(BLURRED_BADGE),
        10.0,
        24.0 + BLUR_REACH + 2.0,
        [255, 255, 255],
        "a blur's region has to stop three deviations past the border box, not six",
    );
}

#[test]
fn overflowing_child_paints_without_opacity() {
    // Control: the same markup with no opacity already paints correctly.
    assert_pixel(
        &badge_page("max-height:24px; position:relative;"),
        10.0,
        40.0,
        [255, 0, 0],
        "`max-height` with `overflow: visible` must not clip",
    );
}

#[test]
fn opacity_does_not_clip_absolutely_positioned_descendant() {
    // Out-of-flow descendants take a different paint path, but the same rule
    // applies: `opacity` must not clip them.
    assert_pixel(
        r#"<html><body style="margin:0; background:#ffffff;">
            <div style="max-height:24px; position:relative; opacity:0.999;">
                <div style="position:absolute; top:30px; left:0; width:56px; height:56px; background-color:#ff0000;"></div>
            </div>
        </body></html>"#,
        10.0,
        40.0,
        [255, 0, 0],
        "an abspos descendant of an `opacity` box must not be clipped by it",
    );
}

#[test]
fn opacity_does_not_clip_z_index_hoisted_descendant() {
    // A positioned descendant with a z-index is hoisted to its stacking
    // context, which is the `opacity` box itself.
    assert_pixel(
        r#"<html><body style="margin:0; background:#ffffff;">
            <div style="max-height:24px; position:relative; opacity:0.999;">
                <div style="position:relative; z-index:2; top:30px; width:56px; height:56px; background-color:#ff0000;"></div>
            </div>
        </body></html>"#,
        10.0,
        40.0,
        [255, 0, 0],
        "a z-index-hoisted descendant of an `opacity` box must not be clipped by it",
    );
}

#[test]
fn scroll_container_with_opacity_still_clips() {
    assert_pixel(
        &badge_page("max-height:24px; position:relative; opacity:0.999; overflow:scroll;"),
        10.0,
        40.0,
        [255, 255, 255],
        "a scroll container must keep clipping under an `opacity` layer",
    );
}

#[test]
fn nested_opacity_layers_do_not_clip() {
    assert_pixel(
        r#"<html><body style="margin:0; background:#ffffff;">
            <div style="max-height:24px; position:relative; opacity:0.999;">
                <div style="max-height:12px; opacity:0.999;">
                    <div style="width:56px; height:56px; background-color:#ff0000;"></div>
                </div>
            </div>
        </body></html>"#,
        10.0,
        40.0,
        [255, 0, 0],
        "nested `opacity` layers must each stay unclipped",
    );
}
