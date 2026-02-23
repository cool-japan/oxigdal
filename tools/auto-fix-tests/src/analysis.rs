use crate::parser::TestFailure;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use syn::{Attribute, File, Item, ItemFn};

/// Result of file analysis
#[derive(Debug)]
pub struct FileAnalysis {
    pub file_path: PathBuf,
    pub test_functions: Vec<String>,
    pub has_cfg_test: bool,
}

/// Find all potential test files for a package
pub fn find_test_files(crates_dir: &Path, package: &str) -> Vec<PathBuf> {
    let mut test_files = Vec::new();
    let package_dir = crates_dir.join(package);

    // Find files in tests/ directory
    let tests_dir = package_dir.join("tests");
    if tests_dir.exists() {
        if let Ok(entries) = fs::read_dir(&tests_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                    test_files.push(path);
                }
            }
        }
    }

    // Find files in src/ directory (for #[cfg(test)] modules)
    let src_dir = package_dir.join("src");
    if src_dir.exists() {
        for entry in walkdir::WalkDir::new(&src_dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
        {
            test_files.push(entry.path().to_path_buf());
        }
    }

    test_files
}

/// Analyze a Rust file to find test functions
pub fn analyze_file(file_path: &Path) -> Result<FileAnalysis> {
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

    let syntax_tree: File = syn::parse_file(&content)
        .with_context(|| format!("Failed to parse file: {}", file_path.display()))?;

    let mut test_functions = Vec::new();
    let mut has_cfg_test = false;

    // Scan top-level items
    for item in &syntax_tree.items {
        match item {
            Item::Fn(item_fn) => {
                if is_test_function(item_fn) {
                    test_functions.push(item_fn.sig.ident.to_string());
                }
            }
            Item::Mod(item_mod) => {
                // Check if this is a test module
                if has_cfg_test_attr(&item_mod.attrs) {
                    has_cfg_test = true;
                }

                // Scan inline module items
                if let Some((_, items)) = &item_mod.content {
                    for item in items {
                        if let Item::Fn(item_fn) = item {
                            if is_test_function(item_fn) {
                                // Store with module path for nested modules
                                let module_name = &item_mod.ident;
                                test_functions.push(format!("{}::{}", module_name, item_fn.sig.ident));
                                test_functions.push(item_fn.sig.ident.to_string()); // Also store simple name
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(FileAnalysis {
        file_path: file_path.to_path_buf(),
        test_functions,
        has_cfg_test,
    })
}

/// Check if a function is a test function
pub fn is_test_function(item_fn: &ItemFn) -> bool {
    has_test_attr(&item_fn.attrs)
}

/// Check if attributes contain #[test] or #[tokio::test]
pub fn has_test_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        // Check for #[test]
        if attr.path().is_ident("test") {
            return true;
        }

        // Check for #[tokio::test] - path has two segments
        if attr.path().segments.len() == 2 {
            let seg0 = &attr.path().segments[0].ident;
            let seg1 = &attr.path().segments[1].ident;
            if seg0 == "tokio" && seg1 == "test" {
                return true;
            }
        }

        false
    })
}

/// Check if attributes contain #[cfg(test)]
fn has_cfg_test_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("cfg") {
            // Check meta content for "test"
            if let syn::Meta::List(meta_list) = &attr.meta {
                return meta_list.tokens.to_string().contains("test");
            }
        }
        false
    })
}

/// Check if a test already has #[ignore] attribute
pub fn has_ignore_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if let Ok(meta) = attr.meta.require_path_only() {
            meta.is_ident("ignore")
        } else {
            false
        }
    })
}

/// Check if a test already has #[should_panic] attribute
pub fn has_should_panic_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("should_panic")
    })
}

/// Find the file containing a specific test function
pub fn locate_test_file(
    crates_dir: &Path,
    failure: &TestFailure,
) -> Result<Option<PathBuf>> {
    let test_files = find_test_files(crates_dir, &failure.package);
    let function_name = failure.function_name();

    for file_path in test_files {
        let analysis = analyze_file(&file_path)?;

        // Check if this file contains the test function
        if analysis.test_functions.iter().any(|name| {
            name == function_name || name.ends_with(&format!("::{}", function_name))
        }) {
            return Ok(Some(file_path));
        }
    }

    Ok(None)
}

/// Batch locate test files for multiple failures
pub fn locate_test_files(
    crates_dir: &Path,
    failures: &[&TestFailure],
) -> HashMap<String, Result<Option<PathBuf>>> {
    use std::collections::HashMap;

    let mut results = HashMap::new();

    for failure in failures {
        let key = format!("{}::{}", failure.package, failure.test_name);
        let result = locate_test_file(crates_dir, failure);
        results.insert(key, result);
    }

    results
}

/// Verify that a test function exists and is eligible for fixing
pub fn verify_test_function(
    file_path: &Path,
    function_name: &str,
) -> Result<Option<ItemFn>> {
    let content = fs::read_to_string(file_path)?;
    let syntax_tree: File = syn::parse_file(&content)?;

    // Search top-level functions
    for item in &syntax_tree.items {
        match item {
            Item::Fn(item_fn) => {
                if item_fn.sig.ident == function_name && is_test_function(item_fn) {
                    return Ok(Some(item_fn.clone()));
                }
            }
            Item::Mod(item_mod) => {
                // Search inline modules
                if let Some((_, items)) = &item_mod.content {
                    for item in items {
                        if let Item::Fn(item_fn) = item {
                            if item_fn.sig.ident == function_name && is_test_function(item_fn) {
                                return Ok(Some(item_fn.clone()));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(None)
}

use std::collections::HashMap;
