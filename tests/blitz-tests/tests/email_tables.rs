//! Table layouts HTML email depends on, checked by the pixels they paint: block-level
//! children of a table stacking as anonymous cells, collapsed borders taking each edge's
//! own colour and the rows' separators, `valign`, a fixed-width cell beside an auto
//! one sizing its row, and a cell width that is only a floor for its column.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const SIZE: usize = 240;

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

fn document(html: &str) -> HtmlDocument {
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
    doc
}

fn render(html: &str) -> Vec<u8> {
    let mut doc = document(html);
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, SIZE as u32, SIZE as u32, 0, 0),
        SIZE as u32,
        SIZE as u32,
    )
}

fn pixel(buffer: &[u8], x: usize, y: usize) -> [u8; 3] {
    let idx = (y * SIZE + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

fn page(body: &str) -> String {
    format!(r#"<html><body style="margin:0; background:#ffffff;">{body}</body></html>"#)
}

#[test]
fn block_children_of_a_table_stack_as_anonymous_cells() {
    let buffer = render(&page(
        r#"<table cellpadding="0" cellspacing="0" style="width:200px"><tr>
            <td style="display:block"><div style="height:40px; background:#ff0000"></div></td>
            <td style="display:block"><div style="height:40px; background:#0000ff"></div></td>
        </tr></table>"#,
    ));
    assert_eq!(
        pixel(&buffer, 100, 20),
        RED,
        "the first block is the first row"
    );
    assert_eq!(
        pixel(&buffer, 100, 60),
        BLUE,
        "the second block stacks under the first, full width, rather than being dropped"
    );
}

#[test]
fn a_fixed_width_cell_beside_an_auto_cell_sizes_its_row() {
    let buffer = render(&page(
        r#"<table cellpadding="0" cellspacing="0"><tr>
            <td></td>
            <td width="150"><div style="height:100px; background:#ff0000"></div></td>
        </tr></table>
        <div style="height:40px; background:#0000ff"></div>"#,
    ));
    assert_eq!(
        pixel(&buffer, 50, 50),
        RED,
        "the fixed-width cell keeps its height"
    );
    assert_eq!(
        pixel(&buffer, 50, 120),
        BLUE,
        "the block after the table starts below the 100px cell"
    );
}

#[test]
fn a_borderless_collapsed_table_draws_no_grid() {
    let buffer = render(&page(
        r#"<table style="border-collapse:collapse" cellpadding="0"><tr>
            <td style="background:#0000ff; width:100px; height:40px"></td>
            <td style="background:#0000ff; width:100px; height:40px"></td>
        </tr></table>"#,
    ));
    for x in 95..106 {
        assert_eq!(
            pixel(&buffer, x, 20),
            BLUE,
            "no grid line at x={x}: borderless sides draw nothing"
        );
    }
}

#[test]
fn a_collapsed_cell_border_keeps_its_own_colour() {
    let buffer = render(&page(
        r#"<table style="border-collapse:collapse" cellpadding="0"><tr>
            <td style="background:#0000ff; width:100px; height:40px; border-right:10px solid #00ff00"></td>
            <td style="background:#0000ff; width:100px; height:40px"></td>
        </tr></table>"#,
    ));
    let seam = (0..SIZE)
        .map(|x| pixel(&buffer, x, 20))
        .find(|colour| *colour != BLUE);
    assert_eq!(
        seam,
        Some(GREEN),
        "the gutter is the cell's own green border, not its border-top colour or nothing"
    );
}

#[test]
fn a_collapsed_row_border_separates_rows() {
    let buffer = render(&page(
        r#"<table style="border-collapse:collapse; width:200px" cellpadding="0">
            <tr style="border-bottom:4px solid #ff0000"><td style="height:40px"></td></tr>
            <tr><td style="height:40px"></td></tr>
        </table>"#,
    ));
    let line = (0..100).find(|y| pixel(&buffer, 100, *y) == RED);
    assert!(
        line.is_some_and(|y| (36..48).contains(&y)),
        "a red separator between the rows, found at {line:?}"
    );
}

#[test]
fn valign_bottom_anchors_a_cell_to_its_bottom() {
    let buffer = render(&page(
        r#"<table style="width:100px; border-collapse:collapse">
            <tr><td valign="bottom" style="height:110px; font-size:0; line-height:0">
                <div style="height:55px; background:#ff0000"></div>
            </td></tr>
        </table>"#,
    ));
    assert_eq!(
        pixel(&buffer, 30, 30),
        WHITE,
        "the top of the cell is empty"
    );
    assert_eq!(
        pixel(&buffer, 30, 100),
        RED,
        "the content sits at the bottom"
    );
}

#[test]
fn a_cell_width_narrower_than_its_content_widens_the_column() {
    let buffer = render(&page(
        r#"<table cellpadding="0" cellspacing="0" style="width:200px"><tr>
            <td style="width:10px"><div style="width:100px; height:40px; background:#ff0000"></div></td>
            <td style="width:100%; background:#0000ff"><div style="height:40px"></div></td>
        </tr></table>"#,
    ));
    assert_eq!(
        pixel(&buffer, 50, 20),
        RED,
        "the column grows to the content's 100px rather than staying at 10px"
    );
    assert_eq!(
        pixel(&buffer, 150, 20),
        BLUE,
        "the next column starts after the widened one"
    );
}

#[test]
fn a_cell_width_still_sizes_a_fixed_layout_column() {
    let buffer = render(&page(
        r#"<table cellpadding="0" cellspacing="0" style="width:200px; table-layout:fixed"><tr>
            <td style="width:10px; overflow:hidden"><div style="width:100px; height:40px; background:#ff0000"></div></td>
            <td style="background:#0000ff"><div style="height:40px"></div></td>
        </tr></table>"#,
    ));
    assert_eq!(
        pixel(&buffer, 50, 20),
        BLUE,
        "fixed layout ignores the content, so the second column starts at 10px"
    );
}

fn inline_rects(body: &str) -> impl Fn(&str) -> (f32, f32) {
    let doc = document(&page(body));
    move |selector| {
        let id = doc.query_selector(selector).unwrap().unwrap();
        let node = doc.get_node(id).unwrap();
        let top = node.absolute_position(0.0, 0.0).y;
        (top, top + node.final_layout().size.height)
    }
}

#[test]
fn a_middle_aligned_table_cell_inside_an_inline_sits_on_its_bottom_edge() {
    let rect = inline_rects(
        r#"<div style="font-size:20px; line-height:30px"><a>
            <span id="box" style="display:inline-block; width:20px; height:40px"></span>
            <span id="cell" style="display:table-cell; vertical-align:middle">3</span>
        </a></div>"#,
    );
    assert_eq!(
        rect("#cell").1,
        rect("#box").1,
        "a row without baseline-aligned cells takes its baseline from the cell's bottom \
         edge, so the anonymous inline-table sits on the line by it, like an empty \
         inline-block"
    );
}

#[test]
fn a_baseline_aligned_table_cell_inside_an_inline_sits_on_its_text() {
    let rect = inline_rects(
        r#"<div style="font-size:20px; line-height:30px"><a>
            <span style="display:inline-block; width:20px; height:40px"></span>
            <span id="block" style="display:inline-block">3</span>
            <span id="cell" style="display:table-cell">3</span>
        </a></div>"#,
    );
    assert_eq!(rect("#cell").0, rect("#block").0);
}
