use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};

use crate::app::types::{Focus, ListType};
use crate::app::ui::widgets::apply_look_ahead_scroll;
use crate::app::App;

impl App {
    pub fn handle_event(&mut self, event: Event) -> Result<()> {
        if let Event::Key(key) = event {
            if key.kind != KeyEventKind::Press {
                return Ok(());
            }

            // Global keybindings - always allow quit
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('c') | KeyCode::Char('q') => {
                        self.searcher.cancel(); // Cancel any ongoing search
                        self.should_quit = true;
                        return Ok(());
                    }
                    KeyCode::Char('s') if !self.is_searching => {
                        self.save_config()?;
                        return Ok(());
                    }
                    _ => {}
                }
            }

            // Block most input during search
            if self.is_searching {
                // Only allow Escape to cancel search
                if key.code == KeyCode::Esc {
                    self.searcher.cancel();
                    self.is_searching = false;
                    self.status_message = Some("Search cancelled".to_string());
                }
                return Ok(());
            }

            if key.code == KeyCode::F(1) {
                self.show_help = !self.show_help;
                return Ok(());
            }

            if self.show_help {
                // Any key closes help
                self.show_help = false;
                return Ok(());
            }

            // Handle description popup if it's open
            if self.description_popup.show {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.description_popup.scroll_offset =
                            self.description_popup.scroll_offset.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let max_scroll = self
                            .description_popup
                            .total_lines
                            .saturating_sub(self.description_popup.visible_lines);
                        if self.description_popup.scroll_offset < max_scroll {
                            self.description_popup.scroll_offset += 1;
                        }
                    }
                    KeyCode::PageUp => {
                        self.description_popup.scroll_offset = self
                            .description_popup
                            .scroll_offset
                            .saturating_sub(self.description_popup.visible_lines.saturating_sub(1));
                    }
                    KeyCode::PageDown => {
                        let max_scroll = self
                            .description_popup
                            .total_lines
                            .saturating_sub(self.description_popup.visible_lines);
                        self.description_popup.scroll_offset =
                            (self.description_popup.scroll_offset
                                + self.description_popup.visible_lines.saturating_sub(1))
                            .min(max_scroll);
                    }
                    KeyCode::Home => {
                        self.description_popup.scroll_offset = 0;
                    }
                    KeyCode::End => {
                        self.description_popup.scroll_offset = self
                            .description_popup
                            .total_lines
                            .saturating_sub(self.description_popup.visible_lines);
                    }
                    _ => {
                        // Any other key closes the popup
                        self.description_popup.show = false;
                        self.description_popup.scroll_offset = 0;
                    }
                }
                return Ok(());
            }

            // Handle rebuild prompt if it's open
            if self.rebuild_prompt.show {
                self.handle_rebuild_prompt_input(key.code)?;
                return Ok(());
            }

            // Handle property editor if it's open
            if self.prop_editor.show {
                self.handle_property_editor_input(key.code)?;
                return Ok(());
            }

            match self.focus {
                Focus::SearchBar => self.handle_search_input(key.code)?,
                Focus::Programs => self.handle_list_input(key.code, ListType::Programs)?,
                Focus::Services => self.handle_list_input(key.code, ListType::Services)?,
                Focus::Packages => self.handle_list_input(key.code, ListType::Packages)?,
                Focus::PropertyEditor => self.handle_property_editor_input(key.code)?,
            }
        } else if let Event::Mouse(mouse) = event {
            if !self.is_searching {
                if self.prop_editor.show {
                    self.handle_property_editor_mouse(mouse)?;
                } else {
                    self.handle_mouse_event(mouse)?;
                }
            }
        }

        Ok(())
    }

    /// Handle mouse events in the property editor popup
    fn handle_property_editor_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> Result<()> {
        let x = mouse.column;
        let y = mouse.row;

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                // Scroll up in property list
                self.move_property_selection(-3);
            }
            MouseEventKind::ScrollDown => {
                // Scroll down in property list
                self.move_property_selection(3);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if click is in the property list area
                if self.property_list_area.contains((x, y).into()) {
                    // Calculate which item was clicked
                    let relative_y = (y - self.property_list_area.y) as usize;
                    let scroll_offset = self.prop_editor.list_state.offset();
                    let clicked_idx = scroll_offset + relative_y;
                    let len = if self.prop_editor.showing_available {
                        self.prop_editor.available_options.len()
                    } else {
                        self.prop_editor
                            .entry
                            .as_ref()
                            .and_then(|(name, entry_type)| self.config.get_entry(name, entry_type))
                            .map(|e| e.properties.len())
                            .unwrap_or(0)
                    };
                    if clicked_idx < len {
                        self.prop_editor.list_state.select(Some(clicked_idx));
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_mouse_event(&mut self, mouse: crossterm::event::MouseEvent) -> Result<()> {
        let x = mouse.column;
        let y = mouse.row;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Check which area was clicked
                if self.search_area.contains((x, y).into()) {
                    self.focus = Focus::SearchBar;
                } else if self.programs_area.contains((x, y).into()) {
                    self.focus = Focus::Programs;
                    // Calculate which item was clicked (accounting for border and scroll offset)
                    if y > self.programs_area.y
                        && y < self.programs_area.y + self.programs_area.height - 1
                    {
                        let scroll_offset = self.program_state.offset();
                        let clicked_idx = scroll_offset + (y - self.programs_area.y - 1) as usize;
                        if clicked_idx < self.programs.len() {
                            self.program_state.select(Some(clicked_idx));
                        }
                    }
                } else if self.services_area.contains((x, y).into()) {
                    self.focus = Focus::Services;
                    if y > self.services_area.y
                        && y < self.services_area.y + self.services_area.height - 1
                    {
                        let scroll_offset = self.service_state.offset();
                        let clicked_idx = scroll_offset + (y - self.services_area.y - 1) as usize;
                        if clicked_idx < self.services.len() {
                            self.service_state.select(Some(clicked_idx));
                        }
                    }
                } else if self.packages_area.contains((x, y).into()) {
                    self.focus = Focus::Packages;
                    if y > self.packages_area.y
                        && y < self.packages_area.y + self.packages_area.height - 1
                    {
                        let scroll_offset = self.package_state.offset();
                        let clicked_idx = scroll_offset + (y - self.packages_area.y - 1) as usize;
                        if clicked_idx < self.packages.len() {
                            self.package_state.select(Some(clicked_idx));
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                // Right click toggles the item under cursor
                if self.programs_area.contains((x, y).into()) {
                    if y > self.programs_area.y
                        && y < self.programs_area.y + self.programs_area.height - 1
                    {
                        let scroll_offset = self.program_state.offset();
                        let clicked_idx = scroll_offset + (y - self.programs_area.y - 1) as usize;
                        if clicked_idx < self.programs.len() {
                            self.program_state.select(Some(clicked_idx));
                            self.toggle_selected(&ListType::Programs)?;
                        }
                    }
                } else if self.services_area.contains((x, y).into()) {
                    if y > self.services_area.y
                        && y < self.services_area.y + self.services_area.height - 1
                    {
                        let scroll_offset = self.service_state.offset();
                        let clicked_idx = scroll_offset + (y - self.services_area.y - 1) as usize;
                        if clicked_idx < self.services.len() {
                            self.service_state.select(Some(clicked_idx));
                            self.toggle_selected(&ListType::Services)?;
                        }
                    }
                } else if self.packages_area.contains((x, y).into()) {
                    if y > self.packages_area.y
                        && y < self.packages_area.y + self.packages_area.height - 1
                    {
                        let scroll_offset = self.package_state.offset();
                        let clicked_idx = scroll_offset + (y - self.packages_area.y - 1) as usize;
                        if clicked_idx < self.packages.len() {
                            self.package_state.select(Some(clicked_idx));
                            self.toggle_selected(&ListType::Packages)?;
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                // Scroll up in the focused list
                match self.focus {
                    Focus::Programs => self.move_selection(-3, &ListType::Programs),
                    Focus::Services => self.move_selection(-3, &ListType::Services),
                    Focus::Packages => self.move_selection(-3, &ListType::Packages),
                    _ => {}
                }
            }
            MouseEventKind::ScrollDown => {
                // Scroll down in the focused list
                match self.focus {
                    Focus::Programs => self.move_selection(3, &ListType::Programs),
                    Focus::Services => self.move_selection(3, &ListType::Services),
                    Focus::Packages => self.move_selection(3, &ListType::Packages),
                    _ => {}
                }
            }
            _ => {}
        }

        Ok(())
    }

    pub(crate) fn handle_search_input(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Char(c) => {
                self.search_query.insert(self.search_cursor, c);
                self.search_cursor += 1;
            }
            KeyCode::Backspace => {
                if self.search_cursor > 0 {
                    self.search_cursor -= 1;
                    self.search_query.remove(self.search_cursor);
                }
            }
            KeyCode::Delete => {
                if self.search_cursor < self.search_query.len() {
                    self.search_query.remove(self.search_cursor);
                }
            }
            KeyCode::Left => {
                self.search_cursor = self.search_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                self.search_cursor = (self.search_cursor + 1).min(self.search_query.len());
            }
            KeyCode::Home => {
                self.search_cursor = 0;
            }
            KeyCode::End => {
                self.search_cursor = self.search_query.len();
            }
            KeyCode::Enter => {
                self.perform_search()?;
            }
            KeyCode::Tab => {
                self.focus = Focus::Programs;
            }
            KeyCode::Down => {
                self.focus = Focus::Programs;
            }
            KeyCode::Esc => {
                self.search_query.clear();
                self.search_cursor = 0;
                self.load_from_config(); // Reset to config entries
            }
            _ => {}
        }

        Ok(())
    }

    pub(crate) fn handle_list_input(&mut self, code: KeyCode, list_type: ListType) -> Result<()> {
        match code {
            KeyCode::Up => {
                self.move_selection(-1, &list_type);
            }
            KeyCode::Down => {
                self.move_selection(1, &list_type);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.toggle_selected(&list_type)?;
            }
            KeyCode::Tab => {
                self.focus = match list_type {
                    ListType::Programs => Focus::Services,
                    ListType::Services => Focus::Packages,
                    ListType::Packages => Focus::SearchBar,
                };
            }
            KeyCode::BackTab => {
                self.focus = match list_type {
                    ListType::Programs => Focus::SearchBar,
                    ListType::Services => Focus::Programs,
                    ListType::Packages => Focus::Services,
                };
            }
            KeyCode::Left => {
                self.focus = match list_type {
                    ListType::Programs => Focus::SearchBar,
                    ListType::Services => Focus::Programs,
                    ListType::Packages => Focus::Services,
                };
            }
            KeyCode::Right => {
                self.focus = match list_type {
                    ListType::Programs => Focus::Services,
                    ListType::Services => Focus::Packages,
                    ListType::Packages => Focus::Packages,
                };
            }
            KeyCode::Char('/') | KeyCode::Esc => {
                self.focus = Focus::SearchBar;
            }
            KeyCode::Char('e') => {
                // Open property editor for the selected entry (only for programs/services)
                self.open_property_editor(&list_type)?;
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                // Show description popup for the selected entry
                self.show_description_popup(&list_type);
            }
            _ => {}
        }

        Ok(())
    }

    /// Get the viewport height for a list area (area height minus borders)
    pub(crate) fn get_list_viewport_height(&self, list_type: &ListType) -> usize {
        let area = match list_type {
            ListType::Programs => self.programs_area,
            ListType::Services => self.services_area,
            ListType::Packages => self.packages_area,
        };
        // Subtract 2 for top and bottom borders
        area.height.saturating_sub(2) as usize
    }

    pub(crate) fn move_selection(&mut self, delta: i32, list_type: &ListType) {
        // Calculate viewport height first to avoid borrow issues
        let viewport_height = self.get_list_viewport_height(list_type);

        let (state, len) = match list_type {
            ListType::Programs => (&mut self.program_state, self.programs.len()),
            ListType::Services => (&mut self.service_state, self.services.len()),
            ListType::Packages => (&mut self.package_state, self.packages.len()),
        };

        if len == 0 {
            return;
        }

        let current = state.selected().unwrap_or(0);
        let new = if delta > 0 {
            (current + delta as usize).min(len - 1)
        } else {
            current.saturating_sub((-delta) as usize)
        };

        state.select(Some(new));

        // Apply look-ahead scrolling
        let direction = if delta > 0 {
            1
        } else if delta < 0 {
            -1
        } else {
            0
        };
        apply_look_ahead_scroll(new, len, viewport_height, state, direction);
    }

    fn handle_rebuild_prompt_input(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Left | KeyCode::Char('h') => {
                self.rebuild_prompt.selected = 0;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.rebuild_prompt.selected = 1;
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.rebuild_prompt.selected = 0;
                self.rebuild_prompt.pending_rebuild = true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.rebuild_prompt.show = false;
            }
            KeyCode::Enter => {
                if self.rebuild_prompt.selected == 0 {
                    self.rebuild_prompt.pending_rebuild = true;
                } else {
                    self.rebuild_prompt.show = false;
                }
            }
            KeyCode::Tab => {
                self.rebuild_prompt.selected = if self.rebuild_prompt.selected == 0 {
                    1
                } else {
                    0
                };
            }
            _ => {}
        }
        Ok(())
    }

    /// Show description popup for the currently selected entry
    fn show_description_popup(&mut self, list_type: &ListType) {
        let entry = match list_type {
            ListType::Programs => self
                .program_state
                .selected()
                .and_then(|i| self.programs.get(i)),
            ListType::Services => self
                .service_state
                .selected()
                .and_then(|i| self.services.get(i)),
            ListType::Packages => self
                .package_state
                .selected()
                .and_then(|i| self.packages.get(i)),
        };

        if let Some(entry) = entry {
            self.description_popup.name = entry.name.clone();
            self.description_popup.description = if entry.description.is_empty() {
                "No description available".to_string()
            } else {
                entry.description.clone()
            };
            self.description_popup.scroll_offset = 0; // Reset scroll when opening
            self.description_popup.show = true;
        }
    }
}
