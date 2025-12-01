use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
    Frame,
};

use crate::app::ui::widgets::{calculate_scrollbar_position, type_indicator_for_nix_type};
use crate::app::App;
use crate::config_parser::PropertyType;

impl App {
    pub fn draw_property_editor(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Create a popup in the center of the screen
        let popup_width = (area.width * 3 / 4).min(90);
        let popup_height = (area.height * 3 / 4).min(35);
        let popup_x = (area.width - popup_width) / 2;
        let popup_y = (area.height - popup_height) / 2;

        let popup_area = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        };

        // Clear the background
        frame.render_widget(Clear, popup_area);

        // Get the entry name for the title
        let title = if let Some((ref name, ref entry_type)) = self.prop_editor.entry {
            let type_str = match entry_type {
                crate::config_parser::EntryType::Program => "program",
                crate::config_parser::EntryType::Service => "service",
                crate::config_parser::EntryType::Package => "package",
            };
            format!(" Properties: {}.{} ", type_str, name)
        } else {
            " Properties ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(title);

        // Split the popup into list and help areas
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // Property list
                Constraint::Length(4), // Description area
                Constraint::Length(3), // Input area (for new property or editing)
                Constraint::Length(2), // Help text
            ])
            .split(inner);

        // Draw property list
        self.draw_property_list(frame, chunks[0]);

        // Draw description of selected item
        self.draw_property_description(frame, chunks[1]);

        // Draw input area
        self.draw_property_input(frame, chunks[2]);

        // Draw help text
        let help_text = if self.prop_editor.adding_new {
            "Tab: Switch field | Enter: Save | Esc: Cancel"
        } else if self.prop_editor.edit_state.is_some() {
            "Enter: Save | Esc: Cancel"
        } else if self.prop_editor.showing_available {
            "Tab: Configured | Enter/Space: Add | Esc/q: Close"
        } else {
            "Tab: Available | e/Enter: Edit | a/n: Add | d/Del: Delete | Esc/q: Close"
        };
        let help = Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(help, chunks[3]);
    }

    fn draw_property_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.prop_editor.showing_available {
            self.draw_available_options(frame, area);
        } else {
            self.draw_configured_properties(frame, area);
        }
    }

    /// Draw description of the currently selected property/option
    fn draw_property_description(&self, frame: &mut Frame, area: Rect) {
        let description = if self.prop_editor.showing_available {
            // Get description from available options
            self.prop_editor
                .list_state
                .selected()
                .and_then(|idx| self.prop_editor.available_options.get(idx))
                .map(|(name, info)| {
                    let desc = info.description.trim();
                    if desc.is_empty() {
                        format!("{}: No description available", name)
                    } else {
                        // Clean up NixOS markdown formatting
                        let clean = desc
                            .replace("{command}", "")
                            .replace("{file}", "")
                            .replace("`", "'")
                            .replace('\n', " ");
                        format!("{}: {}", name, clean)
                    }
                })
                .unwrap_or_else(|| "Select an option to see its description".to_string())
        } else {
            // For configured properties, try to find in available options list
            // or show the property name and value
            if let Some((ref entry_name, ref entry_type)) = self.prop_editor.entry {
                if let Some(entry) = self.config.get_entry(entry_name, entry_type) {
                    self.prop_editor
                        .list_state
                        .selected()
                        .and_then(|idx| entry.properties.get(idx))
                        .map(|prop| {
                            // Show property info with type annotation
                            format!(
                                "{} = {} ({})",
                                prop.name,
                                prop.value,
                                match prop.property_type {
                                    PropertyType::Bool => "boolean",
                                    PropertyType::String => "string",
                                    PropertyType::Int => "integer",
                                    PropertyType::Path => "path",
                                    PropertyType::List => "list",
                                    PropertyType::AttrSet =>
                                        if prop.name.contains('.') {
                                            "nested attribute"
                                        } else {
                                            "attribute set"
                                        },
                                    PropertyType::Expression => "expression",
                                }
                            )
                        })
                        .unwrap_or_else(|| "Select a property to see details".to_string())
                } else {
                    "No entry selected".to_string()
                }
            } else {
                "No entry selected".to_string()
            }
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Description ");

        // Wrap text to fit in area
        let inner_width = area.width.saturating_sub(2) as usize;
        let wrapped = textwrap::fill(&description, inner_width);

        let para = Paragraph::new(wrapped)
            .block(block)
            .style(Style::default().fg(Color::Gray))
            .wrap(ratatui::widgets::Wrap { trim: true });

        frame.render_widget(para, area);
    }

    fn draw_configured_properties(&mut self, frame: &mut Frame, area: Rect) {
        let properties = if let Some((ref name, ref entry_type)) = self.prop_editor.entry {
            self.config
                .get_entry(name, entry_type)
                .map(|e| e.properties.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Add title block
        let title = format!(" Configured ({}) - Tab for available ", properties.len());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Store property list area for mouse hit detection
        self.property_list_area = inner;

        let items: Vec<ListItem> = if properties.is_empty() {
            vec![ListItem::new(Line::from(vec![Span::styled(
                "  (no properties defined - press Tab to see available)",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )]))]
        } else {
            properties
                .iter()
                .enumerate()
                .map(|(idx, prop)| {
                    // Determine type indicator - prefer schema info if we have it
                    let (type_indicator, type_label) =
                        self.get_property_type_info(&prop.name, &prop.property_type);

                    let is_selected = self.prop_editor.list_state.selected() == Some(idx);
                    let is_editing = self
                        .prop_editor
                        .edit_state
                        .as_ref()
                        .map(|s| s.property_index == idx)
                        .unwrap_or(false);

                    let value_display = if is_editing {
                        if let Some(ref edit_state) = self.prop_editor.edit_state {
                            // Show with cursor
                            let before = &edit_state.edit_buffer[..edit_state.cursor_pos];
                            let after = &edit_state.edit_buffer[edit_state.cursor_pos..];
                            format!("{}│{}", before, after)
                        } else {
                            prop.value.clone()
                        }
                    } else {
                        // Truncate long values
                        if prop.value.len() > 30 {
                            format!("{}...", &prop.value[..27])
                        } else {
                            prop.value.clone()
                        }
                    };

                    let style = if is_editing {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else if is_selected {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::Gray)
                    };

                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{} ", type_indicator),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(format!("{}", prop.name), style.add_modifier(Modifier::BOLD)),
                        Span::styled(
                            format!(" [{}]", type_label),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(" = ", style),
                        Span::styled(value_display, style),
                    ]))
                })
                .collect()
        };

        let item_count = items.len();
        let mut state = self.prop_editor.list_state.clone();
        let list = List::new(items)
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, inner, &mut state);

        // Render scrollbar if there are more items than fit in the area
        let visible_height = inner.height as usize;
        if item_count > visible_height {
            let viewport_start = state.offset();
            let (content_len, position, use_decorators, viewport_for_thumb) =
                calculate_scrollbar_position(viewport_start, item_count, visible_height);

            let scrollbar = if use_decorators {
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("▲"))
                    .end_symbol(Some("▼"))
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
                inner.inner(Margin {
                    horizontal: 0,
                    vertical: 0,
                }),
                &mut scrollbar_state,
            );
        }
    }

    /// Get type indicator and label for a property, using schema if available
    pub(crate) fn get_property_type_info(
        &self,
        prop_name: &str,
        fallback_type: &PropertyType,
    ) -> (&'static str, String) {
        // Try to get info from schema via cached available_options or infer from property type
        // First check if this property appears in available_options (shouldn't, but just in case)
        if let Some((_, info)) = self
            .prop_editor
            .available_options
            .iter()
            .find(|(n, _)| n == prop_name)
        {
            return (
                type_indicator_for_nix_type(&info.option_type),
                info.option_type.clone(),
            );
        }

        // Check if we can get schema info - for nested properties, show parent type context
        if prop_name.contains('.') || prop_name.contains('"') {
            // This is a nested property like virtualHosts."example.com"
            let base = prop_name.split('.').next().unwrap_or(prop_name);
            // Look for base in available options to get type hint
            if let Some((_, info)) = self
                .prop_editor
                .available_options
                .iter()
                .find(|(n, _)| n == base)
            {
                return ("🔧", format!("nested in {}", info.option_type));
            }
            return ("🔧", "nested attr".to_string());
        }

        // Fall back to property type from parsing
        let indicator = match fallback_type {
            PropertyType::Bool => "⚡",
            PropertyType::String => "📝",
            PropertyType::Int => "🔢",
            PropertyType::Path => "📁",
            PropertyType::List => "📋",
            PropertyType::AttrSet => "🔧",
            PropertyType::Expression => "λ",
        };
        let label = match fallback_type {
            PropertyType::Bool => "boolean",
            PropertyType::String => "string",
            PropertyType::Int => "integer",
            PropertyType::Path => "path",
            PropertyType::List => "list",
            PropertyType::AttrSet => "attrset",
            PropertyType::Expression => "expr",
        };
        (indicator, label.to_string())
    }

    fn draw_available_options(&mut self, frame: &mut Frame, area: Rect) {
        // Add title block
        let title = format!(
            " Available ({}) - Tab for configured ",
            self.prop_editor.available_options.len()
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Store property list area for mouse hit detection
        self.property_list_area = inner;

        let items: Vec<ListItem> = if self.prop_editor.available_options.is_empty() {
            vec![ListItem::new(Line::from(vec![Span::styled(
                "  (no available options found - schema may not be loaded)",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )]))]
        } else {
            self.prop_editor
                .available_options
                .iter()
                .enumerate()
                .map(|(idx, (opt_name, opt_info))| {
                    let type_indicator = type_indicator_for_nix_type(&opt_info.option_type);

                    let is_selected = self.prop_editor.list_state.selected() == Some(idx);

                    // Get default value for display
                    let default_str = opt_info
                        .default
                        .as_ref()
                        .map(|v| match v {
                            serde_json::Value::Bool(b) => b.to_string(),
                            serde_json::Value::Number(n) => n.to_string(),
                            serde_json::Value::String(s) => {
                                if s.len() > 15 {
                                    format!("\"{}...\"", &s[..12])
                                } else {
                                    format!("\"{}\"", s)
                                }
                            }
                            serde_json::Value::Null => "null".to_string(),
                            _ => "(complex)".to_string(),
                        })
                        .unwrap_or_else(|| "—".to_string());

                    // Truncate type for display
                    let type_display = if opt_info.option_type.len() > 20 {
                        format!("{}...", &opt_info.option_type[..17])
                    } else {
                        opt_info.option_type.clone()
                    };

                    let style = if is_selected {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::Gray)
                    };

                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{} ", type_indicator),
                            Style::default().fg(Color::Blue),
                        ),
                        Span::styled(opt_name.clone(), style.add_modifier(Modifier::BOLD)),
                        Span::styled(
                            format!(" [{}]", type_display),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(
                            format!(" = {}", default_str),
                            Style::default().fg(Color::Cyan),
                        ),
                    ]))
                })
                .collect()
        };

        let item_count = items.len();
        let mut state = self.prop_editor.list_state.clone();
        let list = List::new(items)
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, inner, &mut state);

        // Render scrollbar if there are more items than fit in the area
        let visible_height = inner.height as usize;
        if item_count > visible_height {
            let viewport_start = state.offset();
            let (content_len, position, use_decorators, viewport_for_thumb) =
                calculate_scrollbar_position(viewport_start, item_count, visible_height);

            let scrollbar = if use_decorators {
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("▲"))
                    .end_symbol(Some("▼"))
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
                inner.inner(Margin {
                    horizontal: 0,
                    vertical: 0,
                }),
                &mut scrollbar_state,
            );
        }
    }

    fn draw_property_input(&self, frame: &mut Frame, area: Rect) {
        if self.prop_editor.adding_new {
            // Show input fields for new property
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(40),
                    Constraint::Length(3),
                    Constraint::Percentage(57),
                ])
                .split(area);

            // Name field
            let name_style = if self.prop_editor.editing_name {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            let name_display = if self.prop_editor.editing_name {
                let before = &self.prop_editor.new_name[..self.prop_editor.new_cursor];
                let after = &self.prop_editor.new_name[self.prop_editor.new_cursor..];
                format!("{}│{}", before, after)
            } else {
                self.prop_editor.new_name.clone()
            };
            let name_block = Block::default()
                .borders(Borders::ALL)
                .border_style(name_style)
                .title(" Name ");
            let name_para = Paragraph::new(name_display).block(name_block);
            frame.render_widget(name_para, chunks[0]);

            // Equals sign
            let eq = Paragraph::new(" = ").style(Style::default().fg(Color::White));
            frame.render_widget(eq, chunks[1]);

            // Value field
            let value_style = if !self.prop_editor.editing_name {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            let value_display = if !self.prop_editor.editing_name {
                let before = &self.prop_editor.new_value[..self.prop_editor.new_cursor];
                let after = &self.prop_editor.new_value[self.prop_editor.new_cursor..];
                format!("{}│{}", before, after)
            } else {
                self.prop_editor.new_value.clone()
            };
            let value_block = Block::default()
                .borders(Borders::ALL)
                .border_style(value_style)
                .title(" Value ");
            let value_para = Paragraph::new(value_display).block(value_block);
            frame.render_widget(value_para, chunks[2]);
        } else if self.prop_editor.edit_state.is_none() {
            // Show hint when not editing
            let hint = Paragraph::new("Press 'a' or 'n' to add a new property")
                .style(
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
            frame.render_widget(hint, area);
        }
    }
}
