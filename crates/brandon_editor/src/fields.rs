//! Reading and changing the fields of a behaviour the editor has no Rust
//! type for. The data is scene-file text such as `(speed: 460, kind: Rock)`.

/// One `name: value` pair of a behaviour's data.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    /// The value as written, for example `460` or `"hello"` or `Rock`.
    pub value: String,
}

/// What kind of editor a value gets.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Number(f64),
    Bool(bool),
    Text(String),
    /// Anything else is edited as written.
    Other,
}

impl Field {
    pub fn kind(&self) -> Kind {
        let value = self.value.trim();
        if let Some(number) = value.parse::<f64>().ok().filter(|n| n.is_finite()) {
            return Kind::Number(number);
        }
        match value {
            "true" => return Kind::Bool(true),
            "false" => return Kind::Bool(false),
            _ => {}
        }
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            if let Ok(text) = ron::from_str::<String>(value) {
                return Kind::Text(text);
            }
        }
        Kind::Other
    }
}

/// Splits `(a: 1, b: "x")` into its fields. `None` if the text is not a
/// plain list of named fields, in which case it is edited as one piece.
pub fn split(ron: &str) -> Option<Vec<Field>> {
    let inner = ron.trim().strip_prefix('(')?.strip_suffix(')')?;
    let mut fields = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0;
    let mut pieces = Vec::new();
    for (index, c) in inner.char_indices() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                pieces.push(&inner[start..index]);
                start = index + 1;
            }
            _ => {}
        }
        if depth < 0 {
            return None;
        }
    }
    if depth != 0 || in_string {
        return None;
    }
    pieces.push(&inner[start..]);
    for piece in pieces {
        if piece.trim().is_empty() {
            continue; // A trailing comma.
        }
        let (name, value) = piece.split_once(':')?;
        let name = name.trim();
        let is_name = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !is_name || value.trim().is_empty() {
            return None;
        }
        fields.push(Field {
            name: name.to_string(),
            value: value.trim().to_string(),
        });
    }
    Some(fields)
}

/// The opposite of [`split`].
pub fn join(fields: &[Field]) -> String {
    let body: Vec<String> = fields
        .iter()
        .map(|f| format!("{}: {}", f.name, f.value))
        .collect();
    format!("({})", body.join(", "))
}

/// Is this number written without a decimal point? The editor does not
/// know the game's types, so a whole number is kept whole: it may be read
/// into an integer, which would refuse `3.5`.
pub fn is_whole(text: &str) -> bool {
    !text.trim().contains(['.', 'e', 'E'])
}

/// Writes a number the way scene files expect. A whole number is written
/// without a decimal point; any other number keeps one, so it stays a
/// decimal the next time it is read.
pub fn number_text(value: f64, whole: bool) -> String {
    if whole && value.abs() < 1e15 {
        return format!("{}", value.round() as i64);
    }
    let rounded = (value * 1000.0).round() / 1000.0;
    let text = format!("{rounded}");
    if text.contains(['.', 'e', 'E', 'i', 'N']) {
        text
    } else {
        format!("{text}.0")
    }
}

/// Writes text as a quoted scene-file string.
pub fn text_value(text: &str) -> String {
    ron::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

/// Is this valid scene-file data?
pub fn is_valid(ron: &str) -> bool {
    ron::value::RawValue::from_ron(ron).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, value: &str) -> Field {
        Field {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn splits_and_joins_fields() {
        let text = r#"(speed: 460, kind: Rock, path: [(1, 2), (3, 4)], label: "a), b", on: true,)"#;
        let fields = split(text).unwrap();
        assert_eq!(
            fields,
            [
                field("speed", "460"),
                field("kind", "Rock"),
                field("path", "[(1, 2), (3, 4)]"),
                field("label", r#""a), b""#),
                field("on", "true"),
            ]
        );
        assert_eq!(fields[0].kind(), Kind::Number(460.0));
        assert_eq!(fields[1].kind(), Kind::Other);
        assert_eq!(fields[3].kind(), Kind::Text("a), b".to_string()));
        assert_eq!(fields[4].kind(), Kind::Bool(true));
        let joined = join(&fields);
        assert_eq!(split(&joined).unwrap(), fields);
        assert!(is_valid(&joined));
    }

    #[test]
    fn empty_and_unusual_data() {
        assert_eq!(split("()").unwrap(), []);
        assert_eq!(join(&[]), "()");
        assert_eq!(split("(1, 2)"), None, "a tuple has no names");
        assert_eq!(split("Rock"), None);
        assert_eq!(split("(a: (1, 2)"), None);
        assert_eq!(split(r#"(a: "unclosed)"#), None);
        assert_eq!(split(r#"(a: "quote \" inside", b: 2)"#).unwrap().len(), 2);
    }

    #[test]
    fn values_are_written_cleanly() {
        assert_eq!(number_text(460.0, true), "460");
        assert_eq!(number_text(3.4, true), "3");
        assert_eq!(number_text(460.0, false), "460.0");
        assert_eq!(number_text(0.35, false), "0.35");
        assert_eq!(number_text(-1.23456, false), "-1.235");
        assert!(is_whole("90"));
        assert!(is_whole("-3"));
        assert!(!is_whole("0.7"));
        assert!(!is_whole("1e3"));
        assert_eq!(text_value("say \"hi\""), r#""say \"hi\"""#);
        assert!(is_valid("(speed: 1)") && !is_valid("(speed: "));
    }
}
