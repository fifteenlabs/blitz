use blitz_dom::util::ToColorColor as _;
use style::color::AbsoluteColor;
pub(crate) use style::computed_values::filter::single_value::T as StyloFilter;

use anyrender::filters::{Filter, FilterEffect};

/// Convert a computed `filter`/`backdrop-filter` list into an anyrender filter graph, with
/// every length in the device pixels the rest of the painted geometry is written in.
///
/// A filter's lengths live in the same space as the geometry the filter is applied to, and
/// nothing between here and a backend can put them there: the CTM this file builds never
/// carries the device scale — `create_css_rect` multiplies the box into device pixels, a
/// clip path gets an explicit `Affine::scale(self.scale)`, and the layer transform is a
/// translation by an already-scaled position — so a backend has no factor to recover.
/// `expansion_rect`, which decides how far the filtered layer's own region is grown, is
/// derived from these same numbers, so leaving them in CSS pixels also grows the region by
/// too little at any scale above one.
///
/// This is the same rule `box_shadow::std_dev` follows for a `box-shadow`'s blur, for the
/// same reason.
pub(crate) fn convert_filters(filters: &[StyloFilter], scale: f32) -> Option<Filter> {
    if filters.is_empty() {
        return None;
    }

    Some(Filter::linear_list(
        filters
            .iter()
            .filter_map(|filter| convert_single_filter(filter, scale)),
    ))
}

pub(crate) fn convert_single_filter(filter: &StyloFilter, scale: f32) -> Option<FilterEffect> {
    Some(match filter {
        StyloFilter::Blur(radius) => FilterEffect::blur(radius.px() * scale),
        StyloFilter::Brightness(amount) => FilterEffect::brightness(amount.0),
        StyloFilter::Contrast(amount) => FilterEffect::contrast(amount.0),
        StyloFilter::Grayscale(amount) => FilterEffect::grayscale(amount.0),
        StyloFilter::HueRotate(angle) => FilterEffect::hue_rotate(angle.radians()),
        StyloFilter::Invert(amount) => FilterEffect::invert(amount.0),
        StyloFilter::Opacity(amount) => FilterEffect::opacity(amount.0),
        StyloFilter::Saturate(amount) => FilterEffect::saturate(amount.0),
        StyloFilter::Sepia(amount) => FilterEffect::sepia(amount.0),
        StyloFilter::DropShadow(shadow) => FilterEffect::drop_shadow(
            shadow.horizontal.px() * scale,
            shadow.vertical.px() * scale,
            // `drop-shadow()`'s third length is a blur radius, interpreted as
            // `box-shadow`'s is, so the deviation is half of it. `blur()` above
            // is different: CSS gives that one the deviation directly.
            shadow.blur.px() * 0.5 * scale,
            // TODO: pass in correct currentColor
            shadow
                .color
                .resolve_to_absolute(&AbsoluteColor::BLACK)
                .as_color_color(),
        ),
        StyloFilter::Url(_) => return None,
    })
}
