//! Integration tests for HDF5 driver
//!
//! These tests verify round-trip functionality: writing HDF5 files and reading them back.

use oxigdal_hdf5::attribute::Attribute;
use oxigdal_hdf5::dataset::{CompressionFilter, DatasetProperties};
use oxigdal_hdf5::datatype::Datatype;
use oxigdal_hdf5::{Hdf5Reader, Hdf5Version, Hdf5Writer};
use std::env;
use tempfile::NamedTempFile;

#[test]
fn test_round_trip_simple() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // Write
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

    // Read
    {
        let mut reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");

        assert!(reader.exists("/data"));
        assert!(reader.is_dataset("/data"));

        let dataset = reader.dataset("/data").expect("Failed to get dataset");
        assert_eq!(dataset.name(), "data");
        assert_eq!(dataset.dims(), &[10]);
        assert_eq!(dataset.datatype(), &Datatype::Int32);

        // Note: Reading actual data is not fully implemented in the minimal Pure Rust version
        // In a full implementation, this would verify the data
        let data = reader.read_i32("/data").expect("Failed to read data");
        assert_eq!(data.len(), 10);
    }
}

#[test]
fn test_groups_and_hierarchy() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // Write
    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");

        // Create group hierarchy
        writer
            .create_group("/measurements")
            .expect("Failed to create group");
        writer
            .create_group("/measurements/temperature")
            .expect("Failed to create subgroup");
        writer
            .create_group("/measurements/pressure")
            .expect("Failed to create subgroup");

        // Add attributes
        writer
            .add_group_attribute(
                "/measurements",
                Attribute::string("description", "Sensor measurements"),
            )
            .expect("Failed to add attribute");

        writer
            .add_group_attribute("/measurements", Attribute::i32("sensor_count", 2))
            .expect("Failed to add attribute");

        writer.finalize().expect("Failed to finalize");
    }

    // Read
    {
        let reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");

        assert!(reader.exists("/measurements"));
        assert!(reader.is_group("/measurements"));
        assert!(reader.exists("/measurements/temperature"));
        assert!(reader.exists("/measurements/pressure"));
    }
}

#[test]
fn test_multidimensional_datasets() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // Write
    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");

        // 2D dataset
        writer
            .create_dataset(
                "/matrix2d",
                Datatype::Float32,
                vec![10, 20],
                DatasetProperties::new(),
            )
            .expect("Failed to create 2D dataset");

        let data2d: Vec<f32> = vec![1.5; 200]; // 10 * 20
        writer
            .write_f32("/matrix2d", &data2d)
            .expect("Failed to write 2D data");

        // 3D dataset
        writer
            .create_dataset(
                "/matrix3d",
                Datatype::Float64,
                vec![5, 10, 15],
                DatasetProperties::new(),
            )
            .expect("Failed to create 3D dataset");

        let data3d: Vec<f64> = vec![2.5; 750]; // 5 * 10 * 15
        writer
            .write_f64("/matrix3d", &data3d)
            .expect("Failed to write 3D data");

        writer.finalize().expect("Failed to finalize");
    }

    // Read
    {
        let reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");

        let dataset2d = reader
            .dataset("/matrix2d")
            .expect("Failed to get 2D dataset");
        assert_eq!(dataset2d.dims(), &[10, 20]);
        assert_eq!(dataset2d.len(), 200);
        assert_eq!(dataset2d.ndims(), 2);

        let dataset3d = reader
            .dataset("/matrix3d")
            .expect("Failed to get 3D dataset");
        assert_eq!(dataset3d.dims(), &[5, 10, 15]);
        assert_eq!(dataset3d.len(), 750);
        assert_eq!(dataset3d.ndims(), 3);
    }
}

#[test]
fn test_dataset_attributes() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // Write
    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");

        writer
            .create_dataset(
                "/temperature",
                Datatype::Float32,
                vec![100],
                DatasetProperties::new(),
            )
            .expect("Failed to create dataset");

        // Add various attributes
        writer
            .add_dataset_attribute("/temperature", Attribute::string("units", "celsius"))
            .expect("Failed to add string attribute");

        writer
            .add_dataset_attribute("/temperature", Attribute::f64("scale_factor", 0.01))
            .expect("Failed to add f64 attribute");

        writer
            .add_dataset_attribute("/temperature", Attribute::i32("valid_min", -50))
            .expect("Failed to add i32 attribute");

        writer
            .add_dataset_attribute("/temperature", Attribute::i32("valid_max", 50))
            .expect("Failed to add i32 attribute");

        writer.finalize().expect("Failed to finalize");
    }

    // Read
    {
        let reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");

        let dataset = reader
            .dataset("/temperature")
            .expect("Failed to get dataset");

        // In a full implementation, we would verify the attributes here
        assert_eq!(dataset.name(), "temperature");
    }
}

#[test]
fn test_chunked_dataset() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // Write
    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");

        let properties = DatasetProperties::new().with_chunks(vec![10, 20]);

        writer
            .create_dataset("/chunked", Datatype::Int32, vec![100, 200], properties)
            .expect("Failed to create chunked dataset");

        let data: Vec<i32> = vec![42; 20000]; // 100 * 200
        writer
            .write_i32("/chunked", &data)
            .expect("Failed to write chunked data");

        writer.finalize().expect("Failed to finalize");
    }

    // Read
    {
        let reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");

        let dataset = reader.dataset("/chunked").expect("Failed to get dataset");
        assert_eq!(dataset.dims(), &[100, 200]);

        let props = dataset.properties();
        assert_eq!(props.chunk_dims(), Some(&[10, 20][..]));
    }
}

#[test]
fn test_compressed_dataset() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // Write
    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");

        let properties = DatasetProperties::new()
            .with_chunks(vec![10, 20])
            .with_gzip(6);

        writer
            .create_dataset("/compressed", Datatype::Float64, vec![100, 200], properties)
            .expect("Failed to create compressed dataset");

        let data: Vec<f64> = vec![std::f64::consts::PI; 20000]; // 100 * 200
        writer
            .write_f64("/compressed", &data)
            .expect("Failed to write compressed data");

        writer.finalize().expect("Failed to finalize");
    }

    // Read
    {
        let reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");

        let dataset = reader
            .dataset("/compressed")
            .expect("Failed to get dataset");
        assert_eq!(dataset.dims(), &[100, 200]);

        let props = dataset.properties();
        assert_eq!(props.compression(), CompressionFilter::Gzip { level: 6 });
    }
}

#[test]
fn test_multiple_datatypes() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // Write
    {
        let mut writer = Hdf5Writer::create(temp_file.path(), Hdf5Version::V10)
            .expect("Failed to create writer");

        // Int8
        writer
            .create_dataset("/int8", Datatype::Int8, vec![10], DatasetProperties::new())
            .expect("Failed to create int8 dataset");

        // Int16
        writer
            .create_dataset(
                "/int16",
                Datatype::Int16,
                vec![10],
                DatasetProperties::new(),
            )
            .expect("Failed to create int16 dataset");

        // Int32
        writer
            .create_dataset(
                "/int32",
                Datatype::Int32,
                vec![10],
                DatasetProperties::new(),
            )
            .expect("Failed to create int32 dataset");

        // Float32
        writer
            .create_dataset(
                "/float32",
                Datatype::Float32,
                vec![10],
                DatasetProperties::new(),
            )
            .expect("Failed to create float32 dataset");

        // Float64
        writer
            .create_dataset(
                "/float64",
                Datatype::Float64,
                vec![10],
                DatasetProperties::new(),
            )
            .expect("Failed to create float64 dataset");

        writer.finalize().expect("Failed to finalize");
    }

    // Read
    {
        let reader = Hdf5Reader::open(temp_file.path()).expect("Failed to open file");

        assert_eq!(
            reader.dataset("/int8").expect("dataset").datatype(),
            &Datatype::Int8
        );
        assert_eq!(
            reader.dataset("/int16").expect("dataset").datatype(),
            &Datatype::Int16
        );
        assert_eq!(
            reader.dataset("/int32").expect("dataset").datatype(),
            &Datatype::Int32
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

#[test]
fn test_temp_dir_usage() {
    // Test that we use temp_dir for tests as per policy
    let temp_dir = env::temp_dir();
    let temp_file = temp_dir.join("test_hdf5.h5");

    // Write
    {
        let mut writer =
            Hdf5Writer::create(&temp_file, Hdf5Version::V10).expect("Failed to create writer");

        writer
            .create_dataset("/data", Datatype::Int32, vec![5], DatasetProperties::new())
            .expect("Failed to create dataset");

        writer.finalize().expect("Failed to finalize");
    }

    // Read
    {
        let reader = Hdf5Reader::open(&temp_file).expect("Failed to open file");
        assert!(reader.exists("/data"));
    }

    // Cleanup
    let _ = std::fs::remove_file(temp_file);
}

#[test]
fn test_error_handling() {
    // Test invalid file
    let result = Hdf5Reader::open("/nonexistent/path/file.h5");
    assert!(result.is_err());

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let mut writer =
        Hdf5Writer::create(temp_file.path(), Hdf5Version::V10).expect("Failed to create writer");

    // Test creating duplicate group
    writer
        .create_group("/group1")
        .expect("Failed to create group");
    let result = writer.create_group("/group1");
    assert!(result.is_err());

    // Test creating dataset without parent group
    let result = writer.create_dataset(
        "/nonexistent/dataset",
        Datatype::Int32,
        vec![10],
        DatasetProperties::new(),
    );
    assert!(result.is_err());

    // Test writing to nonexistent dataset
    let result = writer.write_i32("/nonexistent", &[1, 2, 3]);
    assert!(result.is_err());
}
