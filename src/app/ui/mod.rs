pub mod property_popup;
pub mod widgets;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::app::types::Focus;
use crate::app::ui::widgets::draw_list;

impl App {
    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        
        // Fixed layout: 3 lines for search, 2 lines for help+status at bottom
        // Rest goes to the scrollable center columns
        let search_height = 3;
        let bottom_height = 2; // One for help, one for status
        
        let margin = if area.width < 40 || area.height < 10 { 0 } else { 1 };
        
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(margin)
            .constraints([
                Constraint::Length(search_height), // Search bar (always 3)
                Constraint::Min(1),                // Main content (scrollable)
                Constraint::Length(bottom_height), // Help + Status (always 2)
            ])
            .split(area);

        // Save areas for mouse handling
        self.search_area = chunks[0];

        self.draw_search_bar(frame, chunks[0]);
        self.draw_columns(frame, chunks[1]);
        self.draw_bottom_bar(frame, chunks[2]);

        if self.show_help {
            self.draw_help_popup(frame);
        }

        if self.prop_editor.show {
            self.draw_property_editor(frame);
        }
    }

    fn draw_search_bar(&self, frame: &mut Frame, area: Rect) {
        let is_focused = self.focus == Focus::SearchBar;
        let style = if is_focused {
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

        let title = if area.width > 40 {
            " Search (Enter to search, Esc to clear) "
        } else if area.width > 20 {
            " Search "
        } else {
            ""
        };

        let search_block = Block::default()
            .borders(Borders::ALL)
            .border_set(border_set)
            .border_style(style)
            .title(title);

        // Create search text with cursor
        let display_text = if self.focus == Focus::SearchBar {
            let before = &self.search_query[..self.search_cursor];
            let cursor = "│";
            let after = &self.search_query[self.search_cursor..];
            format!("{}{}{}", before, cursor, after)
        } else {
            self.search_query.clone()
        };

        let search_text = Paragraph::new(display_text).block(search_block);

        frame.render_widget(search_text, area);
    }

    fn draw_columns(&mut self, frame: &mut Frame, area: Rect) {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(34),
                Constraint::Percentage(33),
            ])
            .split(area);

        // Save column areas for mouse handling
        self.programs_area = columns[0];
        self.services_area = columns[1];
        self.packages_area = columns[2];

        // Draw programs
        draw_list(
            frame,
            columns[0],
            "Programs",
            &self.programs,
            &mut self.program_state,
            self.focus == Focus::Programs,
        );

        // Draw services
        draw_list(
            frame,
            columns[1],
            "Services",
            &self.services,
            &mut self.service_state,
            self.focus == Focus::Services,
        );

        // Draw packages
        draw_list(
            frame,
            columns[2],
            "Packages",
            &self.packages,
            &mut self.package_state,
            self.focus == Focus::Packages,
        );
    }

    fn draw_bottom_bar(&self, frame: &mut Frame, area: Rect) {
        // Split area into two lines: help (gray) and status (yellow if present)
        let lines = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Help line
                Constraint::Length(1), // Status line
            ])
            .split(area);

        // Help line (always shown, gray)
        let help_text = "F1: Help | Ctrl+S: Save | Ctrl+Q: Quit | Tab: Switch | Space: Toggle | e: Edit props";
        let help_style = Style::default().fg(Color::DarkGray);
        let help_bar = Paragraph::new(help_text).style(help_style);
        frame.render_widget(help_bar, lines[0]);

        // Status line (yellow when there's a message, otherwise empty)
        if let Some(ref msg) = self.status_message {
            let status_style = Style::default().fg(Color::Yellow);
            let status_bar = Paragraph::new(msg.as_str()).style(status_style);
            frame.render_widget(status_bar, lines[1]);
        }
    }

    fn draw_help_popup(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_area = Rect {
            x: area.width / 4,
            y: area.height / 4,
            width: area.width / 2,
            height: area.height / 2,
        };

        let help_text = vec![
            "",
            "  Keyboard Shortcuts:",
            "  ──────────────────────────",
            "  Ctrl+Q / Ctrl+C  Quit",
            "  Ctrl+S           Save config",
            "  F1               Toggle help",
            "",
            "  Search Bar:",
            "  ──────────────────────────",
            "  Enter            Perform search",
            "  Esc              Clear search",
            "  Tab / Down       Move to lists",
            "",
            "  Lists:",
            "  ──────────────────────────",
            "  Up/Down          Navigate",
            "  Space/Enter      Toggle item",
            "  e                Edit properties",
            "  Tab              Next column",
            "  Shift+Tab        Previous column",
            "  / or Esc         Go to search",
            "",
            "  Property Editor:",
            "  ──────────────────────────",
            "  Tab              Toggle configured/available",
            "  e/Enter          Edit/Add property",
            "  a/n              Add property (manual)",
            "  d/Del            Delete property",
            "  Esc/q            Close editor",
            "",
            "  Legend:",
            "  ──────────────────────────",
            "  [✓]  Enabled     ⚙ Has properties",
            "  [ ]  Disabled    + Not in config",
            "",
            "  Press any key to close",
        ];

        let help = Paragraph::new(help_text.join("\n"))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .style(Style::default().fg(Color::White));

        frame.render_widget(Clear, popup_area);
        frame.render_widget(help, popup_area);
    }
}
