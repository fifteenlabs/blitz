//! `draw_box_shadow` takes a *standard deviation*, in the coordinate space of the
//! `rect` it is handed alongside it — a backend applies the one `transform` to the
//! rect, the corner radius and the deviation together.
//!
//! CSS gives a `box-shadow` a blur *radius*, and defines the gaussian it asks for as
//! having a standard deviation of half that radius (CSS Backgrounds 3 § 5.4). Every
//! other length painted from `blitz-paint` has already been multiplied by the device
//! scale by the time it reaches a backend. Both of those have to happen to the blur
//! too, at both call sites, or an inset shadow and an outset one of the same radius
//! come out different sizes.

use anyrender::Scene;
use anyrender::recording::{BoxShadowCommand, RenderCommand};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const SIZE: u32 = 200;

const BLUR: f64 = 12.0;

const SIGMA: f64 = BLUR / 2.0;

const OFFSET: f64 = 20.0;

fn box_shadows(shadow: &str, scale: f32) -> Vec<BoxShadowCommand> {
    let html = format!(
        r#"<html><body style="margin:0"><div style="width:100px; height:60px; box-shadow:{shadow};"></div></body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(SIZE, SIZE, scale, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let mut scene = Scene::new();
    paint_scene(&mut scene, &mut doc, scale as f64, SIZE, SIZE, 0, 0);
    scene
        .commands
        .into_iter()
        .filter_map(|command| match command {
            RenderCommand::BoxShadow(shadow) => Some(shadow),
            _ => None,
        })
        .collect()
}

#[track_caller]
fn only_shadow(shadow: &str, scale: f32) -> BoxShadowCommand {
    let found = box_shadows(shadow, scale);
    assert_eq!(
        found.len(),
        1,
        "expected `box-shadow: {shadow}` to paint exactly one blurred rect at scale {scale}"
    );
    found.into_iter().next().unwrap()
}

#[test]
fn outset_blur_is_half_the_radius_in_device_pixels() {
    for scale in [1.0, 2.0, 3.0] {
        let shadow = only_shadow(&format!("0 0 {BLUR}px #000"), scale);
        assert_eq!(
            shadow.std_dev,
            SIGMA * scale as f64,
            "a {BLUR}px blur radius is a gaussian of {SIGMA} CSS pixels' deviation, and the \
             rect it is handed with is in device pixels, so at scale {scale} it has to be \
             {}",
            SIGMA * scale as f64
        );
    }
}

#[test]
fn inset_blur_is_half_the_radius_in_device_pixels() {
    for scale in [1.0, 2.0, 3.0] {
        let shadow = only_shadow(&format!("inset 0 0 {BLUR}px #000"), scale);
        assert_eq!(
            shadow.std_dev,
            SIGMA * scale as f64,
            "an inset shadow is the same gaussian punched out of a fill; at scale {scale} it \
             has to be {}",
            SIGMA * scale as f64
        );
    }
}

#[test]
fn an_inset_and_an_outset_shadow_of_one_radius_are_one_gaussian() {
    for scale in [1.0, 2.0, 3.0] {
        let outset = only_shadow(&format!("0 0 {BLUR}px #000"), scale);
        let inset = only_shadow(&format!("inset 0 0 {BLUR}px #000"), scale);
        assert_eq!(
            outset.std_dev, inset.std_dev,
            "the two call sites paint the same blur radius at scale {scale}, so they cannot \
             disagree about what it means"
        );
    }
}

#[test]
fn a_shadow_offset_is_in_device_pixels_like_the_rect_it_moves() {
    for shadow in ["{OFFSET}px 0 0 #000", "inset {OFFSET}px 0 0 #000"] {
        let shadow = shadow.replace("{OFFSET}", &OFFSET.to_string());
        for scale in [1.0, 2.0, 3.0] {
            let still = only_shadow(&shadow.replace(&format!("{OFFSET}px 0"), "0 0"), scale);
            let moved = only_shadow(&shadow, scale);
            assert_eq!(
                moved.transform.translation().x - still.transform.translation().x,
                OFFSET * scale as f64,
                "`{shadow}` moves the shadow {OFFSET} CSS pixels, which is {} device pixels \
                 at scale {scale}",
                OFFSET * scale as f64
            );
        }
    }
}

#[test]
fn the_blurred_rect_is_the_border_box_in_device_pixels() {
    for scale in [1.0, 2.0, 3.0] {
        let shadow = only_shadow(&format!("0 0 {BLUR}px #000"), scale);
        assert_eq!(
            (shadow.rect.width(), shadow.rect.height()),
            (100.0 * scale as f64, 60.0 * scale as f64),
            "the rect the deviation is measured against is the border box in device pixels; \
             a deviation in CSS pixels would not be in the same space as it"
        );
    }
}
