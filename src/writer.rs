use crate::metadata::{AttributeArgument, PhpClassMetadata};
use anyhow::Result;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn write_php_cache(
    metadata_list: &[PhpClassMetadata],
    output_path: &Path,
    pretty: bool,
) -> Result<()> {
    // Ensure directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(output_path)?;
    let mut writer = PhpFormatter::new(file, pretty);

    writer.writeln("<?php")?;
    if pretty {
        writer.writeln("")?;
    } else {
        writer.write(" ")?;
    }
    writer.writeln("declare(strict_types=1);")?;
    if pretty {
        writer.writeln("")?;
    }

    writer.write("return ")?;
    writer.array_start()?;

    let metadata_count = metadata_list.len();
    for (i, metadata) in metadata_list.iter().enumerate() {
        let is_last = i == metadata_count - 1;
        let fqcn = escape_php_string(&metadata.fqcn);

        writer.write_indent()?;
        writer.write("'")?;
        writer.write(&fqcn)?;
        writer.write("'")?;
        writer.write_arrow()?;
        writer.array_start()?;

        // File path
        let file_path = metadata.file.to_string_lossy();
        let escaped_path = escape_php_string(&file_path);
        writer.key_value_string("file", &escaped_path, false)?;

        // Type
        writer.key_value_string("type", &metadata.kind, false)?;

        // Modifiers
        writer.key_array_start("modifiers")?;
        writer.key_value_bool("abstract", metadata.modifiers.is_abstract, false)?;
        writer.key_value_bool("final", metadata.modifiers.is_final, false)?;
        writer.key_value_bool("readonly", metadata.modifiers.is_readonly, true)?;
        writer.array_end(true)?;

        // Attributes
        writer.write_attributes(&metadata.attributes, false)?;

        // Extends
        if let Some(parent) = &metadata.extends {
            let escaped_parent = escape_php_string(parent);
            writer.key_value_string("extends", &escaped_parent, false)?;
        } else {
            writer.key_value_null("extends", false)?;
        }

        // Implements
        if metadata.implements.is_empty() {
            writer.key_array_empty("implements", false)?;
        } else {
            writer.key_array_start("implements")?;
            let impl_count = metadata.implements.len();
            for (j, interface) in metadata.implements.iter().enumerate() {
                let is_last_impl = j == impl_count - 1;
                let escaped_interface = escape_php_string(interface);
                writer.write_indent()?;
                writer.write("'")?;
                writer.write(&escaped_interface)?;
                writer.write("'")?;
                writer.write_comma_newline(is_last_impl)?;
            }
            writer.array_end(true)?;
        }

        // Methods
        if metadata.methods.is_empty() {
            writer.key_array_empty("methods", false)?;
        } else {
            writer.key_array_start("methods")?;
            let method_count = metadata.methods.len();
            for (j, method) in metadata.methods.iter().enumerate() {
                let is_last_method = j == method_count - 1;
                let escaped_method_name = escape_php_string(&method.name);
                writer.write_indent()?;
                writer.write("'")?;
                writer.write(&escaped_method_name)?;
                writer.write("'")?;
                writer.write_arrow()?;
                writer.array_start()?;

                // Visibility
                writer.key_value_string("visibility", &method.visibility, false)?;

                // Modifiers
                writer.key_array_start("modifiers")?;
                writer.key_value_bool("abstract", method.modifiers.is_abstract, false)?;
                writer.key_value_bool("final", method.modifiers.is_final, false)?;
                writer.key_value_bool("static", method.modifiers.is_static, true)?;
                writer.array_end(true)?;

                // Attributes
                writer.write_attributes(&method.attributes, false)?;

                // Parameters
                if method.parameters.is_empty() {
                    writer.key_array_empty("parameters", false)?;
                } else {
                    writer.key_array_start("parameters")?;
                    let param_count = method.parameters.len();
                    for (k, param) in method.parameters.iter().enumerate() {
                        let is_last_param = k == param_count - 1;
                        let escaped_param_name = escape_php_string(&param.name);
                        writer.write_indent()?;
                        writer.write("'")?;
                        writer.write(&escaped_param_name)?;
                        writer.write("'")?;
                        writer.write_arrow()?;
                        writer.array_start()?;

                        // Type hint
                        if let Some(type_hint) = &param.type_hint {
                            let escaped_type = escape_php_string(type_hint);
                            writer.key_value_string("type", &escaped_type, false)?;
                        } else {
                            writer.key_value_null("type", false)?;
                        }

                        // Default value
                        if let Some(default) = &param.default_value {
                            let formatted_default = format_php_value(default);
                            writer.key_value_raw("default", &formatted_default, false)?;
                        } else {
                            writer.key_value_null("default", false)?;
                        }

                        // Parameter attributes
                        writer.write_attributes(&param.attributes, true)?;

                        writer.array_end(pretty || !is_last_param)?;
                    }
                    writer.array_end(true)?;
                }

                // Return type
                if let Some(return_type) = &method.return_type {
                    let escaped_return = escape_php_string(return_type);
                    writer.key_value_string("return_type", &escaped_return, true)?;
                } else {
                    writer.key_value_null("return_type", true)?;
                }

                writer.array_end(pretty || !is_last_method)?;
            }
            writer.array_end(true)?;
        }

        // Properties
        if metadata.properties.is_empty() {
            writer.key_array_empty("properties", metadata.kind != "enum")?;
        } else {
            writer.key_array_start("properties")?;
            let prop_count = metadata.properties.len();
            for (j, property) in metadata.properties.iter().enumerate() {
                let is_last_prop = j == prop_count - 1;
                let escaped_name = escape_php_string(&property.name);
                writer.write_indent()?;
                writer.write("'")?;
                writer.write(&escaped_name)?;
                writer.write("'")?;
                writer.write_arrow()?;
                writer.array_start()?;

                // Visibility
                writer.key_value_string("visibility", &property.visibility, false)?;

                // Modifiers
                writer.key_array_start("modifiers")?;
                writer.key_value_bool("static", property.modifiers.is_static, false)?;
                writer.key_value_bool("readonly", property.modifiers.is_readonly, true)?;
                writer.array_end(true)?;

                // Type
                if let Some(type_hint) = &property.type_hint {
                    let escaped_type = escape_php_string(type_hint);
                    writer.key_value_string("type", &escaped_type, false)?;
                } else {
                    writer.key_value_null("type", false)?;
                }

                // Default value
                if let Some(default) = &property.default_value {
                    let formatted_default = format_php_value(default);
                    writer.key_value_raw("default", &formatted_default, false)?;
                } else {
                    writer.key_value_null("default", false)?;
                }

                // Attributes
                writer.write_attributes(&property.attributes, true)?;

                writer.array_end(pretty || !is_last_prop)?;
            }
            writer.array_end(pretty || metadata.kind == "enum")?;
        }

        // Enum backing type (only for enums)
        if metadata.kind == "enum" {
            if let Some(backing_type) = &metadata.backing_type {
                let escaped_type = escape_php_string(backing_type);
                writer.key_value_string("backing_type", &escaped_type, false)?;
            } else {
                writer.key_value_null("backing_type", false)?;
            }
        }

        // Enum cases (only for enums)
        if metadata.kind == "enum" {
            if metadata.cases.is_empty() {
                writer.key_array_empty("cases", true)?;
            } else {
                writer.key_array_start("cases")?;
                let case_count = metadata.cases.len();
                for (j, case) in metadata.cases.iter().enumerate() {
                    let is_last_case = j == case_count - 1;
                    let escaped_case_name = escape_php_string(&case.name);
                    writer.write_indent()?;
                    writer.write("'")?;
                    writer.write(&escaped_case_name)?;
                    writer.write("'")?;
                    writer.write_arrow()?;
                    writer.array_start()?;

                    // Case value (for backed enums)
                    if let Some(value) = &case.value {
                        let formatted_value = format_php_value(value);
                        writer.key_value_raw("value", &formatted_value, false)?;
                    } else {
                        writer.key_value_null("value", false)?;
                    }

                    // Case attributes
                    writer.write_attributes(&case.attributes, true)?;

                    writer.array_end(pretty || !is_last_case)?;
                }
                writer.array_end(pretty)?;
            }
        }

        writer.array_end(pretty || !is_last)?;
    }

    writer.write("];")?;
    if pretty {
        writer.writeln("")?;
    }

    Ok(())
}

struct PhpFormatter<W: Write> {
    writer: W,
    pretty: bool,
    indent: usize,
}

impl<W: Write> PhpFormatter<W> {
    const fn new(writer: W, pretty: bool) -> Self {
        Self {
            writer,
            pretty,
            indent: 0,
        }
    }

    const fn indent(&mut self) {
        self.indent += 1;
    }

    const fn dedent(&mut self) {
        if self.indent > 0 {
            self.indent -= 1;
        }
    }

    fn write_indent(&mut self) -> std::io::Result<()> {
        if self.pretty {
            for _ in 0..self.indent {
                write!(self.writer, "    ")?;
            }
        }
        Ok(())
    }

    fn write(&mut self, s: &str) -> std::io::Result<()> {
        write!(self.writer, "{s}")
    }

    fn writeln(&mut self, s: &str) -> std::io::Result<()> {
        if self.pretty {
            writeln!(self.writer, "{s}")
        } else {
            write!(self.writer, "{s}")
        }
    }

    fn write_arrow(&mut self) -> std::io::Result<()> {
        if self.pretty {
            write!(self.writer, " => ")
        } else {
            write!(self.writer, "=>")
        }
    }

    fn array_start(&mut self) -> std::io::Result<()> {
        self.write("[")?;
        if self.pretty {
            self.writeln("")?;
            self.indent();
        }
        Ok(())
    }

    fn array_end(&mut self, trailing_comma: bool) -> std::io::Result<()> {
        if self.pretty {
            self.dedent();
            self.write_indent()?;
        }
        self.write("]")?;
        if trailing_comma {
            self.write(",")?;
        }
        if self.pretty {
            self.writeln("")?;
        }
        Ok(())
    }

    fn key_array_start(&mut self, key: &str) -> std::io::Result<()> {
        self.write_indent()?;
        self.write("'")?;
        self.write(key)?;
        self.write("'")?;
        self.write_arrow()?;
        self.array_start()
    }

    fn write_attributes(
        &mut self,
        attributes: &std::collections::HashMap<String, Vec<Vec<AttributeArgument>>>,
        is_last_block: bool,
    ) -> std::io::Result<()> {
        if attributes.is_empty() {
            return self.key_array_empty("attributes", is_last_block);
        }

        self.key_array_start("attributes")?;
        let attr_count = attributes.len();
        for (j, (attr_name, instances)) in attributes.iter().enumerate() {
            let is_last_attr = j == attr_count - 1;
            let escaped_attr = escape_php_string(attr_name);
            self.write_indent()?;
            self.write("'")?;
            self.write(&escaped_attr)?;
            self.write("'")?;
            self.write_arrow()?;

            self.array_start()?; // Start list of instances
            let instance_count = instances.len();
            for (k, args) in instances.iter().enumerate() {
                let is_last_instance = k == instance_count - 1;

                if args.is_empty() {
                    self.write("[]")?;
                } else {
                    self.array_start()?; // Start arguments
                    let arg_count = args.len();
                    for (l, arg) in args.iter().enumerate() {
                        let is_last_arg = l == arg_count - 1;
                        match arg {
                            AttributeArgument::Named { key, value } => {
                                let escaped_key = escape_php_string(key);
                                let formatted_value = format_php_value(value);
                                self.key_value_raw(&escaped_key, &formatted_value, is_last_arg)?;
                            }
                            AttributeArgument::Positional(value) => {
                                let formatted_value = format_php_value(value);
                                self.write_indent()?;
                                self.write(&formatted_value)?;
                                self.write_comma_newline(is_last_arg)?;
                            }
                        }
                    }
                    self.array_end(self.pretty || !is_last_instance)?;
                }
                self.write_comma_newline(is_last_instance)?;
            }
            self.array_end(self.pretty || !is_last_attr)?;
        }
        self.array_end(self.pretty || !is_last_block)
    }

    fn key_array_empty(&mut self, key: &str, is_last: bool) -> std::io::Result<()> {
        self.write_indent()?;
        self.write("'")?;
        self.write(key)?;
        self.write("'")?;
        self.write_arrow()?;
        self.write("[]")?;
        self.write_comma_newline(is_last)
    }

    fn write_comma_newline(&mut self, is_last: bool) -> std::io::Result<()> {
        if self.pretty || !is_last {
            self.write(",")?;
        }
        if self.pretty {
            self.writeln("")?;
        }
        Ok(())
    }

    fn key_value_string(&mut self, key: &str, value: &str, is_last: bool) -> std::io::Result<()> {
        self.write_indent()?;
        self.write("'")?;
        self.write(key)?;
        self.write("'")?;
        self.write_arrow()?;
        self.write("'")?;
        self.write(value)?;
        self.write("'")?;
        self.write_comma_newline(is_last)
    }

    fn key_value_bool(&mut self, key: &str, value: bool, is_last: bool) -> std::io::Result<()> {
        self.write_indent()?;
        self.write("'")?;
        self.write(key)?;
        self.write("'")?;
        self.write_arrow()?;
        self.write(if value { "true" } else { "false" })?;
        self.write_comma_newline(is_last)
    }

    fn key_value_null(&mut self, key: &str, is_last: bool) -> std::io::Result<()> {
        self.write_indent()?;
        self.write("'")?;
        self.write(key)?;
        self.write("'")?;
        self.write_arrow()?;
        self.write("null")?;
        self.write_comma_newline(is_last)
    }

    fn key_value_raw(&mut self, key: &str, value: &str, is_last: bool) -> std::io::Result<()> {
        self.write_indent()?;
        self.write("'")?;
        self.write(key)?;
        self.write("'")?;
        self.write_arrow()?;
        self.write(value)?;
        self.write_comma_newline(is_last)
    }
}

/// Escape a string for use in single-quoted PHP string
fn escape_php_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Format a value for PHP output
fn format_php_value(value: &str) -> String {
    let trimmed = value.trim();

    // Check if it's an array with 'new' expressions - these should be strings
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        if trimmed.contains("new ") {
            return format!("'{}'", escape_php_string(value));
        }
        return value.to_string();
    }

    // Check if it's a 'new' expression - these should be strings
    if trimmed.starts_with("new ") {
        return format!("'{}'", escape_php_string(value));
    }

    // Check if it's a class constant reference (e.g., \App\Enum\Status::ACTIVE)
    if value.contains("::") && !value.ends_with("::class") {
        // Make sure it's a simple constant reference, not a new expression
        if !value.contains('(') && !value.contains("new ") {
            // It's already resolved to FQCN, return as-is
            return value.to_string();
        }
    }

    // Check if it's ::class reference
    if value.ends_with("::class") && !value.contains('(') && !value.contains("new ") {
        return value.to_string();
    }

    // Check if it's a number
    if value.parse::<f64>().is_ok() {
        return value.to_string();
    }

    // Check if it's a boolean
    if value == "true" || value == "false" {
        return value.to_string();
    }

    // Check if it's null
    if value == "null" {
        return value.to_string();
    }

    // Otherwise, treat as string
    // If it's already quoted, keep it as-is
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        return value.to_string();
    }

    // Escape and quote
    format!("'{}'", escape_php_string(value))
}

pub fn write_json_cache(
    metadata_list: &[PhpClassMetadata],
    output_path: &Path,
    pretty: bool,
) -> Result<()> {
    // Ensure directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(output_path)?;
    if pretty {
        serde_json::to_writer_pretty(file, metadata_list)?;
    } else {
        serde_json::to_writer(file, metadata_list)?;
    }

    Ok(())
}
