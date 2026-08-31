//! An element is skipped entirely when the rect it could ink falls outside the clip
//! rectangle it is being painted into. That rect is `scrollable_overflow` — the element's
//! own border box unioned with everything its subtree hangs outside it — and
//! `resolve_transforms` stores it in the element's *own* coordinate space, in device
//! pixels.
//!
//! Both of those have to hold at the point it is mapped to the screen. The screen
//! transform already carries the element's position, so a border box built at that
//! position is translated by it twice, and one built from an unscaled size is too small at
//! any scale above one. Because the two are unioned the error only ever over-includes, so
//! nothing visible is lost — the symptom is content that has scrolled far enough off the
//! top of a clip rectangle still being painted, by roughly its own offset within its
//! parent.
//!
//! The recording backend is used rather than a rasterizer because a culling bug that
//! over-includes is invisible in the pixels by construction: the extra element is painted
//! off-screen. What can be observed is whether the paint command was issued at all.

use anyrender::Paint;
use anyrender::Scene;
use anyrender::recording::RenderCommand;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

/// The page's size in CSS pixels; the buffer is this many device pixels times the scale.
const SIZE: f32 = 100.0;

const SCALES: [f32; 2] = [1.0, 2.0];

/// The marker box is 100 CSS px tall and sits this far down inside its parent.
const MARKER_TOP: f64 = 1000.0;

const MARKER_HEIGHT: f64 = 100.0;

/// A tall parent pulled `pull` CSS pixels above the top of the page, holding a red marker
/// box `MARKER_TOP` pixels down. The parent is tall enough that it always straddles the
/// viewport itself, so whether the marker is painted is decided by the marker's own cull
/// and not by its parent's.
fn page(pull: f64) -> String {
    format!(
        r#"<html><body style="margin:0; background:#ffffff;">
            <div style="position:relative; top:-{pull}px; width:{SIZE}px; height:3000px;">
                <div style="width:{SIZE}px; height:{MARKER_TOP}px;"></div>
                <div style="width:{SIZE}px; height:{MARKER_HEIGHT}px; background:#ff0000;"></div>
            </div>
        </body></html>"#
    )
}

fn is_marker(paint: &Paint) -> bool {
    match paint {
        Paint::Solid(color) => {
            let [r, g, b, a] = color.components;
            r > 0.99 && g < 0.01 && b < 0.01 && a > 0.99
        }
        _ => false,
    }
}

/// How many fills of the marker colour painting this page records.
fn marker_fills(html: &str, scale: f32) -> usize {
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
    let mut scene = Scene::new();
    paint_scene(&mut scene, &mut doc, scale as f64, side, side, 0, 0);
    scene
        .commands
        .iter()
        .filter(|command| match command {
            RenderCommand::Fill(fill) => is_marker(&fill.brush),
            _ => false,
        })
        .count()
}

#[track_caller]
fn assert_marker_fills(pull: f64, expected: usize, message: &str) {
    let html = page(pull);
    for scale in SCALES {
        let found = marker_fills(&html, scale);
        assert_eq!(
            found, expected,
            "{message}: pulled up {pull}px at scale {scale}, expected {expected} marker \
             fill(s), found {found}"
        );
    }
}

#[test]
fn a_marker_inside_the_viewport_is_painted() {
    // Pulled up 950px, the marker spans y 50..150 — its top half is on screen.
    assert_marker_fills(
        MARKER_TOP - 50.0,
        1,
        "a marker overlapping the viewport must be painted",
    );
}

#[test]
fn a_marker_straddling_the_top_edge_is_painted() {
    // Pulled up 1050px, the marker spans y -50..50 — its bottom half is on screen. This
    // is the case the cull rect must not shrink past.
    assert_marker_fills(
        MARKER_TOP + 50.0,
        1,
        "a marker straddling the top edge of the viewport must be painted",
    );
}

#[test]
fn a_marker_above_the_viewport_is_culled() {
    // Pulled up 2000px, the marker spans y -1000..-900: a full 900px clear of the top
    // edge, and further above it than the 1000px offset a double-translated cull rect
    // would have added back.
    assert_marker_fills(
        MARKER_TOP + MARKER_HEIGHT + 900.0,
        0,
        "a marker entirely above the viewport must be culled",
    );
}
