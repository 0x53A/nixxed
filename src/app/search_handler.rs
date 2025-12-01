use std::collections::HashSet;
use anyhow::Result;

use crate::app::App;
use crate::app::types::ListEntry;
use crate::config_parser::EntryType;
use crate::search::{SearchCategory, SearchMessage, SearchResult};

impl App {
    /// Poll for search results (call this regularly)
    pub fn poll_search(&mut self) {
        if let Some(msg) = self.searcher.poll_results() {
            match msg {
                SearchMessage::Started => {
                    self.is_searching = true;
                    self.status_message = Some("Searching...".to_string());
                }
                SearchMessage::Completed(results) => {
                    self.is_searching = false;
                    self.process_search_results(results);
                }
                SearchMessage::Error(e) => {
                    self.is_searching = false;
                    self.status_message = Some(format!("Search error: {}", e));
                }
            }
        }
    }

    pub fn perform_search(&mut self) -> Result<()> {
        if self.search_query.is_empty() {
            self.load_from_config();
            return Ok(());
        }

        // Check if we have cached results
        if let Some(cached) = self.searcher.get_cached(&self.search_query) {
            self.process_search_results(cached.clone());
            return Ok(());
        }

        // Start async search
        self.searcher.start_search(self.search_query.clone());
        self.is_searching = true;
        self.status_message = Some("Searching...".to_string());

        Ok(())
    }

    fn process_search_results(&mut self, results: Vec<SearchResult>) {
        self.search_results = results;

        // Get current config entries as a set for quick lookup
        let config_programs: HashSet<String> = self.config
            .get_entries_by_type(&EntryType::Program)
            .iter()
            .map(|e| e.name.clone())
            .collect();
        let config_services: HashSet<String> = self.config
            .get_entries_by_type(&EntryType::Service)
            .iter()
            .map(|e| e.name.clone())
            .collect();
        let config_packages: HashSet<String> = self.config
            .get_entries_by_type(&EntryType::Package)
            .iter()
            .map(|e| e.name.clone())
            .collect();

        // Clear current lists
        self.programs.clear();
        self.services.clear();
        self.packages.clear();

        // Add results from config that match the query
        let query_lower = self.search_query.to_lowercase();

        for entry in self.config.get_entries_by_type(&EntryType::Program) {
            if entry.name.to_lowercase().contains(&query_lower) {
                self.programs.push(ListEntry {
                    name: entry.name.clone(),
                    enabled: entry.enabled,
                    in_config: true,
                    has_extra_config: entry.has_extra_config,
                    relevance_order: 0,
                });
            }
        }

        for entry in self.config.get_entries_by_type(&EntryType::Service) {
            if entry.name.to_lowercase().contains(&query_lower) {
                self.services.push(ListEntry {
                    name: entry.name.clone(),
                    enabled: entry.enabled,
                    in_config: true,
                    has_extra_config: entry.has_extra_config,
                    relevance_order: 0,
                });
            }
        }

        for entry in self.config.get_entries_by_type(&EntryType::Package) {
            if entry.name.to_lowercase().contains(&query_lower) {
                self.packages.push(ListEntry {
                    name: entry.name.clone(),
                    enabled: entry.enabled,
                    in_config: true,
                    has_extra_config: false,
                    relevance_order: 0,
                });
            }
        }

        // Add search results that aren't already in config
        // Keep track of their original order (relevance)
        for (relevance_order, result) in self.search_results.iter().enumerate() {
            match result.category {
                SearchCategory::Program => {
                    if !config_programs.contains(&result.name)
                        && !self.programs.iter().any(|p| p.name == result.name)
                    {
                        self.programs.push(ListEntry {
                            name: result.name.clone(),
                            enabled: false,
                            in_config: false,
                            has_extra_config: false,
                            relevance_order,
                        });
                    }
                }
                SearchCategory::Service => {
                    if !config_services.contains(&result.name)
                        && !self.services.iter().any(|s| s.name == result.name)
                    {
                        self.services.push(ListEntry {
                            name: result.name.clone(),
                            enabled: false,
                            in_config: false,
                            has_extra_config: false,
                            relevance_order,
                        });
                    }
                }
                SearchCategory::Package => {
                    if !config_packages.contains(&result.name)
                        && !self.packages.iter().any(|p| p.name == result.name)
                    {
                        self.packages.push(ListEntry {
                            name: result.name.clone(),
                            enabled: false,
                            in_config: false,
                            has_extra_config: false,
                            relevance_order,
                        });
                    }
                }
            }
        }

        // Sort lists: config entries first (by name), then search results (by relevance)
        let sort_fn = |a: &ListEntry, b: &ListEntry| {
            match (a.in_config, b.in_config) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (true, true) => a.name.cmp(&b.name),
                (false, false) => a.relevance_order.cmp(&b.relevance_order),
            }
        };

        self.programs.sort_by(sort_fn);
        self.services.sort_by(sort_fn);
        self.packages.sort_by(sort_fn);

        // Reset selections
        self.program_state.select(if self.programs.is_empty() {
            None
        } else {
            Some(0)
        });
        self.service_state.select(if self.services.is_empty() {
            None
        } else {
            Some(0)
        });
        self.package_state.select(if self.packages.is_empty() {
            None
        } else {
            Some(0)
        });

        let total = self.programs.len() + self.services.len() + self.packages.len();
        self.status_message = Some(format!("Found {} results", total));
    }
}
