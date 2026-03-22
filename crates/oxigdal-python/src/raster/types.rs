//! Python-exposed raster types for metadata and window specification.
//!
//! This module provides PyO3-wrapped types for raster metadata and
//! window (sub-region) specification.

use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Metadata for raster datasets exposed to Python
#[pyclass(name = "RasterMetadata", from_py_object)]
#[derive(Clone)]
pub struct RasterMetadataPy {
    /// Width in pixels
    #[pyo3(get, set)]
    pub width: u64,
    /// Height in pixels
    #[pyo3(get, set)]
    pub height: u64,
    /// Number of bands
    #[pyo3(get, set)]
    pub band_count: u32,
    /// Data type as string
    #[pyo3(get, set)]
    pub data_type: String,
    /// CRS as WKT string
    #[pyo3(get, set)]
    pub crs: Option<String>,
    /// NoData value
    #[pyo3(get, set)]
    pub nodata: Option<f64>,
    /// GeoTransform coefficients
    #[pyo3(get, set)]
    pub geotransform: Option<Vec<f64>>,
}

#[pymethods]
impl RasterMetadataPy {
    /// Creates new raster metadata.
    ///
    /// Args:
    ///     width (int): Width in pixels
    ///     height (int): Height in pixels
    ///     band_count (int): Number of bands
    ///     data_type (str): Data type (e.g., 'float32', 'uint8')
    ///     crs (str, optional): CRS as WKT or EPSG code
    ///     nodata (float, optional): NoData value
    ///     geotransform (list, optional): GeoTransform as [x_min, pixel_width, 0, y_max, 0, -pixel_height]
    ///
    /// Returns:
    ///     RasterMetadata: New metadata object
    #[new]
    #[pyo3(signature = (width, height, band_count=1, data_type="float32", crs=None, nodata=None, geotransform=None))]
    pub fn new(
        width: u64,
        height: u64,
        band_count: u32,
        data_type: &str,
        crs: Option<String>,
        nodata: Option<f64>,
        geotransform: Option<Vec<f64>>,
    ) -> PyResult<Self> {
        if let Some(ref gt) = geotransform {
            if gt.len() != 6 {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "GeoTransform must have 6 elements",
                ));
            }
        }

        Ok(Self {
            width,
            height,
            band_count,
            data_type: data_type.to_string(),
            crs,
            nodata,
            geotransform,
        })
    }

    /// Returns string representation
    fn __repr__(&self) -> String {
        format!(
            "RasterMetadata(width={}, height={}, bands={}, dtype={})",
            self.width, self.height, self.band_count, self.data_type
        )
    }

    /// Converts to dictionary
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("width", self.width)?;
        dict.set_item("height", self.height)?;
        dict.set_item("band_count", self.band_count)?;
        dict.set_item("data_type", &self.data_type)?;
        if let Some(ref crs) = self.crs {
            dict.set_item("crs", crs)?;
        }
        if let Some(nodata) = self.nodata {
            dict.set_item("nodata", nodata)?;
        }
        if let Some(ref gt) = self.geotransform {
            dict.set_item("geotransform", gt.clone())?;
        }
        Ok(dict)
    }

    /// Creates metadata from dictionary
    #[staticmethod]
    fn from_dict(dict: &Bound<'_, PyDict>) -> PyResult<Self> {
        let width = dict
            .get_item("width")?
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'width'"))?
            .extract()?;
        let height = dict
            .get_item("height")?
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'height'"))?
            .extract()?;
        let band_count = dict
            .get_item("band_count")?
            .and_then(|v| v.extract().ok())
            .unwrap_or(1);
        let data_type = dict
            .get_item("data_type")?
            .and_then(|v| v.extract().ok())
            .unwrap_or_else(|| "float32".to_string());
        let crs = dict.get_item("crs")?.and_then(|v| v.extract().ok());
        let nodata = dict.get_item("nodata")?.and_then(|v| v.extract().ok());
        let geotransform = dict
            .get_item("geotransform")?
            .and_then(|v| v.extract().ok());

        Self::new(
            width,
            height,
            band_count,
            &data_type,
            crs,
            nodata,
            geotransform,
        )
    }

    /// Gets bounding box [minx, miny, maxx, maxy]
    fn get_bounds(&self) -> PyResult<Option<Vec<f64>>> {
        if let Some(ref gt) = self.geotransform {
            let min_x = gt[0];
            let max_y = gt[3];
            let max_x = min_x + (self.width as f64 * gt[1]);
            let min_y = max_y + (self.height as f64 * gt[5]);
            Ok(Some(vec![min_x, min_y, max_x, max_y]))
        } else {
            Ok(None)
        }
    }

    /// Gets pixel resolution [x_res, y_res]
    fn get_resolution(&self) -> Option<Vec<f64>> {
        self.geotransform.as_ref().map(|gt| vec![gt[1], -gt[5]])
    }
}

/// Window specification for reading sub-regions
#[pyclass(name = "Window", from_py_object)]
#[derive(Clone)]
pub struct WindowPy {
    #[pyo3(get, set)]
    pub col_off: u64,
    #[pyo3(get, set)]
    pub row_off: u64,
    #[pyo3(get, set)]
    pub width: u64,
    #[pyo3(get, set)]
    pub height: u64,
}

#[pymethods]
impl WindowPy {
    #[new]
    pub fn new(col_off: u64, row_off: u64, width: u64, height: u64) -> Self {
        Self {
            col_off,
            row_off,
            width,
            height,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Window(col_off={}, row_off={}, width={}, height={})",
            self.col_off, self.row_off, self.width, self.height
        )
    }

    /// Creates window from bounds
    #[staticmethod]
    fn from_bounds(bounds: Vec<f64>, metadata: &RasterMetadataPy) -> PyResult<Self> {
        if bounds.len() != 4 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Bounds must have 4 elements [minx, miny, maxx, maxy]",
            ));
        }

        let gt = metadata.geotransform.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("Metadata must have geotransform")
        })?;

        let minx = bounds[0];
        let miny = bounds[1];
        let maxx = bounds[2];
        let maxy = bounds[3];

        let col_off = ((minx - gt[0]) / gt[1]).floor() as u64;
        let row_off = ((maxy - gt[3]) / gt[5]).floor() as u64;
        let width = ((maxx - minx) / gt[1]).ceil() as u64;
        let height = ((miny - maxy) / gt[5]).ceil() as u64;

        Ok(Self::new(col_off, row_off, width, height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raster_metadata_creation() {
        let meta = RasterMetadataPy::new(
            512,
            512,
            3,
            "float32",
            Some("EPSG:4326".to_string()),
            Some(-9999.0),
            Some(vec![0.0, 1.0, 0.0, 100.0, 0.0, -1.0]),
        )
        .expect("Failed to create metadata");

        assert_eq!(meta.width, 512);
        assert_eq!(meta.height, 512);
        assert_eq!(meta.band_count, 3);
        assert_eq!(meta.data_type, "float32");
        assert_eq!(meta.crs, Some("EPSG:4326".to_string()));
        assert_eq!(meta.nodata, Some(-9999.0));
    }

    #[test]
    fn test_raster_metadata_repr() {
        let meta = RasterMetadataPy::new(100, 200, 1, "uint8", None, None, None)
            .expect("Failed to create metadata");

        let repr = meta.__repr__();
        assert!(repr.contains("width=100"));
        assert!(repr.contains("height=200"));
        assert!(repr.contains("bands=1"));
        assert!(repr.contains("dtype=uint8"));
    }

    #[test]
    fn test_window_creation() {
        let window = WindowPy::new(10, 20, 100, 200);
        assert_eq!(window.col_off, 10);
        assert_eq!(window.row_off, 20);
        assert_eq!(window.width, 100);
        assert_eq!(window.height, 200);
    }

    #[test]
    fn test_metadata_get_resolution() {
        let meta = RasterMetadataPy::new(
            100,
            100,
            1,
            "float32",
            None,
            None,
            Some(vec![0.0, 30.0, 0.0, 100.0, 0.0, -30.0]),
        )
        .expect("Failed to create metadata");

        let res = meta.get_resolution();
        assert!(res.is_some());
        let res_vec = res.expect("Resolution not found");
        assert_eq!(res_vec[0], 30.0);
        assert_eq!(res_vec[1], 30.0);
    }
}
