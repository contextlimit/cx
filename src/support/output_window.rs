use std::borrow::Cow;

use crate::support::output_projection;
use crate::support::utils::fallback_window;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedOutput {
    pub(crate) text: String,
    pub(crate) line_windowed: bool,
    pub(crate) generated_lines_bounded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputWindow {
    head_lines: usize,
    tail_lines: usize,
}

impl OutputWindow {
    pub(crate) const fn new(head_lines: usize, tail_lines: usize) -> Self {
        Self {
            head_lines,
            tail_lines,
        }
    }

    pub(crate) const fn line_limit(self) -> usize {
        self.head_lines + self.tail_lines
    }

    pub(crate) fn should_window(self, output: &str) -> bool {
        output.lines().count() > self.line_limit()
    }

    pub(crate) fn render(self, output: &str) -> String {
        if self.should_window(output) {
            self.render_normalized(output)
        } else {
            output.to_string()
        }
    }

    pub(crate) fn render_normalized(self, output: &str) -> String {
        fallback_window(output, self.head_lines, self.tail_lines)
    }

    pub(crate) fn project(self, output: &str, max_line_chars: usize) -> ProjectedOutput {
        let line_windowed = self.should_window(output);
        let line_projection = if line_windowed {
            Cow::Owned(self.render(output))
        } else {
            Cow::Borrowed(output)
        };
        let text = output_projection::bound_generated_lines(&line_projection, max_line_chars);
        let generated_lines_bounded = text != line_projection.as_ref();
        ProjectedOutput {
            text,
            line_windowed,
            generated_lines_bounded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_window_preserves_short_output() {
        let policy = OutputWindow::new(1, 1);
        assert_eq!(policy.render("one\ntwo"), "one\ntwo");
    }

    #[test]
    fn output_window_bounds_long_output() {
        let policy = OutputWindow::new(1, 1);
        assert_eq!(
            policy.render("one\ntwo\nthree"),
            "one\n... [1 lines omitted] ...\nthree"
        );
    }

    #[test]
    fn normalized_output_drops_a_short_trailing_newline() {
        let policy = OutputWindow::new(1, 1);
        assert_eq!(policy.render_normalized("one\ntwo\n"), "one\ntwo");
    }

    #[test]
    fn projection_bounds_generated_single_lines_without_line_windowing() {
        let policy = OutputWindow::new(1, 1);
        let raw = format!(r#"{{"payload":"{}","tail":"done"}}"#, "A".repeat(2_000));
        let projected = policy.project(&raw, 240);

        assert!(!projected.line_windowed);
        assert!(projected.generated_lines_bounded);
        assert!(projected.text.contains("generated line truncated"));
        assert!(projected.text.ends_with(r#""tail":"done"}"#));
    }
}
