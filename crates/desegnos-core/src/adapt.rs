//! Deterministic linked-size layout. Base geometry remains the source of truth.
use crate::{
    Element, ElementKind, ElementLayout, ElementOverride, Page, Rect, Result, StackDirection,
};
use uuid::Uuid;

/// Rebuild automatic overrides, retaining every manually authored override.
pub fn adapt_size(page: &mut Page, size_id: Uuid) -> Result<()> {
    adapt_with_styles(page, size_id, &crate::ProjectStyles::default())
}

pub(crate) fn adapt_with_styles(
    page: &mut Page,
    size_id: Uuid,
    styles: &crate::ProjectStyles,
) -> Result<()> {
    let base = page.base_size().clone();
    let size = page
        .size(size_id)
        .ok_or_else(|| crate::Error("This size no longer exists.".into()))?;
    if size.width >= base.width {
        return Ok(());
    }
    let width = f64::from(size.width);
    let minimum_height = f64::from(size.height);
    let mut measure = crate::render::ResponsiveTextMeasure::new(styles.clone());
    let height = children(
        &mut page.elements,
        size_id,
        f64::from(base.width),
        width,
        &ElementLayout::Free,
        &mut measure,
    );
    page.size_mut(size_id).unwrap().height = height
        .ceil()
        .max(minimum_height)
        .min(f64::from(crate::MAX_VIEWPORT_SIDE))
        as u32;
    Ok(())
}

pub(crate) fn adapt_changed_pages(
    before: &crate::Project,
    after: &mut crate::Project,
) -> Result<()> {
    for page in &mut after.pages {
        let old = before.pages.iter().find(|p| p.id == page.id);
        // Request history, names and size-only edits do not invalidate base geometry.
        fn same_base(a: &[Element], b: &[Element]) -> bool {
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| {
                    a.id == b.id
                        && a.rect == b.rect
                        && a.style == b.style
                        && a.layout == b.layout
                        && a.kind == b.kind
                        && a.text == b.text
                        && a.visible == b.visible
                        && a.rotation == b.rotation
                        && a.link == b.link
                        && same_base(&a.children, &b.children)
                })
        }
        if before.styles == after.styles
            && old.is_some_and(|p| {
                p.base_size().width == page.base_size().width
                    && p.base_size().height == page.base_size().height
                    && same_base(&p.elements, &page.elements)
                    && p.sizes
                        .iter()
                        .map(|s| (s.id, s.width))
                        .eq(page.sizes.iter().map(|s| (s.id, s.width)))
            })
        {
            continue;
        }
        let ids: Vec<_> = page
            .sizes
            .iter()
            .filter(|s| s.width < page.base_size().width)
            .map(|s| s.id)
            .collect();
        for id in ids {
            adapt_with_styles(page, id, &after.styles)?;
        }
    }
    Ok(())
}

fn decoration(e: &Element, siblings: &[Element], base_width: f64) -> bool {
    fn visual_only(e: &Element) -> bool {
        e.text.is_empty()
            && matches!(
                e.kind,
                ElementKind::Box | ElementKind::Ellipse | ElementKind::Line | ElementKind::Group
            )
            && e.children.iter().all(visual_only)
    }
    if !visual_only(e) {
        return false;
    }
    e.rect.width >= base_width * 0.8
        || matches!(
            e.kind,
            ElementKind::Ellipse | ElementKind::Line | ElementKind::Group
        )
        || siblings.iter().any(|s| {
            s.id != e.id
                && s.visible
                && s.rect.x >= e.rect.x
                && s.rect.y >= e.rect.y
                && s.rect.x + s.rect.width <= e.rect.x + e.rect.width
                && s.rect.y + s.rect.height <= e.rect.y + e.rect.height
        })
}

fn effective(e: &Element, id: Uuid) -> Rect {
    e.overrides.get(&id).and_then(|o| o.rect).unwrap_or(e.rect)
}
fn manual(e: &Element, id: Uuid) -> bool {
    e.overrides.get(&id).is_some_and(|o| !o.automatic)
}

fn place(
    e: &mut Element,
    id: Uuid,
    mut rect: Rect,
    measure: &mut crate::render::ResponsiveTextMeasure,
) -> Rect {
    if manual(e, id) {
        let rect = effective(e, id);
        let layout = e.overrides[&id]
            .layout
            .clone()
            .unwrap_or_else(|| e.layout.clone());
        children(
            &mut e.children,
            id,
            e.rect.width,
            rect.width,
            &layout,
            measure,
        );
        return rect;
    }
    let ratio = (rect.width / e.rect.width.max(1.0)).min(1.0);
    let mut style = measure.resolve_style(&e.style);
    style.references.text = None;
    style.sizing.width = crate::SizingMode::Fixed;
    style.sizing.height = crate::SizingMode::Fixed;
    style.sizing.min_width = style.sizing.min_width.map(|v| v.min(rect.width));
    if !e.text.is_empty() {
        let original = style.text_style.size;
        let minimum = if original >= 28.0 {
            (original * 0.6).clamp(22.0, 40.0)
        } else {
            14.0
        };
        style.text_style.size = (original * ratio)
            .max(minimum)
            .min((rect.width / 4.0).max(14.0));
        rect.height = measure.height(e, &style, rect.width).max(
            if matches!(e.kind, ElementKind::Button | ElementKind::Link) {
                40.0
            } else {
                0.0
            },
        );
    } else if matches!(e.kind, ElementKind::Image) {
        rect.height = e.rect.height * ratio;
    }
    if matches!(
        e.kind,
        ElementKind::Button
            | ElementKind::Input { .. }
            | ElementKind::Textarea
            | ElementKind::Select { .. }
            | ElementKind::Checkbox
            | ElementKind::Radio
    ) {
        rect.height = rect.height.max(40.0);
        style.text_style.size = style.text_style.size.max(14.0);
    }
    let mut layout = e.layout.clone();
    match &mut layout {
        ElementLayout::Stack {
            direction,
            gap,
            padding,
            ..
        } => {
            *padding = (*padding * ratio).min(24.0);
            *gap = (*gap * ratio).clamp(8.0, 24.0);
            let needed = e.children.iter().map(|c| c.rect.width).sum::<f64>()
                + *gap * e.children.len().saturating_sub(1) as f64;
            if matches!(direction, StackDirection::Row) && needed > rect.width - 2.0 * *padding {
                *direction = StackDirection::Column;
            }
        }
        ElementLayout::Grid {
            columns,
            gap,
            padding,
        } => {
            *padding = (*padding * ratio).min(24.0);
            *gap = (*gap * ratio).clamp(8.0, 24.0);
            *columns = (*columns).min(
                ((rect.width - 2.0 * *padding + *gap) / (220.0 + *gap))
                    .floor()
                    .max(1.0) as u32,
            );
        }
        _ => {}
    }
    if !e.children.is_empty() {
        let bottom = children(
            &mut e.children,
            id,
            e.rect.width,
            rect.width,
            &layout,
            measure,
        );
        rect.height = rect.height.max(bottom);
    }
    e.overrides.insert(
        id,
        ElementOverride {
            automatic: true,
            rect: Some(rect),
            style: Some(style),
            layout: e.kind.holds_children().then_some(layout),
            ..Default::default()
        },
    );
    rect
}

fn children(
    elements: &mut [Element],
    id: Uuid,
    base_width: f64,
    width: f64,
    layout: &ElementLayout,
    measure: &mut crate::render::ResponsiveTextMeasure,
) -> f64 {
    let ratio = (width / base_width.max(1.0)).min(1.0);
    let mut bottom: f64 = 0.0;
    if !layout.is_free() {
        let (padding, gap, columns) = match layout {
            ElementLayout::Stack {
                padding,
                gap,
                direction,
                ..
            } => (
                *padding,
                *gap,
                if matches!(direction, StackDirection::Row) {
                    elements.len().max(1)
                } else {
                    1
                },
            ),
            ElementLayout::Grid {
                padding,
                gap,
                columns,
            } => (*padding, *gap, *columns as usize),
            _ => unreachable!(),
        };
        let cell = ((width - padding * 2.0 - gap * columns.saturating_sub(1) as f64)
            / columns as f64)
            .max(1.0);
        let mut y = padding;
        for row in elements.chunks_mut(columns) {
            let mut height: f64 = 0.0;
            for (i, e) in row.iter_mut().enumerate() {
                let r = place(
                    e,
                    id,
                    Rect {
                        x: padding + i as f64 * (cell + gap),
                        y,
                        width: cell,
                        height: e.rect.height,
                    },
                    measure,
                );
                height = height.max(r.height);
            }
            y += height + gap;
        }
        return (y - gap + padding).max(1.0);
    }
    // Coincident copies of the same text are decorative shadow/glitch layers. Keep the
    // frontmost copy at narrow sizes rather than turning a single heading into repeated text.
    let duplicates: Vec<_> = elements
        .iter()
        .enumerate()
        .map(|(i, e)| {
            !e.text.is_empty()
                && elements[i + 1..].iter().any(|s| {
                    e.text == s.text
                        && (e.rect.x - s.rect.x).abs() < e.rect.width * 0.1
                        && (e.rect.y - s.rect.y).abs() < e.rect.height * 0.1
                })
        })
        .collect();
    for (i, e) in elements.iter_mut().enumerate() {
        if duplicates[i] && !manual(e, id) {
            e.overrides.insert(
                id,
                ElementOverride {
                    automatic: true,
                    hidden: true,
                    dismissed: true,
                    ..Default::default()
                },
            );
        }
    }
    let decor: Vec<_> = elements
        .iter()
        .map(|e| decoration(e, elements, base_width))
        .collect();
    let mut indices: Vec<_> = (0..elements.len())
        .filter(|&i| !decor[i] && !duplicates[i] && elements[i].visible)
        .collect();
    indices.sort_by(|&a, &b| {
        elements[a]
            .rect
            .y
            .total_cmp(&elements[b].rect.y)
            .then(elements[a].rect.x.total_cmp(&elements[b].rect.x))
    });
    let gutter = if base_width >= 600.0 {
        20.0_f64.min(width * 0.06)
    } else {
        0.0
    };
    let available = (width - 2.0 * gutter).max(1.0);
    let mut offset: f64 = 0.0;
    let mut cursor = 0;
    while cursor < indices.len() {
        let start = cursor;
        let top = elements[indices[start]].rect.y;
        let mut end = top + elements[indices[start]].rect.height;
        cursor += 1;
        while cursor < indices.len() && elements[indices[cursor]].rect.y < end - 2.0 {
            end = end.max(elements[indices[cursor]].rect.y + elements[indices[cursor]].rect.height);
            cursor += 1;
        }
        let mut row = indices[start..cursor].to_vec();
        row.sort_by(|&a, &b| {
            elements[a]
                .rect
                .x
                .total_cmp(&elements[b].rect.x)
                .then(elements[a].rect.y.total_cmp(&elements[b].rect.y))
        });
        let left = row
            .iter()
            .map(|&i| elements[i].rect.x)
            .fold(f64::INFINITY, f64::min);
        let right = row
            .iter()
            .map(|&i| elements[i].rect.x + elements[i].rect.width)
            .fold(0.0, f64::max);
        let overlapping = row.iter().enumerate().any(|(a, &i)| {
            row[a + 1..].iter().any(|&j| {
                let a = elements[i].rect;
                let b = elements[j].rect;
                a.x < b.x + b.width - 2.0 && b.x < a.x + a.width - 2.0
            })
        });
        let gaps: Vec<_> = row
            .windows(2)
            .map(|pair| {
                let a = elements[pair[0]].rect;
                let b = elements[pair[1]].rect;
                ((b.x - a.x - a.width) * ratio).clamp(8.0, 24.0)
            })
            .collect();
        let packed_width =
            row.iter().map(|&i| elements[i].rect.width).sum::<f64>() + gaps.iter().sum::<f64>();
        let stack =
            row.len() > 1 && (right - left > available || packed_width > available || overlapping);
        let y = top * ratio + offset;
        let mut next_y = y;
        let mut row_bottom = y;
        let mut next_x = (left * ratio).clamp(gutter, (width - gutter - packed_width).max(gutter));
        for (column, &i) in row.iter().enumerate() {
            let e = &mut elements[i];
            let full_bleed =
                e.rect.x.abs() < 1.0 && e.rect.width >= base_width - 1.0 && !e.children.is_empty();
            let w = if full_bleed {
                width
            } else {
                e.rect.width.min(available)
            }
            .max(1.0);
            let x = if full_bleed {
                0.0
            } else if stack {
                gutter
            } else {
                (e.rect.x * ratio).clamp(gutter, (width - gutter - w).max(gutter))
            };
            let x = if !stack && !full_bleed && row.len() > 1 {
                next_x
            } else {
                x
            };
            let r = place(
                e,
                id,
                Rect {
                    x,
                    y: if stack {
                        next_y
                    } else {
                        y + (e.rect.y - top) * ratio
                    },
                    width: w,
                    height: e.rect.height * ratio,
                },
                measure,
            );
            row_bottom = row_bottom.max(r.y + r.height);
            next_y = r.y + r.height + 16.0;
            next_x = r.x + r.width + gaps.get(column).copied().unwrap_or(0.0);
        }
        offset += (row_bottom - (end * ratio + offset)).max(0.0);
        bottom = bottom.max(row_bottom);
    }
    for (i, e) in elements.iter_mut().enumerate() {
        if decor[i] && !manual(e, id) {
            fn scale(e: &mut Element, id: Uuid, ratio: f64) {
                if manual(e, id) {
                    return;
                }
                let mut rect = e.rect;
                rect.x *= ratio;
                rect.y *= ratio;
                rect.width *= ratio;
                rect.height *= ratio;
                e.overrides.insert(
                    id,
                    ElementOverride {
                        automatic: true,
                        rect: Some(rect),
                        ..Default::default()
                    },
                );
                for c in &mut e.children {
                    scale(c, id, ratio);
                }
            }
            scale(e, id, ratio);
            if e.rect.x.abs() < 1.0
                && e.rect.y.abs() < 1.0
                && e.rect.width >= base_width * 0.8
                && e.rect.height > 2.0
            {
                e.overrides
                    .get_mut(&id)
                    .unwrap()
                    .rect
                    .as_mut()
                    .unwrap()
                    .height = (e.rect.height * ratio).max(bottom);
            }
        }
    }
    (bottom + gutter).max(1.0)
}
