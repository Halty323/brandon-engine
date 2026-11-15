//! Lays out scene files the way people write them by hand: short values on
//! one line, and nodes, children and behaviours one per line, so that a
//! saved scene reads well and changes show up as small diffs.

/// Lines longer than this are broken up.
const WIDTH: usize = 80;
const INDENT: &str = "    ";

enum Item {
    Atom(String),
    Colon,
    Group(Group),
}

struct Group {
    open: char,
    close: char,
    entries: Vec<Vec<Item>>,
    /// Always one entry per line, however short.
    force: bool,
}

/// Re-lays out RON text. Text it does not fully understand, such as text
/// with comments, comes back unchanged.
pub(crate) fn tidy(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut pos = 0;
    let Some(mut entry) = parse_entry(&chars, &mut pos) else {
        return text.to_string();
    };
    skip_space(&chars, &mut pos);
    if pos != chars.len() || entry.is_empty() {
        return text.to_string();
    }
    mark_forced(&mut entry, true);
    let mut out = String::new();
    write_entry(&entry, 0, &mut out);
    out
}

fn skip_space(c: &[char], pos: &mut usize) {
    while c.get(*pos).is_some_and(|c| c.is_whitespace()) {
        *pos += 1;
    }
}

/// Reads the items of one entry, up to a comma or a closing bracket.
fn parse_entry(c: &[char], pos: &mut usize) -> Option<Vec<Item>> {
    let mut items = Vec::new();
    loop {
        skip_space(c, pos);
        let Some(&ch) = c.get(*pos) else { break };
        match ch {
            ',' | ')' | ']' | '}' => break,
            ':' => {
                *pos += 1;
                items.push(Item::Colon);
            }
            '(' | '[' | '{' => items.push(Item::Group(parse_group(c, pos)?)),
            // Comments are kept by leaving the text alone.
            '/' => return None,
            '"' | '\'' => items.push(Item::Atom(read_quoted(c, pos, ch)?)),
            _ => items.push(Item::Atom(read_atom(c, pos)?)),
        }
    }
    Some(items)
}

fn parse_group(c: &[char], pos: &mut usize) -> Option<Group> {
    let open = c[*pos];
    let close = match open {
        '(' => ')',
        '[' => ']',
        _ => '}',
    };
    *pos += 1;
    let mut entries = Vec::new();
    loop {
        let entry = parse_entry(c, pos)?;
        let next = *c.get(*pos)?;
        if !entry.is_empty() {
            entries.push(entry);
        } else if next == ',' {
            return None;
        }
        *pos += 1;
        if next == close {
            break;
        }
        if next != ',' {
            return None;
        }
    }
    Some(Group {
        open,
        close,
        entries,
        force: false,
    })
}

fn read_quoted(c: &[char], pos: &mut usize, quote: char) -> Option<String> {
    let start = *pos;
    *pos += 1;
    loop {
        match *c.get(*pos)? {
            '\\' => *pos += 2,
            '\n' => return None,
            ch if ch == quote => {
                *pos += 1;
                return Some(c[start..*pos].iter().collect());
            }
            _ => *pos += 1,
        }
    }
}

fn read_atom(c: &[char], pos: &mut usize) -> Option<String> {
    let start = *pos;
    while let Some(&ch) = c.get(*pos) {
        if ch.is_whitespace() || ",:()[]{}/".contains(ch) {
            break;
        }
        // Raw and byte strings (`r#"…"#`, `b"…"`) are left alone.
        if ch == '"' || ch == '\'' || ch == '#' {
            return None;
        }
        *pos += 1;
    }
    (*pos > start).then(|| c[start..*pos].iter().collect())
}

/// The root node, every node's `children` and every `behaviours` list get
/// one entry per line.
fn mark_forced(entry: &mut [Item], is_node: bool) {
    let key = match entry.first() {
        Some(Item::Atom(key)) if matches!(entry.get(1), Some(Item::Colon)) => Some(key.clone()),
        _ => None,
    };
    for item in entry.iter_mut() {
        if let Item::Group(group) = item {
            match key.as_deref() {
                Some("children") => {
                    group.force = true;
                    for child in &mut group.entries {
                        mark_forced(child, true);
                    }
                    continue;
                }
                Some("behaviours") => group.force = true,
                _ => {}
            }
            if is_node {
                group.force = true;
            }
            for inner in &mut group.entries {
                mark_forced(inner, false);
            }
        }
    }
}

fn must_break(group: &Group) -> bool {
    group.force
        || group.entries.iter().any(|entry| {
            entry
                .iter()
                .any(|item| matches!(item, Item::Group(g) if must_break(g)))
        })
}

fn flat_entry(items: &[Item], out: &mut String) {
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::Atom(atom) => {
                if index > 0 && matches!(items[index - 1], Item::Atom(_)) {
                    out.push(' ');
                }
                out.push_str(atom);
            }
            Item::Colon => out.push_str(": "),
            Item::Group(group) => flat_group(group, out),
        }
    }
}

fn flat_group(group: &Group, out: &mut String) {
    out.push(group.open);
    for (index, entry) in group.entries.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        flat_entry(entry, out);
    }
    out.push(group.close);
}

fn column(out: &str) -> usize {
    out.rsplit('\n').next().unwrap_or("").chars().count()
}

fn write_entry(items: &[Item], indent: usize, out: &mut String) {
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::Atom(atom) => {
                if index > 0 && matches!(items[index - 1], Item::Atom(_)) {
                    out.push(' ');
                }
                out.push_str(atom);
            }
            Item::Colon => out.push_str(": "),
            Item::Group(group) => write_group(group, indent, out),
        }
    }
}

fn write_group(group: &Group, indent: usize, out: &mut String) {
    if group.entries.is_empty() {
        out.push(group.open);
        out.push(group.close);
        return;
    }
    if !must_break(group) {
        let mut flat = String::new();
        flat_group(group, &mut flat);
        // One more for the comma that usually follows.
        if column(out) + flat.chars().count() < WIDTH {
            out.push_str(&flat);
            return;
        }
    }
    out.push(group.open);
    out.push('\n');
    for entry in &group.entries {
        out.push_str(&INDENT.repeat(indent + 1));
        write_entry(entry, indent + 1, out);
        out.push_str(",\n");
    }
    out.push_str(&INDENT.repeat(indent));
    out.push(group.close);
}

#[cfg(test)]
mod tests {
    use super::tidy;

    #[test]
    fn short_values_share_a_line_and_nodes_do_not() {
        let pretty = r#"(
    name: "Main",
    behaviours: {
        "Round": (lives: 3),
    },
    children: [
        (
            name: "Ship",
            transform: (
                position: (480.0, 450.0),
                z: 5,
            ),
            shape: Circle(
                radius: 22.0,
            ),
            tags: [],
        ),
    ],
)"#;
        let expected = r#"(
    name: "Main",
    behaviours: {
        "Round": (lives: 3),
    },
    children: [
        (
            name: "Ship",
            transform: (position: (480.0, 450.0), z: 5),
            shape: Circle(radius: 22.0),
            tags: [],
        ),
    ],
)"#;
        assert_eq!(tidy(pretty), expected);
        // Laying out the result again changes nothing.
        assert_eq!(tidy(expected), expected);
    }

    #[test]
    fn long_values_are_broken_up() {
        let text = format!(
            r#"(name: "a", text: (text: "{}", size: 12.0))"#,
            "x".repeat(70)
        );
        let out = tidy(&text);
        assert!(out.lines().count() > 3, "{out}");
        assert!(out.contains("        text: \""), "{out}");
        assert!(out.contains("        size: 12.0,"), "{out}");
    }

    #[test]
    fn strings_keep_their_punctuation() {
        let out = tidy(r#"(name: "a, b: (c)", count: 'x')"#);
        assert_eq!(out, "(\n    name: \"a, b: (c)\",\n    count: 'x',\n)");
    }

    #[test]
    fn text_with_comments_is_left_alone() {
        let text = "(\n    // the speed\n    speed: 4,\n)";
        assert_eq!(tidy(text), text);
        let broken = "(speed: 4";
        assert_eq!(tidy(broken), broken);
    }
}
