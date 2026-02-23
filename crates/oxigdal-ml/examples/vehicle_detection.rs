//! Vehicle detection in aerial imagery using YOLO

use std::collections::HashMap;

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use oxigdal_ml::detection::{BoundingBox, Detection, NmsConfig, non_maximum_suppression};
use oxigdal_ml::error::Result;
use oxigdal_ml::zoo::ModelZoo;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("Vehicle Detection Example");
    println!("========================\n");

    // Get YOLO model
    let mut zoo = ModelZoo::new()?;
    let model_path = zoo.get_model("yolov5_vehicles")?;

    println!("Model loaded: {:?}", model_path);

    // Load aerial image
    let _image = RasterBuffer::zeros(640, 640, RasterDataType::Float32);

    // Run detection (simulated)
    let detections = vec![
        Detection {
            bbox: BoundingBox::new(100.0, 100.0, 50.0, 50.0),
            class_id: 0, // Car
            class_label: Some("car".to_string()),
            confidence: 0.95,
            attributes: HashMap::new(),
        },
        Detection {
            bbox: BoundingBox::new(200.0, 150.0, 40.0, 30.0),
            class_id: 1, // Truck
            class_label: Some("truck".to_string()),
            confidence: 0.88,
            attributes: HashMap::new(),
        },
    ];

    println!("Detected {} vehicles before NMS", detections.len());

    // Apply non-maximum suppression
    let nms_config = NmsConfig {
        iou_threshold: 0.45,
        confidence_threshold: 0.5,
        max_detections: Some(100),
    };
    let filtered = non_maximum_suppression(&detections, &nms_config)?;

    println!("Detected {} vehicles after NMS\n", filtered.len());

    for (i, det) in filtered.iter().enumerate() {
        println!(
            "Vehicle {}: {} (confidence: {:.2})",
            i + 1,
            get_vehicle_type(det.class_id),
            det.confidence
        );
    }

    Ok(())
}

fn get_vehicle_type(class_id: usize) -> &'static str {
    match class_id {
        0 => "Car",
        1 => "Truck",
        2 => "Bus",
        _ => "Unknown",
    }
}
