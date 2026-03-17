//! Tests for the Mapbox GL Style Specification v8 implementation.

use std::collections::HashMap;

use oxigdal_services::style::{
    Color, DemEncoding, Expression, Filter, GeomFilter, Interpolation, Layer, LayerType, Layout,
    LightAnchor, LineCap, LineJoin, Paint, PropertyValue, Source, StyleRenderer, StyleSpec,
    StyleValidator, SymbolPlacement, Transition, Visibility,
};

// ─────────────────────────────────────────────────────────────────────────────
// Color::parse tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_color_parse_hex6() {
    let c = Color::parse("#ff8800").expect("parse hex6 color");
    assert_eq!(c.r, 0xff);
    assert_eq!(c.g, 0x88);
    assert_eq!(c.b, 0x00);
    assert!((c.a - 1.0).abs() < 1e-6);
}

#[test]
fn test_color_parse_hex6_uppercase() {
    let c = Color::parse("#AABBCC").expect("parse uppercase hex6 color");
    assert_eq!(c.r, 0xaa);
    assert_eq!(c.g, 0xbb);
    assert_eq!(c.b, 0xcc);
}

#[test]
fn test_color_parse_hex3() {
    let c = Color::parse("#f80").expect("parse hex3 color");
    assert_eq!(c.r, 0xff);
    assert_eq!(c.g, 0x88);
    assert_eq!(c.b, 0x00);
}

#[test]
fn test_color_parse_rgb() {
    let c = Color::parse("rgb(10,20,30)").expect("parse rgb color");
    assert_eq!(c.r, 10);
    assert_eq!(c.g, 20);
    assert_eq!(c.b, 30);
    assert!((c.a - 1.0).abs() < 1e-6);
}

#[test]
fn test_color_parse_rgb_with_spaces() {
    let c = Color::parse("rgb(10, 20, 30)").expect("parse rgb with spaces");
    assert_eq!(c.r, 10);
    assert_eq!(c.g, 20);
    assert_eq!(c.b, 30);
}

#[test]
fn test_color_parse_rgba() {
    let c = Color::parse("rgba(255,128,0,0.5)").expect("parse rgba color");
    assert_eq!(c.r, 255);
    assert_eq!(c.g, 128);
    assert_eq!(c.b, 0);
    assert!((c.a - 0.5).abs() < 1e-4);
}

#[test]
fn test_color_parse_rgba_fully_transparent() {
    let c = Color::parse("rgba(0,0,0,0)").expect("parse fully transparent rgba");
    assert!((c.a).abs() < 1e-6);
}

#[test]
fn test_color_parse_rgba_fully_opaque() {
    let c = Color::parse("rgba(255,255,255,1)").expect("parse fully opaque rgba");
    assert!((c.a - 1.0).abs() < 1e-6);
}

#[test]
fn test_color_parse_invalid_format() {
    assert!(Color::parse("hsl(120,100%,50%)").is_err());
    assert!(Color::parse("blue").is_err());
}

#[test]
fn test_color_parse_invalid_hex() {
    assert!(Color::parse("#gg0011").is_err());
    assert!(Color::parse("#12345").is_err());
}

#[test]
fn test_color_parse_rgb_wrong_component_count() {
    assert!(Color::parse("rgb(1,2)").is_err());
    assert!(Color::parse("rgb(1,2,3,4)").is_err());
}

#[test]
fn test_color_parse_rgba_wrong_component_count() {
    assert!(Color::parse("rgba(1,2,3)").is_err());
}

// ─────────────────────────────────────────────────────────────────────────────
// Color::to_css round-trip tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_color_to_css_roundtrip_hex() {
    let original = Color::parse("#1a2b3c").expect("parse hex color for roundtrip");
    let css = original.to_css();
    // to_css produces rgba(...); parse that back
    let parsed = Color::parse(&css).expect("parse CSS roundtrip color");
    assert_eq!(parsed.r, original.r);
    assert_eq!(parsed.g, original.g);
    assert_eq!(parsed.b, original.b);
    assert!((parsed.a - original.a).abs() < 1e-4);
}

#[test]
fn test_color_to_css_roundtrip_rgba() {
    let original = Color::parse("rgba(10,20,30,0.75)").expect("parse rgba for roundtrip");
    let css = original.to_css();
    let parsed = Color::parse(&css).expect("parse CSS roundtrip color");
    assert_eq!(parsed.r, original.r);
    assert_eq!(parsed.g, original.g);
    assert_eq!(parsed.b, original.b);
    assert!((parsed.a - original.a).abs() < 1e-4);
}

#[test]
fn test_color_to_css_format() {
    let c = Color {
        r: 1,
        g: 2,
        b: 3,
        a: 0.5,
    };
    assert!(c.to_css().starts_with("rgba(1,2,3,"));
}

// ─────────────────────────────────────────────────────────────────────────────
// StyleValidator tests
// ─────────────────────────────────────────────────────────────────────────────

fn minimal_style(version: u8) -> StyleSpec {
    StyleSpec {
        version,
        name: None,
        metadata: None,
        center: None,
        zoom: None,
        bearing: None,
        pitch: None,
        light: None,
        sources: HashMap::new(),
        sprite: None,
        glyphs: None,
        transition: None,
        layers: Vec::new(),
    }
}

fn make_layer(id: &str, layer_type: LayerType, source: Option<&str>) -> Layer {
    Layer {
        id: id.to_string(),
        layer_type,
        source: source.map(|s| s.to_string()),
        source_layer: None,
        min_zoom: None,
        max_zoom: None,
        filter: None,
        layout: None,
        paint: None,
    }
}

#[test]
fn test_validator_valid_style() {
    let mut style = minimal_style(8);
    style.sources.insert(
        "tiles".to_string(),
        Source::Vector {
            url: Some("https://example.com/tiles".to_string()),
            tiles: None,
            min_zoom: None,
            max_zoom: None,
            attribution: None,
        },
    );
    style
        .layers
        .push(make_layer("roads", LayerType::Line, Some("tiles")));
    let errors = StyleValidator::validate(&style);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn test_validator_invalid_version() {
    let style = minimal_style(7);
    let errors = StyleValidator::validate(&style);
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.message.contains("version")));
}

#[test]
fn test_validator_unknown_source_ref() {
    let mut style = minimal_style(8);
    style
        .layers
        .push(make_layer("roads", LayerType::Line, Some("nonexistent")));
    let errors = StyleValidator::validate(&style);
    assert!(errors.iter().any(|e| e.message.contains("nonexistent")));
}

#[test]
fn test_validator_zoom_range_violation() {
    let mut style = minimal_style(8);
    style.sources.insert(
        "src".to_string(),
        Source::Vector {
            url: Some("https://example.com".to_string()),
            tiles: None,
            min_zoom: None,
            max_zoom: None,
            attribution: None,
        },
    );
    let mut layer = make_layer("l", LayerType::Fill, Some("src"));
    layer.min_zoom = Some(12.0);
    layer.max_zoom = Some(8.0);
    style.layers.push(layer);
    let errors = StyleValidator::validate(&style);
    assert!(errors.iter().any(|e| e.message.contains("minzoom")));
}

#[test]
fn test_validator_duplicate_layer_ids() {
    let mut style = minimal_style(8);
    style.sources.insert(
        "src".to_string(),
        Source::Vector {
            url: Some("https://example.com".to_string()),
            tiles: None,
            min_zoom: None,
            max_zoom: None,
            attribution: None,
        },
    );
    style
        .layers
        .push(make_layer("dupe", LayerType::Line, Some("src")));
    style
        .layers
        .push(make_layer("dupe", LayerType::Fill, Some("src")));
    let errors = StyleValidator::validate(&style);
    assert!(errors.iter().any(|e| e.message.contains("duplicate")));
}

#[test]
fn test_validator_background_with_source() {
    let mut style = minimal_style(8);
    style.sources.insert(
        "src".to_string(),
        Source::Vector {
            url: Some("https://example.com".to_string()),
            tiles: None,
            min_zoom: None,
            max_zoom: None,
            attribution: None,
        },
    );
    style
        .layers
        .push(make_layer("bg", LayerType::Background, Some("src")));
    let errors = StyleValidator::validate(&style);
    assert!(errors.iter().any(|e| e.message.contains("background")));
}

#[test]
fn test_validator_fill_without_source() {
    let mut style = minimal_style(8);
    style.layers.push(make_layer("fill", LayerType::Fill, None));
    let errors = StyleValidator::validate(&style);
    assert!(errors.iter().any(|e| e.message.contains("source")));
}

#[test]
fn test_validator_line_without_source() {
    let mut style = minimal_style(8);
    style.layers.push(make_layer("line", LayerType::Line, None));
    let errors = StyleValidator::validate(&style);
    assert!(errors.iter().any(|e| e.message.contains("source")));
}

#[test]
fn test_validator_symbol_without_source() {
    let mut style = minimal_style(8);
    style
        .layers
        .push(make_layer("sym", LayerType::Symbol, None));
    let errors = StyleValidator::validate(&style);
    assert!(errors.iter().any(|e| e.message.contains("source")));
}

#[test]
fn test_validator_circle_without_source() {
    let mut style = minimal_style(8);
    style
        .layers
        .push(make_layer("cir", LayerType::Circle, None));
    let errors = StyleValidator::validate(&style);
    assert!(errors.iter().any(|e| e.message.contains("source")));
}

#[test]
fn test_validator_background_no_source_is_valid() {
    let mut style = minimal_style(8);
    style
        .layers
        .push(make_layer("bg", LayerType::Background, None));
    let errors = StyleValidator::validate(&style);
    assert!(
        errors.is_empty(),
        "background without source should be valid"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Filter evaluation tests
// ─────────────────────────────────────────────────────────────────────────────

fn props(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[test]
fn test_filter_eq_match() {
    let filter = Filter::Eq {
        property: "class".to_string(),
        value: serde_json::json!("motorway"),
    };
    let p = props(&[("class", serde_json::json!("motorway"))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_eq_no_match() {
    let filter = Filter::Eq {
        property: "class".to_string(),
        value: serde_json::json!("motorway"),
    };
    let p = props(&[("class", serde_json::json!("primary"))]);
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_ne_match() {
    let filter = Filter::Ne {
        property: "class".to_string(),
        value: serde_json::json!("motorway"),
    };
    let p = props(&[("class", serde_json::json!("primary"))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_ne_no_match() {
    let filter = Filter::Ne {
        property: "class".to_string(),
        value: serde_json::json!("motorway"),
    };
    let p = props(&[("class", serde_json::json!("motorway"))]);
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_lt() {
    let filter = Filter::Lt {
        property: "speed".to_string(),
        value: 50.0,
    };
    let p_yes = props(&[("speed", serde_json::json!(30))]);
    let p_no = props(&[("speed", serde_json::json!(80))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p_yes));
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p_no));
}

#[test]
fn test_filter_lte() {
    let filter = Filter::Lte {
        property: "speed".to_string(),
        value: 50.0,
    };
    let p_yes = props(&[("speed", serde_json::json!(50))]);
    let p_no = props(&[("speed", serde_json::json!(51))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p_yes));
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p_no));
}

#[test]
fn test_filter_gt() {
    let filter = Filter::Gt {
        property: "pop".to_string(),
        value: 1000.0,
    };
    let p_yes = props(&[("pop", serde_json::json!(5000))]);
    let p_no = props(&[("pop", serde_json::json!(100))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p_yes));
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p_no));
}

#[test]
fn test_filter_gte() {
    let filter = Filter::Gte {
        property: "pop".to_string(),
        value: 1000.0,
    };
    let p_yes = props(&[("pop", serde_json::json!(1000))]);
    let p_no = props(&[("pop", serde_json::json!(999))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p_yes));
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p_no));
}

#[test]
fn test_filter_in_match() {
    let filter = Filter::In {
        property: "class".to_string(),
        values: vec![serde_json::json!("motorway"), serde_json::json!("trunk")],
    };
    let p = props(&[("class", serde_json::json!("trunk"))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_in_no_match() {
    let filter = Filter::In {
        property: "class".to_string(),
        values: vec![serde_json::json!("motorway"), serde_json::json!("trunk")],
    };
    let p = props(&[("class", serde_json::json!("primary"))]);
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_has_present() {
    let filter = Filter::Has("name".to_string());
    let p = props(&[("name", serde_json::json!("Main St"))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_has_absent() {
    let filter = Filter::Has("name".to_string());
    let p = props(&[("class", serde_json::json!("road"))]);
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_not_has() {
    let filter = Filter::NotHas("name".to_string());
    let p = props(&[("class", serde_json::json!("road"))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_all_passes() {
    let filter = Filter::All(vec![
        Filter::Has("name".to_string()),
        Filter::Gt {
            property: "pop".to_string(),
            value: 100.0,
        },
    ]);
    let p = props(&[
        ("name", serde_json::json!("City")),
        ("pop", serde_json::json!(500)),
    ]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_all_fails_on_one() {
    let filter = Filter::All(vec![
        Filter::Has("name".to_string()),
        Filter::Gt {
            property: "pop".to_string(),
            value: 1000.0,
        },
    ]);
    let p = props(&[
        ("name", serde_json::json!("City")),
        ("pop", serde_json::json!(100)),
    ]);
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_any_at_least_one() {
    let filter = Filter::Any(vec![
        Filter::Eq {
            property: "class".to_string(),
            value: serde_json::json!("motorway"),
        },
        Filter::Eq {
            property: "class".to_string(),
            value: serde_json::json!("primary"),
        },
    ]);
    let p = props(&[("class", serde_json::json!("primary"))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_any_none_match() {
    let filter = Filter::Any(vec![
        Filter::Eq {
            property: "class".to_string(),
            value: serde_json::json!("motorway"),
        },
        Filter::Eq {
            property: "class".to_string(),
            value: serde_json::json!("primary"),
        },
    ]);
    let p = props(&[("class", serde_json::json!("tertiary"))]);
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_none_passes_when_none_match() {
    let filter = Filter::None(vec![Filter::Eq {
        property: "class".to_string(),
        value: serde_json::json!("motorway"),
    }]);
    let p = props(&[("class", serde_json::json!("primary"))]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_none_fails_when_one_matches() {
    let filter = Filter::None(vec![Filter::Eq {
        property: "class".to_string(),
        value: serde_json::json!("motorway"),
    }]);
    let p = props(&[("class", serde_json::json!("motorway"))]);
    assert!(!StyleRenderer::feature_matches_filter(&filter, &p));
}

#[test]
fn test_filter_geometry_type_passes_by_default() {
    let filter = Filter::GeometryType(GeomFilter::Point);
    let p = props(&[]);
    assert!(StyleRenderer::feature_matches_filter(&filter, &p));
}

// ─────────────────────────────────────────────────────────────────────────────
// Expression / interpolation evaluation tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_eval_zoom_f64_literal() {
    let v = PropertyValue::Literal(5.0_f64);
    assert!((StyleRenderer::eval_zoom_f64(&v, 10.0) - 5.0).abs() < 1e-10);
}

#[test]
fn test_eval_zoom_f64_zoom_expression() {
    let v = PropertyValue::Expression(Expression::Zoom);
    assert!((StyleRenderer::eval_zoom_f64(&v, 12.0) - 12.0).abs() < 1e-10);
}

#[test]
fn test_eval_zoom_f64_linear_interpolation() {
    let v = PropertyValue::Expression(Expression::Interpolate {
        interpolation: Interpolation::Linear,
        input: Box::new(Expression::Zoom),
        stops: vec![
            (0.0, Expression::Literal(serde_json::json!(1.0))),
            (10.0, Expression::Literal(serde_json::json!(11.0))),
        ],
    });
    // At zoom 5 (halfway) -> should be 6
    let result = StyleRenderer::eval_zoom_f64(&v, 5.0);
    assert!((result - 6.0).abs() < 1e-6, "expected 6.0 got {result}");
}

#[test]
fn test_eval_zoom_f64_linear_clamp_below() {
    let v = PropertyValue::Expression(Expression::Interpolate {
        interpolation: Interpolation::Linear,
        input: Box::new(Expression::Zoom),
        stops: vec![
            (5.0, Expression::Literal(serde_json::json!(10.0))),
            (10.0, Expression::Literal(serde_json::json!(20.0))),
        ],
    });
    // Below first stop -> clamp to first stop value
    let result = StyleRenderer::eval_zoom_f64(&v, 0.0);
    assert!((result - 10.0).abs() < 1e-6, "expected 10.0 got {result}");
}

#[test]
fn test_eval_zoom_f64_linear_clamp_above() {
    let v = PropertyValue::Expression(Expression::Interpolate {
        interpolation: Interpolation::Linear,
        input: Box::new(Expression::Zoom),
        stops: vec![
            (5.0, Expression::Literal(serde_json::json!(10.0))),
            (10.0, Expression::Literal(serde_json::json!(20.0))),
        ],
    });
    // Above last stop -> clamp to last stop value
    let result = StyleRenderer::eval_zoom_f64(&v, 15.0);
    assert!((result - 20.0).abs() < 1e-6, "expected 20.0 got {result}");
}

#[test]
fn test_eval_zoom_f64_exponential_interpolation() {
    let v = PropertyValue::Expression(Expression::Interpolate {
        interpolation: Interpolation::Exponential(2.0),
        input: Box::new(Expression::Zoom),
        stops: vec![
            (0.0, Expression::Literal(serde_json::json!(0.0))),
            (4.0, Expression::Literal(serde_json::json!(256.0))),
        ],
    });
    // At zoom 2 with base 2: t = (2^2 - 1)/(2^4 - 1) = 3/15 = 0.2
    // result = 0 + 0.2 * 256 = 51.2
    let result = StyleRenderer::eval_zoom_f64(&v, 2.0);
    assert!((result - 51.2).abs() < 1e-4, "expected ~51.2 got {result}");
}

#[test]
fn test_eval_zoom_f64_step_expression() {
    let v = PropertyValue::Expression(Expression::Step {
        input: Box::new(Expression::Zoom),
        default: Box::new(Expression::Literal(serde_json::json!(1.0))),
        stops: vec![
            (5.0, Expression::Literal(serde_json::json!(2.0))),
            (10.0, Expression::Literal(serde_json::json!(3.0))),
        ],
    });
    assert!((StyleRenderer::eval_zoom_f64(&v, 3.0) - 1.0).abs() < 1e-10);
    assert!((StyleRenderer::eval_zoom_f64(&v, 7.0) - 2.0).abs() < 1e-10);
    assert!((StyleRenderer::eval_zoom_f64(&v, 12.0) - 3.0).abs() < 1e-10);
}

#[test]
fn test_eval_zoom_color_literal() {
    let c = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 1.0,
    };
    let v: PropertyValue<Color> = PropertyValue::Literal(c.clone());
    let result = StyleRenderer::eval_zoom_color(&v, 8.0);
    assert_eq!(result.r, c.r);
    assert_eq!(result.g, c.g);
    assert_eq!(result.b, c.b);
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON round-trip tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_style_spec_json_roundtrip_minimal() {
    let style = minimal_style(8);
    let json1 = serde_json::to_string(&style).expect("serialize style");
    let parsed: StyleSpec = serde_json::from_str(&json1).expect("deserialize style");
    let json2 = serde_json::to_string(&parsed).expect("re-serialize style");
    assert_eq!(json1, json2);
}

#[test]
fn test_style_spec_json_roundtrip_with_layers() {
    let mut style = minimal_style(8);
    style.sources.insert(
        "roads".to_string(),
        Source::Vector {
            url: Some("https://example.com/tiles.json".to_string()),
            tiles: None,
            min_zoom: None,
            max_zoom: Some(14),
            attribution: None,
        },
    );
    style.transition = Some(Transition {
        duration: 300,
        delay: 0,
    });
    style
        .layers
        .push(make_layer("road-line", LayerType::Line, Some("roads")));
    style
        .layers
        .push(make_layer("bg", LayerType::Background, None));

    let json1 = serde_json::to_string(&style).expect("serialize style");
    let parsed: StyleSpec = serde_json::from_str(&json1).expect("deserialize style");
    let json2 = serde_json::to_string(&parsed).expect("re-serialize style");
    assert_eq!(json1, json2);
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer type parsing tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_layer_type_serde_fill() {
    let json = r#"{"id":"l","type":"fill","source":"s"}"#;
    let layer: Layer = serde_json::from_str(json).expect("deserialize layer");
    assert_eq!(layer.layer_type, LayerType::Fill);
}

#[test]
fn test_layer_type_serde_line() {
    let json = r#"{"id":"l","type":"line","source":"s"}"#;
    let layer: Layer = serde_json::from_str(json).expect("deserialize layer");
    assert_eq!(layer.layer_type, LayerType::Line);
}

#[test]
fn test_layer_type_serde_background() {
    let json = r#"{"id":"bg","type":"background"}"#;
    let layer: Layer = serde_json::from_str(json).expect("deserialize layer");
    assert_eq!(layer.layer_type, LayerType::Background);
}

#[test]
fn test_layer_type_serde_symbol() {
    let json = r#"{"id":"sym","type":"symbol","source":"s"}"#;
    let layer: Layer = serde_json::from_str(json).expect("deserialize layer");
    assert_eq!(layer.layer_type, LayerType::Symbol);
}

#[test]
fn test_layer_type_serde_raster() {
    let json = r#"{"id":"r","type":"raster","source":"s"}"#;
    let layer: Layer = serde_json::from_str(json).expect("deserialize layer");
    assert_eq!(layer.layer_type, LayerType::Raster);
}

#[test]
fn test_layer_type_serde_hillshade() {
    let json = r#"{"id":"h","type":"hillshade","source":"s"}"#;
    let layer: Layer = serde_json::from_str(json).expect("deserialize layer");
    assert_eq!(layer.layer_type, LayerType::Hillshade);
}

// ─────────────────────────────────────────────────────────────────────────────
// Source type parsing tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_source_vector_serde() {
    let json = r#"{"type":"vector","url":"https://example.com/tiles","maxzoom":14}"#;
    let src: Source = serde_json::from_str(json).expect("deserialize layer");
    assert!(matches!(src, Source::Vector { .. }));
    let back = serde_json::to_string(&src).expect("re-serialize source");
    let src2: Source = serde_json::from_str(&back).expect("deserialize re-serialized source");
    assert!(matches!(src2, Source::Vector { .. }));
}

#[test]
fn test_source_raster_serde() {
    let json = r#"{"type":"raster","url":"https://example.com","tileSize":256}"#;
    let src: Source = serde_json::from_str(json).expect("deserialize layer");
    assert!(matches!(src, Source::Raster { .. }));
}

#[test]
fn test_source_geojson_serde() {
    let json = r#"{"type":"geojson","data":{"type":"FeatureCollection","features":[]}}"#;
    let src: Source = serde_json::from_str(json).expect("deserialize layer");
    assert!(matches!(src, Source::GeoJson { .. }));
}

#[test]
fn test_source_raster_dem_default_encoding() {
    let json = r#"{"type":"raster-dem","url":"https://example.com/dem"}"#;
    let src: Source = serde_json::from_str(json).expect("deserialize layer");
    if let Source::RasterDem { encoding, .. } = src {
        assert_eq!(encoding, DemEncoding::Mapbox);
    } else {
        unreachable!("expected RasterDem");
    }
}

#[test]
fn test_source_raster_dem_terrarium_encoding() {
    let json = r#"{"type":"raster-dem","url":"https://example.com/dem","encoding":"terrarium"}"#;
    let src: Source = serde_json::from_str(json).expect("deserialize layer");
    if let Source::RasterDem { encoding, .. } = src {
        assert_eq!(encoding, DemEncoding::Terrarium);
    } else {
        unreachable!("expected RasterDem");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layout enum parsing tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_visibility_default() {
    let layout = Layout(HashMap::new());
    assert_eq!(layout.visibility(), Visibility::Visible);
}

#[test]
fn test_visibility_none() {
    let mut map = HashMap::new();
    map.insert("visibility".to_string(), serde_json::json!("none"));
    let layout = Layout(map);
    assert_eq!(layout.visibility(), Visibility::None);
}

#[test]
fn test_line_cap_default() {
    let layout = Layout(HashMap::new());
    assert_eq!(layout.line_cap(), LineCap::Butt);
}

#[test]
fn test_line_cap_round() {
    let mut map = HashMap::new();
    map.insert("line-cap".to_string(), serde_json::json!("round"));
    let layout = Layout(map);
    assert_eq!(layout.line_cap(), LineCap::Round);
}

#[test]
fn test_line_cap_square() {
    let mut map = HashMap::new();
    map.insert("line-cap".to_string(), serde_json::json!("square"));
    let layout = Layout(map);
    assert_eq!(layout.line_cap(), LineCap::Square);
}

#[test]
fn test_line_join_default() {
    let layout = Layout(HashMap::new());
    assert_eq!(layout.line_join(), LineJoin::Bevel);
}

#[test]
fn test_line_join_miter() {
    let mut map = HashMap::new();
    map.insert("line-join".to_string(), serde_json::json!("miter"));
    let layout = Layout(map);
    assert_eq!(layout.line_join(), LineJoin::Miter);
}

#[test]
fn test_symbol_placement_default() {
    let layout = Layout(HashMap::new());
    assert_eq!(layout.symbol_placement(), SymbolPlacement::Point);
}

#[test]
fn test_symbol_placement_line() {
    let mut map = HashMap::new();
    map.insert("symbol-placement".to_string(), serde_json::json!("line"));
    let layout = Layout(map);
    assert_eq!(layout.symbol_placement(), SymbolPlacement::Line);
}

#[test]
fn test_symbol_placement_line_center() {
    let mut map = HashMap::new();
    map.insert(
        "symbol-placement".to_string(),
        serde_json::json!("line-center"),
    );
    let layout = Layout(map);
    assert_eq!(layout.symbol_placement(), SymbolPlacement::LineCenter);
}

// ─────────────────────────────────────────────────────────────────────────────
// Paint accessor tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paint_fill_opacity_literal() {
    let mut map = HashMap::new();
    map.insert("fill-opacity".to_string(), serde_json::json!(0.8));
    let paint = Paint(map);
    let val = paint.fill_opacity().expect("should have fill-opacity");
    if let PropertyValue::Literal(v) = val {
        assert!((v - 0.8).abs() < 1e-10);
    } else {
        unreachable!("expected Literal");
    }
}

#[test]
fn test_paint_background_color_absent() {
    let paint = Paint(HashMap::new());
    assert!(paint.background_color().is_none());
}

#[test]
fn test_paint_line_width_literal() {
    let mut map = HashMap::new();
    map.insert("line-width".to_string(), serde_json::json!(2.5));
    let paint = Paint(map);
    if let Some(PropertyValue::Literal(v)) = paint.line_width() {
        assert!((v - 2.5).abs() < 1e-10);
    } else {
        unreachable!("expected Some(Literal(2.5))");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Light anchor serde test
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_light_anchor_default_viewport() {
    assert_eq!(LightAnchor::default(), LightAnchor::Viewport);
}

#[test]
fn test_light_anchor_serde_map() {
    let json = r#""map""#;
    let anchor: LightAnchor = serde_json::from_str(json).expect("deserialize layer");
    assert_eq!(anchor, LightAnchor::Map);
}
