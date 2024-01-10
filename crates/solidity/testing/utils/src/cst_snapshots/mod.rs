use std::cmp::max;
use std::collections::HashSet;
use std::fmt::Write;
use std::ops::Range;

use anyhow::Result;
use codegen_language_definition::model::Item;
use inflector::Inflector;
use once_cell::sync::Lazy;
use slang_solidity::cst::Node;
use slang_solidity::cursor::CursorWithNames;
use slang_solidity::kinds::RuleKind;
use slang_solidity::text_index::TextRangeExtensions;
use solidity_language::SolidityDefinition;

pub struct CstSnapshots;

impl CstSnapshots {
    pub fn render(source: &str, errors: &Vec<String>, cursor: CursorWithNames) -> Result<String> {
        let mut w = String::new();

        write_source(&mut w, source)?;
        writeln!(&mut w)?;

        write_errors(&mut w, errors)?;
        writeln!(&mut w)?;

        write_tree(&mut w, cursor, source)?;

        Ok(w)
    }
}

fn write_source(w: &mut String, source: &str) -> Result<()> {
    if source.is_empty() {
        writeln!(w, "Source: \"\"")?;
        return Ok(());
    }

    let line_data = source
        .lines()
        .enumerate()
        .map(|(index, line)| (index, line, line.bytes().len(), line.chars().count()))
        .collect::<Vec<_>>();

    let source_width = {
        let source_width = line_data
            .iter()
            .map(|(_, _, _, chars)| *chars)
            .max()
            .unwrap_or(0);

        max(source_width, 80)
    };

    writeln!(w, "Source: >")?;

    let mut offset = 0;
    for (index, line, bytes, chars) in &line_data {
        let range = offset..(offset + bytes);
        writeln!(
            w,
            "  {line_number: <2} │ {line}{padding} │ {range:?}",
            line_number = index + 1,
            padding = " ".repeat(source_width - chars),
        )?;

        offset = range.end + 1;
    }

    Ok(())
}

fn write_errors(w: &mut String, errors: &Vec<String>) -> Result<()> {
    if errors.is_empty() {
        writeln!(w, "Errors: []")?;
        return Ok(());
    }

    writeln!(w, "Errors: # {count} total", count = errors.len())?;

    for error in errors {
        writeln!(w, "  - >")?;
        for line in error.lines() {
            writeln!(w, "    {line}")?;
        }
    }

    Ok(())
}

fn write_tree(w: &mut String, mut cursor: CursorWithNames, source: &str) -> Result<()> {
    writeln!(w, "Tree:")?;
    write_node(w, &mut cursor, source, 0)?;

    assert!(!cursor.go_to_next());
    Ok(())
}

fn write_node(
    w: &mut String,
    cursor: &mut CursorWithNames,
    source: &str,
    depth: usize,
) -> Result<()> {
    let indentation = " ".repeat(4 * depth);
    write!(w, "{indentation}  - ")?;

    loop {
        write!(w, "{key}", key = render_key(cursor))?;

        // If it is a parent wih a single child, inline them on the same line:
        if matches!(cursor.node(), Node::Rule(rule) if rule.children.len() == 1 && !NON_INLINABLE.contains(&rule.kind))
        {
            let parent_range = cursor.text_range();
            assert!(cursor.go_to_next());

            let child_range = cursor.text_range();
            assert_eq!(parent_range, child_range);

            write!(w, " ► ")?;
            continue;
        }

        break;
    }

    writeln!(w, ": {value}", value = render_value(cursor, source))?;

    for _ in cursor.node().children() {
        assert!(cursor.go_to_next());
        write_node(w, cursor, source, depth + 1)?;
    }

    Ok(())
}

fn render_key(cursor: &mut CursorWithNames) -> String {
    let kind = match cursor.node() {
        Node::Rule(rule) => rule.kind.to_string(),
        Node::Token(token) => token.kind.to_string(),
    };

    if let Some(name) = cursor.node_name() {
        format!("({name}꞉ {kind})", name = name.as_ref().to_snake_case())
    } else {
        format!("({kind})")
    }
}

fn render_value(cursor: &mut CursorWithNames, source: &str) -> String {
    let range = cursor.text_range().utf8();
    let preview = render_preview(source, &range);

    match cursor.node() {
        Node::Rule(rule) if rule.children.is_empty() => format!("[] # ({range:?})"),
        Node::Rule(_) => format!("# {preview} ({range:?})"),
        Node::Token(_) => format!("{preview} # ({range:?})"),
    }
}

fn render_preview(source: &str, range: &Range<usize>) -> String {
    let length = range.len();

    // Trim long values:
    let max_length = 50;
    let contents = source
        .bytes()
        .skip(range.start)
        .take(length.clamp(0, max_length))
        .collect();

    // Add terminator if trimmed:
    let mut contents = String::from_utf8(contents).unwrap();
    if length > max_length {
        contents.push_str("...");
    }

    // Escape line breaks:
    let contents = contents
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n");

    // Surround by quotes for use in yaml:
    if contents.contains('"') {
        let contents = contents.replace('\'', "''");
        format!("'{contents}'")
    } else {
        let contents = contents.replace('"', "\\\"");
        format!("\"{contents}\"")
    }
}

static NON_INLINABLE: Lazy<HashSet<RuleKind>> = Lazy::new(|| {
    let mut kinds = HashSet::new();

    for item in SolidityDefinition::create().items() {
        match item {
            Item::Repeated { .. } | Item::Separated { .. } => {
                // Do not inline these parents, even if they have a single child.
                kinds.insert(item.name().parse().unwrap());
            }
            Item::Struct { .. } | Item::Enum { .. } | Item::Precedence { .. } => {
                // These non-terminals can be inlined if they have a single child.
                // Note: same goes for 'PrecedenceExpression' items under each 'Precedence' item.
            }
            Item::Trivia { .. }
            | Item::Keyword { .. }
            | Item::Token { .. }
            | Item::Fragment { .. } => {
                // These are terminals (no children).
            }
        }
    }

    kinds
});
