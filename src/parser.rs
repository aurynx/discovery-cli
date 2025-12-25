use crate::error::{AurynxError, Result};
use crate::metadata::{AttributeArgument, EnumCase, PhpClassMetadata};
use std::collections::HashMap;
use std::path::PathBuf;
use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator, Tree};
use tree_sitter_php::LANGUAGE_PHP;

pub struct PhpMetadataExtractor {
    parser: Parser,
    imports_query: Query,
}

impl PhpMetadataExtractor {
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();
        let language = LANGUAGE_PHP.into();
        parser.set_language(&language).map_err(|e| {
            AurynxError::tree_sitter_error(format!("Error loading PHP grammar: {e:?}"))
        })?;

        let imports_query = Query::new(
            &language,
            r"
            (namespace_definition name: (_) @namespace)
            (namespace_use_clause
              [
                (qualified_name)
                (name)
              ] @fqcn
              alias: (name)? @alias
            )
            ",
        )
        .map_err(|e| {
            AurynxError::tree_sitter_error(format!("Error compiling imports query: {e:?}"))
        })?;

        Ok(Self {
            parser,
            imports_query,
        })
    }

    /// Extract all class/interface/trait/enum metadata from PHP source code
    pub fn extract_metadata(
        &mut self, content: &str, file_path: PathBuf,
    ) -> Result<Vec<PhpClassMetadata>> {
        let tree = self
            .parser
            .parse(content, None)
            .ok_or_else(|| AurynxError::parse_error(file_path.clone(), "Error parsing PHP code"))?;

        let mut context = FileContext::new(content);
        self.extract_namespace_and_imports(&tree, &mut context)?;

        let metadata = self.extract_declarations(&tree, &context, file_path)?;

        Ok(metadata)
    }

    /// Extract namespace and use imports from the file
    fn extract_namespace_and_imports(&self, tree: &Tree, context: &mut FileContext) -> Result<()> {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(
            &self.imports_query,
            tree.root_node(),
            context.source.as_bytes(),
        );

        let namespace_idx = self
            .imports_query
            .capture_index_for_name("namespace")
            .ok_or_else(|| {
                AurynxError::tree_sitter_error("Missing 'namespace' capture in query")
            })?;
        let fqcn_idx = self
            .imports_query
            .capture_index_for_name("fqcn")
            .ok_or_else(|| AurynxError::tree_sitter_error("Missing 'fqcn' capture in query"))?;
        let alias_idx = self
            .imports_query
            .capture_index_for_name("alias")
            .ok_or_else(|| AurynxError::tree_sitter_error("Missing 'alias' capture in query"))?;

        while let Some(match_) = matches.next() {
            // Check if it's a namespace match
            if let Some(cap) = match_.captures.iter().find(|c| c.index == namespace_idx) {
                let ns = self.node_text(&cap.node, context.source);
                context.namespace = Some(ns);
                continue;
            }

            // Check if it's an import match
            if let Some(fqcn_cap) = match_.captures.iter().find(|c| c.index == fqcn_idx) {
                // Verify that fqcn_cap.node is NOT the alias field of its parent
                if let Some(parent) = fqcn_cap.node.parent()
                    && let Some(alias_node) = parent.child_by_field_name("alias")
                        && alias_node.id() == fqcn_cap.node.id() {
                            continue;
                        }

                let fqcn = self.node_text(&fqcn_cap.node, context.source);
                let alias = match_
                    .captures
                    .iter()
                    .find(|c| c.index == alias_idx).map_or_else(|| fqcn.split('\\').next_back().unwrap_or(&fqcn).to_string(), |c| self.node_text(&c.node, context.source));

                context.imports.insert(alias, self.normalize_fqcn(&fqcn));
            }
        }

        Ok(())
    }

    /// Extract all class/interface/trait/enum declarations
    fn extract_declarations(
        &self, tree: &Tree, context: &FileContext, file_path: PathBuf,
    ) -> Result<Vec<PhpClassMetadata>> {
        let mut declarations = Vec::new();
        let root = tree.root_node();

        self.walk_declarations(root, context, &file_path, &mut declarations)?;

        Ok(declarations)
    }

    /// Recursively walk the tree to find declarations
    fn walk_declarations(
        &self, node: Node, context: &FileContext, file_path: &PathBuf,
        declarations: &mut Vec<PhpClassMetadata>,
    ) -> Result<()> {
        match node.kind() {
            "class_declaration" => {
                if let Some(metadata) =
                    self.extract_class_metadata(node, context, file_path.clone(), "class")?
                {
                    declarations.push(metadata);
                }
            },
            "interface_declaration" => {
                if let Some(metadata) =
                    self.extract_class_metadata(node, context, file_path.clone(), "interface")?
                {
                    declarations.push(metadata);
                }
            },
            "trait_declaration" => {
                if let Some(metadata) =
                    self.extract_class_metadata(node, context, file_path.clone(), "trait")?
                {
                    declarations.push(metadata);
                }
            },
            "enum_declaration" => {
                if let Some(metadata) =
                    self.extract_class_metadata(node, context, file_path.clone(), "enum")?
                {
                    declarations.push(metadata);
                }
            },
            _ => {
                // Recursively check children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.walk_declarations(child, context, file_path, declarations)?;
                }
            },
        }

        Ok(())
    }

    /// Extract metadata for a single class/interface/trait/enum
    fn extract_class_metadata(
        &self, node: Node, context: &FileContext, file_path: PathBuf, kind: &str,
    ) -> Result<Option<PhpClassMetadata>> {
        // Get class name
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return Ok(None),
        };

        let class_name = self.node_text(&name_node, context.source);
        let fqcn = context.resolve_fqcn(&class_name);

        let mut metadata = PhpClassMetadata::new(fqcn, file_path, kind.to_string());

        // Extract class modifiers (abstract, final, readonly)
        self.extract_class_modifiers(&node, &mut metadata);

        // Extract attributes - look for attribute_list child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "attribute_list" {
                // attribute_list contains attribute_group nodes
                let mut attr_cursor = child.walk();
                for attr_group in child.children(&mut attr_cursor) {
                    if attr_group.kind() == "attribute_group" {
                        self.extract_attributes_from_group(&attr_group, context, &mut metadata)?;
                    }
                }
            }
        }

        // Extract extends (for classes and interfaces)
        if kind == "class" || kind == "interface" {
            // Look for base_clause - try both as field and as child
            let mut base_clause_opt = node.child_by_field_name("base_clause");

            if base_clause_opt.is_none() {
                let mut cursor = node.walk();
                base_clause_opt = node
                    .children(&mut cursor)
                    .find(|n| n.kind() == "base_clause");
            }

            if let Some(base_clause) = base_clause_opt {
                // base_clause contains the parent class name
                let mut base_cursor = base_clause.walk();
                for child in base_clause.children(&mut base_cursor) {
                    if child.kind() == "name" || child.kind() == "qualified_name" {
                        let parent_name = self.node_text(&child, context.source);
                        metadata.extends = Some(context.resolve_fqcn(&parent_name));
                        break;
                    }
                }
            }
        }

        // Extract implements (for classes and enums)
        if kind == "class" || kind == "enum" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "class_interface_clause" {
                    metadata.implements = self.extract_interface_list(&child, context)?;
                    break;
                }
            }
        }

        // Extract methods (for classes, interfaces, traits, enums)
        if kind == "class" || kind == "interface" || kind == "trait" || kind == "enum" {
            self.extract_methods(&node, context, &mut metadata)?;
        }

        // Extract properties (for classes, traits, enums)
        if kind == "class" || kind == "trait" || kind == "enum" {
            self.extract_properties(&node, context, &mut metadata)?;
        }

        // Extract enum cases (only for enums)
        if kind == "enum" {
            self.extract_enum_cases(&node, context, &mut metadata)?;
        }

        Ok(Some(metadata))
    }

    /// Extract attributes from an `attribute_group` node
    fn extract_attributes_from_group(
        &self, group_node: &Node, context: &FileContext, metadata: &mut PhpClassMetadata,
    ) -> Result<()> {
        let mut cursor = group_node.walk();
        for child in group_node.children(&mut cursor) {
            if child.kind() == "attribute" {
                self.extract_single_attribute(&child, context, metadata)?;
            }
        }
        Ok(())
    }

    /// Extract a single attribute with its arguments
    fn extract_single_attribute(
        &self, attr_node: &Node, context: &FileContext, metadata: &mut PhpClassMetadata,
    ) -> Result<()> {
        // Try to get attribute name from field or from first child
        let attr_name = if let Some(name_node) = attr_node.child_by_field_name("name") {
            self.node_text(&name_node, context.source)
        } else {
            // Fall back to looking for name/qualified_name child
            let mut cursor = attr_node.walk();
            let mut name_str = String::new();
            for child in attr_node.children(&mut cursor) {
                if child.kind() == "name" || child.kind() == "qualified_name" {
                    name_str = self.node_text(&child, context.source);
                    break;
                }
            }
            if name_str.is_empty() {
                return Ok(());
            }
            name_str
        };

        let attr_fqcn = context.resolve_fqcn(&attr_name);

        // Extract arguments if present
        let arguments = self.extract_attribute_arguments(attr_node, context)?;

        metadata
            .attributes
            .entry(attr_fqcn)
            .or_default()
            .push(arguments);

        Ok(())
    }

    /// Extract attribute arguments
    fn extract_attribute_arguments(
        &self, attr_node: &Node, context: &FileContext,
    ) -> Result<Vec<AttributeArgument>> {
        let mut arguments = Vec::new();

        // Find 'arguments' node within attribute
        let mut attr_cursor = attr_node.walk();
        let args_node = attr_node
            .children(&mut attr_cursor)
            .find(|child| child.kind() == "arguments");

        let args_node = match args_node {
            Some(node) => node,
            None => return Ok(arguments), // No arguments
        };

        let mut cursor = args_node.walk();

        for child in args_node.children(&mut cursor) {
            if child.kind() == "argument" {
                // Check if it's a named argument (name: value)
                let mut has_name = false;
                let mut arg_name = String::new();
                let mut arg_value = String::new();

                let mut arg_cursor = child.walk();
                for arg_child in child.children(&mut arg_cursor) {
                    if arg_child.kind() == "name" && arg_name.is_empty() {
                        arg_name = self.node_text(&arg_child, context.source);
                        has_name = true;
                    } else if arg_child.kind() == ":" {
                        // Named argument separator
                        continue;
                    } else if !arg_child.kind().starts_with('(')
                        && !arg_child.kind().starts_with(')')
                        && arg_child.kind() != ","
                        && arg_child.kind() != "argument"
                    {
                        // This is the value
                        arg_value = self.resolve_argument_value(&arg_child, context)?;
                    }
                }

                if !arg_value.is_empty() {
                    if has_name && !arg_name.is_empty() {
                        arguments.push(AttributeArgument::Named {
                            key: arg_name,
                            value: arg_value,
                        });
                    } else {
                        arguments.push(AttributeArgument::Positional(arg_value));
                    }
                }
            }
        }

        Ok(arguments)
    }

    /// Resolve an argument value, converting class references to FQCN
    fn resolve_argument_value(&self, node: &Node, context: &FileContext) -> Result<String> {
        // Handle different node types
        match node.kind() {
            // Class constant reference: Status::ACTIVE
            "class_constant_access_expression" => {
                let value_text = self.node_text(node, context.source);
                Ok(context.resolve_constant_reference(&value_text))
            },
            // String literals, numbers, etc. - return as-is
            "string" | "integer" | "float" | "boolean" => {
                Ok(self.node_text(node, context.source))
            },
            // Encapsed strings might contain constants
            "encapsed_string" => {
                let value_text = self.node_text(node, context.source);
                Ok(self.resolve_constants_in_text(&value_text, context))
            },
            // For arrays, recursively process constants inside
            "array" => {
                let value_text = self.node_text(node, context.source);
                Ok(self.resolve_constants_in_text(&value_text, context))
            },
            // For other expressions (arrays, object creation, etc.), return text as-is
            _ => {
                let value_text = self.node_text(node, context.source);

                // Only try to resolve if it looks like a simple class reference
                if value_text.ends_with("::class")
                    && !value_text.contains('[')
                    && !value_text.contains('(')
                {
                    let class_name = value_text.trim_end_matches("::class");
                    let resolved_class = context.resolve_fqcn(class_name);
                    return Ok(format!("{resolved_class}::class"));
                }

                // Try to resolve constants in the text (handles complex expressions)
                Ok(self.resolve_constants_in_text(&value_text, context))
            },
        }
    }

    /// Recursively resolve class constants in text (e.g., `Status::PENDING` inside arrays)
    fn resolve_constants_in_text(&self, text: &str, context: &FileContext) -> String {
        // Use regex-like approach with a simple state machine
        // Find patterns like "\\ClassName::CONSTANT" or "ClassName::CONSTANT"
        let mut result = String::new();
        let mut i = 0;
        let chars: Vec<char> = text.chars().collect();

        while i < chars.len() {
            // Try to find a potential class name starting at position i
            let mut j = i;
            let mut found_double_colon = false;

            // Collect characters for a potential class name (including namespace separators)
            while j < chars.len() {
                if chars[j].is_alphanumeric() || chars[j] == '_' {
                    j += 1;
                } else if chars[j] == '\\'
                    && j + 1 < chars.len()
                    && (chars[j + 1].is_alphabetic() || chars[j + 1] == '_')
                {
                    j += 1; // Skip the backslash, next char is alphanumeric
                } else if j + 1 < chars.len() && chars[j] == ':' && chars[j + 1] == ':' {
                    found_double_colon = true;
                    j += 2; // Skip ::
                    break;
                } else {
                    break;
                }
            }

            if found_double_colon && j > i + 2 {
                // We found "Something::" - now get the constant name
                let class_part: String = chars[i..j - 2].iter().collect();
                let mut const_part = String::new();

                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    const_part.push(chars[j]);
                    j += 1;
                }

                if !const_part.is_empty() {
                    let const_ref = format!("{class_part}::{const_part}");
                    let resolved = context.resolve_constant_reference(&const_ref);
                    result.push_str(&resolved);
                    i = j;
                    continue;
                }
            }

            // No constant found at position i, just add the character
            if i < chars.len() {
                result.push(chars[i]);
                i += 1;
            }
        }

        result
    }

    /// Extract list of interfaces
    fn extract_interface_list(&self, node: &Node, context: &FileContext) -> Result<Vec<String>> {
        let mut interfaces = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "name" || child.kind() == "qualified_name" {
                let interface_name = self.node_text(&child, context.source);
                interfaces.push(context.resolve_fqcn(&interface_name));
            }
        }

        Ok(interfaces)
    }

    /// Get text content of a node
    fn node_text(&self, node: &Node, source: &str) -> String {
        node.utf8_text(source.as_bytes()).unwrap_or("").to_string()
    }

    /// Normalize FQCN to ensure it starts with backslash
    fn normalize_fqcn(&self, name: &str) -> String {
        if name.starts_with('\\') {
            name.to_string()
        } else {
            format!("\\{name}")
        }
    }

    /// Extract class modifiers (abstract, final, readonly)
    fn extract_class_modifiers(&self, node: &Node, metadata: &mut PhpClassMetadata) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "abstract_modifier" {
                metadata.modifiers.is_abstract = true;
            } else if child.kind() == "final_modifier" {
                metadata.modifiers.is_final = true;
            } else if child.kind() == "readonly_modifier" {
                metadata.modifiers.is_readonly = true;
            }
        }
    }

    /// Extract methods from a class declaration
    fn extract_methods(
        &self, node: &Node, context: &FileContext, metadata: &mut PhpClassMetadata,
    ) -> Result<()> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // Handle both declaration_list (class/interface/trait) and enum_declaration_list (enum)
            if child.kind() == "declaration_list" || child.kind() == "enum_declaration_list" {
                let mut decl_cursor = child.walk();
                for decl_child in child.children(&mut decl_cursor) {
                    if decl_child.kind() == "method_declaration"
                        && let Some(method) = self.extract_method(&decl_child, context)? {
                            metadata.methods.push(method);
                        }
                }
                break;
            }
        }
        Ok(())
    }

    /// Extract a single method
    fn extract_method(
        &self, node: &Node, context: &FileContext,
    ) -> Result<Option<crate::metadata::PhpMethodMetadata>> {
        use crate::metadata::{MethodModifiers, PhpMethodMetadata};

        // Get method name
        let name = match node.child_by_field_name("name") {
            Some(name_node) => self.node_text(&name_node, context.source),
            None => return Ok(None),
        };

        // Extract visibility and modifiers
        let mut visibility = "public".to_string();
        let mut modifiers = MethodModifiers::default();
        let mut attributes: HashMap<String, Vec<Vec<AttributeArgument>>> = HashMap::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "visibility_modifier" => {
                    let vis_text = self.node_text(&child, context.source);
                    if !vis_text.is_empty() {
                        visibility = vis_text;
                    }
                },
                "static_modifier" => modifiers.is_static = true,
                "abstract_modifier" => modifiers.is_abstract = true,
                "final_modifier" => modifiers.is_final = true,
                "attribute_list" => {
                    // Extract method attributes
                    let mut attr_cursor = child.walk();
                    for attr_group in child.children(&mut attr_cursor) {
                        if attr_group.kind() == "attribute_group" {
                            self.extract_method_attributes(&attr_group, context, &mut attributes)?;
                        }
                    }
                },
                _ => {},
            }
        }

        // Extract parameters
        let parameters = self.extract_parameters(node, context)?;

        // Extract return type
        let return_type = if let Some(rt_node) = node.child_by_field_name("return_type") {
            // return_type might have children, find the actual type
            let mut rt_cursor = rt_node.walk();
            let mut found_type = None;
            for rt_child in rt_node.children(&mut rt_cursor) {
                if rt_child.kind() != ":" && rt_child.kind() != "?" {
                    let type_text = self.node_text(&rt_child, context.source);
                    if !type_text.is_empty() {
                        found_type = Some(context.resolve_fqcn(&type_text));
                        break;
                    }
                }
            }

            // If no child type found, check if the node itself is the type
            if found_type.is_none() {
                let type_text = self.node_text(&rt_node, context.source);
                if !type_text.is_empty() {
                    found_type = Some(context.resolve_fqcn(&type_text));
                }
            }

            found_type
        } else {
            // Fallback: look for type nodes after parameters
            let mut cursor = node.walk();
            let mut found_type = None;
            let mut seen_params = false;
            for child in node.children(&mut cursor) {
                if child.kind() == "formal_parameters" {
                    seen_params = true;
                } else if seen_params
                    && (child.kind() == "primitive_type"
                        || child.kind() == "named_type"
                        || child.kind() == "union_type"
                        || child.kind() == "intersection_type"
                        || child.kind() == "optional_type")
                {
                    let type_text = self.node_text(&child, context.source);
                    found_type = Some(context.resolve_fqcn(&type_text));
                    break;
                }
            }
            found_type
        };

        Ok(Some(PhpMethodMetadata {
            name,
            visibility,
            modifiers,
            attributes,
            parameters,
            return_type,
        }))
    }

    /// Extract properties from a class/trait/enum declaration
    fn extract_properties(
        &self, node: &Node, context: &FileContext, metadata: &mut PhpClassMetadata,
    ) -> Result<()> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "declaration_list" {
                let mut decl_cursor = child.walk();
                for decl_child in child.children(&mut decl_cursor) {
                    if decl_child.kind() == "property_declaration"
                        && let Some(properties) =
                            self.extract_property_declaration(&decl_child, context)?
                        {
                            metadata.properties.extend(properties);
                        }
                }
                break;
            }
        }
        Ok(())
    }

    /// Extract property declaration (can contain multiple properties)
    fn extract_property_declaration(
        &self, node: &Node, context: &FileContext,
    ) -> Result<Option<Vec<crate::metadata::PhpPropertyMetadata>>> {
        use crate::metadata::PropertyModifiers;

        let mut properties = Vec::new();

        // Extract visibility
        let mut visibility = "public".to_string();
        let mut modifiers = PropertyModifiers::default();
        let mut attributes: HashMap<String, Vec<Vec<AttributeArgument>>> = HashMap::new();
        let mut type_hint: Option<String> = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "visibility_modifier" => {
                    visibility = self.node_text(&child, context.source);
                },
                "static_modifier" => modifiers.is_static = true,
                "readonly_modifier" => modifiers.is_readonly = true,
                "attribute_list" => {
                    // Extract property attributes
                    let mut attr_cursor = child.walk();
                    for attr_group in child.children(&mut attr_cursor) {
                        if attr_group.kind() == "attribute_group" {
                            self.extract_method_attributes(&attr_group, context, &mut attributes)?;
                        }
                    }
                },
                "union_type" | "intersection_type" | "primitive_type" | "optional_type"
                | "named_type" => {
                    let type_text = self.node_text(&child, context.source);
                    type_hint = Some(context.resolve_fqcn(&type_text));
                },
                "property_element" => {
                    // Extract individual property from property_element
                    if let Some(prop) = self.extract_single_property(
                        &child,
                        context,
                        &visibility,
                        &modifiers,
                        &attributes,
                        &type_hint,
                    )? {
                        properties.push(prop);
                    }
                },
                _ => {},
            }
        }

        if properties.is_empty() {
            Ok(None)
        } else {
            Ok(Some(properties))
        }
    }

    /// Extract a single property element
    fn extract_single_property(
        &self, node: &Node, context: &FileContext, visibility: &str,
        modifiers: &crate::metadata::PropertyModifiers,
        attributes: &HashMap<String, Vec<Vec<AttributeArgument>>>, type_hint: &Option<String>,
    ) -> Result<Option<crate::metadata::PhpPropertyMetadata>> {
        // Get property name from variable_name child
        let name = if let Some(var_name_node) = node.child_by_field_name("name") {
            let text = self.node_text(&var_name_node, context.source);
            // Remove $ prefix
            text.trim_start_matches('$').to_string()
        } else {
            // Try to find variable_name child
            let mut cursor = node.walk();
            let mut found_name = None;
            for child in node.children(&mut cursor) {
                if child.kind() == "variable_name" {
                    let text = self.node_text(&child, context.source);
                    found_name = Some(text.trim_start_matches('$').to_string());
                    break;
                }
            }
            match found_name {
                Some(name) => name,
                None => return Ok(None),
            }
        };

        // Extract default value - look for property_initializer
        let default_value: Result<Option<String>> = {
            let mut cursor = node.walk();
            let mut found_default = None;
            let mut found_equals = false;

            // First try property_initializer
            for child in node.children(&mut cursor) {
                if child.kind() == "property_initializer" {
                    // Get the value after '='
                    let mut init_cursor = child.walk();
                    for init_child in child.children(&mut init_cursor) {
                        if init_child.kind() != "=" {
                            found_default =
                                Some(self.resolve_argument_value(&init_child, context)?);
                            break;
                        }
                    }
                    break;
                }
            }

            if found_default.is_none() {
                // Fallback: look for = and value directly in property_element
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "=" {
                        found_equals = true;
                    } else if found_equals {
                        found_default = Some(self.resolve_argument_value(&child, context)?);
                        break;
                    }
                }
            }

            Ok(found_default)
        };
        let default_value = default_value?;

        Ok(Some(crate::metadata::PhpPropertyMetadata {
            name,
            visibility: visibility.to_string(),
            modifiers: modifiers.clone(),
            type_hint: type_hint.clone(),
            default_value,
            attributes: attributes.clone(),
        }))
    }

    /// Extract enum cases from an enum declaration
    fn extract_enum_cases(
        &self, node: &Node, context: &FileContext, metadata: &mut PhpClassMetadata,
    ) -> Result<()> {
        // First, extract backing type if it's a backed enum
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "primitive_type" {
                // This is the backing type (string or int)
                let backing = self.node_text(&child, context.source);
                metadata.backing_type = Some(backing);
                break;
            }
        }

        // Now extract enum cases from enum_declaration_list
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "enum_declaration_list" {
                let mut decl_cursor = child.walk();
                for decl_child in child.children(&mut decl_cursor) {
                    if decl_child.kind() == "enum_case"
                        && let Some(case) = self.extract_enum_case(&decl_child, context)? {
                            metadata.cases.push(case);
                        }
                }
                break;
            }
        }
        Ok(())
    }

    /// Extract a single enum case
    fn extract_enum_case(&self, node: &Node, context: &FileContext) -> Result<Option<EnumCase>> {
        // Get case name
        let name = match node.child_by_field_name("name") {
            Some(n) => self.node_text(&n, context.source),
            None => return Ok(None),
        };

        // Extract value for backed enums
        let value = if let Some(value_node) = node
            .children(&mut node.walk())
            .find(|n| n.kind() == "string" || n.kind() == "integer" || n.kind() == "float")
        {
            let value_text = self.node_text(&value_node, context.source);
            // Remove quotes if it's a string literal
            Some(
                if (value_text.starts_with('"') && value_text.ends_with('"'))
                    || (value_text.starts_with('\'') && value_text.ends_with('\''))
                {
                    value_text[1..value_text.len() - 1].to_string()
                } else {
                    value_text
                },
            )
        } else {
            None
        };

        // Extract attributes
        let mut attributes = HashMap::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "attribute_list" {
                let mut attr_cursor = child.walk();
                for attr_group in child.children(&mut attr_cursor) {
                    if attr_group.kind() == "attribute_group" {
                        self.extract_case_attributes(&attr_group, context, &mut attributes)?;
                    }
                }
            }
        }

        Ok(Some(EnumCase {
            name,
            value,
            attributes,
        }))
    }

    /// Extract attributes for an enum case
    fn extract_case_attributes(
        &self, group_node: &Node, context: &FileContext,
        attributes: &mut HashMap<String, Vec<Vec<AttributeArgument>>>,
    ) -> Result<()> {
        let mut cursor = group_node.walk();
        for child in group_node.children(&mut cursor) {
            if child.kind() == "attribute" {
                self.extract_attribute_to_map(&child, context, attributes)?;
            }
        }
        Ok(())
    }

    /// Extract method attributes
    fn extract_method_attributes(
        &self, group_node: &Node, context: &FileContext,
        attributes: &mut HashMap<String, Vec<Vec<AttributeArgument>>>,
    ) -> Result<()> {
        let mut cursor = group_node.walk();
        for child in group_node.children(&mut cursor) {
            if child.kind() == "attribute" {
                self.extract_attribute_to_map(&child, context, attributes)?;
            }
        }
        Ok(())
    }

    /// Extract attribute to a `HashMap`
    fn extract_attribute_to_map(
        &self, attr_node: &Node, context: &FileContext,
        attributes: &mut HashMap<String, Vec<Vec<AttributeArgument>>>,
    ) -> Result<()> {
        // Try field first, then find by child kind
        let mut cursor = attr_node.walk();
        let name_node = if let Some(n) = attr_node.child_by_field_name("name") {
            n
        } else {
            // Try finding 'name' or 'qualified_name' child
            let mut found_node = None;
            for child in attr_node.children(&mut cursor) {
                if child.kind() == "name" || child.kind() == "qualified_name" {
                    found_node = Some(child);
                    break;
                }
            }
            match found_node {
                Some(n) => n,
                None => return Ok(()),
            }
        };

        let attr_name = self.node_text(&name_node, context.source);
        let fqcn = context.resolve_fqcn(&attr_name);
        let arguments = self.extract_attribute_arguments(attr_node, context)?;

        attributes.entry(fqcn).or_default().push(arguments);
        Ok(())
    }

    /// Extract parameters from method
    fn extract_parameters(
        &self, node: &Node, context: &FileContext,
    ) -> Result<Vec<crate::metadata::PhpParameterMetadata>> {
        let mut parameters = Vec::new();

        // Find formal_parameters node
        let params_node = match node.child_by_field_name("parameters") {
            Some(p) => p,
            None => return Ok(parameters),
        };

        let mut cursor = params_node.walk();
        for child in params_node.children(&mut cursor) {
            if (child.kind() == "simple_parameter" || child.kind() == "property_promotion_parameter")
                && let Some(param) = self.extract_single_parameter(&child, context)? {
                    parameters.push(param);
                }
        }

        Ok(parameters)
    }

    /// Extract a single parameter
    fn extract_single_parameter(
        &self, node: &Node, context: &FileContext,
    ) -> Result<Option<crate::metadata::PhpParameterMetadata>> {
        // Get parameter name
        let name = match node.child_by_field_name("name") {
            Some(name_node) => {
                let text = self.node_text(&name_node, context.source);
                // Remove $ prefix
                text.trim_start_matches('$').to_string()
            },
            None => return Ok(None),
        };

        // Extract type hint
        let type_hint = node.child_by_field_name("type").map(|type_node| {
            let type_text = self.node_text(&type_node, context.source);
            context.resolve_fqcn(&type_text)
        });

        // Extract default value
        let default_value = node
            .child_by_field_name("default_value")
            .map(|default_node| self.resolve_argument_value(&default_node, context))
            .transpose()?;

        // Extract parameter attributes
        let mut attributes: HashMap<String, Vec<Vec<AttributeArgument>>> = HashMap::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "attribute_list" {
                let mut attr_cursor = child.walk();
                for attr_group in child.children(&mut attr_cursor) {
                    if attr_group.kind() == "attribute_group" {
                        self.extract_method_attributes(&attr_group, context, &mut attributes)?;
                    }
                }
            }
        }

        Ok(Some(crate::metadata::PhpParameterMetadata {
            name,
            type_hint,
            default_value,
            attributes,
        }))
    }
}

/// Context for a single PHP file (namespace, imports)
struct FileContext<'a> {
    source: &'a str,
    namespace: Option<String>,
    imports: HashMap<String, String>,
}

impl<'a> FileContext<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            namespace: None,
            imports: HashMap::new(),
        }
    }

    /// Resolve a class name to its FQCN based on namespace and imports
    fn resolve_fqcn(&self, name: &str) -> String {
        // Already fully qualified
        if name.starts_with('\\') {
            return name.to_string();
        }

        // Built-in types should not be resolved
        let builtin_types = [
            "int", "float", "string", "bool", "array", "object", "callable", "iterable", "void",
            "never", "mixed", "null", "true", "false", "self", "parent", "static",
        ];

        if builtin_types.contains(&name.to_lowercase().as_str()) {
            return name.to_lowercase();
        }

        // Check if it's an imported alias
        let first_part = name.split('\\').next().unwrap_or(name);
        if let Some(imported) = self.imports.get(first_part) {
            if name == first_part {
                return imported.clone();
            } else {
                // Replace first part with imported FQCN
                let rest = &name[first_part.len()..];
                return format!("{imported}{rest}");
            }
        }

        // Use current namespace
        if let Some(ns) = &self.namespace {
            format!("\\{ns}\\{name}")
        } else {
            format!("\\{name}")
        }
    }

    /// Resolve constant reference (`ClassName::CONSTANT`) to FQCN
    /// Example: `UserStatus::ACTIVE` -> \`App\Enum\UserStatus::ACTIVE`
    fn resolve_constant_reference(&self, value: &str) -> String {
        // If value doesn't contain "::", return as-is
        if !value.contains("::") {
            return value.to_string();
        }

        // Split by "::" to separate class and constant parts
        let parts: Vec<&str> = value.splitn(2, "::").collect();
        if parts.len() != 2 {
            return value.to_string();
        }

        let class_name = parts[0];
        let constant_name = parts[1];

        // Resolve the class name part to FQCN
        let resolved_class = self.resolve_fqcn(class_name);

        // Reassemble as FQCN::CONSTANT
        format!("{resolved_class}::{constant_name}")
    }
}

// Keep the old API for backward compatibility during migration
pub struct AttributeChecker {
    pub query: Arc<Query>,
}

use std::sync::Arc;

impl AttributeChecker {
    pub fn new() -> Result<Self> {
        let query = Query::new(&LANGUAGE_PHP.into(), "(attribute_group) @attr").map_err(|e| {
            AurynxError::tree_sitter_error(format!("Error compiling query: {e:?}"))
        })?;
        Ok(Self {
            query: Arc::new(query),
        })
    }
}

pub struct ThreadLocalParser {
    parser: Parser,
    cursor: QueryCursor,
    query: Arc<Query>,
}

impl ThreadLocalParser {
    pub fn new(query: Arc<Query>) -> Result<Self> {
        let mut parser = Parser::new();
        parser.set_language(&LANGUAGE_PHP.into()).map_err(|e| {
            AurynxError::tree_sitter_error(format!("Error loading PHP grammar: {e:?}"))
        })?;
        let cursor = QueryCursor::new();

        Ok(Self {
            parser,
            cursor,
            query,
        })
    }

    pub fn has_attributes(&mut self, content: &str) -> Result<bool> {
        let tree = self
            .parser
            .parse(content, None)
            .ok_or_else(|| AurynxError::other("Error parsing code"))?;

        let mut matches = self
            .cursor
            .matches(&self.query, tree.root_node(), content.as_bytes());

        // Check if there's at least one match
        Ok(matches.next().is_some())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn test_extract_simple_class() {
        let code = r#"<?php
namespace App\Entity;

class User {}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/User.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Entity\\User");
        assert_eq!(metadata[0].kind, "class");
    }

    #[test]
    fn test_extract_class_with_namespace_and_imports() {
        let code = r#"<?php
namespace App\Entity;

use Doctrine\ORM\Mapping as ORM;
use App\Trait\Timestampable;

#[ORM\Entity]
class User {
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/User.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Entity\\User");
        assert_eq!(metadata[0].kind, "class");
        assert!(
            metadata[0]
                .attributes
                .contains_key("\\Doctrine\\ORM\\Mapping\\Entity")
        );
    }

    #[test]
    fn test_extract_class_with_extends() {
        let code = r#"<?php
namespace App\Entity;

use App\Base\BaseEntity;

class User extends BaseEntity {
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/User.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(
            metadata[0].extends,
            Some("\\App\\Base\\BaseEntity".to_string())
        );
    }

    #[test]
    fn test_extract_class_with_implements() {
        let code = r#"<?php
namespace App\Entity;

class User implements \JsonSerializable, \Stringable {
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/User.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].implements.len(), 2);
        assert!(
            metadata[0]
                .implements
                .contains(&"\\JsonSerializable".to_string())
        );
        assert!(metadata[0].implements.contains(&"\\Stringable".to_string()));
    }

    #[test]
    fn test_extract_interface() {
        let code = r#"<?php
namespace App\Contract;

interface Timestampable {
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Timestampable.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Contract\\Timestampable");
        assert_eq!(metadata[0].kind, "interface");
    }

    #[test]
    fn test_extract_trait() {
        let code = r#"<?php
namespace App\Trait;

trait Loggable {
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Loggable.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Trait\\Loggable");
        assert_eq!(metadata[0].kind, "trait");
    }

    #[test]
    fn test_extract_enum() {
        let code = r#"<?php
namespace App\Enum;

enum Status {
    case ACTIVE;
    case INACTIVE;
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Status.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Enum\\Status");
        assert_eq!(metadata[0].kind, "enum");
    }

    #[test]
    fn test_extract_multiple_classes_in_one_file() {
        let code = r#"<?php
namespace App\Entity;

class User {}
class Admin {}
interface Manageable {}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Multi.php"))
            .unwrap();

        assert_eq!(metadata.len(), 3);

        let fqcns: Vec<String> = metadata.iter().map(|m| m.fqcn.clone()).collect();
        assert!(fqcns.contains(&"\\App\\Entity\\User".to_string()));
        assert!(fqcns.contains(&"\\App\\Entity\\Admin".to_string()));
        assert!(fqcns.contains(&"\\App\\Entity\\Manageable".to_string()));
    }

    #[test]
    fn test_extract_attribute_with_arguments() {
        let code = r#"<?php
namespace App\Entity;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Table(name: 'users')]
class User {}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/User.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let table_attr = metadata[0]
            .attributes
            .get("\\Doctrine\\ORM\\Mapping\\Table");
        assert!(table_attr.is_some());
    }

    #[test]
    fn test_class_without_namespace() {
        let code = r#"<?php

class User {}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/User.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\User");
    }

    // Keep old tests for backward compatibility
    #[test]
    fn test_detects_simple_attribute() {
        let code = "<?php #[Attribute] class Foo {}";
        let checker = AttributeChecker::new().unwrap();
        let mut parser = ThreadLocalParser::new(checker.query.clone()).unwrap();
        assert!(parser.has_attributes(code).unwrap());
    }

    #[test]
    fn test_detects_multiline_attribute() {
        let code = "<?php
        #[
            Route('/path')
        ]
        class Foo {}";
        let checker = AttributeChecker::new().unwrap();
        let mut parser = ThreadLocalParser::new(checker.query.clone()).unwrap();
        assert!(parser.has_attributes(code).unwrap());
    }

    #[test]
    fn test_ignores_comments() {
        let code = "<?php
        // #[Attribute]
        /* #[Attribute] */
        class Foo {}";
        let checker = AttributeChecker::new().unwrap();
        let mut parser = ThreadLocalParser::new(checker.query.clone()).unwrap();
        assert!(!parser.has_attributes(code).unwrap());
    }

    #[test]
    fn test_ignores_strings() {
        let code = "<?php
        class Foo {
            public string $x = '#[Attribute]';
        }";
        let checker = AttributeChecker::new().unwrap();
        let mut parser = ThreadLocalParser::new(checker.query.clone()).unwrap();
        assert!(!parser.has_attributes(code).unwrap());
    }

    #[test]
    fn test_detects_multiple_attributes() {
        let code = "<?php #[Route] #[Auth] class Foo {}";
        let checker = AttributeChecker::new().unwrap();
        let mut parser = ThreadLocalParser::new(checker.query.clone()).unwrap();
        assert!(parser.has_attributes(code).unwrap());
    }

    // Tests for method metadata extraction
    #[test]
    fn test_extract_method_with_visibility() {
        let code = r#"<?php
namespace App;

class Test {
    public function publicMethod() {}
    protected function protectedMethod() {}
    private function privateMethod() {}
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let methods = &metadata[0].methods;
        assert_eq!(methods.len(), 3);

        assert_eq!(methods[0].name, "publicMethod");
        assert_eq!(methods[0].visibility, "public");

        assert_eq!(methods[1].name, "protectedMethod");
        assert_eq!(methods[1].visibility, "protected");

        assert_eq!(methods[2].name, "privateMethod");
        assert_eq!(methods[2].visibility, "private");
    }

    #[test]
    fn test_extract_method_modifiers() {
        let code = r#"<?php
namespace App;

abstract class Test {
    abstract public function abstractMethod();
    final public function finalMethod() {}
    public static function staticMethod() {}
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let methods = &metadata[0].methods;
        assert_eq!(methods.len(), 3);

        assert!(methods[0].modifiers.is_abstract);
        assert!(!methods[0].modifiers.is_final);
        assert!(!methods[0].modifiers.is_static);

        assert!(!methods[1].modifiers.is_abstract);
        assert!(methods[1].modifiers.is_final);
        assert!(!methods[1].modifiers.is_static);

        assert!(!methods[2].modifiers.is_abstract);
        assert!(!methods[2].modifiers.is_final);
        assert!(methods[2].modifiers.is_static);
    }

    #[test]
    fn test_extract_class_modifiers() {
        let code = r#"<?php
namespace App;

abstract class AbstractClass {}
final class FinalClass {}
readonly class ReadonlyClass {}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 3);

        assert!(metadata[0].modifiers.is_abstract);
        assert!(!metadata[0].modifiers.is_final);
        assert!(!metadata[0].modifiers.is_readonly);

        assert!(!metadata[1].modifiers.is_abstract);
        assert!(metadata[1].modifiers.is_final);
        assert!(!metadata[1].modifiers.is_readonly);

        assert!(!metadata[2].modifiers.is_abstract);
        assert!(!metadata[2].modifiers.is_final);
        assert!(metadata[2].modifiers.is_readonly);
    }

    #[test]
    fn test_extract_method_attributes() {
        let code = r#"<?php
namespace App;

use App\Attribute\Route;

class Test {
    #[Route('/test')]
    public function testMethod() {}
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let methods = &metadata[0].methods;
        assert_eq!(methods.len(), 1);

        let method = &methods[0];
        assert_eq!(method.attributes.len(), 1);
        assert!(method.attributes.contains_key("\\App\\Attribute\\Route"));
    }

    #[test]
    fn test_extract_method_with_parameters() {
        let code = r#"<?php
namespace App;

class Test {
    public function withParams(int $id, string $name = 'default') {}
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let methods = &metadata[0].methods;
        assert_eq!(methods.len(), 1);

        let method = &methods[0];
        assert_eq!(method.parameters.len(), 2);

        assert_eq!(method.parameters[0].name, "id");
        assert_eq!(method.parameters[0].type_hint, Some("int".to_string()));
        assert_eq!(method.parameters[0].default_value, None);

        assert_eq!(method.parameters[1].name, "name");
        assert_eq!(method.parameters[1].type_hint, Some("string".to_string()));
        assert!(method.parameters[1].default_value.is_some());
    }

    #[test]
    fn test_extract_method_return_type() {
        let code = r#"<?php
namespace App;

class Test {
    public function noReturn() {}
    public function voidReturn(): void {}
    public function arrayReturn(): array {}
    public function selfReturn(): self {}
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let methods = &metadata[0].methods;
        assert_eq!(methods.len(), 4);

        assert_eq!(methods[0].return_type, None);
        assert_eq!(methods[1].return_type, Some("void".to_string()));
        assert_eq!(methods[2].return_type, Some("array".to_string()));
        assert_eq!(methods[3].return_type, Some("self".to_string()));
    }

    #[test]
    fn test_extract_parameter_attributes() {
        let code = r#"<?php
namespace App;

use App\Attribute\Inject;

class Test {
    public function __construct(#[Inject] string $service) {}
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let methods = &metadata[0].methods;
        assert_eq!(methods.len(), 1);

        let method = &methods[0];
        assert_eq!(method.parameters.len(), 1);

        let param = &method.parameters[0];
        assert_eq!(param.attributes.len(), 1);
        assert!(param.attributes.contains_key("\\App\\Attribute\\Inject"));
    }

    #[test]
    fn test_extract_multiple_method_attributes() {
        let code = r#"<?php
namespace App;

use App\Attribute\Route;
use App\Attribute\Cache;

class Test {
    #[Route('/test')]
    #[Cache(ttl: 300)]
    public function multiAttr() {}
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let methods = &metadata[0].methods;
        assert_eq!(methods.len(), 1);

        let method = &methods[0];
        assert_eq!(method.attributes.len(), 2);
        assert!(method.attributes.contains_key("\\App\\Attribute\\Route"));
        assert!(method.attributes.contains_key("\\App\\Attribute\\Cache"));
    }

    #[test]
    fn test_builtin_types_not_resolved_as_fqcn() {
        let code = r#"<?php
namespace App\Controller;

class Test {
    public function test(int $id, array $data, string $name): bool {
        return true;
    }
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let methods = &metadata[0].methods;
        assert_eq!(methods.len(), 1);

        let method = &methods[0];

        // Check parameter types are lowercase built-in types
        assert_eq!(method.parameters[0].type_hint, Some("int".to_string()));
        assert_eq!(method.parameters[1].type_hint, Some("array".to_string()));
        assert_eq!(method.parameters[2].type_hint, Some("string".to_string()));

        // Check return type is lowercase built-in type
        assert_eq!(method.return_type, Some("bool".to_string()));
    }

    // Tests for property metadata extraction
    #[test]
    fn test_extract_simple_properties() {
        let code = r#"<?php
namespace App;

class Test {
    public int $id;
    private string $name;
    protected array $data;
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let properties = &metadata[0].properties;
        assert_eq!(properties.len(), 3);

        assert_eq!(properties[0].name, "id");
        assert_eq!(properties[0].visibility, "public");
        assert_eq!(properties[0].type_hint, Some("int".to_string()));

        assert_eq!(properties[1].name, "name");
        assert_eq!(properties[1].visibility, "private");
        assert_eq!(properties[1].type_hint, Some("string".to_string()));

        assert_eq!(properties[2].name, "data");
        assert_eq!(properties[2].visibility, "protected");
        assert_eq!(properties[2].type_hint, Some("array".to_string()));
    }

    #[test]
    fn test_extract_property_with_default() {
        let code = r#"<?php
namespace App;

class Test {
    public int $count = 0;
    public string $status = 'active';
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let properties = &metadata[0].properties;
        assert_eq!(properties.len(), 2);

        assert_eq!(properties[0].name, "count");
        assert_eq!(properties[0].default_value, Some("0".to_string()));

        assert_eq!(properties[1].name, "status");
        assert_eq!(properties[1].default_value, Some("'active'".to_string()));
    }

    #[test]
    fn test_extract_property_modifiers() {
        let code = r#"<?php
namespace App;

class Test {
    public static int $counter = 0;
    public readonly string $immutable;
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let properties = &metadata[0].properties;
        assert_eq!(properties.len(), 2);

        assert_eq!(properties[0].name, "counter");
        assert!(properties[0].modifiers.is_static);
        assert!(!properties[0].modifiers.is_readonly);

        assert_eq!(properties[1].name, "immutable");
        assert!(!properties[1].modifiers.is_static);
        assert!(properties[1].modifiers.is_readonly);
    }

    #[test]
    fn test_extract_property_attributes() {
        let code = r#"<?php
namespace App;

use Doctrine\ORM\Mapping\Column;

class Test {
    #[Column(type: 'integer')]
    private int $id;
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let properties = &metadata[0].properties;
        assert_eq!(properties.len(), 1);

        let property = &properties[0];
        assert_eq!(property.name, "id");
        assert_eq!(property.attributes.len(), 1);
        assert!(
            property
                .attributes
                .contains_key("\\Doctrine\\ORM\\Mapping\\Column")
        );
    }

    #[test]
    fn test_extract_multiple_properties_one_line() {
        let code = r#"<?php
namespace App;

class Test {
    public int $x, $y, $z;
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Test.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let properties = &metadata[0].properties;
        assert_eq!(properties.len(), 3);

        assert_eq!(properties[0].name, "x");
        assert_eq!(properties[1].name, "y");
        assert_eq!(properties[2].name, "z");

        // All should have same type and visibility
        for prop in properties {
            assert_eq!(prop.visibility, "public");
            assert_eq!(prop.type_hint, Some("int".to_string()));
        }
    }

    #[test]
    fn test_property_type_resolution() {
        let code = r#"<?php
namespace App\Entity;

use App\ValueObject\Email;

class User {
    private Email $email;
    private int $age;
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/User.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        let properties = &metadata[0].properties;
        assert_eq!(properties.len(), 2);

        // Custom class should be resolved to FQCN
        assert_eq!(
            properties[0].type_hint,
            Some("\\App\\ValueObject\\Email".to_string())
        );

        // Built-in type should be lowercase
        assert_eq!(properties[1].type_hint, Some("int".to_string()));
    }

    #[test]
    fn test_simple_enum() {
        let code = r#"<?php
namespace App\Enum;

enum Priority
{
    case LOW;
    case MEDIUM;
    case HIGH;
    case URGENT;
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Priority.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Enum\\Priority");
        assert_eq!(metadata[0].kind, "enum");
        assert_eq!(metadata[0].backing_type, None);
        assert_eq!(metadata[0].cases.len(), 4);

        assert_eq!(metadata[0].cases[0].name, "LOW");
        assert_eq!(metadata[0].cases[0].value, None);
        assert_eq!(metadata[0].cases[1].name, "MEDIUM");
        assert_eq!(metadata[0].cases[2].name, "HIGH");
        assert_eq!(metadata[0].cases[3].name, "URGENT");
    }

    #[test]
    fn test_backed_enum_string() {
        let code = r#"<?php
namespace App\Enum;

enum Status: string
{
    case PENDING = 'pending';
    case ACTIVE = 'active';
    case COMPLETED = 'completed';
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Status.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Enum\\Status");
        assert_eq!(metadata[0].kind, "enum");
        assert_eq!(metadata[0].backing_type, Some("string".to_string()));
        assert_eq!(metadata[0].cases.len(), 3);

        assert_eq!(metadata[0].cases[0].name, "PENDING");
        assert_eq!(metadata[0].cases[0].value, Some("pending".to_string()));
        assert_eq!(metadata[0].cases[1].name, "ACTIVE");
        assert_eq!(metadata[0].cases[1].value, Some("active".to_string()));
        assert_eq!(metadata[0].cases[2].name, "COMPLETED");
        assert_eq!(metadata[0].cases[2].value, Some("completed".to_string()));
    }

    #[test]
    fn test_backed_enum_int() {
        let code = r#"<?php
namespace App\Enum;

enum HttpCode: int
{
    case OK = 200;
    case NOT_FOUND = 404;
    case INTERNAL_ERROR = 500;
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/HttpCode.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Enum\\HttpCode");
        assert_eq!(metadata[0].kind, "enum");
        assert_eq!(metadata[0].backing_type, Some("int".to_string()));
        assert_eq!(metadata[0].cases.len(), 3);

        assert_eq!(metadata[0].cases[0].name, "OK");
        assert_eq!(metadata[0].cases[0].value, Some("200".to_string()));
        assert_eq!(metadata[0].cases[1].name, "NOT_FOUND");
        assert_eq!(metadata[0].cases[1].value, Some("404".to_string()));
        assert_eq!(metadata[0].cases[2].name, "INTERNAL_ERROR");
        assert_eq!(metadata[0].cases[2].value, Some("500".to_string()));
    }

    #[test]
    fn test_enum_with_attributes() {
        let code = r#"<?php
namespace App\Enum;

use App\Attribute\Description;

#[Description('User role definitions')]
enum UserRole: string
{
    #[Description('Administrator')]
    case ADMIN = 'admin';

    #[Description('Regular user')]
    case USER = 'user';
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/UserRole.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Enum\\UserRole");

        // Check enum-level attribute
        assert!(
            metadata[0]
                .attributes
                .contains_key("\\App\\Attribute\\Description")
        );

        // Check case-level attributes
        assert_eq!(metadata[0].cases.len(), 2);
        assert!(
            metadata[0].cases[0]
                .attributes
                .contains_key("\\App\\Attribute\\Description")
        );
        assert!(
            metadata[0].cases[1]
                .attributes
                .contains_key("\\App\\Attribute\\Description")
        );
    }

    #[test]
    fn test_class_attribute_with_arguments() {
        let code = r#"<?php
namespace App\Controller;

use App\Attribute\Route;

#[Route('/api/users', methods: ['GET', 'POST'])]
class UserController
{
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/UserController.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Controller\\UserController");

        let route_attr = metadata[0]
            .attributes
            .get("\\App\\Attribute\\Route")
            .expect("Route attribute not found");

        assert_eq!(route_attr.len(), 1);
        let args = &route_attr[0];
        assert_eq!(args.len(), 2);

        // Check first argument (positional)
        match &args[0] {
            AttributeArgument::Positional(val) => assert_eq!(val, "'/api/users'"),
            _ => panic!("Expected positional argument"),
        }

        // Check second argument (named)
        match &args[1] {
            AttributeArgument::Named { key, value } => {
                assert_eq!(key, "methods");
                // The value might be formatted differently depending on how array is extracted,
                // but based on previous output it seems to be "['GET', 'POST']"
                assert!(value.contains("'GET'"));
                assert!(value.contains("'POST'"));
            },
            _ => panic!("Expected named argument"),
        }
    }

    #[test]
    fn test_enum_with_methods() {
        let code = r#"<?php
namespace App\Enum;

enum Color: string
{
    case RED = 'red';
    case GREEN = 'green';
    case BLUE = 'blue';

    public function label(): string
    {
        return match($this) {
            self::RED => 'Red Color',
            self::GREEN => 'Green Color',
            self::BLUE => 'Blue Color',
        };
    }

    public function hexCode(): string
    {
        return match($this) {
            self::RED => '#FF0000',
            self::GREEN => '#00FF00',
            self::BLUE => '#0000FF',
        };
    }
}
"#;
        let mut extractor = PhpMetadataExtractor::new().unwrap();
        let metadata = extractor
            .extract_metadata(code, PathBuf::from("/test/Color.php"))
            .unwrap();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].fqcn, "\\App\\Enum\\Color");
        assert_eq!(metadata[0].kind, "enum");

        // Enum should have cases
        assert_eq!(metadata[0].cases.len(), 3);
        assert_eq!(metadata[0].cases[0].name, "RED");
        assert_eq!(metadata[0].cases[1].name, "GREEN");
        assert_eq!(metadata[0].cases[2].name, "BLUE");

        // Enum should extract methods
        assert_eq!(metadata[0].methods.len(), 2);
        assert_eq!(metadata[0].methods[0].name, "label");
        assert_eq!(
            metadata[0].methods[0].return_type,
            Some("string".to_string())
        );
        assert_eq!(metadata[0].methods[1].name, "hexCode");
        assert_eq!(
            metadata[0].methods[1].return_type,
            Some("string".to_string())
        );
    }
}
