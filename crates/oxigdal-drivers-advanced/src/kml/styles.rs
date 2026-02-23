//! KML style structures.

use serde::{Deserialize, Serialize};

/// KML Style.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Style {
    /// Style ID
    pub id: Option<String>,
    /// Icon style
    pub icon_style: Option<IconStyle>,
    /// Line style
    pub line_style: Option<LineStyle>,
    /// Poly style
    pub poly_style: Option<PolyStyle>,
    /// Label style
    pub label_style: Option<LabelStyle>,
}

impl Style {
    /// Create new style.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set ID.
    pub fn with_id<S: Into<String>>(mut self, id: S) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set icon style.
    pub fn with_icon_style(mut self, icon_style: IconStyle) -> Self {
        self.icon_style = Some(icon_style);
        self
    }

    /// Set line style.
    pub fn with_line_style(mut self, line_style: LineStyle) -> Self {
        self.line_style = Some(line_style);
        self
    }

    /// Set poly style.
    pub fn with_poly_style(mut self, poly_style: PolyStyle) -> Self {
        self.poly_style = Some(poly_style);
        self
    }
}

/// Icon style for points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconStyle {
    /// Color (ABGR format)
    pub color: Option<String>,
    /// Scale factor
    pub scale: f64,
    /// Icon href
    pub href: Option<String>,
}

impl IconStyle {
    /// Create new icon style.
    pub fn new() -> Self {
        Self {
            color: None,
            scale: 1.0,
            href: None,
        }
    }

    /// Set color (ABGR hex string).
    pub fn with_color<S: Into<String>>(mut self, color: S) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set scale.
    pub fn with_scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    /// Set icon URL.
    pub fn with_href<S: Into<String>>(mut self, href: S) -> Self {
        self.href = Some(href.into());
        self
    }
}

impl Default for IconStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Line style for LineStrings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineStyle {
    /// Color (ABGR format)
    pub color: Option<String>,
    /// Width in pixels
    pub width: f64,
}

impl LineStyle {
    /// Create new line style.
    pub fn new() -> Self {
        Self {
            color: None,
            width: 1.0,
        }
    }

    /// Set color.
    pub fn with_color<S: Into<String>>(mut self, color: S) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set width.
    pub fn with_width(mut self, width: f64) -> Self {
        self.width = width;
        self
    }
}

impl Default for LineStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Poly style for Polygons.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyStyle {
    /// Color (ABGR format)
    pub color: Option<String>,
    /// Fill flag
    pub fill: bool,
    /// Outline flag
    pub outline: bool,
}

impl PolyStyle {
    /// Create new poly style.
    pub fn new() -> Self {
        Self {
            color: None,
            fill: true,
            outline: true,
        }
    }

    /// Set color.
    pub fn with_color<S: Into<String>>(mut self, color: S) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set fill.
    pub fn with_fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self
    }

    /// Set outline.
    pub fn with_outline(mut self, outline: bool) -> Self {
        self.outline = outline;
        self
    }
}

impl Default for PolyStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Label style.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelStyle {
    /// Color (ABGR format)
    pub color: Option<String>,
    /// Scale factor
    pub scale: f64,
}

impl Default for LabelStyle {
    fn default() -> Self {
        Self {
            color: None,
            scale: 1.0,
        }
    }
}

/// Style map for hover effects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleMap {
    /// Style map ID
    pub id: Option<String>,
    /// Normal style URL
    pub normal: String,
    /// Highlight style URL
    pub highlight: String,
}

impl StyleMap {
    /// Create new style map.
    pub fn new<S: Into<String>>(normal: S, highlight: S) -> Self {
        Self {
            id: None,
            normal: normal.into(),
            highlight: highlight.into(),
        }
    }

    /// Set ID.
    pub fn with_id<S: Into<String>>(mut self, id: S) -> Self {
        self.id = Some(id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_style_creation() {
        let style = Style::new().with_id("test-style");
        assert_eq!(style.id, Some("test-style".to_string()));
    }

    #[test]
    fn test_icon_style() {
        let icon = IconStyle::new().with_color("ff0000ff").with_scale(1.5);

        assert_eq!(icon.color, Some("ff0000ff".to_string()));
        assert_eq!(icon.scale, 1.5);
    }

    #[test]
    fn test_line_style() {
        let line = LineStyle::new().with_color("ff00ff00").with_width(2.0);

        assert_eq!(line.color, Some("ff00ff00".to_string()));
        assert_eq!(line.width, 2.0);
    }

    #[test]
    fn test_poly_style() {
        let poly = PolyStyle::new()
            .with_color("7fff0000")
            .with_fill(true)
            .with_outline(false);

        assert!(poly.fill);
        assert!(!poly.outline);
    }

    #[test]
    fn test_style_map() {
        let style_map = StyleMap::new("#normalStyle", "#highlightStyle").with_id("test-map");

        assert_eq!(style_map.id, Some("test-map".to_string()));
        assert_eq!(style_map.normal, "#normalStyle");
        assert_eq!(style_map.highlight, "#highlightStyle");
    }
}
