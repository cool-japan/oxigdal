//! Interactive widgets example notebook
//!
//! This example demonstrates interactive widgets for geospatial data.

use oxigdal_jupyter::{
    widgets::{BasemapProvider, DropdownWidget, MapWidget, SliderWidget, Widget},
    Result,
};

fn main() -> Result<()> {
    println!("# Interactive Widgets Examples");
    println!();

    // Example 1: Map Widget
    println!("## Example 1: Map Widget");
    let mut map = MapWidget::new("map1", (0.0, 0.0), 5)
        .with_dimensions(800, 600)
        .with_basemap(BasemapProvider::OpenStreetMap);

    println!("Map center: {:?}", map.center());
    println!("Map zoom: {}", map.zoom());

    let html = map.render()?;
    println!("Map HTML length: {} chars", html.len());
    println!();

    // Example 2: Slider Widget
    println!("## Example 2: Slider Widget");
    let mut slider = SliderWidget::new("opacity", 0.0, 1.0)
        .with_label("Layer Opacity")
        .with_step(0.1);

    println!("Initial value: {}", slider.value());
    slider.set_value(0.5)?;
    println!("Updated value: {}", slider.value());

    let html = slider.render()?;
    println!("Slider HTML length: {} chars", html.len());
    println!();

    // Example 3: Dropdown Widget
    println!("## Example 3: Dropdown Widget");
    let mut dropdown = DropdownWidget::new(
        "basemap",
        vec![
            "OpenStreetMap".to_string(),
            "Satellite".to_string(),
            "Terrain".to_string(),
        ],
    )?
    .with_label("Basemap");

    println!("Selected: {}", dropdown.selected_value());
    dropdown.set_selected_index(1)?;
    println!("After change: {}", dropdown.selected_value());

    let html = dropdown.render()?;
    println!("Dropdown HTML length: {} chars", html.len());
    println!();

    // Example 4: Widget State
    println!("## Example 4: Widget State Management");
    let state = map.state()?;
    println!("Map state: {} keys", state.len());
    for (key, value) in &state {
        println!("  {}: {:?}", key, value);
    }
    println!();

    Ok(())
}
