//! CRAP score presentation helpers shared by every view.

use codeuml_metrics::CrapLevel;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A score together with the band it falls in under the configured limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CrapScore {
    pub value: f64,
    pub level: CrapLevel,
}

impl CrapScore {
    pub(crate) fn new(level: CrapLevel, value: f64) -> Self {
        Self { value, level }
    }

    /// Short display text: one decimal, none once the score reaches 100.
    pub(crate) fn text(&self) -> String {
        if self.value >= 100.0 {
            format!("{:.0}", self.value)
        } else {
            format!("{:.1}", self.value)
        }
    }
}

/// Split a row into its (possibly truncated) label and, when the score fits,
/// the padding that right-aligns `score` within `width` columns.
///
/// Without a score the label is only truncated. When the width cannot fit a
/// label character, a gap, and the score, the score is dropped (`None`) instead
/// of overflowing the row.
pub(crate) fn fit_row(label: &str, score: Option<&str>, width: usize) -> (String, Option<String>) {
    let Some(score) = score else {
        return (truncate(label, width), None);
    };
    let score_width = score.width();
    let label_width = width.saturating_sub(score_width + 1);
    if label_width == 0 {
        return (truncate(label, width), None);
    }
    let label = truncate(label, label_width);
    let padding = width.saturating_sub(label.width() + score_width);
    (label, Some(" ".repeat(padding)))
}

fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let mut kept = String::new();
    let mut used = 0;
    for character in text.chars() {
        let advance = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + advance + 1 > width {
            break;
        }
        kept.push(character);
        used += advance;
    }
    if width > 0 {
        kept.push('…');
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_text_uses_one_decimal_until_it_reaches_one_hundred() {
        let text = |value| CrapScore::new(CrapLevel::Bad, value).text();
        assert_eq!(text(3.0), "3.0");
        assert_eq!(text(11.3596), "11.4");
        assert_eq!(text(99.96), "100.0");
        assert_eq!(text(100.0), "100");
        assert_eq!(text(823.0826), "823");
    }

    #[test]
    fn scores_are_right_aligned_to_the_row_width() {
        let (label, padding) = fit_row("Alpha", Some("11.4"), 20);
        let padding = padding.expect("score fits");
        assert_eq!(label, "Alpha");
        assert_eq!(label.width() + padding.len() + 4, 20);

        let (label, padding) = fit_row("Alpha", None, 20);
        assert_eq!((label.as_str(), padding), ("Alpha", None));
        assert_eq!(fit_row("Alphabet", None, 5).0, "Alph…");
    }

    #[test]
    fn long_labels_are_truncated_to_keep_the_score_visible() {
        let (label, padding) = fit_row("AVeryLongTypeName", Some("11.4"), 12);
        assert_eq!(label, "AVeryL…");
        assert_eq!(label.width() + padding.expect("fits").len() + 4, 12);

        let (label, padding) = fit_row("Wide世界世界", Some("1.0"), 9);
        assert!(label.width() + padding.expect("fits").len() + 3 <= 9);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn scores_are_dropped_when_the_row_is_too_narrow() {
        let (label, padding) = fit_row("Alpha", Some("11.4"), 4);
        assert_eq!((label.as_str(), padding), ("Alp…", None));
        let (label, padding) = fit_row("Alpha", Some("11.4"), 5);
        assert_eq!((label.as_str(), padding), ("Alpha", None));
        let (label, padding) = fit_row("Alpha", Some("11.4"), 0);
        assert_eq!((label.as_str(), padding), ("", None));
        assert_eq!(truncate("Alpha", 0), "");
    }
}
