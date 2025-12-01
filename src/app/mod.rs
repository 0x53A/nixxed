//! Application module - main TUI application logic
//!
//! This module is split into several submodules for better organization:
//! - `types`: Core types like Focus, ListEntry, PropertyEditState
//! - `input`: Event handling (keyboard, mouse)
//! - `property_editor`: Property editor logic
//! - `search_handler`: Search processing
//! - `ui`: All rendering code

mod input;
mod property_editor;
mod search_handler;
pub mod types;
pub mod ui;

use anyhow::Result;
use ratatui::{layout::Rect, widgets::ListState};

use crate::config_parser::{EntryType, NixConfig, SchemaCache};
use crate::search::{NixSearcher, SearchResult};

use types::{Focus, ListEntry, PropertyEditorState, RebuildPromptState};

pub struct App {
    pub config: NixConfig,
    pub searcher: NixSearcher,
    pub schema_cache: SchemaCache,
    pub search_query: String,
    pub search_cursor: usize,
    pub focus: Focus,
    pub programs: Vec<ListEntry>,
    pub services: Vec<ListEntry>,
    pub packages: Vec<ListEntry>,
    pub program_state: ListState,
    pub service_state: ListState,
    pub package_state: ListState,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub is_searching: bool,
    pub search_results: Vec<SearchResult>,
    pub show_help: bool,
    // Layout areas for mouse handling
    pub search_area: Rect,
    pub programs_area: Rect,
    pub services_area: Rect,
    pub packages_area: Rect,
    // Property editor state
    pub prop_editor: PropertyEditorState,
    // Property editor area for mouse handling
    pub property_list_area: Rect,
    // Rebuild prompt state
    pub rebuild_prompt: RebuildPromptState,
    // Track unsaved changes
    pub is_dirty: bool,
}

impl App {
    pub fn new(mut config: NixConfig) -> Self {
        let searcher = NixSearcher::new();
        let schema_cache = SchemaCache::new();

        // Verify that disabled packages actually exist in nixpkgs
        config.verify_packages(&searcher);

        let mut app = App {
            config,
            searcher,
            schema_cache,
            search_query: String::new(),
            search_cursor: 0,
            focus: Focus::SearchBar,
            programs: Vec::new(),
            services: Vec::new(),
            packages: Vec::new(),
            program_state: ListState::default(),
            service_state: ListState::default(),
            package_state: ListState::default(),
            should_quit: false,
            status_message: None,
            is_searching: false,
            search_results: Vec::new(),
            show_help: false,
            search_area: Rect::default(),
            programs_area: Rect::default(),
            services_area: Rect::default(),
            packages_area: Rect::default(),
            prop_editor: PropertyEditorState::default(),
            property_list_area: Rect::default(),
            rebuild_prompt: RebuildPromptState::default(),
            is_dirty: false,
        };

        app.load_from_config();
        app
    }

    pub fn load_from_config(&mut self) {
        // Load programs from config
        self.programs = self
            .config
            .get_entries_by_type(&EntryType::Program)
            .into_iter()
            .map(|e| ListEntry {
                name: e.name.clone(),
                enabled: e.enabled,
                in_config: true,
                has_extra_config: e.has_extra_config,
                relevance_order: 0,
            })
            .collect();

        // Load services from config
        self.services = self
            .config
            .get_entries_by_type(&EntryType::Service)
            .into_iter()
            .map(|e| ListEntry {
                name: e.name.clone(),
                enabled: e.enabled,
                in_config: true,
                has_extra_config: e.has_extra_config,
                relevance_order: 0,
            })
            .collect();

        // Load packages from config
        self.packages = self
            .config
            .get_entries_by_type(&EntryType::Package)
            .into_iter()
            .map(|e| ListEntry {
                name: e.name.clone(),
                enabled: e.enabled,
                in_config: true,
                has_extra_config: false,
                relevance_order: 0,
            })
            .collect();

        // Sort all lists
        self.programs.sort_by(|a, b| a.name.cmp(&b.name));
        self.services.sort_by(|a, b| a.name.cmp(&b.name));
        self.packages.sort_by(|a, b| a.name.cmp(&b.name));

        // Select first item in each list if available
        if !self.programs.is_empty() {
            self.program_state.select(Some(0));
        }
        if !self.services.is_empty() {
            self.service_state.select(Some(0));
        }
        if !self.packages.is_empty() {
            self.package_state.select(Some(0));
        }
    }

    pub fn save_config(&mut self) -> Result<()> {
        match self.config.save() {
            Ok(()) => {
                self.is_dirty = false;
                self.status_message = Some("Configuration saved!".to_string());
                // Show rebuild prompt after successful save
                self.rebuild_prompt.show = true;
                self.rebuild_prompt.selected = 0;
                self.rebuild_prompt.pending_rebuild = false;
            }
            Err(e) => {
                self.status_message = Some(format!("Save error: {}", e));
            }
        }
        Ok(())
    }

    pub fn toggle_selected(&mut self, list_type: &types::ListType) -> Result<()> {
        let (entry_type, idx, name, enabled, in_config) = match list_type {
            types::ListType::Programs => {
                let idx = self.program_state.selected();
                if let Some(idx) = idx {
                    if idx < self.programs.len() {
                        let entry = &self.programs[idx];
                        (
                            EntryType::Program,
                            idx,
                            entry.name.clone(),
                            entry.enabled,
                            entry.in_config,
                        )
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            }
            types::ListType::Services => {
                let idx = self.service_state.selected();
                if let Some(idx) = idx {
                    if idx < self.services.len() {
                        let entry = &self.services[idx];
                        (
                            EntryType::Service,
                            idx,
                            entry.name.clone(),
                            entry.enabled,
                            entry.in_config,
                        )
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            }
            types::ListType::Packages => {
                let idx = self.package_state.selected();
                if let Some(idx) = idx {
                    if idx < self.packages.len() {
                        let entry = &self.packages[idx];
                        (
                            EntryType::Package,
                            idx,
                            entry.name.clone(),
                            entry.enabled,
                            entry.in_config,
                        )
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            }
        };

        let new_enabled = !enabled;

        if in_config {
            // Modify existing entry
            if let Err(e) = self
                .config
                .set_entry_enabled(&name, &entry_type, new_enabled)
            {
                self.status_message = Some(format!("Error: {}", e));
                return Ok(());
            }

            self.is_dirty = true;

            // Update the local entry
            match list_type {
                types::ListType::Programs => self.programs[idx].enabled = new_enabled,
                types::ListType::Services => self.services[idx].enabled = new_enabled,
                types::ListType::Packages => self.packages[idx].enabled = new_enabled,
            }

            self.status_message = Some(format!(
                "{} {} {}",
                if new_enabled { "Enabled" } else { "Disabled" },
                match entry_type {
                    EntryType::Program => "program",
                    EntryType::Service => "service",
                    EntryType::Package => "package",
                },
                name
            ));
        } else {
            // Add new entry to config
            if let Err(e) = self.config.add_entry(&name, &entry_type) {
                self.status_message = Some(format!("Error: {}", e));
                return Ok(());
            }

            self.is_dirty = true;

            // Update the local entry
            match list_type {
                types::ListType::Programs => {
                    self.programs[idx].enabled = true;
                    self.programs[idx].in_config = true;
                }
                types::ListType::Services => {
                    self.services[idx].enabled = true;
                    self.services[idx].in_config = true;
                }
                types::ListType::Packages => {
                    self.packages[idx].enabled = true;
                    self.packages[idx].in_config = true;
                }
            }

            self.status_message = Some(format!(
                "Added {} {}",
                match entry_type {
                    EntryType::Program => "program",
                    EntryType::Service => "service",
                    EntryType::Package => "package",
                },
                name
            ));
        }

        Ok(())
    }
}
