use std::{ops::Range, sync::Arc};

use atomic_refcell::AtomicRefCell;
use markup5ever::local_name;
use style::properties::style_structs::Border;
use style::servo_arc::Arc as ServoArc;
use style::values::computed::{BorderSideWidth, BorderStyle};
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::{
    Atom, computed_values::border_collapse::T as BorderCollapse,
    computed_values::table_layout::T as TableLayout,
};
use taffy::{
    DetailedGridInfo, LayoutPartialTree as _, ResolveOrZero, TrackSizingFunction, style_helpers,
};

use crate::BaseDocument;

use super::damage::{CONSTRUCT_BOX, CONSTRUCT_DESCENDENT, CONSTRUCT_FC};
use super::resolve_calc_value;

pub struct TableTreeWrapper<'doc> {
    pub(crate) doc: &'doc mut BaseDocument,
    pub(crate) ctx: Arc<TableContext>,
}

#[derive(Debug, Clone)]
pub struct TableContext {
    pub style: taffy::Style<Atom>,
    pub cells: Vec<TableCell>,
    pub rows: Vec<TableRow>,
    pub computed_grid_info: AtomicRefCell<Option<DetailedGridInfo>>,
    pub border_style: Option<ServoArc<Border>>,
    pub border_collapse: BorderCollapse,
}

// #[derive(Debug, Clone, Eq, PartialEq)]
// pub enum TableItemKind {
//     Row,
//     Cell,
// }

#[derive(Debug, Clone)]
pub struct TableCell {
    // kind: TableItemKind,
    node_id: usize,
    style: taffy::Style<Atom>,
}

#[derive(Debug, Clone)]
pub struct TableRow {
    // kind: TableItemKind,
    pub node_id: usize,
    pub height: f32,
}

/// The used width of one border side: zero when the side's style is
/// `none`/`hidden`. Stylo keeps the *computed* width (the initial `medium`
/// resolves to 3px) even for borderless sides, so reading the width without
/// checking the style gives every borderless side a phantom 3px border.
fn used_border_width(style: BorderStyle, width: &BorderSideWidth) -> f32 {
    if style.none_or_hidden() {
        0.0
    } else {
        width.0.to_f32_px()
    }
}

/// The widths a border contributes to the collapsed border grid, per axis:
/// x is its widest visible left/right side, y its widest visible top/bottom
/// side.
fn collapsed_axis_widths(border: &Border) -> (f32, f32) {
    let x = used_border_width(border.border_left_style, &border.border_left_width).max(
        used_border_width(border.border_right_style, &border.border_right_width),
    );
    let y = used_border_width(border.border_top_style, &border.border_top_width).max(
        used_border_width(border.border_bottom_style, &border.border_bottom_width),
    );
    (x, y)
}

pub(crate) fn build_table_context(
    doc: &mut BaseDocument,
    table_root_node_id: usize,
) -> (TableContext, Vec<usize>) {
    let mut cells: Vec<TableCell> = Vec::new();
    let mut rows: Vec<TableRow> = Vec::new();
    let mut row = 0u16;
    let mut col = 0u16;

    let root_node = &mut doc.nodes[table_root_node_id];

    let children = std::mem::take(&mut root_node.children);

    let Some(stylo_styles) = root_node.primary_styles() else {
        panic!("Ignoring table because it has no styles");
    };

    let mut style = stylo_taffy::to_taffy_style(&stylo_styles);
    style.item_is_table = true;
    // Use `dense` row-flow so that each cell scans the row from its
    // leftmost column for the first free track. Without `dense`,
    // `place_definite_secondary_axis_item` keeps a per-item secondary
    // cursor across rows, which means cells in later rows do not
    // backfill columns freed up by rowspan cells from earlier rows.
    style.grid_auto_flow = taffy::GridAutoFlow::RowDense;
    style.grid_auto_columns = Vec::new();
    style.grid_auto_rows = Vec::new();

    let is_fixed = match stylo_styles.clone_table_layout() {
        TableLayout::Fixed => true,
        TableLayout::Auto => false,
    };

    let border_collapse = stylo_styles.clone_border_collapse();
    let border_spacing = stylo_styles.clone_border_spacing().0;
    let table_border = stylo_styles.clone_border();

    drop(stylo_styles);

    let mut column_sizes: Vec<taffy::TrackSizingFunction> = Vec::new();
    let mut first_cell_border: Option<ServoArc<Border>> = None;
    let mut first_row_border_y: Option<f32> = None;
    for child_id in children.iter().copied() {
        collect_table_cells(
            doc,
            child_id,
            is_fixed,
            border_collapse,
            &mut row,
            &mut col,
            &mut cells,
            &mut rows,
            &mut column_sizes,
            &mut first_cell_border,
            &mut first_row_border_y,
        );
    }
    column_sizes.resize(col as usize, style_helpers::auto());
    // A table whose children are all block-level boxes (anonymous cells —
    // see `collect_table_cells`) discovers no columns; give it one
    // full-width column so the stacked boxes fill the table.
    if column_sizes.is_empty() && !cells.is_empty() {
        column_sizes.push(style_helpers::percent(1.0));
    }

    style.grid_template_columns = column_sizes.into_iter().map(|dim| dim.into()).collect();
    style.grid_template_rows = vec![style_helpers::auto(); row as usize];

    match border_collapse {
        BorderCollapse::Separate => {
            style.gap = taffy::Size {
                width: style_helpers::length(border_spacing.width.px()),
                height: style_helpers::length(border_spacing.height.px()),
            };
        }
        BorderCollapse::Collapse => {
            // Approximate the collapsed border grid: vertical lines take the
            // first cell's widest visible left/right border, horizontal lines
            // the wider of that cell's top/bottom border and the first
            // visible row border.
            let (cell_x, cell_y) = first_cell_border
                .as_deref()
                .map(collapsed_axis_widths)
                .unwrap_or((0.0, 0.0));
            let (grid_x, grid_y) = (cell_x, cell_y.max(first_row_border_y.unwrap_or(0.0)));
            style.gap = taffy::Size {
                width: style_helpers::length(grid_x),
                height: style_helpers::length(grid_y),
            };

            // A collapsed table's outer edge is the wider of the table's own
            // border and the grid line for that axis; `hidden` on the table's
            // side suppresses the edge entirely (CSS 2.2 §17.6.2.1).
            let edge = |side_style: BorderStyle, side_width: &BorderSideWidth, grid: f32| {
                if side_style == BorderStyle::Hidden {
                    style_helpers::length(0.0)
                } else {
                    style_helpers::length(used_border_width(side_style, side_width).max(grid))
                }
            };
            style.border = taffy::Rect {
                left: edge(
                    table_border.border_left_style,
                    &table_border.border_left_width,
                    grid_x,
                ),
                right: edge(
                    table_border.border_right_style,
                    &table_border.border_right_width,
                    grid_x,
                ),
                top: edge(
                    table_border.border_top_style,
                    &table_border.border_top_width,
                    grid_y,
                ),
                bottom: edge(
                    table_border.border_bottom_style,
                    &table_border.border_bottom_width,
                    grid_y,
                ),
            };
        }
    }

    let layout_children = cells.iter().map(|cell| cell.node_id).collect();
    let root_node = &mut doc.nodes[table_root_node_id];
    root_node.children = children;

    (
        TableContext {
            style,
            cells,
            rows,
            computed_grid_info: AtomicRefCell::new(None),
            border_collapse,
            border_style: first_cell_border,
        },
        layout_children,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_table_cells(
    doc: &mut BaseDocument,
    node_id: usize,
    is_fixed: bool,
    border_collapse: BorderCollapse,
    row: &mut u16,
    col: &mut u16,
    cells: &mut Vec<TableCell>,
    rows: &mut Vec<TableRow>,
    columns: &mut Vec<TrackSizingFunction>,
    first_cell_border: &mut Option<ServoArc<Border>>,
    first_row_border_y: &mut Option<f32>,
) {
    let node = &mut doc.nodes[node_id];

    if !node.is_element() {
        return;
    }

    let Some(display) = node.primary_styles().map(|s| s.clone_display()) else {
        #[cfg(feature = "tracing")]
        tracing::info!("Ignoring table descendent because it has no styles");
        return;
    };

    if display.outside() == DisplayOutside::None {
        node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
        return;
    }

    match display.inside() {
        DisplayInside::TableRowGroup
        | DisplayInside::TableHeaderGroup
        | DisplayInside::TableFooterGroup
        | DisplayInside::Contents => {
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                doc.nodes[child_id]
                    .remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                collect_table_cells(
                    doc,
                    child_id,
                    is_fixed,
                    border_collapse,
                    row,
                    col,
                    cells,
                    rows,
                    columns,
                    first_cell_border,
                    first_row_border_y,
                );
            }
            doc.nodes[node_id].children = children;
        }
        DisplayInside::TableRow => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            *row += 1;
            *col = 0;

            // Remember the width of the first visible horizontal row border:
            // rows contribute the horizontal lines of the collapsed border
            // grid (the `tr { border-bottom: … }` separator pattern is
            // common in HTML emails).
            if first_row_border_y.is_none() {
                let y = collapsed_axis_widths(&node.primary_styles().unwrap().clone_border()).1;
                if y > 0.0 {
                    *first_row_border_y = Some(y);
                }
            }

            rows.push(TableRow {
                node_id,
                height: 0.0,
            });

            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                collect_table_cells(
                    doc,
                    child_id,
                    is_fixed,
                    border_collapse,
                    row,
                    col,
                    cells,
                    rows,
                    columns,
                    first_cell_border,
                    first_row_border_y,
                );
            }
            doc.nodes[node_id].children = children;
        }
        DisplayInside::TableCell => {
            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            let stylo_style = &node.primary_styles().unwrap();
            let colspan: u16 = node
                .attr(local_name!("colspan"))
                .and_then(|val| val.parse().ok())
                .unwrap_or(1);
            let rowspan: u16 = node
                .attr(local_name!("rowspan"))
                .and_then(|val| val.parse::<u16>().ok())
                .map(|v| v.clamp(1, 65534))
                .unwrap_or(1);
            let mut style = stylo_taffy::to_taffy_style(stylo_style);

            if first_cell_border.is_none() {
                *first_cell_border = Some(stylo_style.clone_border());
            }

            // TODO: account for padding/border/margin
            if *row == 1 {
                let column = match style.size.width.tag() {
                    taffy::CompactLength::LENGTH_TAG => {
                        let len = style.size.width.value();
                        let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
                        style_helpers::length(len + padding.left + padding.right)
                    }
                    taffy::CompactLength::PERCENT_TAG => {
                        if is_fixed {
                            style_helpers::percent(style.size.width.value())
                        } else {
                            style_helpers::auto()
                        }
                    }
                    taffy::CompactLength::AUTO_TAG => style_helpers::auto(),
                    _ => unreachable!(),
                };
                columns.push(column);
            }

            // Zero-out cell borders is BorderCollapse is Collapse
            // Borders are handled at the table level in this mode
            if border_collapse == BorderCollapse::Collapse {
                style.border = taffy::Rect::ZERO.map(style_helpers::length);
            }

            // Let Taffy auto-place the column. Combined with
            // `grid_auto_flow: RowDense` set on the table root, each cell
            // scans from the first track in its row for a free position,
            // which makes cells automatically skip columns occupied by
            // rowspan cells from earlier rows.
            style.grid_column = taffy::Line {
                start: style_helpers::auto(),
                end: style_helpers::span(colspan),
            };
            style.grid_row = taffy::Line {
                start: style_helpers::line(*row as i16),
                end: style_helpers::span(rowspan),
            };
            style.size.width = style_helpers::auto();
            cells.push(TableCell { node_id, style });

            *col += colspan;
        }
        DisplayInside::Flow
        | DisplayInside::FlowRoot
        | DisplayInside::Flex
        | DisplayInside::Grid => {
            // CSS 2.2 §17.2.1: a block-level box whose parent is a table
            // or table-row gets wrapped in an anonymous table-cell. HTML
            // emails rely on this: responsive templates switch cells to
            // `display:block` below a width breakpoint to stack table
            // columns vertically. Approximate the anonymous cell by
            // placing each such box on its own full-width grid row
            // (previously these boxes were dropped entirely, collapsing
            // such emails to a blank sliver). The column cursor is left
            // untouched: it feeds first-row column discovery and the
            // final `column_sizes.resize(col, …)`, which would truncate a
            // real table's tracks if a trailing block child reset it.
            *row += 1;
            let stylo_style = &node.primary_styles().unwrap();
            let mut style = stylo_taffy::to_taffy_style(stylo_style);
            style.grid_column = taffy::Line {
                start: style_helpers::line(1),
                end: style_helpers::line(-1),
            };
            style.grid_row = taffy::Line {
                start: style_helpers::line(*row as i16),
                end: style_helpers::span(1),
            };
            cells.push(TableCell { node_id, style });
        }
        DisplayInside::TableColumnGroup | DisplayInside::TableColumn | DisplayInside::Table => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            //Ignore
        }
        DisplayInside::None => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            // Ignore
        }
    }
}

pub struct RangeIter(Range<usize>);

impl Iterator for RangeIter {
    type Item = taffy::NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(taffy::NodeId::from)
    }
}

impl taffy::TraversePartialTree for TableTreeWrapper<'_> {
    type ChildIter<'a>
        = RangeIter
    where
        Self: 'a;

    #[inline(always)]
    fn child_ids(&self, _node_id: taffy::NodeId) -> Self::ChildIter<'_> {
        RangeIter(0..self.ctx.cells.len())
    }

    #[inline(always)]
    fn child_count(&self, _node_id: taffy::NodeId) -> usize {
        self.ctx.cells.len()
    }

    #[inline(always)]
    fn get_child_id(&self, _node_id: taffy::NodeId, index: usize) -> taffy::NodeId {
        index.into()
    }
}
impl taffy::TraverseTree for TableTreeWrapper<'_> {}

impl taffy::LayoutPartialTree for TableTreeWrapper<'_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type CustomIdent = Atom;

    fn get_core_container_style(&self, _node_id: taffy::NodeId) -> &taffy::Style<Atom> {
        &self.ctx.style
    }

    fn resolve_calc_value(&self, calc_ptr: *const (), parent_size: f32) -> f32 {
        resolve_calc_value(calc_ptr, parent_size)
    }

    fn set_unrounded_layout(&mut self, node_id: taffy::NodeId, layout: &taffy::Layout) {
        let node_id = taffy::NodeId::from(self.ctx.cells[usize::from(node_id)].node_id);
        self.doc.set_unrounded_layout(node_id, layout)
    }

    fn compute_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: taffy::tree::LayoutInput,
    ) -> taffy::LayoutOutput {
        let cell = &self.ctx.cells[usize::from(node_id)];
        let node_id = taffy::NodeId::from(cell.node_id);
        self.doc.compute_child_layout(node_id, inputs)
    }
}

impl taffy::LayoutGridContainer for TableTreeWrapper<'_> {
    type GridContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type GridItemStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: taffy::NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: taffy::NodeId) -> Self::GridItemStyle<'_> {
        &self.ctx.cells[usize::from(child_node_id)].style
    }

    fn set_detailed_grid_info(
        &mut self,
        _node_id: taffy::NodeId,
        detailed_grid_info: DetailedGridInfo,
    ) {
        *self.ctx.computed_grid_info.borrow_mut() = Some(detailed_grid_info);
    }
}
