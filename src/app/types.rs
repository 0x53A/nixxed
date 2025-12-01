use crate::config_parser::{EntryType, NixOptionInfo};
use ratatui::widgets::ListState;

#[derive(Debug, Clone, PartialEq)]
pub enum Focus {
    SearchBar,
    Programs,
    Services,
    Packages,
    PropertyEditor,
}

/// State for the rebuild confirmation prompt
#[derive(Debug, Default)]
pub struct RebuildPromptState {
    pub show: bool,
    pub selected: usize,       // 0 = Yes, 1 = No
    pub pending_rebuild: bool, // Signal to main loop to run rebuild
}

#[derive(Debug, Clone)]
pub struct ListEntry {
    pub name: String,
    pub description: String, // Description from search results
    pub enabled: bool,
    pub in_config: bool, // Whether this entry exists in the config file
    pub has_extra_config: bool,
    pub relevance_order: usize, // Order from search results (lower = more relevant)
}

/// State for editing a property value
#[derive(Debug, Clone)]
pub struct PropertyEditState {
    pub entry_name: String,
    pub entry_type: EntryType,
    pub property_index: usize,
    pub edit_buffer: String,
    pub cursor_pos: usize,
}

#[derive(Debug, Clone)]
pub enum ListType {
    Programs,
    Services,
    Packages,
}

/// Property editor state - extracted for cleaner organization
#[derive(Debug)]
pub struct PropertyEditorState {
    pub show: bool,
    pub entry: Option<(String, EntryType)>,
    pub list_state: ListState,
    pub edit_state: Option<PropertyEditState>,
    pub adding_new: bool,
    pub new_name: String,
    pub new_value: String,
    pub new_cursor: usize,
    pub editing_name: bool, // true = editing name, false = editing value
    pub available_options: Vec<(String, NixOptionInfo)>,
    pub showing_available: bool, // Toggle between configured and available
}

/// State for showing a description popup
#[derive(Debug, Default)]
pub struct DescriptionPopupState {
    pub show: bool,
    pub name: String,
    pub description: String,
    pub scroll_offset: u16,
    pub total_lines: u16,
    pub visible_lines: u16,
}

impl Default for PropertyEditorState {
    fn default() -> Self {
        Self {
            show: false,
            entry: None,
            list_state: ListState::default(),
            edit_state: None,
            adding_new: false,
            new_name: String::new(),
            new_value: String::new(),
            new_cursor: 0,
            editing_name: true,
            available_options: Vec::new(),
            showing_available: false,
        }
    }
}

impl PropertyEditorState {
    pub fn reset(&mut self) {
        self.show = false;
        self.entry = None;
        self.list_state = ListState::default();
        self.edit_state = None;
        self.adding_new = false;
        self.new_name.clear();
        self.new_value.clear();
        self.new_cursor = 0;
        self.editing_name = true;
        self.showing_available = false;
    }
}
