//! NetCDF variable types and utilities.
//!
//! Variables store the actual data in NetCDF files. They have dimensions,
//! attributes, and a data type.

use serde::{Deserialize, Serialize};

use crate::attribute::Attributes;
use crate::dimension::Dimensions;
use crate::error::{NetCdfError, Result};
use oxigdal_core::error::OxiGdalError;

/// Data types supported by NetCDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    /// 8-bit signed integer
    I8,
    /// 8-bit unsigned integer
    U8,
    /// 16-bit signed integer
    I16,
    /// 16-bit unsigned integer (NetCDF-4 only)
    U16,
    /// 32-bit signed integer
    I32,
    /// 32-bit unsigned integer (NetCDF-4 only)
    U32,
    /// 64-bit signed integer (NetCDF-4 only)
    I64,
    /// 64-bit unsigned integer (NetCDF-4 only)
    U64,
    /// 32-bit floating point
    F32,
    /// 64-bit floating point
    F64,
    /// Text/character
    Char,
    /// String (NetCDF-4 only)
    String,
}

impl DataType {
    /// Get the size in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        match self {
            Self::I8 | Self::U8 | Self::Char => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::I64 | Self::U64 | Self::F64 => 8,
            Self::String => 0, // Variable size
        }
    }

    /// Get the type name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::U8 => "u8",
            Self::I16 => "i16",
            Self::U16 => "u16",
            Self::I32 => "i32",
            Self::U32 => "u32",
            Self::I64 => "i64",
            Self::U64 => "u64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Char => "char",
            Self::String => "string",
        }
    }

    /// Check if this is a floating point type.
    #[must_use]
    pub const fn is_float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// Check if this is an integer type.
    #[must_use]
    pub const fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::I8
                | Self::U8
                | Self::I16
                | Self::U16
                | Self::I32
                | Self::U32
                | Self::I64
                | Self::U64
        )
    }

    /// Check if this is a signed type.
    #[must_use]
    pub const fn is_signed(&self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::F32 | Self::F64
        )
    }

    /// Check if this is available in NetCDF-3.
    #[must_use]
    pub const fn is_netcdf3_compatible(&self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::F32 | Self::F64 | Self::Char
        )
    }
}

/// A variable in a NetCDF file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    /// Name of the variable
    name: String,
    /// Data type
    data_type: DataType,
    /// Dimension names (in order)
    dimension_names: Vec<String>,
    /// Attributes
    attributes: Attributes,
    /// Whether this is a coordinate variable
    is_coordinate: bool,
}

impl Variable {
    /// Create a new variable.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the variable
    /// * `data_type` - Data type
    /// * `dimension_names` - Names of dimensions (in order)
    ///
    /// # Errors
    ///
    /// Returns error if the name is empty.
    pub fn new(
        name: impl Into<String>,
        data_type: DataType,
        dimension_names: Vec<String>,
    ) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(NetCdfError::Core(
                OxiGdalError::invalid_parameter_builder("name", "Variable name cannot be empty")
                    .with_operation("create_netcdf_variable")
                    .with_parameter("data_type", format!("{:?}", data_type))
                    .with_parameter("num_dimensions", dimension_names.len().to_string())
                    .with_suggestion("Provide a non-empty variable name")
                    .build(),
            ));
        }
        Ok(Self {
            name,
            data_type,
            dimension_names,
            attributes: Attributes::new(),
            is_coordinate: false,
        })
    }

    /// Create a coordinate variable.
    ///
    /// A coordinate variable has the same name as its dimension.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the variable and dimension
    /// * `data_type` - Data type
    ///
    /// # Errors
    ///
    /// Returns error if the name is empty.
    pub fn new_coordinate(name: impl Into<String>, data_type: DataType) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(NetCdfError::Core(
                OxiGdalError::invalid_parameter_builder(
                    "name",
                    "Coordinate variable name cannot be empty",
                )
                .with_operation("create_coordinate_variable")
                .with_parameter("data_type", format!("{:?}", data_type))
                .with_suggestion("Provide a non-empty coordinate variable name")
                .build(),
            ));
        }
        let dimension_names = vec![name.clone()];
        Ok(Self {
            name,
            data_type,
            dimension_names,
            attributes: Attributes::new(),
            is_coordinate: true,
        })
    }

    /// Get the variable name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the data type.
    #[must_use]
    pub const fn data_type(&self) -> DataType {
        self.data_type
    }

    /// Get the dimension names.
    #[must_use]
    pub fn dimension_names(&self) -> &[String] {
        &self.dimension_names
    }

    /// Get the number of dimensions.
    #[must_use]
    pub fn ndims(&self) -> usize {
        self.dimension_names.len()
    }

    /// Check if this is a scalar variable (no dimensions).
    #[must_use]
    pub fn is_scalar(&self) -> bool {
        self.dimension_names.is_empty()
    }

    /// Check if this is a coordinate variable.
    #[must_use]
    pub const fn is_coordinate(&self) -> bool {
        self.is_coordinate
    }

    /// Set whether this is a coordinate variable.
    pub fn set_coordinate(&mut self, is_coordinate: bool) {
        self.is_coordinate = is_coordinate;
    }

    /// Get the attributes.
    #[must_use]
    pub const fn attributes(&self) -> &Attributes {
        &self.attributes
    }

    /// Get mutable access to attributes.
    pub fn attributes_mut(&mut self) -> &mut Attributes {
        &mut self.attributes
    }

    /// Get the shape based on dimensions.
    ///
    /// # Errors
    ///
    /// Returns error if any dimension is not found.
    pub fn shape(&self, dimensions: &Dimensions) -> Result<Vec<usize>> {
        self.dimension_names
            .iter()
            .map(|name| {
                dimensions
                    .get(name)
                    .map(|d| d.len())
                    .ok_or_else(|| NetCdfError::DimensionNotFound { name: name.clone() })
            })
            .collect()
    }

    /// Get the total size based on dimensions.
    ///
    /// # Errors
    ///
    /// Returns error if any dimension is not found or if size overflows.
    pub fn size(&self, dimensions: &Dimensions) -> Result<usize> {
        if self.is_scalar() {
            return Ok(1);
        }

        let shape = self.shape(dimensions)?;
        shape
            .iter()
            .try_fold(1usize, |acc, &size| acc.checked_mul(size))
            .ok_or_else(|| {
                NetCdfError::Core(
                    OxiGdalError::io_error_builder("NetCDF variable size overflow")
                        .with_operation("calculate_variable_size")
                        .with_parameter("variable", &self.name)
                        .with_parameter("ndims", self.dimension_names.len().to_string())
                        .with_suggestion(
                            "Variable dimensions result in size overflow. Check dimension sizes",
                        )
                        .build(),
                )
            })
    }

    /// Get the size in bytes based on dimensions.
    ///
    /// # Errors
    ///
    /// Returns error if any dimension is not found or if size overflows.
    pub fn size_bytes(&self, dimensions: &Dimensions) -> Result<usize> {
        let element_size = self.data_type.size();
        if element_size == 0 {
            return Err(NetCdfError::Core(
                OxiGdalError::not_supported_builder("Variable-length data type size calculation")
                    .with_operation("calculate_variable_size_bytes")
                    .with_parameter("variable", &self.name)
                    .with_parameter("data_type", format!("{:?}", self.data_type))
                    .with_suggestion("Variable-length types require special handling")
                    .build(),
            ));
        }

        let num_elements = self.size(dimensions)?;
        num_elements.checked_mul(element_size).ok_or_else(|| {
            NetCdfError::Core(
                OxiGdalError::io_error_builder("NetCDF variable byte size overflow")
                    .with_operation("calculate_variable_size_bytes")
                    .with_parameter("variable", &self.name)
                    .with_parameter("element_size", element_size.to_string())
                    .with_parameter("num_elements", num_elements.to_string())
                    .with_suggestion(
                        "Variable size exceeds maximum. Reduce dimensions or use chunking",
                    )
                    .build(),
            )
        })
    }

    /// Check if compatible with NetCDF-3.
    #[must_use]
    pub fn is_netcdf3_compatible(&self) -> bool {
        self.data_type.is_netcdf3_compatible()
    }
}

/// Collection of variables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Variables {
    variables: Vec<Variable>,
}

impl Variables {
    /// Create a new empty variable collection.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            variables: Vec::new(),
        }
    }

    /// Create from a vector of variables.
    #[must_use]
    pub const fn from_vec(variables: Vec<Variable>) -> Self {
        Self { variables }
    }

    /// Add a variable.
    ///
    /// # Errors
    ///
    /// Returns error if a variable with the same name already exists.
    pub fn add(&mut self, variable: Variable) -> Result<()> {
        if self.contains(variable.name()) {
            return Err(NetCdfError::Core(
                OxiGdalError::invalid_parameter_builder("variable", "Variable already exists")
                    .with_operation("add_netcdf_variable")
                    .with_parameter("variable_name", variable.name())
                    .with_parameter("data_type", format!("{:?}", variable.data_type()))
                    .with_suggestion("Use a unique variable name or retrieve existing variable")
                    .build(),
            ));
        }
        self.variables.push(variable);
        Ok(())
    }

    /// Get a variable by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Variable> {
        self.variables.iter().find(|v| v.name() == name)
    }

    /// Get a mutable reference to a variable by name.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Variable> {
        self.variables.iter_mut().find(|v| v.name() == name)
    }

    /// Get a variable by index.
    #[must_use]
    pub fn get_by_index(&self, index: usize) -> Option<&Variable> {
        self.variables.get(index)
    }

    /// Check if a variable exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.variables.iter().any(|v| v.name() == name)
    }

    /// Get the number of variables.
    #[must_use]
    pub fn len(&self) -> usize {
        self.variables.len()
    }

    /// Check if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }

    /// Get an iterator over variables.
    pub fn iter(&self) -> impl Iterator<Item = &Variable> {
        self.variables.iter()
    }

    /// Get names of all variables.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.variables.iter().map(|v| v.name()).collect()
    }

    /// Get coordinate variables.
    pub fn coordinates(&self) -> impl Iterator<Item = &Variable> {
        self.variables.iter().filter(|v| v.is_coordinate())
    }

    /// Get data variables (non-coordinate).
    pub fn data_variables(&self) -> impl Iterator<Item = &Variable> {
        self.variables.iter().filter(|v| !v.is_coordinate())
    }
}

impl IntoIterator for Variables {
    type Item = Variable;
    type IntoIter = std::vec::IntoIter<Variable>;

    fn into_iter(self) -> Self::IntoIter {
        self.variables.into_iter()
    }
}

impl<'a> IntoIterator for &'a Variables {
    type Item = &'a Variable;
    type IntoIter = std::slice::Iter<'a, Variable>;

    fn into_iter(self) -> Self::IntoIter {
        self.variables.iter()
    }
}

impl FromIterator<Variable> for Variables {
    fn from_iter<T: IntoIterator<Item = Variable>>(iter: T) -> Self {
        Self {
            variables: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimension::Dimension;

    #[test]
    fn test_data_type_properties() {
        assert_eq!(DataType::F32.size(), 4);
        assert_eq!(DataType::F64.size(), 8);
        assert!(DataType::F32.is_float());
        assert!(DataType::I32.is_integer());
        assert!(DataType::I32.is_signed());
        assert!(!DataType::U32.is_signed());
    }

    #[test]
    fn test_netcdf3_compatibility() {
        assert!(DataType::I8.is_netcdf3_compatible());
        assert!(DataType::I16.is_netcdf3_compatible());
        assert!(DataType::I32.is_netcdf3_compatible());
        assert!(DataType::F32.is_netcdf3_compatible());
        assert!(DataType::F64.is_netcdf3_compatible());
        assert!(!DataType::U16.is_netcdf3_compatible());
        assert!(!DataType::U32.is_netcdf3_compatible());
        assert!(!DataType::String.is_netcdf3_compatible());
    }

    #[test]
    fn test_variable_creation() {
        let var = Variable::new(
            "temperature",
            DataType::F32,
            vec!["time".to_string(), "lat".to_string(), "lon".to_string()],
        )
        .expect("Failed to create temperature variable");
        assert_eq!(var.name(), "temperature");
        assert_eq!(var.data_type(), DataType::F32);
        assert_eq!(var.ndims(), 3);
        assert!(!var.is_scalar());
        assert!(!var.is_coordinate());
    }

    #[test]
    fn test_coordinate_variable() {
        let var = Variable::new_coordinate("time", DataType::F64)
            .expect("Failed to create coordinate variable");
        assert_eq!(var.name(), "time");
        assert!(var.is_coordinate());
        assert_eq!(var.ndims(), 1);
        assert_eq!(var.dimension_names()[0], "time");
    }

    #[test]
    fn test_scalar_variable() {
        let var = Variable::new("global_average", DataType::F32, vec![])
            .expect("Failed to create scalar variable");
        assert!(var.is_scalar());
        assert_eq!(var.ndims(), 0);
    }

    #[test]
    fn test_variable_shape() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("time", 10).expect("Failed to create time dimension"))
            .expect("Failed to add time dimension");
        dims.add(Dimension::new("lat", 180).expect("Failed to create lat dimension"))
            .expect("Failed to add lat dimension");
        dims.add(Dimension::new("lon", 360).expect("Failed to create lon dimension"))
            .expect("Failed to add lon dimension");

        let var = Variable::new(
            "temperature",
            DataType::F32,
            vec!["time".to_string(), "lat".to_string(), "lon".to_string()],
        )
        .expect("Failed to create temperature variable");

        let shape = var.shape(&dims).expect("Failed to get variable shape");
        assert_eq!(shape, vec![10, 180, 360]);

        let size = var.size(&dims).expect("Failed to get variable size");
        assert_eq!(size, 10 * 180 * 360);

        let size_bytes = var
            .size_bytes(&dims)
            .expect("Failed to get variable size in bytes");
        assert_eq!(size_bytes, 10 * 180 * 360 * 4);
    }

    #[test]
    fn test_variable_collection() {
        let mut vars = Variables::new();
        vars.add(
            Variable::new_coordinate("time", DataType::F64)
                .expect("Failed to create time coordinate variable"),
        )
        .expect("Failed to add time coordinate variable");
        vars.add(
            Variable::new("temperature", DataType::F32, vec!["time".to_string()])
                .expect("Failed to create temperature variable"),
        )
        .expect("Failed to add temperature variable");

        assert_eq!(vars.len(), 2);
        assert!(vars.contains("time"));
        assert!(vars.contains("temperature"));

        let coords: Vec<_> = vars.coordinates().collect();
        assert_eq!(coords.len(), 1);
        assert_eq!(coords[0].name(), "time");

        let data_vars: Vec<_> = vars.data_variables().collect();
        assert_eq!(data_vars.len(), 1);
        assert_eq!(data_vars[0].name(), "temperature");
    }

    #[test]
    fn test_empty_variable_name() {
        let result = Variable::new("", DataType::F32, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_variable() {
        let mut vars = Variables::new();
        vars.add(
            Variable::new("test", DataType::F32, vec![]).expect("Failed to create test variable"),
        )
        .expect("Failed to add test variable");
        let result = vars.add(
            Variable::new("test", DataType::F64, vec![])
                .expect("Failed to create duplicate test variable"),
        );
        assert!(result.is_err());
    }
}
