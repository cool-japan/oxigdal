//! HDF5 group navigation and hierarchy management.
//!
//! Groups provide a hierarchical organization of datasets, similar to directories
//! in a file system.

use crate::attribute::Attributes;
use crate::error::{Hdf5Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// HDF5 object type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectType {
    /// Group object
    Group,
    /// Dataset object
    Dataset,
    /// Named datatype object
    Datatype,
}

/// HDF5 object reference
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRef {
    /// Object name
    name: String,
    /// Object type
    object_type: ObjectType,
    /// Full path from root
    path: String,
}

impl ObjectRef {
    /// Create a new object reference
    pub fn new(name: String, object_type: ObjectType, path: String) -> Self {
        Self {
            name,
            object_type,
            path,
        }
    }

    /// Get the object name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the object type
    pub fn object_type(&self) -> ObjectType {
        self.object_type
    }

    /// Get the full path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Check if this is a group
    pub fn is_group(&self) -> bool {
        self.object_type == ObjectType::Group
    }

    /// Check if this is a dataset
    pub fn is_dataset(&self) -> bool {
        self.object_type == ObjectType::Dataset
    }
}

/// HDF5 group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// Group name
    name: String,
    /// Full path from root
    path: String,
    /// Group attributes
    attributes: Attributes,
    /// Child objects (name -> ObjectRef)
    children: HashMap<String, ObjectRef>,
}

impl Group {
    /// Create a new group
    pub fn new(name: String, path: String) -> Self {
        Self {
            name,
            path,
            attributes: Attributes::new(),
            children: HashMap::new(),
        }
    }

    /// Create the root group
    pub fn root() -> Self {
        Self::new("/".to_string(), "/".to_string())
    }

    /// Get the group name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the full path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the attributes
    pub fn attributes(&self) -> &Attributes {
        &self.attributes
    }

    /// Get mutable attributes
    pub fn attributes_mut(&mut self) -> &mut Attributes {
        &mut self.attributes
    }

    /// Add a child object
    pub fn add_child(&mut self, object: ObjectRef) {
        self.children.insert(object.name().to_string(), object);
    }

    /// Get a child object by name
    pub fn get_child(&self, name: &str) -> Result<&ObjectRef> {
        self.children
            .get(name)
            .ok_or_else(|| Hdf5Error::PathNotFound(format!("{}/{}", self.path, name)))
    }

    /// Check if a child exists
    pub fn has_child(&self, name: &str) -> bool {
        self.children.contains_key(name)
    }

    /// List all child names
    pub fn child_names(&self) -> Vec<&str> {
        self.children.keys().map(|s| s.as_str()).collect()
    }

    /// List all child objects
    pub fn children(&self) -> impl Iterator<Item = &ObjectRef> {
        self.children.values()
    }

    /// List all groups
    pub fn groups(&self) -> impl Iterator<Item = &ObjectRef> {
        self.children.values().filter(|obj| obj.is_group())
    }

    /// List all datasets
    pub fn datasets(&self) -> impl Iterator<Item = &ObjectRef> {
        self.children.values().filter(|obj| obj.is_dataset())
    }

    /// Get the number of children
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Check if empty (no children)
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Remove a child
    pub fn remove_child(&mut self, name: &str) -> Option<ObjectRef> {
        self.children.remove(name)
    }

    /// Clear all children
    pub fn clear_children(&mut self) {
        self.children.clear();
    }
}

/// HDF5 path utilities
pub struct PathUtils;

impl PathUtils {
    /// Normalize a path (remove trailing slashes, handle "..")
    pub fn normalize(path: &str) -> Result<String> {
        if path.is_empty() {
            return Err(Hdf5Error::InvalidPath("Empty path".to_string()));
        }

        // Root path
        if path == "/" {
            return Ok("/".to_string());
        }

        // Remove trailing slashes
        let path = path.trim_end_matches('/');

        // Split into components
        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        // Resolve ".." references
        let mut resolved = Vec::new();
        for component in components {
            match component {
                "." => continue,
                ".." => {
                    if resolved.is_empty() {
                        return Err(Hdf5Error::InvalidPath(format!(
                            "Path goes above root: {}",
                            path
                        )));
                    }
                    resolved.pop();
                }
                _ => resolved.push(component),
            }
        }

        if resolved.is_empty() {
            return Ok("/".to_string());
        }

        Ok(format!("/{}", resolved.join("/")))
    }

    /// Join two paths
    pub fn join(base: &str, relative: &str) -> Result<String> {
        if relative.starts_with('/') {
            // Absolute path, ignore base
            Self::normalize(relative)
        } else {
            // Relative path
            let combined = if base == "/" {
                format!("/{}", relative)
            } else {
                format!("{}/{}", base, relative)
            };
            Self::normalize(&combined)
        }
    }

    /// Get the parent path
    pub fn parent(path: &str) -> Result<String> {
        let normalized = Self::normalize(path)?;
        if normalized == "/" {
            return Err(Hdf5Error::InvalidPath("Root has no parent".to_string()));
        }

        let last_slash = normalized
            .rfind('/')
            .ok_or_else(|| Hdf5Error::InvalidPath(format!("Invalid path: {}", path)))?;

        if last_slash == 0 {
            Ok("/".to_string())
        } else {
            Ok(normalized[..last_slash].to_string())
        }
    }

    /// Get the basename (last component of path)
    pub fn basename(path: &str) -> Result<String> {
        let normalized = Self::normalize(path)?;
        if normalized == "/" {
            return Ok("/".to_string());
        }

        let last_slash = normalized
            .rfind('/')
            .ok_or_else(|| Hdf5Error::InvalidPath(format!("Invalid path: {}", path)))?;

        Ok(normalized[last_slash + 1..].to_string())
    }

    /// Split path into parent and basename
    pub fn split(path: &str) -> Result<(String, String)> {
        let normalized = Self::normalize(path)?;
        if normalized == "/" {
            return Ok(("/".to_string(), "/".to_string()));
        }

        let parent = Self::parent(&normalized)?;
        let basename = Self::basename(&normalized)?;
        Ok((parent, basename))
    }

    /// Validate a path
    pub fn validate(path: &str) -> Result<()> {
        if path.is_empty() {
            return Err(Hdf5Error::InvalidPath("Empty path".to_string()));
        }

        if !path.starts_with('/') {
            return Err(Hdf5Error::InvalidPath(format!(
                "Path must be absolute (start with /): {}",
                path
            )));
        }

        // Check for invalid characters
        for ch in path.chars() {
            if ch.is_control() {
                return Err(Hdf5Error::InvalidPath(format!(
                    "Path contains control character: {}",
                    path
                )));
            }
        }

        Ok(())
    }

    /// Check if a path is absolute
    pub fn is_absolute(path: &str) -> bool {
        path.starts_with('/')
    }

    /// Check if a path is relative
    pub fn is_relative(path: &str) -> bool {
        !Self::is_absolute(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::Attribute;

    #[test]
    fn test_object_ref() {
        let obj = ObjectRef::new(
            "dataset1".to_string(),
            ObjectType::Dataset,
            "/group1/dataset1".to_string(),
        );
        assert_eq!(obj.name(), "dataset1");
        assert_eq!(obj.object_type(), ObjectType::Dataset);
        assert_eq!(obj.path(), "/group1/dataset1");
        assert!(obj.is_dataset());
        assert!(!obj.is_group());
    }

    #[test]
    fn test_group_creation() {
        let group = Group::new("group1".to_string(), "/group1".to_string());
        assert_eq!(group.name(), "group1");
        assert_eq!(group.path(), "/group1");
        assert!(group.is_empty());
        assert_eq!(group.len(), 0);
    }

    #[test]
    fn test_group_root() {
        let root = Group::root();
        assert_eq!(root.name(), "/");
        assert_eq!(root.path(), "/");
    }

    #[test]
    fn test_group_children() {
        let mut group = Group::new("group1".to_string(), "/group1".to_string());

        let child1 = ObjectRef::new(
            "dataset1".to_string(),
            ObjectType::Dataset,
            "/group1/dataset1".to_string(),
        );
        let child2 = ObjectRef::new(
            "group2".to_string(),
            ObjectType::Group,
            "/group1/group2".to_string(),
        );

        group.add_child(child1);
        group.add_child(child2);

        assert_eq!(group.len(), 2);
        assert!(group.has_child("dataset1"));
        assert!(group.has_child("group2"));
        assert!(!group.has_child("nonexistent"));

        let mut names = group.child_names();
        names.sort();
        assert_eq!(names, vec!["dataset1", "group2"]);

        assert_eq!(group.datasets().count(), 1);
        assert_eq!(group.groups().count(), 1);
    }

    #[test]
    fn test_group_attributes() {
        let mut group = Group::new("group1".to_string(), "/group1".to_string());
        group
            .attributes_mut()
            .add(Attribute::string("description", "Test group"));

        assert!(group.attributes().contains("description"));
    }

    #[test]
    fn test_path_normalize() {
        assert_eq!(PathUtils::normalize("/").ok(), Some("/".to_string()));
        assert_eq!(
            PathUtils::normalize("/group1").ok(),
            Some("/group1".to_string())
        );
        assert_eq!(
            PathUtils::normalize("/group1/").ok(),
            Some("/group1".to_string())
        );
        assert_eq!(
            PathUtils::normalize("/group1//group2").ok(),
            Some("/group1/group2".to_string())
        );
        assert_eq!(
            PathUtils::normalize("/group1/./group2").ok(),
            Some("/group1/group2".to_string())
        );
        assert_eq!(
            PathUtils::normalize("/group1/group2/..").ok(),
            Some("/group1".to_string())
        );
    }

    #[test]
    fn test_path_join() {
        assert_eq!(
            PathUtils::join("/", "group1").ok(),
            Some("/group1".to_string())
        );
        assert_eq!(
            PathUtils::join("/group1", "group2").ok(),
            Some("/group1/group2".to_string())
        );
        assert_eq!(
            PathUtils::join("/group1", "/group2").ok(),
            Some("/group2".to_string())
        );
    }

    #[test]
    fn test_path_parent() {
        assert_eq!(
            PathUtils::parent("/group1/group2").ok(),
            Some("/group1".to_string())
        );
        assert_eq!(PathUtils::parent("/group1").ok(), Some("/".to_string()));
        assert!(PathUtils::parent("/").is_err());
    }

    #[test]
    fn test_path_basename() {
        assert_eq!(
            PathUtils::basename("/group1/dataset1").ok(),
            Some("dataset1".to_string())
        );
        assert_eq!(
            PathUtils::basename("/group1").ok(),
            Some("group1".to_string())
        );
        assert_eq!(PathUtils::basename("/").ok(), Some("/".to_string()));
    }

    #[test]
    fn test_path_split() {
        assert_eq!(
            PathUtils::split("/group1/dataset1").ok(),
            Some(("/group1".to_string(), "dataset1".to_string()))
        );
        assert_eq!(
            PathUtils::split("/group1").ok(),
            Some(("/".to_string(), "group1".to_string()))
        );
    }

    #[test]
    fn test_path_validate() {
        assert!(PathUtils::validate("/").is_ok());
        assert!(PathUtils::validate("/group1").is_ok());
        assert!(PathUtils::validate("").is_err());
        assert!(PathUtils::validate("group1").is_err());
    }

    #[test]
    fn test_path_absolute_relative() {
        assert!(PathUtils::is_absolute("/group1"));
        assert!(!PathUtils::is_absolute("group1"));
        assert!(PathUtils::is_relative("group1"));
        assert!(!PathUtils::is_relative("/group1"));
    }
}
