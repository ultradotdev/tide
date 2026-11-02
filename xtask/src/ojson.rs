//! Order-preserving JSON with JavaScript `JSON.stringify` formatting.
//!
//! Generated references and the site package must keep the key order of their
//! inputs. `serde_json::Value` sorts keys unless `preserve_order` is enabled,
//! and enabling it here would change map ordering for every workspace crate.
use anyhow::{Result, bail, ensure};
use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(text: &str) -> Result<Self> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            text,
            position: 0,
        };
        parser.whitespace();
        let value = parser.value(0)?;
        parser.whitespace();
        ensure!(
            parser.position == parser.bytes.len(),
            "Trailing JSON content at byte {}",
            parser.position
        );
        Ok(value)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .rev()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// Follows a path of object keys.
    pub fn at(&self, path: &[&str]) -> Option<&Json> {
        path.iter().try_fold(self, |value, key| value.get(key))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> &[Json] {
        match self {
            Self::Array(items) => items,
            _ => &[],
        }
    }

    pub fn entries(&self) -> &[(String, Json)] {
        match self {
            Self::Object(entries) => entries,
            _ => &[],
        }
    }

    pub fn str_at(&self, path: &[&str]) -> &str {
        self.at(path).and_then(Json::as_str).unwrap_or_default()
    }

    /// Sets a key in place, or appends it, like assigning a JavaScript property.
    pub fn set(&mut self, key: &str, value: Json) {
        if let Self::Object(entries) = self {
            if let Some(entry) = entries.iter_mut().find(|(name, _)| name == key) {
                entry.1 = value;
            } else {
                entries.push((key.to_owned(), value));
            }
        }
    }

    /// `JSON.stringify(value, null, 2)`.
    pub fn pretty(&self) -> String {
        let mut output = String::new();
        self.write(&mut output, Some(2), 0);
        output
    }

    /// JavaScript `String(value)` for template literals.
    pub fn display(&self) -> String {
        match self {
            Self::Null => "null".to_owned(),
            Self::Bool(value) => value.to_string(),
            Self::Number(number) => format_number(*number),
            Self::String(text) => text.clone(),
            Self::Array(items) => items
                .iter()
                .map(|item| match item {
                    Self::Null => String::new(),
                    other => other.display(),
                })
                .collect::<Vec<_>>()
                .join(","),
            Self::Object(_) => "[object Object]".to_owned(),
        }
    }

    fn write(&self, output: &mut String, indent: Option<usize>, depth: usize) {
        match self {
            Self::Null => output.push_str("null"),
            Self::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Self::Number(number) => output.push_str(&format_number(*number)),
            Self::String(text) => write_string(output, text),
            Self::Array(items) => {
                if items.is_empty() {
                    output.push_str("[]");
                    return;
                }
                output.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    newline(output, indent, depth + 1);
                    item.write(output, indent, depth + 1);
                }
                newline(output, indent, depth);
                output.push(']');
            }
            Self::Object(entries) => {
                if entries.is_empty() {
                    output.push_str("{}");
                    return;
                }
                output.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    newline(output, indent, depth + 1);
                    write_string(output, key);
                    output.push(':');
                    if indent.is_some() {
                        output.push(' ');
                    }
                    value.write(output, indent, depth + 1);
                }
                newline(output, indent, depth);
                output.push('}');
            }
        }
    }
}

fn newline(output: &mut String, indent: Option<usize>, depth: usize) {
    if let Some(width) = indent {
        output.push('\n');
        output.push_str(&" ".repeat(width * depth));
    }
}

fn write_string(output: &mut String, text: &str) {
    output.push('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(output, "\\u{:04x}", u32::from(control));
            }
            other => output.push(other),
        }
    }
    output.push('"');
}

/// JavaScript `Number.prototype.toString` for the finite values JSON carries.
pub fn format_number(number: f64) -> String {
    if number == 0.0 {
        return "0".to_owned();
    }
    if number.fract() == 0.0 && number.abs() < 1e21 {
        return format!("{number:.0}");
    }
    format!("{number}")
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    position: usize,
}

impl Parser<'_> {
    fn whitespace(&mut self) {
        while self
            .bytes
            .get(self.position)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.position += 1;
        }
    }

    fn expect(&mut self, literal: &str) -> Result<()> {
        ensure!(
            self.text[self.position..].starts_with(literal),
            "Expected {literal} at byte {}",
            self.position
        );
        self.position += literal.len();
        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<Json> {
        ensure!(depth < 256, "JSON nesting is too deep");
        match self.bytes.get(self.position) {
            Some(b'n') => self.expect("null").map(|()| Json::Null),
            Some(b't') => self.expect("true").map(|()| Json::Bool(true)),
            Some(b'f') => self.expect("false").map(|()| Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => {
                self.position += 1;
                let mut items = Vec::new();
                self.whitespace();
                if self.bytes.get(self.position) == Some(&b']') {
                    self.position += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    self.whitespace();
                    items.push(self.value(depth + 1)?);
                    self.whitespace();
                    match self.bytes.get(self.position) {
                        Some(b',') => self.position += 1,
                        Some(b']') => {
                            self.position += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => bail!("Expected , or ] at byte {}", self.position),
                    }
                }
            }
            Some(b'{') => {
                self.position += 1;
                let mut entries: Vec<(String, Json)> = Vec::new();
                self.whitespace();
                if self.bytes.get(self.position) == Some(&b'}') {
                    self.position += 1;
                    return Ok(Json::Object(entries));
                }
                loop {
                    self.whitespace();
                    ensure!(
                        self.bytes.get(self.position) == Some(&b'"'),
                        "Expected object key at byte {}",
                        self.position
                    );
                    let key = self.string()?;
                    self.whitespace();
                    self.expect(":")?;
                    self.whitespace();
                    let value = self.value(depth + 1)?;
                    // JSON.parse keeps the first position and the last value.
                    if let Some(entry) = entries.iter_mut().find(|(name, _)| *name == key) {
                        entry.1 = value;
                    } else {
                        entries.push((key, value));
                    }
                    self.whitespace();
                    match self.bytes.get(self.position) {
                        Some(b',') => self.position += 1,
                        Some(b'}') => {
                            self.position += 1;
                            return Ok(Json::Object(entries));
                        }
                        _ => bail!("Expected , or }} at byte {}", self.position),
                    }
                }
            }
            Some(byte) if *byte == b'-' || byte.is_ascii_digit() => self.number(),
            _ => bail!("Unexpected JSON input at byte {}", self.position),
        }
    }

    fn number(&mut self) -> Result<Json> {
        let start = self.position;
        while self.bytes.get(self.position).is_some_and(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E')
        }) {
            self.position += 1;
        }
        let raw = &self.text[start..self.position];
        let number: f64 = raw
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid JSON number {raw}"))?;
        Ok(Json::Number(number))
    }

    fn string(&mut self) -> Result<String> {
        self.position += 1;
        let mut output = String::new();
        loop {
            let start = self.position;
            while self
                .bytes
                .get(self.position)
                .is_some_and(|byte| *byte != b'"' && *byte != b'\\')
            {
                self.position += 1;
            }
            output.push_str(&self.text[start..self.position]);
            match self.bytes.get(self.position) {
                Some(b'"') => {
                    self.position += 1;
                    return Ok(output);
                }
                Some(b'\\') => {
                    self.position += 1;
                    let escape = *self.bytes.get(self.position).ok_or_else(|| {
                        anyhow::anyhow!("Unterminated escape at byte {}", self.position)
                    })?;
                    self.position += 1;
                    match escape {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{8}'),
                        b'f' => output.push('\u{c}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first)
                                && self.text[self.position..].starts_with("\\u")
                            {
                                self.position += 2;
                                let second = self.hex4()?;
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            output.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        other => bail!("Invalid escape \\{}", char::from(other)),
                    }
                }
                _ => bail!("Unterminated JSON string"),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32> {
        let digits = self
            .text
            .get(self.position..self.position + 4)
            .ok_or_else(|| anyhow::anyhow!("Short unicode escape"))?;
        self.position += 4;
        u32::from_str_radix(digits, 16).map_err(|_| anyhow::anyhow!("Invalid unicode escape"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_key_order_and_javascript_formatting() {
        let value =
            Json::parse(r#"{"z":1,"a":[1.5,true,null,{}],"e":[],"s":"tab\there\u0001"}"#).unwrap();
        assert_eq!(
            value.pretty(),
            "{\n  \"z\": 1,\n  \"a\": [\n    1.5,\n    true,\n    null,\n    {}\n  ],\n  \"e\": [],\n  \"s\": \"tab\\there\\u0001\"\n}"
        );
    }

    #[test]
    fn assignment_replaces_in_place_or_appends() {
        let mut value = Json::parse(r#"{"a":1,"b":2}"#).unwrap();
        value.set("a", Json::Bool(false));
        value.set("c", Json::Null);
        assert_eq!(
            value.pretty(),
            "{\n  \"a\": false,\n  \"b\": 2,\n  \"c\": null\n}"
        );
    }

    #[test]
    fn numbers_render_like_javascript() {
        assert_eq!(format_number(4.0), "4");
        assert_eq!(format_number(0.25), "0.25");
        assert_eq!(format_number(-3.0), "-3");
    }
}
