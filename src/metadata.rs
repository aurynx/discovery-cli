use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Represents metadata for a single PHP class/interface/trait/enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhpClassMetadata {
    /// Fully Qualified Class Name (e.g., "App\\Entities\\User")
    pub fqcn: String,
    /// Absolute path to the file containing this class
    pub file: PathBuf,
    /// Type of the definition: 'class', 'interface', 'trait', or 'enum'
    #[serde(rename = "type")]
    pub kind: String,
    /// Class modifiers
    pub modifiers: ClassModifiers,
    /// Attributes applied to this class/interface/trait/enum
    /// Key: FQCN of the attribute (e.g., "Doctrine\\ORM\\Mapping\\Entity")
    /// Value: List of argument lists (one list of arguments per attribute instance)
    pub attributes: HashMap<String, Vec<Vec<AttributeArgument>>>,
    /// Parent class FQCN, if any (only for classes)
    pub extends: Option<String>,
    /// List of implemented interface FQCNs
    pub implements: Vec<String>,
    /// Methods of this class
    pub methods: Vec<PhpMethodMetadata>,
    /// Properties of this class
    pub properties: Vec<PhpPropertyMetadata>,
    /// Enum backing type (for backed enums: 'string' or 'int')
    pub backing_type: Option<String>,
    /// Enum cases (only for enums)
    pub cases: Vec<EnumCase>,
}

/// Class modifiers (abstract, final, readonly)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ClassModifiers {
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
}

/// Represents metadata for a single method
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhpMethodMetadata {
    /// Method name
    pub name: String,
    /// Visibility: public, protected, private
    pub visibility: String,
    /// Method modifiers
    pub modifiers: MethodModifiers,
    /// Attributes applied to this method
    pub attributes: HashMap<String, Vec<Vec<AttributeArgument>>>,
    /// Method parameters
    pub parameters: Vec<PhpParameterMetadata>,
    /// Return type hint, if any
    pub return_type: Option<String>,
}

/// Method modifiers (abstract, final, static)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MethodModifiers {
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_static: bool,
}

/// Represents a method parameter
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhpParameterMetadata {
    /// Parameter name (without $)
    pub name: String,
    /// Type hint, if any
    pub type_hint: Option<String>,
    /// Default value, if any
    pub default_value: Option<String>,
    /// Attributes applied to this parameter
    pub attributes: HashMap<String, Vec<Vec<AttributeArgument>>>,
}

/// Represents a class property
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhpPropertyMetadata {
    /// Property name (without $)
    pub name: String,
    /// Visibility: public, protected, private
    pub visibility: String,
    /// Property modifiers
    pub modifiers: PropertyModifiers,
    /// Type hint, if any
    pub type_hint: Option<String>,
    /// Default value, if any
    pub default_value: Option<String>,
    /// Attributes applied to this property
    pub attributes: HashMap<String, Vec<Vec<AttributeArgument>>>,
}

/// Property modifiers (static, readonly)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PropertyModifiers {
    pub is_static: bool,
    pub is_readonly: bool,
}

/// Represents a single enum case
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnumCase {
    /// Case name
    pub name: String,
    /// Backed value for backed enums (string or int)
    pub value: Option<String>,
    /// Attributes applied to this enum case
    pub attributes: HashMap<String, Vec<Vec<AttributeArgument>>>,
}

/// Represents a single argument in an attribute
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum AttributeArgument {
    /// Named argument: key => value
    Named { key: String, value: String },
    /// Positional argument: just value
    Positional(String),
}

impl PhpClassMetadata {
    #[must_use] 
    pub fn new(fqcn: String, file: PathBuf, kind: String) -> Self {
        Self {
            fqcn,
            file,
            kind,
            modifiers: ClassModifiers::default(),
            attributes: HashMap::new(),
            extends: None,
            implements: Vec::new(),
            methods: Vec::new(),
            properties: Vec::new(),
            backing_type: None,
            cases: Vec::new(),
        }
    }
}
