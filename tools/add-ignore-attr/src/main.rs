use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit_mut::{self, VisitMut};
use syn::{Attribute, File, Item, ItemFn, ItemMod};

#[derive(Parser, Debug)]
#[command(name = "add-ignore-attr")]
#[command(about = "Add #[ignore] attributes to slow test functions", long_about = None)]
struct Args {
    /// Path to slow-tests.json file
    #[arg(short, long)]
    input: PathBuf,

    /// Dry-run mode: preview changes without applying them
    #[arg(long)]
    dry_run: bool,

    /// Apply mode: actually modify the files
    #[arg(long)]
    apply: bool,

    /// Root directory of the crates (defaults to ./crates)
    #[arg(long, default_value = "crates")]
    crates_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct SlowTest {
    package: String,
    test_name: String,
    duration_secs: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SlowTestsData {
    slow_tests: Vec<SlowTest>,
}

/// Visitor that adds #[ignore] attributes to test functions
struct IgnoreAdder {
    slow_tests: HashSet<String>,
    modified_count: usize,
    current_file: PathBuf,
}

impl IgnoreAdder {
    fn new(slow_tests: HashSet<String>, current_file: PathBuf) -> Self {
        Self {
            slow_tests,
            modified_count: 0,
            current_file,
        }
    }

    fn should_add_ignore(&self, attrs: &[Attribute]) -> bool {
        // Check if function has #[test] or #[tokio::test] attribute
        let has_test_attr = attrs.iter().any(|attr| {
            if let Ok(meta) = attr.meta.require_path_only() {
                meta.is_ident("test")
            } else {
                attr.path().is_ident("tokio")
                    || attr.path().segments.len() == 2
                        && attr.path().segments[0].ident == "tokio"
                        && attr.path().segments[1].ident == "test"
            }
        });

        // Check if function already has #[ignore] attribute
        let has_ignore_attr = attrs.iter().any(|attr| {
            if let Ok(meta) = attr.meta.require_path_only() {
                meta.is_ident("ignore")
            } else {
                false
            }
        });

        has_test_attr && !has_ignore_attr
    }

    fn add_ignore_attr(&mut self, item_fn: &mut ItemFn) {
        let fn_name = item_fn.sig.ident.to_string();

        // Check if this test is in our slow tests list
        if self.slow_tests.contains(&fn_name) && self.should_add_ignore(&item_fn.attrs) {
            // Create #[ignore] attribute
            let ignore_attr: Attribute = syn::parse_quote! { #[ignore] };
            item_fn.attrs.push(ignore_attr);
            self.modified_count += 1;
            println!(
                "  [+] Added #[ignore] to test: {} in {}",
                fn_name,
                self.current_file.display()
            );
        }
    }
}

impl VisitMut for IgnoreAdder {
    fn visit_item_fn_mut(&mut self, node: &mut ItemFn) {
        self.add_ignore_attr(node);
        visit_mut::visit_item_fn_mut(self, node);
    }

    fn visit_item_mod_mut(&mut self, node: &mut ItemMod) {
        // Process inline modules (mod tests { ... })
        if let Some((_, items)) = &mut node.content {
            for item in items.iter_mut() {
                if let Item::Fn(item_fn) = item {
                    self.add_ignore_attr(item_fn);
                }
            }
        }
        visit_mut::visit_item_mod_mut(self, node);
    }
}

fn process_rust_file(
    file_path: &Path,
    slow_tests: &HashSet<String>,
    dry_run: bool,
) -> Result<usize, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(file_path)?;
    let mut syntax_tree: File = syn::parse_file(&content)?;

    let mut adder = IgnoreAdder::new(slow_tests.clone(), file_path.to_path_buf());
    adder.visit_file_mut(&mut syntax_tree);

    if adder.modified_count > 0 {
        if dry_run {
            println!(
                "[DRY RUN] Would modify {} tests in {}",
                adder.modified_count,
                file_path.display()
            );
        } else {
            // Format the modified AST back to source code
            let formatted = prettyplease::unparse(&syntax_tree);
            fs::write(file_path, formatted)?;
            println!(
                "[APPLIED] Modified {} tests in {}",
                adder.modified_count,
                file_path.display()
            );
        }
    }

    Ok(adder.modified_count)
}

fn find_test_files(crates_dir: &Path, package: &str) -> Vec<PathBuf> {
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Validate arguments
    if !args.dry_run && !args.apply {
        eprintln!("Error: Must specify either --dry-run or --apply");
        std::process::exit(1);
    }

    if args.dry_run && args.apply {
        eprintln!("Error: Cannot specify both --dry-run and --apply");
        std::process::exit(1);
    }

    // Read slow tests from JSON
    let json_content = fs::read_to_string(&args.input)?;
    let data: SlowTestsData = serde_json::from_str(&json_content)?;

    println!("Loaded {} slow tests from {}", data.slow_tests.len(), args.input.display());

    // Group slow tests by package
    let mut packages_map: std::collections::HashMap<String, HashSet<String>> =
        std::collections::HashMap::new();

    for slow_test in &data.slow_tests {
        packages_map
            .entry(slow_test.package.clone())
            .or_default()
            .insert(slow_test.test_name.clone());
    }

    println!("\nProcessing {} packages...\n", packages_map.len());

    let mut total_modified = 0;

    // Process each package
    for (package, slow_test_names) in packages_map {
        println!("Package: {} ({} slow tests)", package, slow_test_names.len());

        let test_files = find_test_files(&args.crates_dir, &package);
        println!("  Found {} test files", test_files.len());

        for test_file in test_files {
            match process_rust_file(&test_file, &slow_test_names, args.dry_run) {
                Ok(count) => {
                    total_modified += count;
                }
                Err(e) => {
                    eprintln!("  [ERROR] Failed to process {}: {}", test_file.display(), e);
                }
            }
        }
        println!();
    }

    println!("Summary:");
    println!("  Total tests modified: {}", total_modified);
    if args.dry_run {
        println!("  Mode: DRY RUN (no files were changed)");
        println!("\nRun with --apply to actually modify the files.");
    } else {
        println!("  Mode: APPLIED (files were modified)");
    }

    Ok(())
}
