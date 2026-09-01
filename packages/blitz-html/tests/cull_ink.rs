//! An element is skipped entirely when the rect it could ink falls outside the clip
//! rectangle it is being painted into, and "skipped" includes its shadow: nothing of a
//! culled element is drawn.
//!
//! The rect it is culled against is `scrollable_overflow`, which is layout — a border box
//! unioned with the children's layout rects. `box-shadow` and `filter` ink *outside* that
//! box, so culling on it alone drops a shadow whose box has scrolled off the edge of the
//! viewport while its blur or its offset has not. The painter grows the rect by
//! `ink_margin`, the furthest any one element in the document inks past its own box, before
//! comparing.
//!
//! The margin is a document-wide maximum rather than a per-subtree one because the cull
//! that decides a shadow's fate is often an *ancestor's*: `scrollable_overflow` contains
//! the children's layout boxes, so a parent is only culled when every descendant's box is
//! out too — and the early return means the descendant whose shadow reaches back in is
//! never visited to have its own cull relaxed. `a_descendants_shadow_reaching_in_is_painted`
//! is that case, and it is the one a per-element margin does not fix.
//!
//! The recording backend is used rather than a rasterizer because what is being asserted is
//! whether the draw was issued at all, not what it looks like.

use anyrender::Scene;
use anyrender::recording::RenderCommand;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

/// The page's size in CSS pixels.
const SIZE: f32 = 100.0;

const SCALES: [f32; 2] = [1.0, 2.0];

/// How many red box-shadows painting this page records.
fn red_shadows(html: &str, scale: f32) -> usize {
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
            RenderCommand::BoxShadow(shadow) => {
                let [r, g, b, a] = shadow.brush.components;
                r > 0.99 && g < 0.01 && b < 0.01 && a > 0.99
            }
            _ => false,
        })
        .count()
}

#[track_caller]
fn assert_red_shadows(html: &str, expected: usize, message: &str) {
    for scale in SCALES {
        let found = red_shadows(html, scale);
        assert_eq!(
            found, expected,
            "{message}: at scale {scale}, expected {expected} red shadow(s), found {found}"
        );
    }
}

/// A box whose own border box sits 20px clear above the viewport, carrying an unblurred
/// shadow offset 130px down — so the box is off screen and its ink is not.
const OWN_OFFSET: &str = r#"<html><body style="margin:0">
    <div style="position:absolute; top:-120px; left:0; width:100px; height:100px;
                box-shadow: 0 130px 0 0 #ff0000;"></div>
</body></html>"#;

/// The same, with the ink carried by a blur rather than an offset: the box's bottom edge is
/// 10px above the viewport and a 20px blur radius is a 10px deviation, which inks a further
/// 50px down.
const OWN_BLUR: &str = r#"<html><body style="margin:0">
    <div style="position:absolute; top:-110px; left:0; width:100px; height:100px;
                box-shadow: 0 0 20px 0 #ff0000;"></div>
</body></html>"#;

/// The shadow belongs to a child, and the parent wraps it exactly — so the parent's
/// scrollable overflow is entirely off screen and the parent is what gets culled.
const DESCENDANT: &str = r#"<html><body style="margin:0">
    <div style="position:absolute; top:-120px; left:0; width:100px; height:100px;">
        <div style="width:100px; height:100px;
                    box-shadow: 0 130px 0 0 #ff0000;"></div>
    </div>
</body></html>"#;

/// Far enough above that even the 130px the shadow reaches leaves it off screen.
const OUT_OF_REACH: &str = r#"<html><body style="margin:0">
    <div style="position:absolute; top:-400px; left:0; width:100px; height:100px;
                box-shadow: 0 130px 0 0 #ff0000;"></div>
</body></html>"#;

#[test]
fn an_offset_shadow_reaching_into_the_viewport_is_painted() {
    assert_red_shadows(
        OWN_OFFSET,
        1,
        "a box 20px above the viewport with its shadow offset 130px down inks on screen, so \
         culling it on its layout box alone loses that ink",
    );
}

#[test]
fn a_blurred_shadow_reaching_into_the_viewport_is_painted() {
    assert_red_shadows(
        OWN_BLUR,
        1,
        "a box 10px above the viewport with a 20px blur radius inks on screen, so culling it \
         on its layout box alone loses that ink",
    );
}

#[test]
fn a_descendants_shadow_reaching_in_is_painted() {
    assert_red_shadows(
        DESCENDANT,
        1,
        "the shadow is a child's, but the parent wraps it exactly, so the parent's own cull \
         is what decides: a margin that only relaxed the shadowed element's cull would never \
         be reached",
    );
}

#[test]
fn a_shadow_that_cannot_reach_the_viewport_is_still_culled() {
    assert_red_shadows(
        OUT_OF_REACH,
        0,
        "the margin has to be the ink's reach and no more; a box 300px above the viewport \
         with a 130px shadow inks nothing on screen and must still be culled",
    );
}
