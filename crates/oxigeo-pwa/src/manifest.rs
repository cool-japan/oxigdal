//! Web app manifest generation for PWA.

use crate::error::{PwaError, Result};
use serde::{Deserialize, Serialize};

/// Display mode for the PWA.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DisplayMode {
    /// Fullscreen mode
    Fullscreen,

    /// Standalone mode (recommended for PWA)
    Standalone,

    /// Minimal UI mode
    #[serde(rename = "minimal-ui")]
    MinimalUi,

    /// Browser mode
    Browser,
}

impl DisplayMode {
    /// Get display mode as string.
    pub fn as_str(&self) -> &'static str {
        match self {
            DisplayMode::Fullscreen => "fullscreen",
            DisplayMode::Standalone => "standalone",
            DisplayMode::MinimalUi => "minimal-ui",
            DisplayMode::Browser => "browser",
        }
    }
}

/// Orientation mode for the PWA.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Orientation {
    /// Any orientation
    Any,

    /// Natural orientation
    Natural,

    /// Landscape orientation
    Landscape,

    /// Portrait orientation
    Portrait,

    /// Landscape primary
    LandscapePrimary,

    /// Landscape secondary
    LandscapeSecondary,

    /// Portrait primary
    PortraitPrimary,

    /// Portrait secondary
    PortraitSecondary,
}

/// App icon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppIcon {
    /// Icon source URL
    pub src: String,

    /// Icon sizes (e.g., "192x192")
    pub sizes: String,

    /// Icon type (e.g., "image/png")
    #[serde(rename = "type")]
    pub icon_type: String,

    /// Purpose (e.g., "any", "maskable", "monochrome")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

impl AppIcon {
    /// Create a new app icon.
    pub fn new(src: impl Into<String>, sizes: impl Into<String>) -> Self {
        Self {
            src: src.into(),
            sizes: sizes.into(),
            icon_type: "image/png".to_string(),
            purpose: None,
        }
    }

    /// Set the icon type.
    pub fn with_type(mut self, icon_type: impl Into<String>) -> Self {
        self.icon_type = icon_type.into();
        self
    }

    /// Set the purpose.
    pub fn with_purpose(mut self, purpose: impl Into<String>) -> Self {
        self.purpose = Some(purpose.into());
        self
    }

    /// Create a maskable icon.
    pub fn maskable(src: impl Into<String>, sizes: impl Into<String>) -> Self {
        Self::new(src, sizes).with_purpose("maskable")
    }
}

/// Screenshot for app stores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screenshot {
    /// Screenshot source URL
    pub src: String,

    /// Screenshot sizes
    pub sizes: String,

    /// Screenshot type
    #[serde(rename = "type")]
    pub screenshot_type: String,

    /// Form factor (e.g., "wide" for desktop, "narrow" for mobile)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form_factor: Option<String>,

    /// Label for the screenshot
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Screenshot {
    /// Create a new screenshot.
    pub fn new(src: impl Into<String>, sizes: impl Into<String>) -> Self {
        Self {
            src: src.into(),
            sizes: sizes.into(),
            screenshot_type: "image/png".to_string(),
            form_factor: None,
            label: None,
        }
    }

    /// Set the form factor.
    pub fn with_form_factor(mut self, form_factor: impl Into<String>) -> Self {
        self.form_factor = Some(form_factor.into());
        self
    }

    /// Set the label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// Related application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedApplication {
    /// Platform (e.g., "play", "itunes", "windows")
    pub platform: String,

    /// App URL or ID
    pub url: Option<String>,

    /// App ID
    pub id: Option<String>,
}

impl RelatedApplication {
    /// Create a new related application.
    pub fn new(platform: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            url: None,
            id: None,
        }
    }

    /// Set the URL.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Set the ID.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Web app manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAppManifest {
    /// App name
    pub name: String,

    /// Short name (max 12 characters recommended)
    pub short_name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Start URL
    pub start_url: String,

    /// Display mode
    pub display: DisplayMode,

    /// Background color
    pub background_color: String,

    /// Theme color
    pub theme_color: String,

    /// Orientation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<Orientation>,

    /// Icons
    pub icons: Vec<AppIcon>,

    /// Screenshots
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshots: Option<Vec<Screenshot>>,

    /// Categories
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,

    /// IARC rating ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iarc_rating_id: Option<String>,

    /// Related applications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_applications: Option<Vec<RelatedApplication>>,

    /// Prefer related applications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefer_related_applications: Option<bool>,

    /// Scope
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Language
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,

    /// Text direction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

impl WebAppManifest {
    /// Create a new web app manifest.
    pub fn new(name: impl Into<String>, short_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            short_name: short_name.into(),
            description: None,
            start_url: "/".to_string(),
            display: DisplayMode::Standalone,
            background_color: "#ffffff".to_string(),
            theme_color: "#000000".to_string(),
            orientation: None,
            icons: Vec::new(),
            screenshots: None,
            categories: None,
            iarc_rating_id: None,
            related_applications: None,
            prefer_related_applications: None,
            scope: None,
            lang: None,
            dir: None,
        }
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the start URL.
    pub fn with_start_url(mut self, start_url: impl Into<String>) -> Self {
        self.start_url = start_url.into();
        self
    }

    /// Set the display mode.
    pub fn with_display(mut self, display: DisplayMode) -> Self {
        self.display = display;
        self
    }

    /// Set the background color.
    pub fn with_background_color(mut self, color: impl Into<String>) -> Self {
        self.background_color = color.into();
        self
    }

    /// Set the theme color.
    pub fn with_theme_color(mut self, color: impl Into<String>) -> Self {
        self.theme_color = color.into();
        self
    }

    /// Set the orientation.
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = Some(orientation);
        self
    }

    /// Add an icon.
    pub fn add_icon(mut self, icon: AppIcon) -> Self {
        self.icons.push(icon);
        self
    }

    /// Add multiple icons.
    pub fn with_icons(mut self, icons: Vec<AppIcon>) -> Self {
        self.icons = icons;
        self
    }

    /// Add a screenshot.
    pub fn add_screenshot(mut self, screenshot: Screenshot) -> Self {
        self.screenshots
            .get_or_insert_with(Vec::new)
            .push(screenshot);
        self
    }

    /// Set categories.
    pub fn with_categories(mut self, categories: Vec<String>) -> Self {
        self.categories = Some(categories);
        self
    }

    /// Set the scope.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Set the language.
    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = Some(lang.into());
        self
    }

    /// Add a related application.
    pub fn add_related_application(mut self, app: RelatedApplication) -> Self {
        self.related_applications
            .get_or_insert_with(Vec::new)
            .push(app);
        self
    }

    /// Generate the manifest as JSON string.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| PwaError::ManifestGenerationFailed(e.to_string()))
    }

    /// Generate the manifest as compact JSON.
    pub fn to_json_compact(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| PwaError::ManifestGenerationFailed(e.to_string()))
    }

    /// Parse manifest from JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| PwaError::ManifestGenerationFailed(e.to_string()))
    }
}

/// Manifest builder for creating PWA manifests with a fluent API.
pub struct ManifestBuilder {
    manifest: WebAppManifest,
}

impl ManifestBuilder {
    /// Create a new manifest builder.
    pub fn new(name: impl Into<String>, short_name: impl Into<String>) -> Self {
        Self {
            manifest: WebAppManifest::new(name, short_name),
        }
    }

    /// Create a geospatial PWA manifest.
    pub fn geospatial(name: impl Into<String>, short_name: impl Into<String>) -> Self {
        let mut builder = Self::new(name, short_name);
        builder.manifest.categories =
            Some(vec!["productivity".to_string(), "utilities".to_string()]);
        builder.manifest.display = DisplayMode::Standalone;
        builder
    }

    /// Set description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.manifest.description = Some(description.into());
        self
    }

    /// Set start URL.
    pub fn start_url(mut self, url: impl Into<String>) -> Self {
        self.manifest.start_url = url.into();
        self
    }

    /// Set display mode.
    pub fn display(mut self, mode: DisplayMode) -> Self {
        self.manifest.display = mode;
        self
    }

    /// Set colors.
    pub fn colors(mut self, background: impl Into<String>, theme: impl Into<String>) -> Self {
        self.manifest.background_color = background.into();
        self.manifest.theme_color = theme.into();
        self
    }

    /// Add standard icon set.
    pub fn add_standard_icons(mut self, base_path: &str) -> Self {
        let sizes = vec!["192x192", "512x512"];

        for size in sizes {
            self.manifest.icons.push(AppIcon::new(
                format!("{}/icon-{}.png", base_path, size),
                size,
            ));
        }

        // Add maskable icon
        self.manifest.icons.push(AppIcon::maskable(
            format!("{}/icon-maskable-512x512.png", base_path),
            "512x512",
        ));

        self
    }

    /// Build the manifest.
    pub fn build(self) -> WebAppManifest {
        self.manifest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_mode() {
        assert_eq!(DisplayMode::Standalone.as_str(), "standalone");
        assert_eq!(DisplayMode::Fullscreen.as_str(), "fullscreen");
    }

    #[test]
    fn test_app_icon() {
        let icon = AppIcon::new("/icon.png", "192x192")
            .with_type("image/png")
            .with_purpose("any");

        assert_eq!(icon.src, "/icon.png");
        assert_eq!(icon.sizes, "192x192");
        assert_eq!(icon.purpose, Some("any".to_string()));
    }

    #[test]
    fn test_manifest_creation() {
        let manifest = WebAppManifest::new("My App", "App")
            .with_description("A test app")
            .with_start_url("/index.html")
            .add_icon(AppIcon::new("/icon.png", "192x192"));

        assert_eq!(manifest.name, "My App");
        assert_eq!(manifest.short_name, "App");
        assert_eq!(manifest.description, Some("A test app".to_string()));
        assert_eq!(manifest.icons.len(), 1);
    }

    #[test]
    fn test_manifest_json() -> Result<()> {
        let manifest = WebAppManifest::new("Test", "T")
            .with_background_color("#ffffff")
            .with_theme_color("#000000");

        let json = manifest.to_json()?;
        assert!(json.contains("Test"));
        assert!(json.contains("#ffffff"));

        let parsed = WebAppManifest::from_json(&json)?;
        assert_eq!(parsed.name, "Test");

        Ok(())
    }

    #[test]
    fn test_manifest_builder() {
        let manifest = ManifestBuilder::geospatial("GeoApp", "Geo")
            .description("A geospatial PWA")
            .colors("#ffffff", "#007bff")
            .add_standard_icons("/icons")
            .build();

        assert_eq!(manifest.name, "GeoApp");
        assert!(manifest.categories.is_some());
        assert!(manifest.icons.len() >= 3); // 2 standard + 1 maskable
    }

    #[test]
    fn test_screenshot() {
        let screenshot = Screenshot::new("/screenshot.png", "1920x1080")
            .with_form_factor("wide")
            .with_label("Main screen");

        assert_eq!(screenshot.src, "/screenshot.png");
        assert_eq!(screenshot.form_factor, Some("wide".to_string()));
        assert_eq!(screenshot.label, Some("Main screen".to_string()));
    }
}
