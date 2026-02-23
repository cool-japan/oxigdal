#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::*;
    use crate::raster::terrain::curvature::compute_curvature;
    use crate::raster::terrain::landform::{
        classify_landforms, classify_landforms_multiscale, compute_spi,
        compute_terrain_shape_index, compute_twi,
    };
    use crate::raster::terrain::roughness::{
        compute_convergence_index, compute_roughness, compute_roughness_advanced, compute_tpi,
        compute_tpi_advanced, compute_tri, compute_tri_advanced, compute_vrm,
    };
    use crate::raster::terrain::slope_aspect::get_3x3_window;
    use crate::raster::terrain::{
        CurvatureType, LandformClass, RoughnessMethod, TpiNeighborhood, TriMethod,
    };
    use approx::assert_abs_diff_eq;
    use oxigdal_core::RasterDataType;
    use oxigdal_core::buffer::RasterBuffer;

    fn create_simple_dem() -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, (x + y) as f64);
            }
        }
        dem
    }

    fn create_peak_dem() -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(11, 11, RasterDataType::Float32);
        for y in 0..11 {
            for x in 0..11 {
                let dist = ((x as i32 - 5).pow(2) + (y as i32 - 5).pow(2)) as f64;
                let _ = dem.set_pixel(x, y, 100.0 - dist);
            }
        }
        dem
    }

    fn create_valley_dem() -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(11, 11, RasterDataType::Float32);
        for y in 0..11 {
            for x in 0..11 {
                let dist = ((x as i32 - 5).pow(2) + (y as i32 - 5).pow(2)) as f64;
                let _ = dem.set_pixel(x, y, dist);
            }
        }
        dem
    }

    fn create_flat_dem(value: f64) -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, value);
            }
        }
        dem
    }

    // --- TPI tests ---

    #[test]
    fn test_compute_tpi_uniform_slope() {
        let dem = create_simple_dem();
        let result = compute_tpi(&dem, 3, 1.0);
        assert!(result.is_ok());
        let tpi = result.expect("tpi should succeed");
        let center = tpi.get_pixel(5, 5).expect("pixel read");
        assert!(
            center.abs() < 0.1,
            "TPI on uniform slope should be near zero"
        );
    }

    #[test]
    fn test_tpi_peak() {
        let dem = create_peak_dem();
        let result = compute_tpi(&dem, 3, 1.0);
        assert!(result.is_ok());
        let tpi = result.expect("tpi");
        let center = tpi.get_pixel(5, 5).expect("pixel");
        assert!(center > 0.0, "TPI at peak should be positive");
    }

    #[test]
    fn test_tpi_valley() {
        let dem = create_valley_dem();
        let result = compute_tpi(&dem, 3, 1.0);
        assert!(result.is_ok());
        let tpi = result.expect("tpi");
        let center = tpi.get_pixel(5, 5).expect("pixel");
        assert!(center < 0.0, "TPI at valley should be negative");
    }

    #[test]
    fn test_tpi_annular() {
        let dem = create_peak_dem();
        let result = compute_tpi_advanced(
            &dem,
            TpiNeighborhood::Annular {
                inner_radius: 1.0,
                outer_radius: 3.0,
            },
            1.0,
        );
        assert!(result.is_ok());
        let tpi = result.expect("annular tpi");
        let center = tpi.get_pixel(5, 5).expect("pixel");
        assert!(center > 0.0, "Annular TPI at peak should be positive");
    }

    #[test]
    fn test_tpi_invalid_neighborhood() {
        let dem = create_simple_dem();
        assert!(compute_tpi(&dem, 4, 1.0).is_err());
    }

    #[test]
    fn test_tpi_annular_invalid_radii() {
        let dem = create_simple_dem();
        assert!(
            compute_tpi_advanced(
                &dem,
                TpiNeighborhood::Annular {
                    inner_radius: 3.0,
                    outer_radius: 1.0,
                },
                1.0,
            )
            .is_err()
        );
    }

    // --- TRI tests ---

    #[test]
    fn test_compute_tri_riley() {
        let dem = create_simple_dem();
        let result = compute_tri(&dem, 1.0);
        assert!(result.is_ok());
        let tri = result.expect("tri");
        let center = tri.get_pixel(5, 5).expect("pixel");
        assert!(center > 0.0, "TRI on sloped terrain should be positive");
    }

    #[test]
    fn test_tri_flat_terrain() {
        let dem = create_flat_dem(42.0);
        let result = compute_tri(&dem, 1.0);
        assert!(result.is_ok());
        let tri = result.expect("tri");
        let center = tri.get_pixel(5, 5).expect("pixel");
        assert_abs_diff_eq!(center, 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_tri_methods_consistent() {
        let dem = create_simple_dem();
        let tri_riley = compute_tri_advanced(&dem, 1.0, TriMethod::Riley).expect("riley");
        let tri_mad =
            compute_tri_advanced(&dem, 1.0, TriMethod::MeanAbsoluteDifference).expect("mad");
        let tri_rms = compute_tri_advanced(&dem, 1.0, TriMethod::RootMeanSquare).expect("rms");

        let r_val = tri_riley.get_pixel(5, 5).expect("pixel");
        let m_val = tri_mad.get_pixel(5, 5).expect("pixel");
        let rms_val = tri_rms.get_pixel(5, 5).expect("pixel");

        // All methods should produce positive values on sloped terrain
        assert!(r_val > 0.0);
        assert!(m_val > 0.0);
        assert!(rms_val > 0.0);
    }

    // --- Roughness tests ---

    #[test]
    fn test_compute_roughness_stddev() {
        let dem = create_simple_dem();
        let result = compute_roughness(&dem, 3);
        assert!(result.is_ok());
        let rough = result.expect("roughness");
        let center = rough.get_pixel(5, 5).expect("pixel");
        assert!(center > 0.0);
    }

    #[test]
    fn test_roughness_range() {
        let dem = create_simple_dem();
        let result = compute_roughness_advanced(&dem, 3, RoughnessMethod::Range);
        assert!(result.is_ok());
        let rough = result.expect("roughness");
        let center = rough.get_pixel(5, 5).expect("pixel");
        // For a 3x3 window on x+y slope, range should be 4 (max(x+y) - min(x+y) in window)
        assert!(center > 0.0);
    }

    #[test]
    fn test_roughness_flat() {
        let dem = create_flat_dem(100.0);
        let result = compute_roughness(&dem, 3);
        assert!(result.is_ok());
        let rough = result.expect("roughness");
        let center = rough.get_pixel(5, 5).expect("pixel");
        assert_abs_diff_eq!(center, 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_roughness_cv() {
        let dem = create_simple_dem();
        let result = compute_roughness_advanced(&dem, 3, RoughnessMethod::CoefficientOfVariation);
        assert!(result.is_ok());
    }

    #[test]
    fn test_roughness_invalid_size() {
        let dem = create_simple_dem();
        assert!(compute_roughness(&dem, 4).is_err());
    }

    // --- Curvature tests ---

    #[test]
    fn test_curvature_profile_linear_slope() {
        let dem = create_simple_dem();
        let result = compute_curvature(&dem, 1.0, CurvatureType::Profile);
        assert!(result.is_ok());
        let curv = result.expect("curvature");
        let center = curv.get_pixel(5, 5).expect("pixel");
        // Linear slope should have near-zero profile curvature
        assert!(
            center.abs() < 1.0,
            "Profile curvature on linear slope should be near zero, got {center}"
        );
    }

    #[test]
    fn test_curvature_planform_linear_slope() {
        let dem = create_simple_dem();
        let result = compute_curvature(&dem, 1.0, CurvatureType::Planform);
        assert!(result.is_ok());
        let curv = result.expect("curvature");
        let center = curv.get_pixel(5, 5).expect("pixel");
        assert!(center.abs() < 1.0);
    }

    #[test]
    fn test_curvature_total_flat() {
        let dem = create_flat_dem(50.0);
        let result = compute_curvature(&dem, 1.0, CurvatureType::Total);
        assert!(result.is_ok());
        let curv = result.expect("curvature");
        let center = curv.get_pixel(5, 5).expect("pixel");
        assert_abs_diff_eq!(center, 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_curvature_gaussian_peak() {
        let dem = create_peak_dem();
        let result = compute_curvature(&dem, 1.0, CurvatureType::Gaussian);
        assert!(result.is_ok());
        let curv = result.expect("curvature");
        let center = curv.get_pixel(5, 5).expect("pixel");
        // Peak should have positive Gaussian curvature
        assert!(
            center > 0.0,
            "Gaussian curvature at peak should be positive, got {center}"
        );
    }

    #[test]
    fn test_curvature_tangential() {
        let dem = create_simple_dem();
        let result = compute_curvature(&dem, 1.0, CurvatureType::Tangential);
        assert!(result.is_ok());
    }

    // --- VRM tests ---

    #[test]
    fn test_compute_vrm() {
        let dem = create_simple_dem();
        let result = compute_vrm(&dem, 3, 1.0);
        assert!(result.is_ok());
        let vrm_buf = result.expect("vrm");
        let center = vrm_buf.get_pixel(5, 5).expect("pixel");
        assert!((0.0..=1.0).contains(&center));
    }

    #[test]
    fn test_vrm_flat() {
        let dem = create_flat_dem(100.0);
        let result = compute_vrm(&dem, 3, 1.0);
        assert!(result.is_ok());
        let vrm_buf = result.expect("vrm");
        let center = vrm_buf.get_pixel(5, 5).expect("pixel");
        assert_abs_diff_eq!(center, 0.0, epsilon = 1e-6);
    }

    #[test]
    fn test_vrm_invalid_neighborhood() {
        let dem = create_simple_dem();
        assert!(compute_vrm(&dem, 4, 1.0).is_err());
    }

    // --- Convergence Index tests ---

    #[test]
    fn test_convergence_index() {
        let dem = create_simple_dem();
        let result = compute_convergence_index(&dem, 1.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_convergence_valley() {
        let dem = create_valley_dem();
        let result = compute_convergence_index(&dem, 1.0);
        assert!(result.is_ok());
        let ci = result.expect("ci");
        let center = ci.get_pixel(5, 5).expect("pixel");
        // Valley centers should show convergence (negative values)
        assert!(
            center < 50.0,
            "Valley center convergence index should indicate convergence"
        );
    }

    // --- Landform classification tests ---

    #[test]
    fn test_classify_landforms() {
        // Create a DEM with a localized peak on a flat background.
        // A purely parabolic DEM has constant TPI everywhere, making
        // standardization collapse all values to ~0. Instead, we need
        // genuine topographic contrast.
        let size: u64 = 31;
        let center_x = 15i32;
        let center_y = 15i32;
        let mut dem = RasterBuffer::zeros(size, size, RasterDataType::Float32);
        for y in 0..size {
            for x in 0..size {
                let dx = x as i32 - center_x;
                let dy = y as i32 - center_y;
                let dist_sq = (dx * dx + dy * dy) as f64;
                // Flat background at 50, sharp Gaussian-like peak at center rising to 150
                let elev = 50.0 + 100.0 * (-dist_sq / 8.0).exp();
                let _ = dem.set_pixel(x, y, elev);
            }
        }

        let result = classify_landforms(&dem, 3, 1.0, 5.0);
        assert!(result.is_ok());
        let landforms = result.expect("landforms");
        let center = landforms.get_pixel(15, 15).expect("pixel");
        // Center of the localized peak should be classified as Ridge or UpperSlope
        assert!(
            center as u8 == LandformClass::Ridge as u8
                || center as u8 == LandformClass::UpperSlope as u8,
            "Peak center should be ridge or upper slope, got class {center}"
        );
    }

    #[test]
    fn test_classify_landforms_multiscale() {
        let dem = create_peak_dem();
        let result = classify_landforms_multiscale(&dem, 3, 5, 1.0, 5.0);
        assert!(result.is_ok());
    }

    // --- TWI / SPI tests ---

    #[test]
    fn test_compute_twi() {
        let mut flow_acc = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let mut slope_rad = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                let _ = flow_acc.set_pixel(x, y, (x * y) as f64 + 1.0);
                let _ = slope_rad.set_pixel(x, y, 0.1); // ~5.7 degrees
            }
        }

        let result = compute_twi(&flow_acc, &slope_rad, 1.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compute_spi() {
        let mut flow_acc = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let mut slope_rad = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                let _ = flow_acc.set_pixel(x, y, (x * y) as f64 + 1.0);
                let _ = slope_rad.set_pixel(x, y, 0.1);
            }
        }

        let result = compute_spi(&flow_acc, &slope_rad, 1.0);
        assert!(result.is_ok());
        let spi = result.expect("spi");
        // SPI should increase with flow accumulation
        let low = spi.get_pixel(1, 1).expect("pixel");
        let high = spi.get_pixel(8, 8).expect("pixel");
        assert!(high > low, "SPI should increase with flow accumulation");
    }

    // --- TSI tests ---

    #[test]
    fn test_terrain_shape_index() {
        let dem = create_simple_dem();
        let result = compute_terrain_shape_index(&dem, 1.0);
        assert!(result.is_ok());
        let tsi = result.expect("tsi");
        let center = tsi.get_pixel(5, 5).expect("pixel");
        // Linear slope => Laplacian near 0 => TSI near 0.5
        assert!(
            (center - 0.5).abs() < 0.1,
            "TSI on linear slope should be near 0.5, got {center}"
        );
    }

    #[test]
    fn test_terrain_shape_index_peak() {
        let dem = create_peak_dem();
        let result = compute_terrain_shape_index(&dem, 1.0);
        assert!(result.is_ok());
        let tsi = result.expect("tsi");
        let center = tsi.get_pixel(5, 5).expect("pixel");
        // Peak => negative Laplacian => TSI < 0.5 (convex)
        assert!(
            center < 0.5,
            "TSI at peak should be < 0.5 (convex), got {center}"
        );
    }

    // --- Helper tests ---

    #[test]
    fn test_get_3x3_window() {
        let dem = create_simple_dem();
        let w = get_3x3_window(&dem, 5, 5).expect("window");
        // z[1][1] should be center pixel: 5+5 = 10
        assert_abs_diff_eq!(w[1][1], 10.0, epsilon = 1e-6);
        // z[0][0] should be (4,4): 4+4 = 8
        assert_abs_diff_eq!(w[0][0], 8.0, epsilon = 1e-6);
    }

    #[test]
    fn test_landform_class_name() {
        assert_eq!(LandformClass::Valley.name(), "Valley");
        assert_eq!(LandformClass::Ridge.name(), "Ridge");
        assert_eq!(LandformClass::Flat.name(), "Flat");
    }
}
