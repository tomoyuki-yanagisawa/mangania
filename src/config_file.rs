//! Adds missing keys to an existing TOML file without rewriting its values.

use std::io;
use std::path::Path;

use toml_edit::{DocumentMut, Table};

/// Inserts keys from `template` that are absent in `text`.
///
/// Returns the updated document, or `None` when `text` is not valid TOML or
/// when every template key already exists. Existing values, comments, unknown
/// keys, and explicit empty arrays are kept.
pub(crate) fn add_missing(text: &str, template: &str) -> Option<String> {
    let mut document = text.parse::<DocumentMut>().ok()?;
    let template = template.parse::<DocumentMut>().ok()?;
    merge(document.as_table_mut(), template.as_table()).then(|| document.to_string())
}

/// Completes an existing file in place when `valid` accepts its contents.
///
/// A missing file is left to the caller so that it can write the default
/// template with its own creation semantics.
pub(crate) fn complete_file(
    path: &Path,
    template: &str,
    valid: impl Fn(&str) -> bool,
) -> io::Result<bool> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if !valid(&text) {
        return Ok(false);
    }
    let Some(completed) = add_missing(&text, template) else {
        return Ok(false);
    };
    crate::config::atomic_write(path, completed.as_bytes())?;
    Ok(true)
}

fn merge(target: &mut Table, template: &Table) -> bool {
    let mut changed = false;
    for (key, template_item) in template.iter() {
        match target.get_mut(key) {
            Some(target_item) => {
                if let (Some(target_table), Some(template_table)) =
                    (target_item.as_table_mut(), template_item.as_table())
                {
                    changed |= merge(target_table, template_table);
                }
            }
            None => {
                if let Some((template_key, template_item)) = template.get_key_value(key) {
                    target.insert_formatted(template_key, template_item.clone());
                    changed = true;
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::add_missing;

    #[test]
    fn adds_missing_keys_and_keeps_existing_values() {
        let text = "# keep\nmode = \"spread\"\ncustom = true\n";
        let template = "mode = \"single\"\nprefetch = 2\n";
        let completed = add_missing(text, template).unwrap();
        assert!(completed.contains("# keep"));
        assert!(completed.contains("mode = \"spread\""));
        assert!(completed.contains("custom = true"));
        assert!(completed.contains("prefetch = 2"));
    }

    #[test]
    fn adds_missing_keys_inside_existing_tables() {
        let text = "[keys]\nnext = [\"n\"]\nquit = []\n";
        let template = "[keys]\nnext = [\"Right\"]\nprev = [\"Left\"]\nquit = [\"Escape\"]\n";
        let completed = add_missing(text, template).unwrap();
        assert!(completed.contains("next = [\"n\"]"));
        assert!(completed.contains("quit = []"));
        assert!(completed.contains("prev = [\"Left\"]"));
    }

    #[test]
    fn adds_missing_root_values_outside_existing_tables() {
        let text = "[custom]\nvalue = 1\n";
        let template = "mode = \"single\"\n[custom]\nvalue = 0\nother = 2\n";
        let completed = add_missing(text, template).unwrap();
        let mode = completed.find("mode = \"single\"").unwrap();
        let table = completed.find("[custom]").unwrap();
        assert!(mode < table);
        assert!(completed.contains("value = 1"));
        assert!(completed.contains("other = 2"));
    }

    #[test]
    fn keeps_existing_formatting() {
        let text = "# c\nmode    =    \"spread\"   # trailing\n\n[custom]\nvalue = 1\n";
        let completed = add_missing(text, "mode = \"single\"\nprefetch = 2\n").unwrap();
        assert!(completed.contains("mode    =    \"spread\"   # trailing"));
        assert!(completed.contains("prefetch = 2"));
    }

    #[test]
    fn leaves_invalid_toml_alone() {
        assert_eq!(
            add_missing("[keys\nnext = []\n", "[keys]\nnext = []\n"),
            None
        );
    }

    #[test]
    fn reports_no_change_when_all_keys_exist() {
        let text = "mode = \"spread\"\n";
        assert_eq!(add_missing(text, "mode = \"single\"\n"), None);
    }
}
