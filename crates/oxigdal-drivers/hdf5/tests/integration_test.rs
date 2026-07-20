//! Integration tests for the HDF5 driver.
//!
//! These tests verify REAL round-trip functionality: writing genuine `.h5`
//! files through the `oxih5`-backed [`Hdf5Writer`] and reading them back with
//! the `oxih5`-backed [`Hdf5Reader`], asserting real values, shapes, datatypes,
//! and attributes.

use oxigdal_hdf5::attribute::Attribute;
use oxigdal_hdf5::dataset::DatasetProperties;
use oxigdal_hdf5::datatype::Datatype;
use oxigdal_hdf5::{Hdf5Reader, Hdf5Version, Hdf5Writer};
use std::env;
use tempfile::NamedTempFile;

/// Round-trip an i32 dataset and assert the real decoded values.
#[test]
fn test_round_trip_i32_values() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");
        writer
            .create_dataset("/data", Datatype::Int32, vec![10], DatasetProperties::new())
            .expect("Failed to create dataset");
        let data: Vec<i32> = (0..10).collect();
        writer
            .write_i32("/data", &data)
            .expect("Failed to write data");
        writer.finalize().expect("Failed to finalize");
    }

    {
        let mut reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");
        assert!(reader.exists("/data"));
        assert!(reader.is_dataset("/data"));

        {
            let dataset = reader.dataset("/data").expect("Failed to get dataset");
            assert_eq!(dataset.name(), "data");
            assert_eq!(dataset.dims(), &[10]);
            assert_eq!(dataset.datatype(), &Datatype::Int32);
        }

        let data = reader.read_i32("/data").expect("Failed to read data");
        assert_eq!(data, (0..10).collect::<Vec<i32>>());
    }
}

/// Round-trip f32 and f64 datasets and assert the real decoded values.
#[test]
fn test_round_trip_float_values() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");

        writer
            .create_dataset(
                "/f32",
                Datatype::Float32,
                vec![2, 2],
                DatasetProperties::new(),
            )
            .expect("create f32 dataset");
        writer
            .write_f32("/f32", &[1.5, 2.5, 3.5, 4.5])
            .expect("write f32");

        writer
            .create_dataset("/f64", Datatype::Float64, vec![3], DatasetProperties::new())
            .expect("create f64 dataset");
        writer
            .write_f64("/f64", &[10.0, 20.0, 30.0])
            .expect("write f64");

        writer.finalize().expect("Failed to finalize");
    }

    {
        let mut reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");

        {
            let ds = reader.dataset("/f32").expect("f32 dataset");
            assert_eq!(ds.dims(), &[2, 2]);
            assert_eq!(ds.ndims(), 2);
        }
        assert_eq!(
            reader.read_f32("/f32").expect("read f32"),
            vec![1.5, 2.5, 3.5, 4.5]
        );

        {
            let ds = reader.dataset("/f64").expect("f64 dataset");
            assert_eq!(ds.dims(), &[3]);
        }
        assert_eq!(
            reader.read_f64("/f64").expect("read f64"),
            vec![10.0, 20.0, 30.0]
        );
    }
}

/// Zero-filled datasets of several numeric datatypes round-trip their datatype
/// and shape through a real file.
#[test]
fn test_multiple_numeric_datatypes() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");
        for (name, dt) in [
            ("/uint8", Datatype::UInt8),
            ("/int32", Datatype::Int32),
            ("/int64", Datatype::Int64),
            ("/float32", Datatype::Float32),
            ("/float64", Datatype::Float64),
        ] {
            writer
                .create_dataset(name, dt, vec![4], DatasetProperties::new())
                .expect("create dataset");
        }
        writer.finalize().expect("Failed to finalize");
    }

    {
        let reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");
        assert_eq!(
            reader.dataset("/uint8").expect("dataset").datatype(),
            &Datatype::UInt8
        );
        assert_eq!(
            reader.dataset("/int32").expect("dataset").datatype(),
            &Datatype::Int32
        );
        assert_eq!(
            reader.dataset("/int64").expect("dataset").datatype(),
            &Datatype::Int64
        );
        assert_eq!(
            reader.dataset("/float32").expect("dataset").datatype(),
            &Datatype::Float32
        );
        assert_eq!(
            reader.dataset("/float64").expect("dataset").datatype(),
            &Datatype::Float64
        );
    }
}

/// Real dataset attributes (string / f64 / i32) round-trip through the file.
#[test]
fn test_dataset_attributes_roundtrip() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");
        writer
            .create_dataset(
                "/temperature",
                Datatype::Float32,
                vec![8],
                DatasetProperties::new(),
            )
            .expect("Failed to create dataset");
        writer
            .add_dataset_attribute("/temperature", Attribute::string("units", "celsius"))
            .expect("add string attr");
        writer
            .add_dataset_attribute("/temperature", Attribute::f64("scale_factor", 0.01))
            .expect("add f64 attr");
        writer
            .add_dataset_attribute("/temperature", Attribute::i32("valid_min", -50))
            .expect("add i32 attr");
        writer.finalize().expect("Failed to finalize");
    }

    {
        let reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");
        let dataset = reader.dataset("/temperature").expect("dataset");
        let attrs = dataset.attributes();

        assert_eq!(
            attrs.get("units").expect("units").as_string().ok(),
            Some("celsius".to_string())
        );
        let scale = attrs.get("scale_factor").expect("scale").as_f64().ok();
        assert!(scale.map(|v| (v - 0.01).abs() < 1e-12).unwrap_or(false));
        assert_eq!(
            attrs.get("valid_min").expect("valid_min").as_i32().ok(),
            Some(-50)
        );
    }
}

/// A single-level sub-group with a real f64 dataset round-trips.
#[test]
fn test_single_level_group_dataset() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");
        writer
            .create_group("/measurements")
            .expect("Failed to create group");
        writer
            .create_dataset(
                "/measurements/pressure",
                Datatype::Float64,
                vec![3],
                DatasetProperties::new(),
            )
            .expect("Failed to create group dataset");
        writer
            .write_f64("/measurements/pressure", &[101.3, 100.9, 99.8])
            .expect("write f64");
        writer.finalize().expect("Failed to finalize");
    }

    {
        let mut reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");
        assert!(reader.is_group("/measurements"));
        assert!(reader.is_dataset("/measurements/pressure"));

        {
            let ds = reader.dataset("/measurements/pressure").expect("dataset");
            assert_eq!(ds.dims(), &[3]);
        }
        assert_eq!(
            reader.read_f64("/measurements/pressure").expect("read f64"),
            vec![101.3, 100.9, 99.8]
        );
    }
}

/// Root string attributes round-trip through the file.
#[test]
fn test_root_string_attributes() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");
        writer
            .add_group_attribute("/", Attribute::string("title", "Sensor Log"))
            .expect("add root attr");
        writer
            .create_dataset("/data", Datatype::Int32, vec![2], DatasetProperties::new())
            .expect("create dataset");
        writer.write_i32("/data", &[7, 8]).expect("write i32");
        writer.finalize().expect("Failed to finalize");
    }

    {
        let reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");
        let root = reader.root().expect("root");
        assert_eq!(
            root.attributes()
                .get("title")
                .expect("title")
                .as_string()
                .ok(),
            Some("Sensor Log".to_string())
        );
    }
}

/// Nested groups exceed `oxih5`'s writer surface and must fail loud, never
/// silently produce a fake file.
#[test]
fn test_nested_group_fails_loud() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let mut writer =
        Hdf5Writer::create(temp_file.path(), Hdf5Version::V10).expect("Failed to create writer");
    writer.create_group("/a").expect("create group a");
    writer.create_group("/a/b").expect("create nested group b");
    let result = writer.finalize();
    assert!(result.is_err(), "nested groups must fail loud at finalize");
}

#[test]
fn test_temp_dir_usage() {
    // Use temp_dir for the test file, per policy.
    let temp_dir = env::temp_dir();
    let temp_file = temp_dir.join("oxigdal_hdf5_integration_temp_dir.h5");

    {
        let mut writer =
            Hdf5Writer::create(&temp_file, Hdf5Version::V10).expect("Failed to create writer");
        writer
            .create_dataset("/data", Datatype::Int32, vec![5], DatasetProperties::new())
            .expect("Failed to create dataset");
        writer.write_i32("/data", &[1, 2, 3, 4, 5]).expect("write");
        writer.finalize().expect("Failed to finalize");
    }

    {
        let reader = Hdf5Reader::open(&temp_file).expect("Failed to open file");
        assert!(reader.exists("/data"));
    }

    let _ = std::fs::remove_file(temp_file);
}

#[test]
fn test_error_handling() {
    // Opening a missing file errors.
    let result = Hdf5Reader::open("/nonexistent/path/file.h5");
    assert!(result.is_err());

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let mut writer =
        Hdf5Writer::create(temp_file.path(), Hdf5Version::V10).expect("Failed to create writer");

    // Duplicate group.
    writer
        .create_group("/group1")
        .expect("Failed to create group");
    assert!(writer.create_group("/group1").is_err());

    // Dataset without a parent group.
    let result = writer.create_dataset(
        "/nonexistent/dataset",
        Datatype::Int32,
        vec![10],
        DatasetProperties::new(),
    );
    assert!(result.is_err());

    // Writing to a nonexistent dataset.
    let result = writer.write_i32("/nonexistent", &[1, 2, 3]);
    assert!(result.is_err());
}
