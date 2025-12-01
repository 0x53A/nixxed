use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime};
use serde::Deserialize;
use std::sync::mpsc;
use std::thread;

const CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60); // 1 week
const API_URL: &str = "https://search.nixos.org/backend/latest-44-nixos-unstable/_search";
const API_AUTH: &str = "Basic YVdWU0FMWHBadjpYOGdQSG56TDUyd0ZFZWt1eHNmUTljU2g=";

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub name: String,
    pub description: String,
    pub category: SearchCategory,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchCategory {
    Program,
    Service,
    Package,
}

/// Response from NixOS search API
#[derive(Debug, Deserialize)]
struct ElasticResponse {
    hits: ElasticHits,
}

#[derive(Debug, Deserialize)]
struct ElasticHits {
    hits: Vec<ElasticHit>,
}

#[derive(Debug, Deserialize)]
struct ElasticHit {
    #[serde(rename = "_source")]
    source: PackageSource,
}

#[derive(Debug, Deserialize)]
struct PackageSource {
    package_attr_name: String,
    #[serde(default)]
    package_pname: Option<String>,
    #[serde(default)]
    package_description: Option<String>,
    #[serde(default)]
    package_programs: Option<Vec<String>>,
}

/// Message sent from search thread to main thread
pub enum SearchMessage {
    Started,
    Completed(Vec<SearchResult>),
    Error(String),
}

/// HTTP-level cache for API responses
struct HttpCache {
    cache_dir: PathBuf,
}

impl HttpCache {
    fn new() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("nixxed");
        
        // Create cache directory if it doesn't exist
        let _ = fs::create_dir_all(&cache_dir);
        
        HttpCache { cache_dir }
    }

    /// Clean up cache files older than CACHE_MAX_AGE
    fn cleanup_old_entries(&self) {
        if let Ok(entries) = fs::read_dir(&self.cache_dir) {
            let now = SystemTime::now();
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        if let Ok(age) = now.duration_since(modified) {
                            if age > CACHE_MAX_AGE {
                                let _ = fs::remove_file(entry.path());
                            }
                        }
                    }
                }
            }
        }
    }

    /// Generate a cache filename from the request body
    fn cache_key(&self, request_body: &str) -> PathBuf {
        // Use a hash of the request body as filename
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        request_body.hash(&mut hasher);
        let hash = hasher.finish();
        
        self.cache_dir.join(format!("{:x}.json", hash))
    }

    /// Try to get a cached response
    fn get(&self, request_body: &str) -> Option<String> {
        let path = self.cache_key(request_body);
        
        if let Ok(metadata) = fs::metadata(&path) {
            // Check if cache is still valid
            if let Ok(modified) = metadata.modified() {
                if let Ok(age) = SystemTime::now().duration_since(modified) {
                    if age <= CACHE_MAX_AGE {
                        return fs::read_to_string(&path).ok();
                    }
                }
            }
        }
        None
    }

    /// Store a response in the cache
    fn set(&self, request_body: &str, response: &str) {
        let path = self.cache_key(request_body);
        let _ = fs::write(path, response);
    }
}

pub struct NixSearcher {
    /// Cache of parsed search results (in-memory)
    cache: HashMap<String, Vec<SearchResult>>,
    /// HTTP cache for raw API responses
    http_cache: HttpCache,
    /// Receiver for search results
    receiver: Option<mpsc::Receiver<SearchMessage>>,
    /// Current search query (to match results)
    current_query: Option<String>,
}

impl NixSearcher {
    pub fn new() -> Self {
        let http_cache = HttpCache::new();
        // Clean up old cache entries on startup
        http_cache.cleanup_old_entries();
        
        NixSearcher {
            cache: HashMap::new(),
            http_cache,
            receiver: None,
            current_query: None,
        }
    }

    /// Start a background search for packages
    pub fn start_search(&mut self, query: String) {
        if query.is_empty() {
            return;
        }

        // Check in-memory cache first
        if self.cache.contains_key(&query) {
            return;
        }

        // Create channel for communication
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.current_query = Some(query.clone());

        // Send started message
        let _ = tx.send(SearchMessage::Started);

        // Clone cache_dir for the thread
        let cache_dir = self.http_cache.cache_dir.clone();

        // Spawn background thread
        thread::spawn(move || {
            let results = run_nix_search_cached(&query, &cache_dir);
            match results {
                Ok(results) => {
                    let _ = tx.send(SearchMessage::Completed(results));
                }
                Err(e) => {
                    let _ = tx.send(SearchMessage::Error(e.to_string()));
                }
            }
        });
    }

    /// Check if there are search results ready (non-blocking)
    pub fn poll_results(&mut self) -> Option<SearchMessage> {
        if let Some(ref receiver) = self.receiver {
            match receiver.try_recv() {
                Ok(msg) => {
                    if let SearchMessage::Completed(ref results) = msg {
                        // Cache the results in memory
                        if let Some(ref query) = self.current_query {
                            self.cache.insert(query.clone(), results.clone());
                        }
                    }
                    if matches!(msg, SearchMessage::Completed(_) | SearchMessage::Error(_)) {
                        // Search is done, clear receiver
                        self.receiver = None;
                        self.current_query = None;
                    }
                    Some(msg)
                }
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.receiver = None;
                    self.current_query = None;
                    Some(SearchMessage::Error("Search thread disconnected".to_string()))
                }
            }
        } else {
            None
        }
    }

    /// Check if a search is currently in progress
    #[allow(dead_code)]
    pub fn is_searching(&self) -> bool {
        self.receiver.is_some()
    }

    /// Get cached results for a query
    #[allow(dead_code)]
    pub fn get_cached(&self, query: &str) -> Option<&Vec<SearchResult>> {
        self.cache.get(query)
    }

    /// Cancel any ongoing search
    #[allow(dead_code)]
    pub fn cancel(&mut self) {
        self.receiver = None;
        self.current_query = None;
    }

    /// Verify if a package exists by doing an exact match search
    /// Returns true if the package exists in nixpkgs
    pub fn verify_package_exists(&self, package_name: &str) -> bool {
        let cache_dir = self.http_cache.cache_dir.clone();
        
        // Do a synchronous search for the exact package name
        if let Ok(results) = run_nix_search_cached(package_name, &cache_dir) {
            // Check for exact match
            results.iter().any(|r| r.name == package_name)
        } else {
            // If search fails, assume package exists to avoid false negatives
            true
        }
    }
}

impl Default for NixSearcher {
    fn default() -> Self {
        Self::new()
    }
}

fn build_search_body(query: &str) -> String {
    serde_json::json!({
        "from": 0,
        "size": 50,
        "sort": [
            {"_score": "desc"},
            {"package_attr_name": "desc"}
        ],
        "query": {
            "bool": {
                "filter": [
                    {"term": {"type": {"value": "package"}}}
                ],
                "must": [
                    {
                        "dis_max": {
                            "tie_breaker": 0.7,
                            "queries": [
                                {
                                    "multi_match": {
                                        "type": "cross_fields",
                                        "query": query,
                                        "analyzer": "whitespace",
                                        "auto_generate_synonyms_phrase_query": false,
                                        "operator": "and",
                                        "fields": [
                                            "package_attr_name^9",
                                            "package_pname^6",
                                            "package_description^1.3",
                                            "package_programs^9"
                                        ]
                                    }
                                },
                                {
                                    "wildcard": {
                                        "package_attr_name": {
                                            "value": format!("*{}*", query.to_lowercase()),
                                            "case_insensitive": true
                                        }
                                    }
                                }
                            ]
                        }
                    }
                ]
            }
        }
    }).to_string()
}

fn run_nix_search_cached(query: &str, cache_dir: &PathBuf) -> Result<Vec<SearchResult>> {
    let search_body = build_search_body(query);
    
    // Create a temporary HttpCache for this thread
    let http_cache = HttpCache { cache_dir: cache_dir.clone() };
    
    // Check HTTP cache first
    let response = if let Some(cached) = http_cache.get(&search_body) {
        cached
    } else {
        // Make the actual HTTP request
        let output = Command::new("curl")
            .args([
                "-s",
                "-X", "POST",
                API_URL,
                "-H", "Content-Type: application/json",
                "-H", &format!("Authorization: {}", API_AUTH),
                "-d", &search_body,
            ])
            .output()
            .context("Failed to run curl command")?;

        let response = String::from_utf8_lossy(&output.stdout).to_string();
        
        // Cache the response
        if !response.is_empty() && !response.contains("\"error\"") {
            http_cache.set(&search_body, &response);
        }
        
        response
    };

    if response.trim().is_empty() {
        return Ok(Vec::new());
    }

    parse_elastic_response(&response, query)
}

/// Calculate a match score for local sorting (higher = better match)
fn calculate_match_score(name: &str, query: &str) -> u32 {
    let name_lower = name.to_lowercase();
    let query_lower = query.to_lowercase();
    
    if name_lower == query_lower {
        // Exact match - highest priority
        1000
    } else if name_lower.starts_with(&query_lower) {
        // Starts with query - high priority, shorter names ranked higher
        500 + (100 - name.len().min(100)) as u32
    } else if name_lower.contains(&query_lower) {
        // Contains query as substring - medium priority
        // Earlier position = higher score
        let pos = name_lower.find(&query_lower).unwrap_or(0);
        200 + (100 - pos.min(100)) as u32
    } else {
        // Fuzzy match / no direct match - use original API order
        0
    }
}

fn parse_elastic_response(output: &str, query: &str) -> Result<Vec<SearchResult>> {
    let response: ElasticResponse = serde_json::from_str(output)
        .context("Failed to parse search response")?;

    let mut results = Vec::new();

    for (api_order, hit) in response.hits.hits.into_iter().enumerate() {
        let source = hit.source;
        let name = source.package_pname
            .unwrap_or_else(|| source.package_attr_name.clone());
        let description = source.package_description.unwrap_or_default();
        let has_programs = source.package_programs
            .map(|p| !p.is_empty())
            .unwrap_or(false);

        // Categorize based on description and whether it has programs
        let category = if has_programs {
            SearchCategory::Program
        } else {
            categorize_result(&name, &description)
        };

        // Calculate local match score
        let match_score = calculate_match_score(&name, query);
        
        results.push((SearchResult {
            name,
            description,
            category,
        }, match_score, api_order));
    }

    // Sort by: match_score (desc), then api_order (asc) for tie-breaking
    results.sort_by(|a, b| {
        b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2))
    });

    // Extract just the SearchResult
    Ok(results.into_iter().map(|(r, _, _)| r).collect())
}

fn categorize_result(name: &str, description: &str) -> SearchCategory {
    let desc_lower = description.to_lowercase();
    let name_lower = name.to_lowercase();

    // Check for service-like packages
    if desc_lower.contains("daemon")
        || desc_lower.contains("server")
        || desc_lower.contains("service")
        || (name_lower.ends_with("d") && desc_lower.contains("system"))
    {
        return SearchCategory::Service;
    }

    // Check for program-like packages (have a main executable)
    if desc_lower.contains("program")
        || desc_lower.contains("tool")
        || desc_lower.contains("editor")
        || desc_lower.contains("browser")
        || desc_lower.contains("shell")
        || desc_lower.contains("compiler")
        || desc_lower.contains("utility")
    {
        return SearchCategory::Program;
    }

    // Default to Package
    SearchCategory::Package
}
