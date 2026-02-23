//! TypeScript bindings and documentation generation
//!
//! This module provides utilities for generating TypeScript type definitions,
//! API documentation, and example code for the WASM bindings.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// TypeScript type representation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TsType {
    /// String primitive type
    String,
    /// Number primitive type
    Number,
    /// Boolean primitive type
    Boolean,
    /// Void type (function returns nothing)
    Void,
    /// Null type
    Null,
    /// Undefined type
    Undefined,
    /// Any type (accepts anything)
    Any,

    /// Array type
    Array(Box<TsType>),

    /// Object type with fields
    Object(Vec<(String, TsType)>),

    /// Union type
    Union(Vec<TsType>),

    /// Optional type
    Optional(Box<TsType>),

    /// Promise type
    Promise(Box<TsType>),

    /// Tuple type
    Tuple(Vec<TsType>),

    /// Custom type reference
    Reference(String),
}

impl TsType {
    /// Converts to TypeScript string representation
    pub fn to_ts_string(&self) -> String {
        match self {
            Self::String => "string".to_string(),
            Self::Number => "number".to_string(),
            Self::Boolean => "boolean".to_string(),
            Self::Void => "void".to_string(),
            Self::Null => "null".to_string(),
            Self::Undefined => "undefined".to_string(),
            Self::Any => "any".to_string(),
            Self::Array(inner) => format!("{}[]", inner.to_ts_string()),
            Self::Object(fields) => {
                let fields_str = fields
                    .iter()
                    .map(|(name, ty)| format!("{}: {}", name, ty.to_ts_string()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{ {} }}", fields_str)
            }
            Self::Union(types) => types
                .iter()
                .map(|t| t.to_ts_string())
                .collect::<Vec<_>>()
                .join(" | "),
            Self::Optional(inner) => format!("{} | null", inner.to_ts_string()),
            Self::Promise(inner) => format!("Promise<{}>", inner.to_ts_string()),
            Self::Tuple(types) => {
                let types_str = types
                    .iter()
                    .map(|t| t.to_ts_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{}]", types_str)
            }
            Self::Reference(name) => name.clone(),
        }
    }
}

/// Function parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsParameter {
    /// Parameter name
    pub name: String,
    /// Parameter type
    pub ty: TsType,
    /// Is optional
    pub optional: bool,
    /// Default value (as string)
    pub default: Option<String>,
    /// Description
    pub description: Option<String>,
}

impl TsParameter {
    /// Creates a new parameter
    pub fn new(name: impl Into<String>, ty: TsType) -> Self {
        Self {
            name: name.into(),
            ty,
            optional: false,
            default: None,
            description: None,
        }
    }

    /// Makes the parameter optional
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Sets a default value
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self.optional = true;
        self
    }

    /// Sets a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Converts to TypeScript string
    pub fn to_ts_string(&self) -> String {
        let optional_marker = if self.optional { "?" } else { "" };
        let default_str = if let Some(ref default) = self.default {
            format!(" = {}", default)
        } else {
            String::new()
        };

        format!(
            "{}{}: {}{}",
            self.name,
            optional_marker,
            self.ty.to_ts_string(),
            default_str
        )
    }
}

/// Function signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsFunction {
    /// Function name
    pub name: String,
    /// Parameters
    pub parameters: Vec<TsParameter>,
    /// Return type
    pub return_type: TsType,
    /// Is async
    pub is_async: bool,
    /// Description
    pub description: Option<String>,
    /// Example code
    pub examples: Vec<String>,
}

impl TsFunction {
    /// Creates a new function
    pub fn new(name: impl Into<String>, return_type: TsType) -> Self {
        Self {
            name: name.into(),
            parameters: Vec::new(),
            return_type,
            is_async: false,
            description: None,
            examples: Vec::new(),
        }
    }

    /// Adds a parameter
    pub fn parameter(mut self, param: TsParameter) -> Self {
        self.parameters.push(param);
        self
    }

    /// Marks as async
    pub fn async_fn(mut self) -> Self {
        self.is_async = true;
        self
    }

    /// Sets a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds an example
    pub fn with_example(mut self, example: impl Into<String>) -> Self {
        self.examples.push(example.into());
        self
    }

    /// Generates TypeScript declaration
    pub fn to_ts_declaration(&self) -> String {
        let params_str = self
            .parameters
            .iter()
            .map(|p| p.to_ts_string())
            .collect::<Vec<_>>()
            .join(", ");

        let return_type = if self.is_async {
            TsType::Promise(Box::new(self.return_type.clone()))
        } else {
            self.return_type.clone()
        };

        format!(
            "{}({}): {}",
            self.name,
            params_str,
            return_type.to_ts_string()
        )
    }

    /// Generates JSDoc comment
    pub fn to_jsdoc(&self) -> String {
        let mut lines = vec!["/**".to_string()];

        if let Some(ref desc) = self.description {
            lines.push(format!(" * {}", desc));
            if !self.parameters.is_empty() || !self.examples.is_empty() {
                lines.push(" *".to_string());
            }
        }

        for param in &self.parameters {
            let param_desc = param
                .description
                .as_ref()
                .map(|d| format!(" - {}", d))
                .unwrap_or_default();
            lines.push(format!(
                " * @param {{{}}} {}{}",
                param.ty.to_ts_string(),
                param.name,
                param_desc
            ));
        }

        if self.is_async {
            lines.push(format!(
                " * @returns {{Promise<{}>}}",
                self.return_type.to_ts_string()
            ));
        } else {
            lines.push(format!(
                " * @returns {{{}}}",
                self.return_type.to_ts_string()
            ));
        }

        if !self.examples.is_empty() {
            lines.push(" *".to_string());
            for example in &self.examples {
                lines.push(" * @example".to_string());
                for line in example.lines() {
                    lines.push(format!(" * {}", line));
                }
            }
        }

        lines.push(" */".to_string());
        lines.join("\n")
    }
}

/// Class member
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TsMember {
    /// Constructor
    Constructor(TsFunction),
    /// Method
    Method(TsFunction),
    /// Property
    Property {
        name: String,
        ty: TsType,
        readonly: bool,
        description: Option<String>,
    },
}

/// Class definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsClass {
    /// Class name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Members
    pub members: Vec<TsMember>,
    /// Examples
    pub examples: Vec<String>,
}

impl TsClass {
    /// Creates a new class
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            members: Vec::new(),
            examples: Vec::new(),
        }
    }

    /// Sets a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a member
    pub fn member(mut self, member: TsMember) -> Self {
        self.members.push(member);
        self
    }

    /// Adds an example
    pub fn with_example(mut self, example: impl Into<String>) -> Self {
        self.examples.push(example.into());
        self
    }

    /// Generates TypeScript declaration
    pub fn to_ts_declaration(&self) -> String {
        let mut lines = Vec::new();

        // Class JSDoc
        lines.push("/**".to_string());
        if let Some(ref desc) = self.description {
            lines.push(format!(" * {}", desc));
        }
        if !self.examples.is_empty() {
            lines.push(" *".to_string());
            for example in &self.examples {
                lines.push(" * @example".to_string());
                for line in example.lines() {
                    lines.push(format!(" * {}", line));
                }
            }
        }
        lines.push(" */".to_string());

        lines.push(format!("export class {} {{", self.name));

        for member in &self.members {
            match member {
                TsMember::Constructor(func) => {
                    lines.push("".to_string());
                    lines.push(format!("  {}", func.to_jsdoc().replace('\n', "\n  ")));
                    lines.push(format!(
                        "  constructor({});",
                        func.parameters
                            .iter()
                            .map(|p| p.to_ts_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                TsMember::Method(func) => {
                    lines.push("".to_string());
                    lines.push(format!("  {}", func.to_jsdoc().replace('\n', "\n  ")));
                    lines.push(format!("  {};", func.to_ts_declaration()));
                }
                TsMember::Property {
                    name,
                    ty,
                    readonly,
                    description,
                } => {
                    lines.push("".to_string());
                    if let Some(desc) = description {
                        lines.push(format!("  /** {} */", desc));
                    }
                    let readonly_str = if *readonly { "readonly " } else { "" };
                    lines.push(format!(
                        "  {}{}: {};",
                        readonly_str,
                        name,
                        ty.to_ts_string()
                    ));
                }
            }
        }

        lines.push("}".to_string());
        lines.join("\n")
    }
}

/// Interface definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsInterface {
    /// Interface name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Fields
    pub fields: Vec<(String, TsType, Option<String>)>, // (name, type, description)
}

impl TsInterface {
    /// Creates a new interface
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            fields: Vec::new(),
        }
    }

    /// Sets a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a field
    pub fn field(mut self, name: impl Into<String>, ty: TsType) -> Self {
        self.fields.push((name.into(), ty, None));
        self
    }

    /// Adds a field with description
    pub fn field_with_description(
        mut self,
        name: impl Into<String>,
        ty: TsType,
        description: impl Into<String>,
    ) -> Self {
        self.fields
            .push((name.into(), ty, Some(description.into())));
        self
    }

    /// Generates TypeScript declaration
    pub fn to_ts_declaration(&self) -> String {
        let mut lines = Vec::new();

        lines.push("/**".to_string());
        if let Some(ref desc) = self.description {
            lines.push(format!(" * {}", desc));
        }
        lines.push(" */".to_string());

        lines.push(format!("export interface {} {{", self.name));

        for (name, ty, desc) in &self.fields {
            if let Some(description) = desc {
                lines.push(format!("  /** {} */", description));
            }
            lines.push(format!("  {}: {};", name, ty.to_ts_string()));
        }

        lines.push("}".to_string());
        lines.join("\n")
    }
}

/// Type alias definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsTypeAlias {
    /// Alias name
    pub name: String,
    /// Target type
    pub ty: TsType,
    /// Description
    pub description: Option<String>,
}

impl TsTypeAlias {
    /// Creates a new type alias
    pub fn new(name: impl Into<String>, ty: TsType) -> Self {
        Self {
            name: name.into(),
            ty,
            description: None,
        }
    }

    /// Sets a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Generates TypeScript declaration
    pub fn to_ts_declaration(&self) -> String {
        let mut lines = Vec::new();

        if let Some(ref desc) = self.description {
            lines.push("/**".to_string());
            lines.push(format!(" * {}", desc));
            lines.push(" */".to_string());
        }

        lines.push(format!(
            "export type {} = {};",
            self.name,
            self.ty.to_ts_string()
        ));
        lines.join("\n")
    }
}

/// Module definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsModule {
    /// Module name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Classes
    pub classes: Vec<TsClass>,
    /// Interfaces
    pub interfaces: Vec<TsInterface>,
    /// Type aliases
    pub type_aliases: Vec<TsTypeAlias>,
    /// Functions
    pub functions: Vec<TsFunction>,
}

impl TsModule {
    /// Creates a new module
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            classes: Vec::new(),
            interfaces: Vec::new(),
            type_aliases: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// Sets a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a class
    pub fn class(mut self, class: TsClass) -> Self {
        self.classes.push(class);
        self
    }

    /// Adds an interface
    pub fn interface(mut self, interface: TsInterface) -> Self {
        self.interfaces.push(interface);
        self
    }

    /// Adds a type alias
    pub fn type_alias(mut self, alias: TsTypeAlias) -> Self {
        self.type_aliases.push(alias);
        self
    }

    /// Adds a function
    pub fn function(mut self, function: TsFunction) -> Self {
        self.functions.push(function);
        self
    }

    /// Generates TypeScript declarations
    pub fn to_ts_declarations(&self) -> String {
        let mut lines = Vec::new();

        // Module header
        lines.push("/**".to_string());
        lines.push(format!(" * @module {}", self.name));
        if let Some(ref desc) = self.description {
            lines.push(" *".to_string());
            lines.push(format!(" * {}", desc));
        }
        lines.push(" */".to_string());
        lines.push("".to_string());

        // Type aliases
        for alias in &self.type_aliases {
            lines.push(alias.to_ts_declaration());
            lines.push("".to_string());
        }

        // Interfaces
        for interface in &self.interfaces {
            lines.push(interface.to_ts_declaration());
            lines.push("".to_string());
        }

        // Classes
        for class in &self.classes {
            lines.push(class.to_ts_declaration());
            lines.push("".to_string());
        }

        // Functions
        for function in &self.functions {
            lines.push(function.to_jsdoc());
            lines.push(format!(
                "export function {}();",
                function.to_ts_declaration()
            ));
            lines.push("".to_string());
        }

        lines.join("\n")
    }
}

/// Documentation generator
pub struct DocGenerator {
    /// Modules
    modules: HashMap<String, TsModule>,
}

impl DocGenerator {
    /// Creates a new documentation generator
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }

    /// Adds a module
    pub fn add_module(&mut self, module: TsModule) {
        self.modules.insert(module.name.clone(), module);
    }

    /// Generates all declarations
    pub fn generate_all(&self) -> HashMap<String, String> {
        self.modules
            .iter()
            .map(|(name, module)| (name.clone(), module.to_ts_declarations()))
            .collect()
    }

    /// Generates a combined .d.ts file
    pub fn generate_combined(&self) -> String {
        let mut output = Vec::new();

        output.push("// Auto-generated TypeScript declarations for oxigdal-wasm".to_string());
        output.push("// Do not edit manually".to_string());
        output.push("".to_string());

        for module in self.modules.values() {
            output.push(module.to_ts_declarations());
            output.push("".to_string());
        }

        output.join("\n")
    }
}

impl Default for DocGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Creates the standard OxiGDAL WASM bindings documentation
pub fn create_oxigdal_wasm_docs() -> DocGenerator {
    let mut generator = DocGenerator::new();

    // Create main module
    let mut main_module = TsModule::new("oxigdal-wasm")
        .with_description("WebAssembly bindings for OxiGDAL - Browser-based geospatial processing");

    // Add WasmCogViewer class
    let cog_viewer = TsClass::new("WasmCogViewer")
        .with_description("Cloud Optimized GeoTIFF viewer for the browser")
        .member(TsMember::Constructor(
            TsFunction::new("constructor", TsType::Void)
                .with_description("Creates a new COG viewer instance"),
        ))
        .member(TsMember::Method(
            TsFunction::new("open", TsType::Promise(Box::new(TsType::Void)))
                .async_fn()
                .parameter(TsParameter::new("url", TsType::String).with_description("URL to the COG file"))
                .with_description("Opens a COG file from a URL")
                .with_example("const viewer = new WasmCogViewer();\nawait viewer.open('https://example.com/image.tif');"),
        ))
        .member(TsMember::Method(
            TsFunction::new("width", TsType::Number)
                .with_description("Returns the image width in pixels"),
        ))
        .member(TsMember::Method(
            TsFunction::new("height", TsType::Number)
                .with_description("Returns the image height in pixels"),
        ))
        .with_example("const viewer = new WasmCogViewer();\nawait viewer.open('image.tif');\nconsole.log(`Size: ${viewer.width()}x${viewer.height()}`);");

    main_module = main_module.class(cog_viewer);

    // Add TileCoord interface
    let tile_coord = TsInterface::new("TileCoord")
        .with_description("Tile coordinate in a pyramid")
        .field_with_description("level", TsType::Number, "Zoom level")
        .field_with_description("x", TsType::Number, "Column index")
        .field_with_description("y", TsType::Number, "Row index");

    main_module = main_module.interface(tile_coord);

    generator.add_module(main_module);
    generator
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ts_type_string() {
        assert_eq!(TsType::String.to_ts_string(), "string");
        assert_eq!(TsType::Number.to_ts_string(), "number");
        assert_eq!(
            TsType::Array(Box::new(TsType::String)).to_ts_string(),
            "string[]"
        );
        assert_eq!(
            TsType::Promise(Box::new(TsType::Number)).to_ts_string(),
            "Promise<number>"
        );
    }

    #[test]
    fn test_ts_parameter() {
        let param = TsParameter::new("name", TsType::String);
        assert_eq!(param.to_ts_string(), "name: string");

        let optional_param = TsParameter::new("value", TsType::Number).optional();
        assert_eq!(optional_param.to_ts_string(), "value?: number");
    }

    #[test]
    fn test_ts_function() {
        let func = TsFunction::new("greet", TsType::String)
            .parameter(TsParameter::new("name", TsType::String))
            .with_description("Greets a person");

        let declaration = func.to_ts_declaration();
        assert!(declaration.contains("greet"));
        assert!(declaration.contains("name: string"));
        assert!(declaration.contains("string"));
    }

    #[test]
    fn test_ts_class() {
        let class = TsClass::new("MyClass")
            .with_description("A test class")
            .member(TsMember::Method(
                TsFunction::new("method", TsType::Void).with_description("A test method"),
            ));

        let declaration = class.to_ts_declaration();
        assert!(declaration.contains("export class MyClass"));
        assert!(declaration.contains("method"));
    }

    #[test]
    fn test_ts_interface() {
        let interface = TsInterface::new("MyInterface")
            .with_description("A test interface")
            .field("name", TsType::String)
            .field("age", TsType::Number);

        let declaration = interface.to_ts_declaration();
        assert!(declaration.contains("export interface MyInterface"));
        assert!(declaration.contains("name: string"));
        assert!(declaration.contains("age: number"));
    }

    #[test]
    fn test_doc_generator() {
        let generator = create_oxigdal_wasm_docs();
        let combined = generator.generate_combined();

        assert!(combined.contains("WasmCogViewer"));
        assert!(combined.contains("TileCoord"));
    }
}
