//! Minimal reader for the `crapper` report (`.metrics/crap.edn`).
//!
//! The report is a flat EDN map `{:entries [{:name .. :namespace .. :crap ..}]}`.
//! Rather than a full EDN implementation this reads only what is needed:
//! every map that carries `:name`, `:namespace` and a numeric `:crap`.
//! Entries without a numeric score (for example complexity-only runs) are
//! skipped, and unknown keys are ignored so newer report fields are harmless.

use std::iter::Peekable;
use std::str::Chars;
use std::vec::IntoIter;

use crate::MetricsError;

/// One scored function from a CRAP report.
#[derive(Clone, Debug, PartialEq)]
pub struct CrapEntry {
    pub name: String,
    pub namespace: String,
    pub crap: f64,
}

#[derive(Debug, PartialEq)]
enum Token {
    Open,
    Close,
    Keyword(String),
    Text(String),
    Atom(String),
}

#[derive(Default)]
struct Fields {
    name: Option<String>,
    namespace: Option<String>,
    crap: Option<f64>,
}

impl Fields {
    fn set(&mut self, key: &str, token: Token) {
        match (key, token) {
            ("name", Token::Text(text)) => self.name = Some(text),
            ("namespace", Token::Text(text)) => self.namespace = Some(text),
            ("crap", Token::Atom(atom)) => {
                self.crap = atom.parse().ok().filter(|value: &f64| value.is_finite());
            }
            _ => {}
        }
    }

    fn into_entry(self) -> Option<CrapEntry> {
        Some(CrapEntry {
            name: self.name?,
            namespace: self.namespace?,
            crap: self.crap?,
        })
    }
}

/// Parse every scored entry in a `crapper` report.
pub fn parse_report(text: &str) -> Result<Vec<CrapEntry>, MetricsError> {
    let mut entries = Vec::new();
    let mut open: Vec<Fields> = Vec::new();
    let mut tokens = tokenize(text)?.into_iter().peekable();
    while let Some(token) = tokens.next() {
        match token {
            Token::Open => open.push(Fields::default()),
            Token::Close => entries.extend(open.pop().and_then(Fields::into_entry)),
            Token::Keyword(key) => assign_value(&mut tokens, open.last_mut(), &key),
            _ => {}
        }
    }
    Ok(entries)
}

fn assign_value(tokens: &mut Peekable<IntoIter<Token>>, fields: Option<&mut Fields>, key: &str) {
    let is_value = matches!(tokens.peek(), Some(Token::Text(_) | Token::Atom(_)));
    if let (true, Some(fields), Some(value)) = (is_value, fields, tokens.next_if(|_| is_value)) {
        fields.set(key, value);
    }
}

fn tokenize(text: &str) -> Result<Vec<Token>, MetricsError> {
    let mut chars = text.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(&next) = chars.peek() {
        match next {
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '"' => {
                chars.next();
                tokens.push(Token::Text(read_string(&mut chars)?));
                continue;
            }
            ';' => skip_comment(&mut chars),
            ':' => {
                chars.next();
                tokens.push(Token::Keyword(read_word(&mut chars)));
                continue;
            }
            other if is_separator(other) => {}
            _ => {
                tokens.push(Token::Atom(read_word(&mut chars)));
                continue;
            }
        }
        chars.next();
    }
    Ok(tokens)
}

fn is_separator(character: char) -> bool {
    character.is_whitespace() || matches!(character, ',' | '[' | ']' | '(' | ')')
}

fn is_word_end(character: char) -> bool {
    is_separator(character) || matches!(character, '{' | '}' | '"' | ';')
}

fn read_word(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut word = String::new();
    while let Some(character) = chars.next_if(|character| !is_word_end(*character)) {
        word.push(character);
    }
    word
}

fn skip_comment(chars: &mut Peekable<Chars<'_>>) {
    while chars.next_if(|character| *character != '\n').is_some() {}
}

fn read_string(chars: &mut Peekable<Chars<'_>>) -> Result<String, MetricsError> {
    let mut text = String::new();
    while let Some(character) = chars.next() {
        match character {
            '"' => return Ok(text),
            '\\' => text.push(unescape(chars.next())),
            other => text.push(other),
        }
    }
    Err(MetricsError::Report(
        "unterminated string in CRAP report".to_owned(),
    ))
}

fn unescape(escaped: Option<char>) -> char {
    match escaped {
        Some('n') => '\n',
        Some('t') => '\t',
        Some(other) => other,
        None => '\\',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scored_entries_and_ignores_unscored_or_unknown_content() {
        let report = r#"{:entries [
  {:name "run", :namespace "app::cli", :complexity 3, :coverage 100.0, :crap 3.0}
  {:name "fmt", :namespace "app::Kind", :crap 12.25, :extra [1 2 {:crap 99.0}]} ; note
  {:name "skipped", :namespace "app", :crap "N/A"}
  {:name "missing-score", :namespace "app"}
  {:namespace "no-name", :crap 1.0}
  {:name "inf", :namespace "app", :crap NaN}
]}"#;
        let entries = parse_report(report).expect("report");
        assert_eq!(
            entries,
            vec![
                CrapEntry {
                    name: "run".to_owned(),
                    namespace: "app::cli".to_owned(),
                    crap: 3.0
                },
                CrapEntry {
                    name: "fmt".to_owned(),
                    namespace: "app::Kind".to_owned(),
                    crap: 12.25
                },
            ]
        );
    }

    #[test]
    fn string_escapes_and_empty_reports_are_handled() {
        assert!(parse_report("").expect("empty").is_empty());
        assert!(parse_report("{:entries []}").expect("none").is_empty());

        let entries =
            parse_report(r#"{:name "a\"b\n\t\\", :namespace "n\x", :crap 1}"#).expect("escapes");
        assert_eq!(entries[0].name, "a\"b\n\t\\");
        assert_eq!(entries[0].namespace, "nx");
        assert_eq!(unescape(None), '\\');
    }

    #[test]
    fn unterminated_strings_are_reported() {
        let error = parse_report(r#"{:name "oops"#).expect_err("unterminated");
        assert!(error.to_string().contains("unterminated"), "{error}");
    }
}
