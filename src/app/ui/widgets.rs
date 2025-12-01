use ratatui::{
    layout::{Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

use crate::app::types::ListEntry;

/// Calculate scrollbar parameters per spec:
/// - 1-2 lines: thumb = 1 char
/// - 3-4 lines: no decorators, thumb = 1 char  
/// - 5+ lines: decorators, thumb proportional but at most n_lines - 4
/// 
/// Move thumb 1 away from edge as soon as offset >= 1 (works for n_lines >= 3).
/// 
/// Returns (content_length, position, use_decorators, viewport_for_thumb) for ScrollbarState
pub fn calculate_scrollbar_position(
    viewport_start: usize,
    total_items: usize,
    visible_height: usize,
) -> (usize, usize, bool, usize) {
    let max_scroll = total_items.saturating_sub(visible_height);
    
    // Determine decorator usage: 5+ lines
    let use_decorators = visible_height >= 5;
    
    // Calculate track height (space for scrollbar after decorators)
    let track_height = if use_decorators {
        visible_height.saturating_sub(2) // -2 for ▲/▼ symbols
    } else {
        visible_height
    };
    
    // Calculate viewport_for_thumb to control thumb size:
    // - 1-4 lines: thumb = 1 (use total_items as viewport to get minimal thumb)
    // - 5+ lines: thumb proportional, but track is at most n_lines - 4
    //   (2 for decorators, 2 for empty spaces at edges)
    let viewport_for_thumb = if visible_height < 5 {
        // Small: minimal thumb (1 char)
        total_items
    } else {
        // Large: proportional thumb, but leave room for edge indicators
        // Max thumb size = track_height - 2 = visible_height - 4
        let max_thumb = visible_height.saturating_sub(4).max(1);
        let proportional_viewport = visible_height;
        let proportional_thumb = (track_height * proportional_viewport) / total_items.max(1);
        if proportional_thumb <= max_thumb {
            proportional_viewport
        } else {
            (total_items * max_thumb) / track_height.max(1)
        }
    };
    
    if max_scroll == 0 {
        return (1, 0, use_decorators, viewport_for_thumb.max(1));
    }
    
    // "1 from edge" logic per spec: works for visible_height >= 3
    // This gives visual indication that there's more to scroll
    let position = if visible_height < 3 {
        // Not enough space, simple 1:1 mapping
        viewport_start.min(max_scroll)
    } else if viewport_start == 0 {
        // At very top - thumb at top edge
        0
    } else if viewport_start >= max_scroll {
        // At very bottom - thumb at bottom edge
        max_scroll
    } else {
        // In the middle - thumb should be at least 1 away from edges
        // Map scroll positions 1..max_scroll-1 to visual positions 1..max_scroll-1
        // ensuring we never touch the edges unless at actual edge
        if max_scroll <= 2 {
            // Only 3 positions (0, 1, 2) - middle is position 1
            1
        } else {
            // Interpolate: scroll 1 -> pos 1, scroll max_scroll-1 -> pos max_scroll-1
            // Positions 2..max_scroll-2 are interpolated in between
            let middle_scroll_range = max_scroll - 2; // scroll positions 1 to max_scroll-1
            let middle_pos_range = max_scroll - 2;    // visual positions 1 to max_scroll-1
            let scroll_in_middle = viewport_start - 1; // 0-based within middle range
            let pos_in_middle = if middle_scroll_range == 0 {
                0
            } else {
                (scroll_in_middle * middle_pos_range) / middle_scroll_range
            };
            1 + pos_in_middle.min(middle_pos_range)
        }
    };
    
    (max_scroll + 1, position, use_decorators, viewport_for_thumb.max(1))
}

/// Apply look-ahead scrolling: try to show one item ahead of cursor direction
/// This scrolls the viewport only when needed to show context ahead of movement.
/// 
/// Arguments:
/// - new_selection: the newly selected index
/// - len: total number of items  
/// - viewport_height: number of visible items
/// - state: the ListState to update
/// - direction: -1 for up, +1 for down, 0 for absolute positioning
pub fn apply_look_ahead_scroll(
    new_selection: usize,
    len: usize,
    viewport_height: usize,
    state: &mut ListState,
    direction: i32,
) {
    if len == 0 || viewport_height == 0 {
        return;
    }

    let current_offset = state.offset();
    
    // If viewport can show all items, no scrolling needed
    if len <= viewport_height {
        *state.offset_mut() = 0;
        return;
    }

    // Maximum valid scroll offset
    let max_offset = len.saturating_sub(viewport_height);

    // For look-ahead to work properly, we need at least 3 visible items
    // Otherwise we fall back to simple "keep selected visible" behavior
    if viewport_height < 3 {
        // Simple behavior: ensure selected item is visible
        if new_selection < current_offset {
            *state.offset_mut() = new_selection;
        } else if new_selection >= current_offset + viewport_height {
            *state.offset_mut() = (new_selection + 1).saturating_sub(viewport_height).min(max_offset);
        }
        return;
    }

    // Calculate position within viewport (0-indexed)
    let pos_in_viewport = new_selection.saturating_sub(current_offset);

    match direction {
        d if d < 0 => {
            // Moving UP: try to show one item above the selection
            // Scroll up when we reach second visible item (index 1 in viewport)
            // unless we're at the very top of the list
            if new_selection == 0 {
                // At top of list, scroll to show it
                *state.offset_mut() = 0;
            } else if pos_in_viewport <= 1 && current_offset > 0 {
                // At second item from top of viewport, scroll to show one above
                *state.offset_mut() = new_selection.saturating_sub(1);
            } else if new_selection < current_offset {
                // Selection would be above viewport, scroll to show it with one above if possible
                *state.offset_mut() = new_selection.saturating_sub(1);
            }
        }
        d if d > 0 => {
            // Moving DOWN: try to show one item below the selection
            // Scroll down when we reach second-to-last visible item
            // unless we're at the very bottom of the list
            if new_selection == len - 1 {
                // At bottom of list, scroll to show it (with items above)
                *state.offset_mut() = max_offset;
            } else if pos_in_viewport >= viewport_height - 2 && current_offset < max_offset {
                // At second-to-last item in viewport, scroll to show one below
                // New offset should position selection at second-to-last spot
                let desired_offset = (new_selection + 2).saturating_sub(viewport_height);
                *state.offset_mut() = desired_offset.min(max_offset);
            } else if new_selection >= current_offset + viewport_height {
                // Selection would be below viewport, scroll to show it with one below if possible
                let desired_offset = (new_selection + 2).saturating_sub(viewport_height);
                *state.offset_mut() = desired_offset.min(max_offset);
            }
        }
        _ => {
            // Absolute positioning (e.g., mouse click, page jump)
            // Just ensure selected item is visible, preferably with context
            if new_selection < current_offset {
                // Above viewport - scroll so selection is near top with one above if possible
                *state.offset_mut() = new_selection.saturating_sub(1);
            } else if new_selection >= current_offset + viewport_height {
                // Below viewport - scroll so selection is near bottom with one below if possible  
                let desired_offset = (new_selection + 2).saturating_sub(viewport_height);
                *state.offset_mut() = desired_offset.min(max_offset);
            }
            // Otherwise it's already visible, don't scroll
        }
    }

    // Clamp offset to valid range
    let clamped = (*state.offset_mut()).min(max_offset);
    *state.offset_mut() = clamped;
}

/// Draw a list widget with entries, scrollbar, and proper styling
pub fn draw_list(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    entries: &[ListEntry],
    state: &mut ListState,
    is_focused: bool,
) {
    let border_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    // Use thicker border for focused elements
    let border_set = if is_focused {
        border::THICK
    } else {
        border::PLAIN
    };

    // Adaptive title based on width
    let title_text = if area.width > 15 {
        format!(" {} ({}) ", title, entries.len())
    } else if area.width > 8 {
        format!(" {} ", entries.len())
    } else {
        String::new()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border_set)
        .border_style(border_style)
        .title(title_text);

    let items: Vec<ListItem> = entries
        .iter()
        .map(|entry| {
            let checkbox = if entry.enabled { "[✓]" } else { "[ ]" };
            let config_indicator = if entry.in_config { "" } else { " +" };
            let extra_indicator = if entry.has_extra_config { " ⚙" } else { "" };

            let style = if entry.enabled {
                Style::default().fg(Color::Green)
            } else if entry.in_config {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            ListItem::new(Line::from(vec![
                Span::styled(checkbox, style),
                Span::raw(" "),
                Span::styled(&entry.name, style),
                Span::styled(config_indicator, Style::default().fg(Color::Cyan)),
                Span::styled(extra_indicator, Style::default().fg(Color::Magenta)),
            ]))
        })
        .collect();

    // Add scroll indicator
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, state);
    
    // Draw scrollbar if there are more items than visible
    let visible_height = area.height.saturating_sub(2) as usize;
    if entries.len() > visible_height {
        let viewport_start = state.offset();
        let (content_len, position, use_decorators, viewport_for_thumb) = calculate_scrollbar_position(
            viewport_start, entries.len(), visible_height
        );
        
        let scrollbar = if use_decorators {
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"))
        } else {
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
        };
        
        let mut scrollbar_state = ScrollbarState::new(content_len)
            .viewport_content_length(viewport_for_thumb)
            .position(position);
        
        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin { horizontal: 0, vertical: 1 }),
            &mut scrollbar_state,
        );
    }
}

/// Get type indicator emoji for a Nix type string
pub fn type_indicator_for_nix_type(type_str: &str) -> &'static str {
    match type_str {
        "boolean" | "null or boolean" => "⚡",
        "string" | "strings" | "null or string" => "📝",
        "signed integer" | "integer" | "null or signed integer" => "🔢",
        "path" | "null or path" => "📁",
        "package" => "📦",
        s if s.starts_with("list of") => "📋",
        s if s.contains("attribute set") || s.contains("submodule") => "🔧",
        _ => "λ",
    }
}
