//! Shader hot-reload support for the GPU rendering pipeline.
//!
//! Provides [`ShaderWatcher`] for tracking WGSL shader sources and their
//! versions, and [`HotReloadRegistry`] for mapping render pipelines to the
//! shaders they depend on so that pipelines can be invalidated automatically
//! when their source changes.

use std::collections::{HashMap, HashSet};

// ─── Entry points & stage ────────────────────────────────────────────────────

/// Shader pipeline stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
}

/// A named entry point within a shader module.
#[derive(Debug, Clone)]
pub struct EntryPoint {
    pub name: String,
    pub stage: ShaderStage,
}

impl EntryPoint {
    /// Create a new entry point descriptor.
    pub fn new(name: impl Into<String>, stage: ShaderStage) -> Self {
        Self {
            name: name.into(),
            stage,
        }
    }
}

// ─── ShaderSource ─────────────────────────────────────────────────────────────

/// A versioned WGSL shader source record.
#[derive(Debug, Clone)]
pub struct ShaderSource {
    /// Human-readable label used as the map key.
    pub label: String,
    /// Raw WGSL text.
    pub wgsl_source: String,
    /// Declared entry points (computed on insertion / update).
    pub entry_points: Vec<EntryPoint>,
    /// Monotonically increasing version counter; starts at `1`, increments on
    /// every call to [`ShaderWatcher::update_source`].
    pub version: u64,
    /// Unix timestamp (seconds) of the last modification.
    /// In an embedded / no-filesystem context this defaults to `0`.
    pub last_modified: u64,
}

impl ShaderSource {
    /// Construct an initial `ShaderSource` at version `1`.
    fn new(label: impl Into<String>, wgsl_source: impl Into<String>) -> Self {
        let wgsl = wgsl_source.into();
        let entry_points = parse_entry_points(&wgsl);
        Self {
            label: label.into(),
            wgsl_source: wgsl,
            entry_points,
            version: 1,
            last_modified: 0,
        }
    }

    /// Bump the version and replace the WGSL source.
    fn bump(&mut self, new_wgsl: impl Into<String>) {
        self.wgsl_source = new_wgsl.into();
        self.entry_points = parse_entry_points(&self.wgsl_source);
        self.version += 1;
        self.last_modified = current_unix_secs();
    }
}

/// Cheaply parse entry-point names and stages from WGSL source text.
///
/// Looks for `@vertex`, `@fragment`, and `@compute` annotations followed by
/// a `fn <name>` declaration on the same or next line.
fn parse_entry_points(wgsl: &str) -> Vec<EntryPoint> {
    let mut entries = Vec::new();
    let mut lines = wgsl.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();

        // Determine if this line has a stage attribute.
        let stage_opt = if trimmed.contains("@vertex") {
            Some(ShaderStage::Vertex)
        } else if trimmed.contains("@fragment") {
            Some(ShaderStage::Fragment)
        } else if trimmed.contains("@compute") {
            Some(ShaderStage::Compute)
        } else {
            None
        };

        if let Some(stage) = stage_opt {
            // The fn declaration may be on the same line or the next.
            let fn_name = extract_fn_name(trimmed)
                .or_else(|| lines.peek().and_then(|next| extract_fn_name(next.trim())));

            if let Some(name) = fn_name {
                entries.push(EntryPoint::new(name, stage));
            }
        }
    }

    entries
}

/// Extract the function name from a line of the form `fn <name>(...)`.
fn extract_fn_name(line: &str) -> Option<String> {
    let idx = line.find("fn ")?;
    let after = line[idx + 3..].trim();
    // Name ends at '(' or whitespace.
    let end = after
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after.len());
    if end == 0 {
        return None;
    }
    Some(after[..end].to_owned())
}

/// Returns the current time as Unix seconds.  Falls back to `0` if the
/// platform does not expose `SystemTime`.
fn current_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ─── ShaderChangeEvent ────────────────────────────────────────────────────────

/// Emitted by [`ShaderWatcher::poll_changes`] when a source version has
/// changed since the last snapshot.
#[derive(Debug, Clone)]
pub struct ShaderChangeEvent {
    pub label: String,
    pub old_version: u64,
    pub new_version: u64,
}

// ─── ShaderWatcher ────────────────────────────────────────────────────────────

/// Watches a collection of named WGSL shader sources for changes.
///
/// In a no-filesystem context (embedded, WASM, tests) changes are driven
/// explicitly by calling [`ShaderWatcher::update_source`].  On native
/// targets an optional polling loop can check file modification times
/// (see [`ShaderWatcher::poll_changes`]).
pub struct ShaderWatcher {
    /// Filesystem paths added via [`Self::add_path`]; stored for future polling.
    pub watch_paths: Vec<String>,
    /// Polling interval hint in milliseconds (informational only).
    pub poll_interval_ms: u64,
    /// All tracked sources keyed by label.
    pub sources: HashMap<String, ShaderSource>,
    /// Snapshot of versions from the last [`Self::poll_changes`] call.
    snapshot: HashMap<String, u64>,
}

impl ShaderWatcher {
    /// Create a new watcher with the given poll interval.
    pub fn new(poll_interval_ms: u64) -> Self {
        Self {
            watch_paths: Vec::new(),
            poll_interval_ms,
            sources: HashMap::new(),
            snapshot: HashMap::new(),
        }
    }

    /// Register a filesystem path to watch.
    ///
    /// The label used for the source will be the path string itself.
    /// The file is not loaded immediately; call [`Self::update_source`] with its
    /// content or rely on a future polling implementation.
    pub fn add_path(&mut self, path: impl Into<String>) {
        self.watch_paths.push(path.into());
    }

    /// Register an inline WGSL source by label.  If a source with the same
    /// label already exists it is replaced (version resets to `1`).
    pub fn add_inline(&mut self, label: impl Into<String>, wgsl: impl Into<String>) {
        let lbl: String = label.into();
        let src = ShaderSource::new(lbl.clone(), wgsl);
        self.snapshot.insert(lbl.clone(), src.version);
        self.sources.insert(lbl, src);
    }

    /// Check whether any tracked sources have changed since the last call to
    /// `poll_changes`.
    ///
    /// In the current implementation changes are detected purely by comparing
    /// in-memory version numbers; actual filesystem polling is not yet wired up.
    /// Returns the list of [`ShaderChangeEvent`]s describing what changed.
    pub fn poll_changes(&mut self) -> Vec<ShaderChangeEvent> {
        let mut events = Vec::new();

        for (label, src) in &self.sources {
            let snap_version = self.snapshot.get(label).copied().unwrap_or(0);
            if src.version != snap_version {
                events.push(ShaderChangeEvent {
                    label: label.clone(),
                    old_version: snap_version,
                    new_version: src.version,
                });
            }
        }

        // Update snapshot to current state.
        for (label, src) in &self.sources {
            self.snapshot.insert(label.clone(), src.version);
        }

        events
    }

    /// Force-update the WGSL source for an existing label, bumping its
    /// version.  Returns `true` on success, `false` if the label is unknown.
    pub fn update_source(&mut self, label: &str, new_wgsl: impl Into<String>) -> bool {
        if let Some(src) = self.sources.get_mut(label) {
            src.bump(new_wgsl);
            true
        } else {
            false
        }
    }

    /// Look up a source by label.
    pub fn get_source(&self, label: &str) -> Option<&ShaderSource> {
        self.sources.get(label)
    }

    /// Return the current version of a source, or `None` if the label is
    /// not registered.
    pub fn source_version(&self, label: &str) -> Option<u64> {
        self.sources.get(label).map(|s| s.version)
    }
}

// ─── HotReloadRegistry ────────────────────────────────────────────────────────

/// Maps render pipeline IDs to the shader labels they depend on and
/// automatically marks pipelines as invalidated when their shaders change.
pub struct HotReloadRegistry {
    pub watcher: ShaderWatcher,
    /// Set of pipeline IDs that need to be rebuilt.
    pub invalidated_pipelines: HashSet<String>,
    /// Total number of successful reloads processed.
    pub reload_count: u64,
    /// pipeline_id → set of shader labels it depends on.
    pipeline_deps: HashMap<String, HashSet<String>>,
}

impl Default for HotReloadRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HotReloadRegistry {
    /// Create a new registry with a default watcher (500 ms poll interval).
    pub fn new() -> Self {
        Self {
            watcher: ShaderWatcher::new(500),
            invalidated_pipelines: HashSet::new(),
            reload_count: 0,
            pipeline_deps: HashMap::new(),
        }
    }

    /// Register a pipeline as depending on a shader label.
    ///
    /// A pipeline may depend on multiple shaders; call this method once per
    /// shader dependency.
    pub fn register_pipeline(&mut self, pipeline_id: impl Into<String>, shader_label: &str) {
        self.pipeline_deps
            .entry(pipeline_id.into())
            .or_default()
            .insert(shader_label.to_owned());
    }

    /// Poll for shader changes and mark dependent pipelines as invalidated.
    ///
    /// Returns the list of pipeline IDs that have been newly invalidated.
    pub fn process_changes(&mut self) -> Vec<String> {
        let events = self.watcher.poll_changes();
        if events.is_empty() {
            return Vec::new();
        }

        let changed_labels: HashSet<&str> = events.iter().map(|e| e.label.as_str()).collect();

        let mut newly_invalidated = Vec::new();

        for (pipeline_id, deps) in &self.pipeline_deps {
            if deps.iter().any(|l| changed_labels.contains(l.as_str()))
                && !self.invalidated_pipelines.contains(pipeline_id)
            {
                newly_invalidated.push(pipeline_id.clone());
            }
        }

        for id in &newly_invalidated {
            self.invalidated_pipelines.insert(id.clone());
        }

        self.reload_count += events.len() as u64;
        newly_invalidated
    }

    /// Returns `true` if the pipeline is currently marked as invalidated.
    pub fn is_invalidated(&self, pipeline_id: &str) -> bool {
        self.invalidated_pipelines.contains(pipeline_id)
    }

    /// Clear the invalidation flag for a pipeline after it has been rebuilt.
    pub fn clear_invalidated(&mut self, pipeline_id: &str) {
        self.invalidated_pipelines.remove(pipeline_id);
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ShaderSource parsing ─────────────────────────────────────────────────

    #[test]
    fn test_parse_entry_points_compute() {
        let wgsl = "@compute @workgroup_size(64)\nfn main() {}";
        let eps = parse_entry_points(wgsl);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].name, "main");
        assert_eq!(eps[0].stage, ShaderStage::Compute);
    }

    #[test]
    fn test_parse_entry_points_vertex_fragment() {
        let wgsl = "@vertex fn vs_main() {}\n@fragment fn fs_main() {}";
        let eps = parse_entry_points(wgsl);
        assert_eq!(eps.len(), 2);
        assert!(eps.iter().any(|e| e.name == "vs_main"));
        assert!(eps.iter().any(|e| e.name == "fs_main"));
    }

    #[test]
    fn test_parse_no_entry_points() {
        let wgsl = "struct Foo { x: f32 }";
        assert!(parse_entry_points(wgsl).is_empty());
    }

    // ── ShaderWatcher ────────────────────────────────────────────────────────

    #[test]
    fn test_add_inline_and_get() {
        let mut w = ShaderWatcher::new(100);
        w.add_inline("my_shader", "@compute fn main() {}");
        let src = w.get_source("my_shader");
        assert!(src.is_some());
        let src = src.expect("source should exist");
        assert_eq!(src.label, "my_shader");
        assert_eq!(src.version, 1);
    }

    #[test]
    fn test_get_unknown_label_returns_none() {
        let w = ShaderWatcher::new(100);
        assert!(w.get_source("unknown").is_none());
    }

    #[test]
    fn test_source_version_initial() {
        let mut w = ShaderWatcher::new(100);
        w.add_inline("s", "@compute fn main() {}");
        assert_eq!(w.source_version("s"), Some(1));
    }

    #[test]
    fn test_source_version_unknown() {
        let w = ShaderWatcher::new(100);
        assert_eq!(w.source_version("nope"), None);
    }

    #[test]
    fn test_update_source_bumps_version() {
        let mut w = ShaderWatcher::new(100);
        w.add_inline("s", "@compute fn main() {}");
        let ok = w.update_source("s", "@compute fn main_v2() {}");
        assert!(ok);
        assert_eq!(w.source_version("s"), Some(2));
    }

    #[test]
    fn test_update_source_unknown_returns_false() {
        let mut w = ShaderWatcher::new(100);
        assert!(!w.update_source("ghost", "@compute fn x() {}"));
    }

    #[test]
    fn test_update_source_multiple_bumps() {
        let mut w = ShaderWatcher::new(100);
        w.add_inline("s", "fn main() {}");
        for expected in 2..=5_u64 {
            w.update_source("s", format!("fn main_{expected}() {{}}"));
            assert_eq!(w.source_version("s"), Some(expected));
        }
    }

    #[test]
    fn test_poll_changes_after_update() {
        let mut w = ShaderWatcher::new(100);
        w.add_inline("s", "@compute fn main() {}");
        // First poll — nothing has changed since add_inline sets the snapshot.
        let first = w.poll_changes();
        assert!(first.is_empty(), "first poll should be empty");

        // Update source, then poll again.
        w.update_source("s", "@compute fn main_v2() {}");
        let second = w.poll_changes();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].label, "s");
        assert_eq!(second[0].old_version, 1);
        assert_eq!(second[0].new_version, 2);
    }

    #[test]
    fn test_poll_changes_clears_on_second_poll() {
        let mut w = ShaderWatcher::new(100);
        w.add_inline("s", "fn main() {}");
        w.update_source("s", "fn main_v2() {}");
        let _ = w.poll_changes();
        // Without another update, second poll returns nothing.
        assert!(w.poll_changes().is_empty());
    }

    #[test]
    fn test_add_path_stores_path() {
        let mut w = ShaderWatcher::new(100);
        w.add_path("/tmp/test_shader.wgsl");
        assert_eq!(w.watch_paths, vec!["/tmp/test_shader.wgsl".to_owned()]);
    }

    #[test]
    fn test_multiple_inline_sources() {
        let mut w = ShaderWatcher::new(100);
        w.add_inline("a", "fn a() {}");
        w.add_inline("b", "fn b() {}");
        assert_eq!(w.sources.len(), 2);
    }

    // ── HotReloadRegistry ────────────────────────────────────────────────────

    #[test]
    fn test_registry_new_not_invalidated() {
        let reg = HotReloadRegistry::new();
        assert!(!reg.is_invalidated("pipeline_a"));
    }

    #[test]
    fn test_registry_process_changes_invalidates_pipeline() {
        let mut reg = HotReloadRegistry::new();
        reg.watcher.add_inline("my_shader", "@compute fn main() {}");
        reg.register_pipeline("pipeline_a", "my_shader");

        // Consume the "add" snapshot diff, then update.
        reg.watcher.poll_changes();
        reg.watcher
            .update_source("my_shader", "@compute fn main_v2() {}");

        let invalidated = reg.process_changes();
        assert!(invalidated.contains(&"pipeline_a".to_owned()));
        assert!(reg.is_invalidated("pipeline_a"));
    }

    #[test]
    fn test_registry_process_changes_no_change() {
        let mut reg = HotReloadRegistry::new();
        reg.watcher.add_inline("s", "@compute fn main() {}");
        reg.register_pipeline("p", "s");
        reg.watcher.poll_changes(); // drain snapshot diff
        let invalidated = reg.process_changes();
        assert!(invalidated.is_empty());
    }

    #[test]
    fn test_registry_clear_invalidated() {
        let mut reg = HotReloadRegistry::new();
        reg.watcher.add_inline("s", "@compute fn main() {}");
        reg.register_pipeline("p", "s");
        reg.watcher.poll_changes();
        reg.watcher.update_source("s", "@compute fn new_main() {}");
        reg.process_changes();
        assert!(reg.is_invalidated("p"));
        reg.clear_invalidated("p");
        assert!(!reg.is_invalidated("p"));
    }

    #[test]
    fn test_registry_reload_count_increments() {
        let mut reg = HotReloadRegistry::new();
        reg.watcher.add_inline("s", "fn main() {}");
        reg.register_pipeline("p", "s");
        reg.watcher.poll_changes();
        reg.watcher.update_source("s", "fn main_v2() {}");
        reg.process_changes();
        assert_eq!(reg.reload_count, 1);
        reg.watcher.update_source("s", "fn main_v3() {}");
        reg.process_changes();
        assert_eq!(reg.reload_count, 2);
    }

    #[test]
    fn test_registry_unrelated_shader_does_not_invalidate() {
        let mut reg = HotReloadRegistry::new();
        reg.watcher.add_inline("shader_a", "fn a() {}");
        reg.watcher.add_inline("shader_b", "fn b() {}");
        reg.register_pipeline("pipeline_a", "shader_a");
        reg.watcher.poll_changes();

        // Only change shader_b.
        reg.watcher.update_source("shader_b", "fn b_v2() {}");
        let invalidated = reg.process_changes();
        assert!(!invalidated.contains(&"pipeline_a".to_owned()));
        assert!(!reg.is_invalidated("pipeline_a"));
    }

    #[test]
    fn test_entry_point_new() {
        let ep = EntryPoint::new("vs_main", ShaderStage::Vertex);
        assert_eq!(ep.name, "vs_main");
        assert_eq!(ep.stage, ShaderStage::Vertex);
    }

    #[test]
    fn test_shader_source_entry_points_populated() {
        let mut w = ShaderWatcher::new(100);
        w.add_inline("s", "@compute\nfn my_compute() {}");
        let src = w.get_source("s").expect("source should exist");
        assert_eq!(src.entry_points.len(), 1);
        assert_eq!(src.entry_points[0].name, "my_compute");
    }

    #[test]
    fn test_update_source_refreshes_entry_points() {
        let mut w = ShaderWatcher::new(100);
        w.add_inline("s", "@compute fn compute_v1() {}");
        w.update_source("s", "@vertex fn vs_main() {}");
        let src = w.get_source("s").expect("source should exist");
        assert_eq!(src.entry_points[0].stage, ShaderStage::Vertex);
        assert_eq!(src.entry_points[0].name, "vs_main");
    }
}
