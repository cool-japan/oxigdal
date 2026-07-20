//! Built-in workflow template library.

use crate::error::{Result, WorkflowError};
use crate::templates::{
    Parameter, ParameterConstraints, ParameterType, ParameterValue, TemplateCategory,
    WorkflowTemplate,
};
use dashmap::DashMap;
use std::sync::Arc;

/// Workflow template library.
pub struct WorkflowTemplateLibrary {
    templates: Arc<DashMap<String, WorkflowTemplate>>,
}

impl WorkflowTemplateLibrary {
    /// Create a new template library.
    pub fn new() -> Self {
        let library = Self {
            templates: Arc::new(DashMap::new()),
        };

        // Add built-in templates
        library.register_builtin_templates();

        library
    }

    /// Register all built-in templates.
    fn register_builtin_templates(&self) {
        // Satellite processing template
        if let Ok(template) = Self::create_satellite_processing_template() {
            let _ = self.add_template(template);
        }

        // Change detection template
        if let Ok(template) = Self::create_change_detection_template() {
            let _ = self.add_template(template);
        }

        // Batch processing template
        if let Ok(template) = Self::create_batch_processing_template() {
            let _ = self.add_template(template);
        }

        // Terrain analysis template
        if let Ok(template) = Self::create_terrain_analysis_template() {
            let _ = self.add_template(template);
        }
    }

    /// Add a template to the library.
    pub fn add_template(&self, template: WorkflowTemplate) -> Result<()> {
        if self.templates.contains_key(&template.id) {
            return Err(WorkflowError::already_exists(&template.id));
        }

        self.templates.insert(template.id.clone(), template);
        Ok(())
    }

    /// Get a template by ID.
    pub fn get_template(&self, id: &str) -> Option<WorkflowTemplate> {
        self.templates.get(id).map(|entry| entry.clone())
    }

    /// Remove a template.
    pub fn remove_template(&self, id: &str) -> Option<WorkflowTemplate> {
        self.templates.remove(id).map(|(_, template)| template)
    }

    /// List all templates.
    pub fn list_templates(&self) -> Vec<WorkflowTemplate> {
        self.templates
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// List templates by category.
    pub fn list_by_category(&self, category: &TemplateCategory) -> Vec<WorkflowTemplate> {
        self.templates
            .iter()
            .filter(|entry| &entry.value().metadata.category == category)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Search templates by tag.
    pub fn search_by_tag(&self, tag: &str) -> Vec<WorkflowTemplate> {
        self.templates
            .iter()
            .filter(|entry| entry.value().tags.contains(&tag.to_string()))
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Create satellite processing template.
    fn create_satellite_processing_template() -> Result<WorkflowTemplate> {
        let mut template = WorkflowTemplate::new(
            "satellite-processing",
            "Satellite Image Processing",
            "Process satellite imagery with atmospheric correction and cloud masking",
        );

        template.set_category(TemplateCategory::SatelliteProcessing);
        template.metadata.complexity = 3;
        template.add_tag("satellite");
        template.add_tag("processing");
        template.add_tag("imagery");

        template.add_parameter(Parameter {
            name: "input_path".to_string(),
            param_type: ParameterType::FilePath,
            description: "Input satellite image path".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        template.add_parameter(Parameter {
            name: "output_path".to_string(),
            param_type: ParameterType::FilePath,
            description: "Output processed image path".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        template.add_parameter(Parameter {
            name: "apply_cloud_mask".to_string(),
            param_type: ParameterType::Boolean,
            description: "Apply cloud masking".to_string(),
            required: false,
            default_value: Some(ParameterValue::Boolean(true)),
            constraints: None,
        });

        template.set_template(
            r#"{
                "id": "satellite-processing-workflow",
                "name": "Satellite Processing",
                "description": "Process satellite imagery",
                "version": "1.0.0",
                "tasks": [],
                "dependencies": [],
                "metadata": {}
            }"#,
        );

        Ok(template)
    }

    /// Create change detection template.
    fn create_change_detection_template() -> Result<WorkflowTemplate> {
        let mut template = WorkflowTemplate::new(
            "change-detection",
            "Change Detection Workflow",
            "Detect changes between two time periods using multi-temporal imagery",
        );

        template.set_category(TemplateCategory::ChangeDetection);
        template.metadata.complexity = 4;
        template.add_tag("change-detection");
        template.add_tag("temporal");

        template.add_parameter(Parameter {
            name: "before_image".to_string(),
            param_type: ParameterType::FilePath,
            description: "Image from before period".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        template.add_parameter(Parameter {
            name: "after_image".to_string(),
            param_type: ParameterType::FilePath,
            description: "Image from after period".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        template.add_parameter(Parameter {
            name: "threshold".to_string(),
            param_type: ParameterType::Float,
            description: "Change detection threshold".to_string(),
            required: false,
            default_value: Some(ParameterValue::Float(0.5)),
            constraints: Some(ParameterConstraints {
                min: Some(0.0),
                max: Some(1.0),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        });

        template.set_template(
            r#"{
                "id": "change-detection-workflow",
                "name": "Change Detection",
                "description": "Detect changes between imagery",
                "version": "1.0.0",
                "tasks": [],
                "dependencies": [],
                "metadata": {}
            }"#,
        );

        Ok(template)
    }

    /// Create batch processing template.
    fn create_batch_processing_template() -> Result<WorkflowTemplate> {
        let mut template = WorkflowTemplate::new(
            "batch-processing",
            "Batch Processing Workflow",
            "Process multiple files in parallel",
        );

        template.set_category(TemplateCategory::BatchProcessing);
        template.metadata.complexity = 2;
        template.add_tag("batch");
        template.add_tag("parallel");

        template.add_parameter(Parameter {
            name: "input_directory".to_string(),
            param_type: ParameterType::FilePath,
            description: "Input directory containing files to process".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        template.add_parameter(Parameter {
            name: "output_directory".to_string(),
            param_type: ParameterType::FilePath,
            description: "Output directory for processed files".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        template.add_parameter(Parameter {
            name: "parallel_tasks".to_string(),
            param_type: ParameterType::Integer,
            description: "Number of parallel tasks".to_string(),
            required: false,
            default_value: Some(ParameterValue::Integer(4)),
            constraints: Some(ParameterConstraints {
                min: Some(1.0),
                max: Some(32.0),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        });

        template.set_template(
            r#"{
                "id": "batch-processing-workflow",
                "name": "Batch Processing",
                "description": "Process files in batch",
                "version": "1.0.0",
                "tasks": [],
                "dependencies": [],
                "metadata": {}
            }"#,
        );

        Ok(template)
    }

    /// Create terrain analysis template.
    fn create_terrain_analysis_template() -> Result<WorkflowTemplate> {
        let mut template = WorkflowTemplate::new(
            "terrain-analysis",
            "Terrain Analysis Workflow",
            "Analyze terrain from DEM data",
        );

        template.set_category(TemplateCategory::TerrainAnalysis);
        template.metadata.complexity = 3;
        template.add_tag("terrain");
        template.add_tag("dem");
        template.add_tag("analysis");

        template.add_parameter(Parameter {
            name: "dem_path".to_string(),
            param_type: ParameterType::FilePath,
            description: "Digital Elevation Model file path".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        template.add_parameter(Parameter {
            name: "analysis_type".to_string(),
            param_type: ParameterType::Enum {
                allowed_values: vec![
                    "slope".to_string(),
                    "aspect".to_string(),
                    "hillshade".to_string(),
                    "all".to_string(),
                ],
            },
            description: "Type of terrain analysis to perform".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("all".to_string())),
            constraints: None,
        });

        template.set_template(
            r#"{
                "id": "terrain-analysis-workflow",
                "name": "Terrain Analysis",
                "description": "Analyze terrain from DEM",
                "version": "1.0.0",
                "tasks": [],
                "dependencies": [],
                "metadata": {}
            }"#,
        );

        Ok(template)
    }

    /// Export library to JSON.
    pub fn export_to_json(&self) -> Result<String> {
        let templates = self.list_templates();
        serde_json::to_string_pretty(&templates)
            .map_err(|e| WorkflowError::template(format!("Failed to export library: {}", e)))
    }

    /// Import library from JSON.
    pub fn import_from_json(&self, json: &str) -> Result<usize> {
        let templates: Vec<WorkflowTemplate> = serde_json::from_str(json)
            .map_err(|e| WorkflowError::template(format!("Failed to import library: {}", e)))?;

        let mut count = 0;
        for template in templates {
            if self.add_template(template).is_ok() {
                count += 1;
            }
        }

        Ok(count)
    }
}

impl Default for WorkflowTemplateLibrary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_creation() {
        let library = WorkflowTemplateLibrary::new();
        let templates = library.list_templates();

        // Should have built-in templates
        assert!(!templates.is_empty());
    }

    #[test]
    fn test_add_get_template() {
        let library = WorkflowTemplateLibrary::new();

        let template = WorkflowTemplate::new("test", "Test", "Test template");

        assert!(library.add_template(template).is_ok());
        assert!(library.get_template("test").is_some());
    }

    #[test]
    fn test_remove_template() {
        let library = WorkflowTemplateLibrary::new();

        let template = WorkflowTemplate::new("test", "Test", "Test template");
        library.add_template(template).expect("Failed to add");

        assert!(library.remove_template("test").is_some());
        assert!(library.get_template("test").is_none());
    }

    #[test]
    fn test_list_by_category() {
        let library = WorkflowTemplateLibrary::new();

        let satellite_templates = library.list_by_category(&TemplateCategory::SatelliteProcessing);

        assert!(!satellite_templates.is_empty());
    }

    #[test]
    fn test_search_by_tag() {
        let library = WorkflowTemplateLibrary::new();

        let batch_templates = library.search_by_tag("batch");

        assert!(!batch_templates.is_empty());
    }

    #[test]
    fn test_duplicate_template() {
        let library = WorkflowTemplateLibrary::new();

        let template1 = WorkflowTemplate::new("dup", "Dup", "Duplicate");
        let template2 = WorkflowTemplate::new("dup", "Dup2", "Duplicate 2");

        assert!(library.add_template(template1).is_ok());
        assert!(library.add_template(template2).is_err());
    }

    #[test]
    fn test_export_import() {
        let library1 = WorkflowTemplateLibrary::new();

        let json = library1.export_to_json().expect("Failed to export");

        let library2 = WorkflowTemplateLibrary::default();
        library2.templates.clear(); // Clear built-ins for test

        let count = library2.import_from_json(&json).expect("Failed to import");

        assert!(count > 0);
    }
}
