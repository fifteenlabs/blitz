//! A `filter`'s lengths are in the same space as the geometry the filter is applied to,
//! which for `blitz-paint` is device pixels: `create_css_rect` multiplies the box by the
//! device scale, a clip path is given an explicit `Affine::scale`, and the transform a
//! layer carries is a translation by an already-scaled position. Nothing downstream can
//! recover the scale from the CTM, so the lengths have to arrive already carrying it —
//! the same rule `box-shadow`'s blur follows, and for the same reason.
//!
//! `Filter::expansion_rect` is derived from those same lengths and decides how far the
//! filtered layer's own region is grown, so a filter left in CSS pixels also renders into
//! a region that is too small at any scale above one.
//!
//! The recording backend is used rather than a rasterizer because it hands back the exact
//! arguments: `anyrender_vello_cpu` drops filters when its `multithreading` feature is on,
//! which is how this workspace builds it.

use anyrender::Scene;
use anyrender::filters::{Filter, FilterEffect};
use anyrender::recording::{LayerCommand, RenderCommand};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use kurbo::Shape as _;
use std::sync::Arc;

const SIZE: u32 = 400;

const WIDTH: f64 = 100.0;

const HEIGHT: f64 = 40.0;

const BLUR: f32 = 4.0;

const SCALES: [f32; 3] = [1.0, 2.0, 3.0];

/// Every layer the given `style` on a fixed-size box pushes, in paint order.
fn layers(style: &str, scale: f32) -> Vec<LayerCommand> {
    let html = format!(
        r#"<html><body style="margin:0"><div style="width:{WIDTH}px; height:{HEIGHT}px; background:#000; {style}"></div></body></html>"#
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
            RenderCommand::PushLayer(layer) => Some(layer),
            _ => None,
        })
        .collect()
}

#[track_caller]
fn only_filtered_layer(style: &str, scale: f32) -> LayerCommand {
    let found: Vec<LayerCommand> = layers(style, scale)
        .into_iter()
        .filter(|layer| layer.filter.is_some() || layer.backdrop_filter.is_some())
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected `{style}` to push exactly one filtered layer at scale {scale}"
    );
    found.into_iter().next().unwrap()
}

#[track_caller]
fn only_effect(filter: &Option<Arc<Filter>>, what: &str, scale: f32) -> FilterEffect {
    let filter = filter
        .as_ref()
        .unwrap_or_else(|| panic!("no {what} reached the layer at scale {scale}"));
    let nodes = filter.nodes();
    assert_eq!(
        nodes.len(),
        1,
        "expected one {what} primitive at scale {scale}, got {nodes:?}"
    );
    nodes[0].effect.clone()
}

#[track_caller]
fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-3,
        "{what}: expected {expected}, got {actual}"
    );
}

#[test]
fn a_blurs_deviation_is_in_device_pixels() {
    for scale in SCALES {
        let layer = only_filtered_layer(&format!("filter:blur({BLUR}px);"), scale);
        let FilterEffect::GaussianBlur(blur) = only_effect(&layer.filter, "filter", scale) else {
            panic!("blur({BLUR}px) did not reach the layer as a gaussian at scale {scale}");
        };
        assert_close(
            blur.std_deviation as f64,
            (BLUR * scale) as f64,
            &format!("the deviation of blur({BLUR}px) at scale {scale}"),
        );
    }
}

#[test]
fn a_backdrop_filters_deviation_is_in_device_pixels_too() {
    for scale in SCALES {
        let layer = only_filtered_layer(&format!("backdrop-filter:blur({BLUR}px);"), scale);
        let FilterEffect::GaussianBlur(blur) =
            only_effect(&layer.backdrop_filter, "backdrop-filter", scale)
        else {
            panic!("backdrop-filter: blur({BLUR}px) did not reach the layer as a gaussian");
        };
        assert_close(
            blur.std_deviation as f64,
            (BLUR * scale) as f64,
            &format!("the deviation of a backdrop blur({BLUR}px) at scale {scale}"),
        );
    }
}

#[test]
fn a_drop_shadows_offset_and_deviation_are_in_device_pixels() {
    for scale in SCALES {
        let layer = only_filtered_layer("filter:drop-shadow(4px 8px 2px #f00);", scale);
        let FilterEffect::DropShadow(shadow) = only_effect(&layer.filter, "filter", scale) else {
            panic!("drop-shadow() did not reach the layer as a shadow at scale {scale}");
        };
        let scale = scale as f64;
        assert_close(
            shadow.dx as f64,
            4.0 * scale,
            "a drop shadow's horizontal offset",
        );
        assert_close(
            shadow.dy as f64,
            8.0 * scale,
            "a drop shadow's vertical offset",
        );
        assert_close(
            shadow.std_deviation as f64,
            1.0 * scale,
            "a drop shadow's deviation, which is half the 2px blur radius it was given",
        );
    }
}

#[test]
fn a_blurs_layer_region_is_the_border_box_grown_by_three_deviations() {
    // The region a filter renders into is the border box grown by the filter's expansion
    // area, which for a gaussian is three standard deviations. That is the clip the layer
    // is pushed with, and it has to grow with the device scale like the border box does —
    // otherwise the tail is cut off closer in the more pixels there are to cut it at.
    for scale in SCALES {
        let layer = only_filtered_layer(&format!("filter:blur({BLUR}px);"), scale);
        let region = layer.clip.bounding_box();
        let scale = scale as f64;
        let reach = 3.0 * BLUR as f64 * scale;
        assert_close(region.x0, -reach, "the region's left edge");
        assert_close(region.y0, -reach, "the region's top edge");
        assert_close(region.x1, WIDTH * scale + reach, "the region's right edge");
        assert_close(
            region.y1,
            HEIGHT * scale + reach,
            "the region's bottom edge",
        );
    }
}

#[test]
fn a_drop_shadows_layer_region_grows_the_way_the_shadow_reaches() {
    // A drop shadow reaches three deviations in every direction and its offset in one, so
    // its region is asymmetric. Both halves of that are lengths, and both scale.
    for scale in SCALES {
        let layer = only_filtered_layer("filter:drop-shadow(4px 8px 2px #f00);", scale);
        let region = layer.clip.bounding_box();
        let scale = scale as f64;
        let reach = 3.0 * scale;
        assert_close(region.x0, -reach, "the region's left edge");
        assert_close(region.y0, -reach, "the region's top edge");
        assert_close(
            region.x1,
            WIDTH * scale + reach + 4.0 * scale,
            "the region's right edge, which the shadow's own offset pushes out",
        );
        assert_close(
            region.y1,
            HEIGHT * scale + reach + 8.0 * scale,
            "the region's bottom edge, which the shadow's own offset pushes out",
        );
    }
}

#[test]
fn an_unfiltered_layer_is_not_grown() {
    // Control: the expansion is the filter's, so a layer pushed for `opacity` alone keeps
    // whatever region it had.
    for scale in SCALES {
        let found: Vec<LayerCommand> = layers("opacity:0.5;", scale)
            .into_iter()
            .filter(|layer| layer.alpha < 0.999)
            .collect();
        assert_eq!(found.len(), 1, "expected one half-opaque layer");
        let region = found[0].clip.bounding_box();
        let scale = scale as f64;
        assert_close(region.x0, 0.0, "the region's left edge");
        assert_close(region.y0, 0.0, "the region's top edge");
        assert_close(region.x1, WIDTH * scale, "the region's right edge");
        assert_close(region.y1, HEIGHT * scale, "the region's bottom edge");
    }
}
