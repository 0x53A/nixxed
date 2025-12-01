use anyhow::Result;
use crossterm::event::KeyCode;

use crate::app::App;
use crate::app::types::{ListType, PropertyEditState};
use crate::app::ui::widgets::apply_look_ahead_scroll;
use crate::config_parser::{EntryType, PropertyType};

impl App {
    /// Open the property editor for the currently selected entry
    pub fn open_property_editor(&mut self, list_type: &ListType) -> Result<()> {
        let (entry_type, name, in_config) = match list_type {
            ListType::Programs => {
                if let Some(idx) = self.program_state.selected() {
                    if idx < self.programs.len() {
                        let entry = &self.programs[idx];
                        (EntryType::Program, entry.name.clone(), entry.in_config)
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            }
            ListType::Services => {
                if let Some(idx) = self.service_state.selected() {
                    if idx < self.services.len() {
                        let entry = &self.services[idx];
                        (EntryType::Service, entry.name.clone(), entry.in_config)
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            }
            ListType::Packages => {
                // Packages don't have properties to edit
                self.status_message = Some("Packages don't have editable properties".to_string());
                return Ok(());
            }
        };

        if !in_config {
            self.status_message = Some("Add entry to config first before editing properties".to_string());
            return Ok(());
        }

        // Fetch available options from schema
        let configured_props = self.config.get_entry(&name, &entry_type)
            .map(|e| e.properties.clone())
            .unwrap_or_default();
        self.prop_editor.available_options = self.schema_cache.get_available_options(&entry_type, &name, &configured_props);
        // Sort available options by name
        self.prop_editor.available_options.sort_by(|a, b| a.0.cmp(&b.0));

        // Set up property editor state
        self.prop_editor.entry = Some((name, entry_type));
        self.prop_editor.list_state = ratatui::widgets::ListState::default();
        self.prop_editor.list_state.select(Some(0));
        self.prop_editor.edit_state = None;
        self.prop_editor.adding_new = false;
        self.prop_editor.new_name.clear();
        self.prop_editor.new_value.clear();
        self.prop_editor.show = true;
        self.prop_editor.showing_available = false;
        self.focus = crate::app::types::Focus::PropertyEditor;

        Ok(())
    }

    /// Get the viewport height for the property editor list
    pub(crate) fn get_property_list_viewport_height(&self) -> usize {
        // property_list_area is the inner area (already without borders)
        self.property_list_area.height as usize
    }

    /// Move selection in property list by delta with look-ahead scrolling
    pub(crate) fn move_property_selection(&mut self, delta: i32) {
        let len = if self.prop_editor.showing_available {
            self.prop_editor.available_options.len()
        } else {
            self.prop_editor.entry.as_ref()
                .and_then(|(name, entry_type)| self.config.get_entry(name, entry_type))
                .map(|e| e.properties.len())
                .unwrap_or(0)
        };
        
        if len == 0 {
            return;
        }

        let current = self.prop_editor.list_state.selected().unwrap_or(0);
        let new = if delta > 0 {
            (current + delta as usize).min(len - 1)
        } else {
            current.saturating_sub((-delta) as usize)
        };
        self.prop_editor.list_state.select(Some(new));
        
        // Apply look-ahead scrolling
        let viewport_height = self.get_property_list_viewport_height();
        let direction = if delta > 0 { 1 } else if delta < 0 { -1 } else { 0 };
        apply_look_ahead_scroll(new, len, viewport_height, &mut self.prop_editor.list_state, direction);
    }

    /// Handle keyboard input in the property editor
    pub fn handle_property_editor_input(&mut self, code: KeyCode) -> Result<()> {
        // If we're editing a property value
        if let Some(ref mut edit_state) = self.prop_editor.edit_state {
            match code {
                KeyCode::Char(c) => {
                    edit_state.edit_buffer.insert(edit_state.cursor_pos, c);
                    edit_state.cursor_pos += 1;
                }
                KeyCode::Backspace => {
                    if edit_state.cursor_pos > 0 {
                        edit_state.cursor_pos -= 1;
                        edit_state.edit_buffer.remove(edit_state.cursor_pos);
                    }
                }
                KeyCode::Delete => {
                    if edit_state.cursor_pos < edit_state.edit_buffer.len() {
                        edit_state.edit_buffer.remove(edit_state.cursor_pos);
                    }
                }
                KeyCode::Left => {
                    edit_state.cursor_pos = edit_state.cursor_pos.saturating_sub(1);
                }
                KeyCode::Right => {
                    edit_state.cursor_pos = (edit_state.cursor_pos + 1).min(edit_state.edit_buffer.len());
                }
                KeyCode::Home => {
                    edit_state.cursor_pos = 0;
                }
                KeyCode::End => {
                    edit_state.cursor_pos = edit_state.edit_buffer.len();
                }
                KeyCode::Enter => {
                    // Save the edited property
                    let entry_name = edit_state.entry_name.clone();
                    let entry_type = edit_state.entry_type.clone();
                    let new_value = edit_state.edit_buffer.clone();
                    
                    if let Some((ref name, ref etype)) = self.prop_editor.entry {
                        if let Some(entry) = self.config.get_entry(name, etype) {
                            if edit_state.property_index < entry.properties.len() {
                                let prop_name = entry.properties[edit_state.property_index].name.clone();
                                if let Err(e) = self.config.set_property(&entry_name, &entry_type, &prop_name, &new_value) {
                                    self.status_message = Some(format!("Error saving property: {}", e));
                                } else {
                                    self.is_dirty = true;
                                    self.status_message = Some(format!("Updated {} = {}", prop_name, new_value));
                                    self.load_from_config();
                                }
                            }
                        }
                    }
                    self.prop_editor.edit_state = None;
                }
                KeyCode::Esc => {
                    // Cancel editing
                    self.prop_editor.edit_state = None;
                }
                _ => {}
            }
            return Ok(());
        }

        // If we're adding a new property
        if self.prop_editor.adding_new {
            match code {
                KeyCode::Char(c) => {
                    if self.prop_editor.editing_name {
                        self.prop_editor.new_name.insert(self.prop_editor.new_cursor, c);
                        self.prop_editor.new_cursor += 1;
                    } else {
                        self.prop_editor.new_value.insert(self.prop_editor.new_cursor, c);
                        self.prop_editor.new_cursor += 1;
                    }
                }
                KeyCode::Backspace => {
                    if self.prop_editor.editing_name {
                        if self.prop_editor.new_cursor > 0 {
                            self.prop_editor.new_cursor -= 1;
                            self.prop_editor.new_name.remove(self.prop_editor.new_cursor);
                        }
                    } else {
                        if self.prop_editor.new_cursor > 0 {
                            self.prop_editor.new_cursor -= 1;
                            self.prop_editor.new_value.remove(self.prop_editor.new_cursor);
                        }
                    }
                }
                KeyCode::Tab => {
                    // Switch between name and value fields
                    self.prop_editor.editing_name = !self.prop_editor.editing_name;
                    self.prop_editor.new_cursor = if self.prop_editor.editing_name {
                        self.prop_editor.new_name.len()
                    } else {
                        self.prop_editor.new_value.len()
                    };
                }
                KeyCode::Enter => {
                    // Save the new property
                    if !self.prop_editor.new_name.is_empty() && !self.prop_editor.new_value.is_empty() {
                        if let Some((ref name, ref entry_type)) = self.prop_editor.entry {
                            // Determine property type from value
                            let prop_type = if self.prop_editor.new_value == "true" || self.prop_editor.new_value == "false" {
                                PropertyType::Bool
                            } else if self.prop_editor.new_value.parse::<i64>().is_ok() {
                                PropertyType::Int
                            } else {
                                PropertyType::String
                            };
                            
                            if let Err(e) = self.config.add_property(name, entry_type, &self.prop_editor.new_name, &self.prop_editor.new_value, &prop_type) {
                                self.status_message = Some(format!("Error adding property: {}", e));
                            } else {
                                self.is_dirty = true;
                                self.status_message = Some(format!("Added {} = {}", self.prop_editor.new_name, self.prop_editor.new_value));
                                self.load_from_config();
                            }
                        }
                    }
                    self.prop_editor.adding_new = false;
                    self.prop_editor.new_name.clear();
                    self.prop_editor.new_value.clear();
                }
                KeyCode::Esc => {
                    self.prop_editor.adding_new = false;
                    self.prop_editor.new_name.clear();
                    self.prop_editor.new_value.clear();
                }
                _ => {}
            }
            return Ok(());
        }

        // Normal property list navigation
        match code {
            KeyCode::Up => {
                self.move_property_selection(-1);
            }
            KeyCode::Down => {
                self.move_property_selection(1);
            }
            KeyCode::Tab => {
                // Toggle between configured and available options
                self.prop_editor.showing_available = !self.prop_editor.showing_available;
                self.prop_editor.list_state.select(Some(0));
                *self.prop_editor.list_state.offset_mut() = 0; // Reset scroll position
                if self.prop_editor.showing_available {
                    self.status_message = Some("Showing available options (not yet configured)".to_string());
                } else {
                    self.status_message = Some("Showing configured properties".to_string());
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.prop_editor.showing_available {
                    // Add the selected available option
                    self.add_selected_available_option()?;
                } else {
                    // Edit the selected property
                    self.edit_selected_property()?;
                }
            }
            KeyCode::Char('e') => {
                if !self.prop_editor.showing_available {
                    // Edit the selected property
                    self.edit_selected_property()?;
                }
            }
            KeyCode::Char('a') | KeyCode::Char('n') => {
                // Add new property (manual entry)
                self.prop_editor.adding_new = true;
                self.prop_editor.editing_name = true;
                self.prop_editor.new_name.clear();
                self.prop_editor.new_value.clear();
                self.prop_editor.new_cursor = 0;
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if !self.prop_editor.showing_available {
                    // Delete the selected property
                    self.delete_selected_property()?;
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                // Close property editor
                self.prop_editor.reset();
                self.focus = crate::app::types::Focus::Programs; // Go back to the list
            }
            _ => {}
        }

        Ok(())
    }

    /// Edit the currently selected property
    fn edit_selected_property(&mut self) -> Result<()> {
        if let Some((ref name, ref entry_type)) = self.prop_editor.entry {
            if let Some(entry) = self.config.get_entry(name, entry_type) {
                if let Some(idx) = self.prop_editor.list_state.selected() {
                    if idx < entry.properties.len() {
                        let prop = &entry.properties[idx];
                        self.prop_editor.edit_state = Some(PropertyEditState {
                            entry_name: name.clone(),
                            entry_type: entry_type.clone(),
                            property_index: idx,
                            edit_buffer: prop.value.clone(),
                            cursor_pos: prop.value.len(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Add the selected available option to the config
    fn add_selected_available_option(&mut self) -> Result<()> {
        if let Some(idx) = self.prop_editor.list_state.selected() {
            if idx < self.prop_editor.available_options.len() {
                let (opt_name, opt_info) = self.prop_editor.available_options[idx].clone();
                
                if let Some((ref name, ref entry_type)) = self.prop_editor.entry {
                    // Use schema to get the property type
                    let prop_type = if let Some(schema) = self.schema_cache.get_schema(entry_type, name) {
                        schema.property_type_for(&opt_name)
                    } else {
                        PropertyType::Expression
                    };

                    // Get default value or a sensible default based on type
                    let default_value = opt_info.default
                        .map(|v| match v {
                            serde_json::Value::Bool(b) => b.to_string(),
                            serde_json::Value::Number(n) => n.to_string(),
                            serde_json::Value::String(s) => s,
                            serde_json::Value::Null => match opt_info.option_type.as_str() {
                                "boolean" => "false".to_string(),
                                "string" => "\"\"".to_string(),
                                "signed integer" | "integer" => "0".to_string(),
                                _ => "null".to_string(),
                            },
                            _ => serde_json::to_string(&v).unwrap_or_else(|_| "null".to_string()),
                        })
                        .unwrap_or_else(|| match opt_info.option_type.as_str() {
                            "boolean" => "false".to_string(),
                            "string" => "\"\"".to_string(),
                            "signed integer" | "integer" => "0".to_string(),
                            _ => "null".to_string(),
                        });

                    if let Err(e) = self.config.add_property(name, entry_type, &opt_name, &default_value, &prop_type) {
                        self.status_message = Some(format!("Error adding property: {}", e));
                    } else {
                        self.is_dirty = true;
                        self.status_message = Some(format!("Added {} = {}", opt_name, default_value));
                        self.load_from_config();
                        
                        // Remove from available options
                        self.prop_editor.available_options.remove(idx);
                        
                        // Adjust selection
                        if !self.prop_editor.available_options.is_empty() {
                            self.prop_editor.list_state.select(Some(idx.min(self.prop_editor.available_options.len() - 1)));
                        } else {
                            // Switch back to configured view
                            self.prop_editor.showing_available = false;
                            self.prop_editor.list_state.select(Some(0));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Delete the selected property
    fn delete_selected_property(&mut self) -> Result<()> {
        let delete_info = if let Some((ref name, ref entry_type)) = self.prop_editor.entry {
            if let Some(entry) = self.config.get_entry(name, entry_type) {
                if let Some(idx) = self.prop_editor.list_state.selected() {
                    if idx < entry.properties.len() {
                        Some((name.clone(), entry_type.clone(), entry.properties[idx].name.clone(), idx))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some((name, entry_type, prop_name, idx)) = delete_info {
            if let Err(e) = self.config.delete_property(&name, &entry_type, &prop_name) {
                self.status_message = Some(format!("Error deleting property: {}", e));
            } else {
                self.is_dirty = true;
                self.status_message = Some(format!("Deleted property: {}", prop_name));
                self.load_from_config();
                
                // Refresh available options (the deleted one should reappear)
                let configured_props = self.config.get_entry(&name, &entry_type)
                    .map(|e| e.properties.clone())
                    .unwrap_or_default();
                self.prop_editor.available_options = self.schema_cache.get_available_options(&entry_type, &name, &configured_props);
                self.prop_editor.available_options.sort_by(|a, b| a.0.cmp(&b.0));
                
                // Adjust selection
                let new_len = self.config.get_entry(&name, &entry_type)
                    .map(|e| e.properties.len())
                    .unwrap_or(0);
                if new_len > 0 {
                    self.prop_editor.list_state.select(Some(idx.min(new_len - 1)));
                } else {
                    self.prop_editor.list_state.select(None);
                }
            }
        }
        Ok(())
    }
}
