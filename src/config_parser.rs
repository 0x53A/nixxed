use anyhow::{Context, Result};
use rnix::{SyntaxKind, SyntaxNode};
use rowan::ast::AstNode;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq)]
pub enum EntryType {
    Program,
    Service,
    Package,
}

impl EntryType {
    /// Get the Nix attribute prefix for this entry type
    pub fn prefix(&self) -> &'static str {
        match self {
            EntryType::Program => "programs",
            EntryType::Service => "services",
            EntryType::Package => "packages",
        }
    }
}

/// The type of a configuration property value
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyType {
    Bool,
    String,
    Int,
    Path,
    List,
    AttrSet,
    Expression, // For complex Nix expressions we can't categorize
}

/// A single configuration property within a program/service block
#[derive(Debug, Clone)]
pub struct ConfigProperty {
    pub name: String,
    pub value: String,
    pub property_type: PropertyType,
    pub text_range: (usize, usize),
}

/// Information about a NixOS option from the schema
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct NixOptionInfo {
    #[serde(rename = "type")]
    pub option_type: String,
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub description: String,
}

/// Schema for a program or service containing all its available options
#[derive(Debug, Clone)]
pub struct NixSchema {
    pub options: HashMap<String, NixOptionInfo>,
    pub fetched_at: SystemTime,
}

impl NixSchema {
    /// Convert option type string to PropertyType
    pub fn property_type_for(&self, option_name: &str) -> PropertyType {
        if let Some(info) = self.options.get(option_name) {
            match info.option_type.as_str() {
                "boolean" | "null or boolean" => PropertyType::Bool,
                "string" | "strings" | "null or string" => PropertyType::String,
                "signed integer" | "integer" | "null or signed integer" => PropertyType::Int,
                "path" | "null or path" => PropertyType::Path,
                s if s.starts_with("list of") => PropertyType::List,
                s if s.contains("attribute set") => PropertyType::AttrSet,
                _ => PropertyType::Expression,
            }
        } else {
            PropertyType::Expression
        }
    }
}

const SCHEMA_CACHE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

/// Cache for NixOS option schemas
pub struct SchemaCache {
    cache_dir: PathBuf,
    memory_cache: HashMap<String, NixSchema>,
}

impl SchemaCache {
    pub fn new() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("nixxed")
            .join("schemas");

        // Create cache directory if it doesn't exist
        let _ = fs::create_dir_all(&cache_dir);

        SchemaCache {
            cache_dir,
            memory_cache: HashMap::new(),
        }
    }

    /// Get the cache file path for a program/service
    fn cache_path(&self, entry_type: &EntryType, name: &str) -> PathBuf {
        self.cache_dir
            .join(format!("{}.{}.json", entry_type.prefix(), name))
    }

    /// Fetch schema for a program or service
    pub fn get_schema(&mut self, entry_type: &EntryType, name: &str) -> Option<NixSchema> {
        // Packages don't have schemas
        if matches!(entry_type, EntryType::Package) {
            return None;
        }

        let key = format!("{:?}.{}", entry_type, name);

        // Check memory cache first
        if let Some(schema) = self.memory_cache.get(&key) {
            if let Ok(age) = SystemTime::now().duration_since(schema.fetched_at) {
                if age < SCHEMA_CACHE_MAX_AGE {
                    return Some(schema.clone());
                }
            }
        }

        // Check file cache
        let cache_path = self.cache_path(entry_type, name);
        if let Ok(metadata) = fs::metadata(&cache_path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(age) = SystemTime::now().duration_since(modified) {
                    if age < SCHEMA_CACHE_MAX_AGE {
                        if let Ok(content) = fs::read_to_string(&cache_path) {
                            if let Ok(options) =
                                serde_json::from_str::<HashMap<String, NixOptionInfo>>(&content)
                            {
                                let schema = NixSchema {
                                    options,
                                    fetched_at: modified,
                                };
                                self.memory_cache.insert(key.clone(), schema.clone());
                                return Some(schema);
                            }
                        }
                    }
                }
            }
        }

        // Fetch from nix-instantiate
        if let Some(schema) = self.fetch_schema(entry_type, name) {
            // Save to file cache
            if let Ok(json) = serde_json::to_string(&schema.options) {
                let _ = fs::write(&cache_path, json);
            }
            // Save to memory cache
            self.memory_cache.insert(key, schema.clone());
            return Some(schema);
        }

        None
    }

    /// Fetch schema from nix-instantiate
    fn fetch_schema(&self, entry_type: &EntryType, name: &str) -> Option<NixSchema> {
        if matches!(entry_type, EntryType::Package) {
            return None;
        }
        let prefix = entry_type.prefix();

        // Build the nix expression to evaluate
        let expr = format!(
            r#"
let 
  opts = (import <nixpkgs/nixos> {{}}).options.{}.{};
  getInfo = name: opt: {{ 
    type = opt.type.description or "unknown"; 
    default = if builtins.hasAttr "default" opt then opt.default else null;
    description = opt.description or "";
  }};
in builtins.mapAttrs getInfo opts
"#,
            prefix, name
        );

        let output = Command::new("nix-instantiate")
            .args(["--eval", "--strict", "-E", &expr, "--json"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let json_str = String::from_utf8(output.stdout).ok()?;
        let options: HashMap<String, NixOptionInfo> = serde_json::from_str(&json_str).ok()?;

        Some(NixSchema {
            options,
            fetched_at: SystemTime::now(),
        })
    }

    /// Get available options that are not yet configured
    pub fn get_available_options(
        &mut self,
        entry_type: &EntryType,
        name: &str,
        configured: &[ConfigProperty],
    ) -> Vec<(String, NixOptionInfo)> {
        if let Some(schema) = self.get_schema(entry_type, name) {
            let configured_names: std::collections::HashSet<_> =
                configured.iter().map(|p| p.name.as_str()).collect();

            schema
                .options
                .into_iter()
                .filter(|(opt_name, _)| {
                    // Skip 'enable' as it's handled separately
                    opt_name != "enable" && !configured_names.contains(opt_name.as_str())
                })
                .collect()
        } else {
            Vec::new()
        }
    }
}

impl Default for SchemaCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a string looks like a valid Nix package name.
/// Valid names contain only letters, digits, hyphens, and underscores.
/// They should not start with a digit and should not be empty.
/// This is used to distinguish commented-out packages from regular comments.
fn is_valid_package_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    // Filter out section headers - they're typically capitalized single words
    // like "# Development", "# Editors", "# Terminals"
    // Real package names are lowercase (git, vim, rust-analyzer)
    let first_char = s.chars().next().unwrap();
    if first_char.is_ascii_uppercase() {
        return false;
    }

    // Must start with a lowercase letter or underscore
    if !first_char.is_ascii_lowercase() && first_char != '_' {
        return false;
    }

    // Rest can be letters, digits, hyphens, or underscores
    for c in s.chars().skip(1) {
        if !c.is_ascii_alphanumeric() && c != '-' && c != '_' {
            return false;
        }
    }

    true
}

#[derive(Debug, Clone)]
pub struct ConfigEntry {
    pub name: String,
    pub entry_type: EntryType,
    pub enabled: bool,
    pub has_extra_config: bool,
    /// Text range in the source for this entry
    pub text_range: (usize, usize),
    /// Properties defined for this entry (excluding 'enable')
    pub properties: Vec<ConfigProperty>,
}

#[derive(Debug, Clone)]
pub struct NixConfig {
    pub path: String,
    pub content: String,
    pub entries: Vec<ConfigEntry>,
}

impl NixConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        let content = fs::read_to_string(&path).context("Failed to read NixOS config file")?;

        let mut config = NixConfig {
            path: path_str,
            content: content.clone(),
            entries: Vec::new(),
        };

        config.parse()?;
        Ok(config)
    }

    /// Verify that disabled packages actually exist in nixpkgs
    /// Removes any commented entries that don't match real packages
    pub fn verify_packages(&mut self, searcher: &crate::search::NixSearcher) {
        self.entries.retain(|entry| {
            // Keep all enabled entries
            if entry.enabled {
                return true;
            }

            // For disabled packages, verify they exist
            if entry.entry_type == EntryType::Package {
                return searcher.verify_package_exists(&entry.name);
            }

            // Keep disabled programs/services (they might be NixOS options)
            true
        });
    }

    fn parse(&mut self) -> Result<()> {
        let parse = rnix::Root::parse(&self.content);

        // We'll still parse even with errors, as partial parsing often works
        let root = parse.tree();

        self.visit_node(root.syntax());

        Ok(())
    }

    /// Clear entries and re-parse the content
    fn reparse(&mut self) -> Result<()> {
        self.entries.clear();
        self.parse()
    }

    fn visit_node(&mut self, node: &SyntaxNode) {
        // Look for attribute sets and bindings
        match node.kind() {
            SyntaxKind::NODE_ATTRPATH_VALUE => {
                self.check_attrpath_value(node);
            }
            _ => {
                // Recurse into children
                for child in node.children() {
                    self.visit_node(&child);
                }
            }
        }
    }

    fn check_attrpath_value(&mut self, node: &SyntaxNode) {
        // Get the attribute path
        let attrpath = node
            .children()
            .find(|c| c.kind() == SyntaxKind::NODE_ATTRPATH);
        let value = node.children().find(|c| {
            matches!(
                c.kind(),
                SyntaxKind::NODE_ATTR_SET
                    | SyntaxKind::NODE_LITERAL
                    | SyntaxKind::NODE_IDENT
                    | SyntaxKind::NODE_LIST
                    | SyntaxKind::NODE_WITH
            )
        });

        if let Some(attrpath) = attrpath {
            let path_text = self.get_attrpath_text(&attrpath);
            let path_parts: Vec<&str> = path_text.split('.').collect();

            // Check for programs.*.enable pattern
            if path_parts.len() >= 3
                && path_parts[0] == "programs"
                && path_parts.last() == Some(&"enable")
            {
                let program_name = path_parts[1].to_string();
                let enabled = self.get_bool_value(&value);

                self.entries.push(ConfigEntry {
                    name: program_name,
                    entry_type: EntryType::Program,
                    enabled,
                    has_extra_config: false,
                    text_range: (
                        node.text_range().start().into(),
                        node.text_range().end().into(),
                    ),
                    properties: Vec::new(),
                });
            }
            // Check for programs.* = { enable = ...; } pattern
            else if path_parts.len() == 2 && path_parts[0] == "programs" {
                if let Some(ref val) = value {
                    if val.kind() == SyntaxKind::NODE_ATTR_SET {
                        if let Some((enabled, has_extra, properties)) =
                            self.check_attr_set_for_enable(val)
                        {
                            self.entries.push(ConfigEntry {
                                name: path_parts[1].to_string(),
                                entry_type: EntryType::Program,
                                enabled,
                                has_extra_config: has_extra,
                                text_range: (
                                    node.text_range().start().into(),
                                    node.text_range().end().into(),
                                ),
                                properties,
                            });
                        }
                    }
                }
            }
            // Check for services.*.enable pattern
            else if path_parts.len() >= 3
                && path_parts[0] == "services"
                && path_parts.last() == Some(&"enable")
            {
                let service_name = path_parts[1].to_string();
                let enabled = self.get_bool_value(&value);

                self.entries.push(ConfigEntry {
                    name: service_name,
                    entry_type: EntryType::Service,
                    enabled,
                    has_extra_config: false,
                    text_range: (
                        node.text_range().start().into(),
                        node.text_range().end().into(),
                    ),
                    properties: Vec::new(),
                });
            }
            // Check for services.* = { enable = ...; } pattern
            else if path_parts.len() == 2 && path_parts[0] == "services" {
                if let Some(ref val) = value {
                    if val.kind() == SyntaxKind::NODE_ATTR_SET {
                        if let Some((enabled, has_extra, properties)) =
                            self.check_attr_set_for_enable(val)
                        {
                            self.entries.push(ConfigEntry {
                                name: path_parts[1].to_string(),
                                entry_type: EntryType::Service,
                                enabled,
                                has_extra_config: has_extra,
                                text_range: (
                                    node.text_range().start().into(),
                                    node.text_range().end().into(),
                                ),
                                properties,
                            });
                        }
                    }
                }
            }
            // Check for environment.systemPackages
            else if path_text == "environment.systemPackages" {
                if let Some(ref val) = value {
                    self.extract_packages(val);
                }
            }
        }

        // Still recurse for nested structures
        for child in node.children() {
            self.visit_node(&child);
        }
    }

    fn get_attrpath_text(&self, node: &SyntaxNode) -> String {
        let mut parts = Vec::new();
        for child in node.children() {
            if child.kind() == SyntaxKind::NODE_IDENT || child.kind() == SyntaxKind::NODE_STRING {
                parts.push(child.text().to_string().trim_matches('"').to_string());
            }
        }
        parts.join(".")
    }

    fn get_bool_value(&self, value: &Option<SyntaxNode>) -> bool {
        if let Some(val) = value {
            let text = val.text().to_string();
            text.trim() == "true"
        } else {
            false
        }
    }

    fn check_attr_set_for_enable(
        &self,
        attr_set: &SyntaxNode,
    ) -> Option<(bool, bool, Vec<ConfigProperty>)> {
        let mut found_enable = false;
        let mut enabled = false;
        let mut properties = Vec::new();

        for child in attr_set.children() {
            if child.kind() == SyntaxKind::NODE_ATTRPATH_VALUE {
                let attrpath = child
                    .children()
                    .find(|c| c.kind() == SyntaxKind::NODE_ATTRPATH);
                if let Some(ap) = attrpath {
                    let path_text = self.get_attrpath_text(&ap);

                    // Find the value node
                    let value_node = child
                        .children()
                        .find(|c| c.kind() != SyntaxKind::NODE_ATTRPATH);

                    if path_text == "enable" {
                        found_enable = true;
                        if let Some(val_child) = value_node {
                            let text = val_child.text().to_string().trim().to_string();
                            enabled = text == "true";
                        }
                    } else {
                        // Extract this as a property
                        if let Some(val_node) = value_node {
                            let (value, prop_type) = self.extract_property_value(&val_node);
                            properties.push(ConfigProperty {
                                name: path_text,
                                value,
                                property_type: prop_type,
                                text_range: (
                                    child.text_range().start().into(),
                                    child.text_range().end().into(),
                                ),
                            });
                        }
                    }
                }
            }
        }

        if found_enable {
            Some((enabled, !properties.is_empty(), properties))
        } else {
            None
        }
    }

    /// Extract value and determine the property type from a value node
    fn extract_property_value(&self, node: &SyntaxNode) -> (String, PropertyType) {
        let text = node.text().to_string().trim().to_string();

        match node.kind() {
            SyntaxKind::NODE_LITERAL => {
                // Check if it's a string, int, or path
                if text.starts_with('"') || text.starts_with("''") {
                    (
                        text.trim_matches('"')
                            .trim_start_matches("''")
                            .trim_end_matches("''")
                            .to_string(),
                        PropertyType::String,
                    )
                } else if text.parse::<i64>().is_ok() {
                    (text, PropertyType::Int)
                } else if text.starts_with('/') || text.starts_with("./") || text.starts_with("~/")
                {
                    (text, PropertyType::Path)
                } else {
                    (text, PropertyType::Expression)
                }
            }
            SyntaxKind::NODE_IDENT => {
                // true/false are identifiers in rnix
                if text == "true" || text == "false" {
                    (text, PropertyType::Bool)
                } else {
                    (text, PropertyType::Expression)
                }
            }
            SyntaxKind::NODE_STRING => (text.trim_matches('"').to_string(), PropertyType::String),
            SyntaxKind::NODE_LIST => (text, PropertyType::List),
            SyntaxKind::NODE_ATTR_SET => (text, PropertyType::AttrSet),
            _ => (text, PropertyType::Expression),
        }
    }

    fn extract_packages(&mut self, node: &SyntaxNode) {
        // Handle "with pkgs; [ ... ]" pattern
        if node.kind() == SyntaxKind::NODE_WITH {
            for child in node.children() {
                if child.kind() == SyntaxKind::NODE_LIST {
                    self.extract_packages_from_list(&child);
                    return;
                }
            }
        }

        // Handle direct list
        if node.kind() == SyntaxKind::NODE_LIST {
            self.extract_packages_from_list(node);
        }
    }

    fn extract_packages_from_list(&mut self, list_node: &SyntaxNode) {
        // Get the text range of the list to scan for commented packages
        let list_start: usize = list_node.text_range().start().into();
        let list_end: usize = list_node.text_range().end().into();
        let list_text = &self.content[list_start..list_end];

        // First, extract active packages from AST
        for child in list_node.children() {
            match child.kind() {
                SyntaxKind::NODE_IDENT => {
                    let name = child.text().to_string();
                    self.entries.push(ConfigEntry {
                        name,
                        entry_type: EntryType::Package,
                        enabled: true,
                        has_extra_config: false,
                        text_range: (
                            child.text_range().start().into(),
                            child.text_range().end().into(),
                        ),
                        properties: Vec::new(),
                    });
                }
                SyntaxKind::NODE_SELECT => {
                    // Handle things like pkgs.package or lib.package
                    let text = child.text().to_string();
                    if let Some(name) = text.split('.').last() {
                        self.entries.push(ConfigEntry {
                            name: name.to_string(),
                            entry_type: EntryType::Package,
                            enabled: true,
                            has_extra_config: false,
                            text_range: (
                                child.text_range().start().into(),
                                child.text_range().end().into(),
                            ),
                            properties: Vec::new(),
                        });
                    }
                }
                _ => {}
            }
        }

        // Now scan for commented-out packages
        // Look for patterns like "#  package-name" or "# package-name"
        // where package-name is a valid nix identifier (lowercase)
        for line in list_text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix('#') {
                let candidate = rest.trim();

                // Check if the line starts with what looks like a package name
                // Handle cases like "#  vim # comment" by taking just the first word
                let first_word = candidate.split_whitespace().next().unwrap_or("");

                // Check if it looks like a package name (lowercase, valid chars)
                if is_valid_package_name(first_word) {
                    // Calculate the position in the original content
                    // Try to find with various spacing patterns
                    let patterns = [
                        format!("#  {}", first_word),
                        format!("# {}", first_word),
                        format!("#{}", first_word),
                    ];

                    for pattern in &patterns {
                        if let Some(offset) = self.content[list_start..list_end].find(pattern) {
                            let abs_start = list_start + offset;
                            let abs_end = abs_start + pattern.len();
                            self.entries.push(ConfigEntry {
                                name: first_word.to_string(),
                                entry_type: EntryType::Package,
                                enabled: false,
                                has_extra_config: false,
                                text_range: (abs_start, abs_end),
                                properties: Vec::new(),
                            });
                            break;
                        }
                    }
                }
            }
        }
    }

    pub fn set_entry_enabled(
        &mut self,
        name: &str,
        entry_type: &EntryType,
        enabled: bool,
    ) -> Result<()> {
        // Find the entry
        let entry_exists = self
            .entries
            .iter()
            .any(|e| e.name == name && &e.entry_type == entry_type);

        if entry_exists {
            match entry_type {
                EntryType::Program | EntryType::Service => {
                    self.toggle_enable_entry(name, entry_type, enabled)?;
                }
                EntryType::Package => {
                    self.toggle_package(name, enabled)?;
                }
            }
        }

        self.reparse()
    }

    fn toggle_enable_entry(
        &mut self,
        name: &str,
        entry_type: &EntryType,
        enabled: bool,
    ) -> Result<()> {
        if matches!(entry_type, EntryType::Package) {
            return Ok(());
        }
        let prefix = entry_type.prefix();

        // Find and replace enable = true/false
        let patterns = [
            format!("{}.{}.enable = true", prefix, name),
            format!("{}.{}.enable = false", prefix, name),
            format!("{}.{}.enable=true", prefix, name),
            format!("{}.{}.enable=false", prefix, name),
        ];

        let replacement = format!("{}.{}.enable = {}", prefix, name, enabled);

        for pattern in &patterns {
            if self.content.contains(pattern) {
                self.content = self.content.replace(pattern, &replacement);
                return Ok(());
            }
        }

        // Try to find "enable = true/false" within the block
        // This is a simplified approach - for complex cases we'd need more sophisticated editing
        let block_pattern_true = format!("enable = true");
        let block_pattern_false = format!("enable = false");

        // Find the entry's text range and modify within it
        if let Some(entry) = self
            .entries
            .iter()
            .find(|e| e.name == name && &e.entry_type == entry_type)
        {
            let (start, end) = entry.text_range;
            let block_text = &self.content[start..end];

            let new_block = if enabled {
                block_text.replace(&block_pattern_false, &block_pattern_true)
            } else {
                block_text.replace(&block_pattern_true, &block_pattern_false)
            };

            self.content = format!(
                "{}{}{}",
                &self.content[..start],
                new_block,
                &self.content[end..]
            );
        }

        Ok(())
    }

    fn toggle_package(&mut self, name: &str, enabled: bool) -> Result<()> {
        if enabled {
            // Uncomment the package
            let commented = format!("# {}", name);
            let commented_space = format!("#  {}", name);

            if self.content.contains(&commented_space) {
                self.content = self.content.replacen(&commented_space, name, 1);
            } else if self.content.contains(&commented) {
                self.content = self.content.replacen(&commented, name, 1);
            }
        } else {
            // Comment out the package - find it in the packages list context
            // Find the package entry
            if let Some(entry) = self
                .entries
                .iter()
                .find(|e| e.name == name && e.entry_type == EntryType::Package)
            {
                let (start, end) = entry.text_range;
                let before = &self.content[..start];
                let after = &self.content[end..];
                self.content = format!("{}# {}{}", before, name, after);
            }
        }

        Ok(())
    }

    pub fn add_entry(&mut self, name: &str, entry_type: &EntryType) -> Result<()> {
        match entry_type {
            EntryType::Program | EntryType::Service => {
                let new_line = format!("  {}.{}.enable = true;\n", entry_type.prefix(), name);
                self.insert_entry_using_ast(&new_line, entry_type)?;
            }
            EntryType::Package => {
                self.add_package_using_ast(name)?;
            }
        }

        self.reparse()
    }

    /// Use rnix AST to find the correct insertion point for a new entry
    fn insert_entry_using_ast(&mut self, new_line: &str, entry_type: &EntryType) -> Result<()> {
        // Get all entries of this type with their positions
        let mut matching_entries: Vec<(usize, usize)> = self
            .entries
            .iter()
            .filter(|e| &e.entry_type == entry_type)
            .map(|e| e.text_range)
            .collect();

        if matching_entries.is_empty() {
            // No existing entries of this type, insert before the final closing brace
            if let Some(pos) = self.content.rfind('}') {
                self.content.insert_str(pos, &format!("\n{}", new_line));
            }
            return Ok(());
        }

        // Sort by start position
        matching_entries.sort_by_key(|(start, _)| *start);

        // Find the end of the first contiguous group
        // Entries are contiguous if there's no blank line between them
        let mut group_end = matching_entries[0].1;

        for i in 1..matching_entries.len() {
            let (start, end) = matching_entries[i];
            // Check if there's a blank line (two consecutive newlines) between entries
            let between = &self.content[group_end..start];
            if between.contains("\n\n") {
                // Blank line found, stop here - use the first group
                break;
            }
            group_end = end;
        }

        // Insert after the end of the first group
        // Find the next newline after group_end to insert on a new line
        let insert_pos = self.content[group_end..]
            .find('\n')
            .map(|p| group_end + p + 1)
            .unwrap_or(group_end);
        self.content.insert_str(insert_pos, new_line);

        Ok(())
    }

    /// Use rnix AST to find the package list and add a new package
    fn add_package_using_ast(&mut self, name: &str) -> Result<()> {
        let parse = rnix::Root::parse(&self.content);
        let root = parse.tree();

        // Find environment.systemPackages list
        if let Some(list_range) = self.find_packages_list(root.syntax()) {
            // Insert after the opening bracket
            let insert_pos = list_range.0 + 1;
            let indent = "\n    ";
            self.content
                .insert_str(insert_pos, &format!("{}{}", indent, name));
        } else {
            // No systemPackages exists, create it before the final closing brace
            let new_block = format!(
                "\n  environment.systemPackages = with pkgs; [\n    {}\n  ];\n",
                name
            );
            if let Some(pos) = self.content.rfind('}') {
                self.content.insert_str(pos, &new_block);
            }
        }

        Ok(())
    }

    /// Find the text range of the package list (the [ ] part)
    fn find_packages_list(&self, node: &SyntaxNode) -> Option<(usize, usize)> {
        for child in node.children() {
            if child.kind() == SyntaxKind::NODE_ATTRPATH_VALUE {
                if let Some(attrpath) = child
                    .children()
                    .find(|c| c.kind() == SyntaxKind::NODE_ATTRPATH)
                {
                    let path_text = self.get_attrpath_text(&attrpath);
                    if path_text == "environment.systemPackages" {
                        // Found it! Now find the list node
                        for val_child in child.children() {
                            if let Some(list_range) = self.find_list_in_node(&val_child) {
                                return Some(list_range);
                            }
                        }
                    }
                }
            }
            // Recurse
            if let Some(range) = self.find_packages_list(&child) {
                return Some(range);
            }
        }
        None
    }

    /// Find a NODE_LIST within a node (handles "with pkgs; [ ... ]" pattern)
    fn find_list_in_node(&self, node: &SyntaxNode) -> Option<(usize, usize)> {
        if node.kind() == SyntaxKind::NODE_LIST {
            return Some((
                node.text_range().start().into(),
                node.text_range().end().into(),
            ));
        }
        for child in node.children() {
            if let Some(range) = self.find_list_in_node(&child) {
                return Some(range);
            }
        }
        None
    }

    pub fn save(&self) -> Result<()> {
        fs::write(&self.path, &self.content).context("Failed to save NixOS config file")?;
        Ok(())
    }

    pub fn get_entries_by_type(&self, entry_type: &EntryType) -> Vec<&ConfigEntry> {
        self.entries
            .iter()
            .filter(|e| &e.entry_type == entry_type)
            .collect()
    }

    /// Get an entry by name and type
    pub fn get_entry(&self, name: &str, entry_type: &EntryType) -> Option<&ConfigEntry> {
        self.entries
            .iter()
            .find(|e| e.name == name && &e.entry_type == entry_type)
    }

    /// Find the text range of a property within an entry
    fn find_property_range(
        &self,
        entry_name: &str,
        entry_type: &EntryType,
        property_name: &str,
    ) -> Option<(usize, usize)> {
        self.get_entry(entry_name, entry_type).and_then(|entry| {
            entry
                .properties
                .iter()
                .find(|p| p.name == property_name)
                .map(|p| p.text_range)
        })
    }

    /// Set a property value for an entry
    pub fn set_property(
        &mut self,
        entry_name: &str,
        entry_type: &EntryType,
        property_name: &str,
        new_value: &str,
    ) -> Result<()> {
        let property_range = self.find_property_range(entry_name, entry_type, property_name);

        if let Some((start, end)) = property_range {
            // Replace the entire property line
            let old_text = &self.content[start..end];

            // Parse the old text to find just the value part
            // Format is typically: "propertyName = value;"
            if let Some(eq_pos) = old_text.find('=') {
                let before_eq = &old_text[..=eq_pos];
                // Format the new value appropriately
                let formatted_value = self.format_property_value(new_value);
                // Make sure to include the semicolon
                let new_text = format!("{} {};", before_eq, formatted_value);

                self.content = format!(
                    "{}{}{}",
                    &self.content[..start],
                    new_text,
                    &self.content[end..]
                );

                return self.reparse();
            }
        }

        Ok(())
    }

    /// Add a new property to an entry
    pub fn add_property(
        &mut self,
        entry_name: &str,
        entry_type: &EntryType,
        property_name: &str,
        value: &str,
        _property_type: &PropertyType,
    ) -> Result<()> {
        // Find the entry
        let entry = self
            .entries
            .iter()
            .find(|e| e.name == entry_name && &e.entry_type == entry_type);

        if let Some(entry) = entry {
            let (start, end) = entry.text_range;
            let entry_text = &self.content[start..end];

            // Check if this is a block style (has braces) or simple enable style
            if entry_text.contains('{') {
                // Block style: insert before the closing brace
                if let Some(close_brace_pos) = entry_text.rfind('}') {
                    let insert_pos = start + close_brace_pos;
                    let formatted_value = self.format_property_value(value);
                    let new_prop = format!("    {} = {};\n  ", property_name, formatted_value);
                    self.content.insert_str(insert_pos, &new_prop);
                }
            } else {
                // Simple enable style: need to convert to block style
                if matches!(entry_type, EntryType::Package) {
                    return Ok(()); // Packages don't have properties
                }

                let formatted_value = self.format_property_value(value);
                let enabled = if entry.enabled { "true" } else { "false" };
                let new_block = format!(
                    "{}.{} = {{\n    enable = {};\n    {} = {};\n  }};",
                    entry_type.prefix(),
                    entry_name,
                    enabled,
                    property_name,
                    formatted_value
                );

                // Replace the old simple style with block style
                self.content = format!(
                    "{}{}{}",
                    &self.content[..start],
                    new_block,
                    &self.content[end..]
                );
            }

            return self.reparse();
        }

        Ok(())
    }

    /// Delete a property from an entry
    pub fn delete_property(
        &mut self,
        entry_name: &str,
        entry_type: &EntryType,
        property_name: &str,
    ) -> Result<()> {
        let property_range = self.find_property_range(entry_name, entry_type, property_name);

        if let Some((start, end)) = property_range {
            // Find the start of the line (for proper deletion)
            let line_start = self.content[..start]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(start);
            // Find the end of the line (including newline)
            let line_end = self.content[end..]
                .find('\n')
                .map(|p| end + p + 1)
                .unwrap_or(end);

            self.content = format!(
                "{}{}",
                &self.content[..line_start],
                &self.content[line_end..]
            );

            return self.reparse();
        }

        Ok(())
    }

    /// Format a value appropriately for Nix syntax
    fn format_property_value(&self, value: &str) -> String {
        // Check if it's a boolean
        if value == "true" || value == "false" {
            return value.to_string();
        }

        // Check if it's a number
        if value.parse::<i64>().is_ok() {
            return value.to_string();
        }

        // Check if it's already a list or attrset
        if (value.starts_with('[') && value.ends_with(']'))
            || (value.starts_with('{') && value.ends_with('}'))
        {
            return value.to_string();
        }

        // Check if it's a path
        if value.starts_with('/') || value.starts_with("./") || value.starts_with("~/") {
            return value.to_string();
        }

        // Otherwise, treat as string and quote it
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_program() {
        let content = r#"
{ config, pkgs, ... }:
{
  programs.git.enable = true;
  programs.vim.enable = false;
}
"#;
        let mut config = NixConfig {
            path: "test.nix".to_string(),
            content: content.to_string(),
            entries: Vec::new(),
        };
        config.parse().unwrap();

        assert!(config.entries.iter().any(|e| e.name == "git" && e.enabled));
        assert!(config.entries.iter().any(|e| e.name == "vim" && !e.enabled));
    }

    #[test]
    fn test_parse_program_block() {
        let content = r#"
{ config, pkgs, ... }:
{
  programs.neovim = {
    enable = true;
    defaultEditor = true;
  };
}
"#;
        let mut config = NixConfig {
            path: "test.nix".to_string(),
            content: content.to_string(),
            entries: Vec::new(),
        };
        config.parse().unwrap();

        let neovim = config.entries.iter().find(|e| e.name == "neovim");
        assert!(neovim.is_some());
        assert!(neovim.unwrap().enabled);
        assert!(neovim.unwrap().has_extra_config);
    }

    #[test]
    fn test_extract_properties() {
        let content = r#"
{ config, pkgs, ... }:
{
  programs.neovim = {
    enable = true;
    defaultEditor = true;
    viAlias = true;
    vimAlias = false;
  };
}
"#;
        let mut config = NixConfig {
            path: "test.nix".to_string(),
            content: content.to_string(),
            entries: Vec::new(),
        };
        config.parse().unwrap();

        let neovim = config.entries.iter().find(|e| e.name == "neovim");
        assert!(neovim.is_some());
        let neovim = neovim.unwrap();

        // Should have 3 properties (excluding 'enable')
        assert_eq!(neovim.properties.len(), 3);

        // Check properties exist
        assert!(neovim
            .properties
            .iter()
            .any(|p| p.name == "defaultEditor" && p.value == "true"));
        assert!(neovim
            .properties
            .iter()
            .any(|p| p.name == "viAlias" && p.value == "true"));
        assert!(neovim
            .properties
            .iter()
            .any(|p| p.name == "vimAlias" && p.value == "false"));

        // Check property types
        let default_editor = neovim
            .properties
            .iter()
            .find(|p| p.name == "defaultEditor")
            .unwrap();
        assert_eq!(default_editor.property_type, PropertyType::Bool);
    }

    #[test]
    fn test_extract_string_property() {
        let content = r#"
{ config, pkgs, ... }:
{
  services.nginx = {
    enable = true;
    user = "nginx";
    package = pkgs.nginx;
  };
}
"#;
        let mut config = NixConfig {
            path: "test.nix".to_string(),
            content: content.to_string(),
            entries: Vec::new(),
        };
        config.parse().unwrap();

        let nginx = config.entries.iter().find(|e| e.name == "nginx");
        assert!(nginx.is_some());
        let nginx = nginx.unwrap();

        // Check string property
        let user_prop = nginx.properties.iter().find(|p| p.name == "user");
        assert!(user_prop.is_some());
        let user_prop = user_prop.unwrap();
        assert_eq!(user_prop.value, "nginx");
        assert_eq!(user_prop.property_type, PropertyType::String);
    }

    #[test]
    fn test_parse_packages() {
        let content = r#"
{ config, pkgs, ... }:
{
  environment.systemPackages = with pkgs; [
    git
    vim
    htop
  ];
}
"#;
        let mut config = NixConfig {
            path: "test.nix".to_string(),
            content: content.to_string(),
            entries: Vec::new(),
        };
        config.parse().unwrap();

        let packages: Vec<_> = config
            .entries
            .iter()
            .filter(|e| e.entry_type == EntryType::Package)
            .collect();
        assert_eq!(packages.len(), 3);
        assert!(packages.iter().any(|e| e.name == "git"));
        assert!(packages.iter().any(|e| e.name == "vim"));
        assert!(packages.iter().any(|e| e.name == "htop"));
    }

    #[test]
    fn test_add_program_inserts_after_first_group() {
        // Test that new programs are inserted after the first contiguous group,
        // separated by a blank line from programs elsewhere in the file
        let content = r#"{ config, pkgs, ... }:
{
  programs.git.enable = true;
  programs.vim.enable = true;
  programs.neovim = {
    enable = true;
  };

  services.openssh.enable = true;

  programs.hyprland.enable = true;
}
"#;
        let mut config = NixConfig {
            path: "test.nix".to_string(),
            content: content.to_string(),
            entries: Vec::new(),
        };
        config.parse().unwrap();

        // Add a new program
        config.add_entry("firefox", &EntryType::Program).unwrap();

        // The new entry should be inserted after neovim block, before services
        // Not at the very end after hyprland
        let firefox_pos = config
            .content
            .find("programs.firefox.enable = true")
            .unwrap();
        let neovim_end = config.content.find("};").unwrap() + 2; // end of neovim block
        let services_pos = config.content.find("services.openssh").unwrap();

        assert!(
            firefox_pos > neovim_end,
            "firefox should be after neovim block"
        );
        assert!(
            firefox_pos < services_pos,
            "firefox should be before services"
        );
    }
}
