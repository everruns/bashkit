//! Source location tracking for error messages and $LINENO
//!
//! Provides position and span types for tracking source locations through
//! lexing, parsing, and execution.

/// A position in source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    /// 1-based line number
    pub line: usize,
    /// 1-based column number (byte offset within line)
    pub column: usize,
    /// 0-based byte offset from start of input
    pub offset: usize,
}

impl Position {
    /// Create a new position at line 1, column 1, offset 0.
    pub fn new() -> Self {
        Self {
            line: 1,
            column: 1,
            offset: 0,
        }
    }

    /// Advance position by one character.
    pub fn advance(&mut self, ch: char) {
        self.offset += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// A span of source code (start to end position).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    /// Start position (inclusive)
    pub start: Position,
    /// End position (exclusive)
    pub end: Position,
}

impl Span {
    /// Create an empty span at the default position.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a span from start to end positions.
    pub fn from_positions(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    /// Create a span covering a single position.
    pub fn at(pos: Position) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    /// Merge two spans into one covering both.
    pub fn merge(self, other: Span) -> Self {
        let start = if self.start.offset <= other.start.offset {
            self.start
        } else {
            other.start
        };
        let end = if self.end.offset >= other.end.offset {
            self.end
        } else {
            other.end
        };
        Self { start, end }
    }

    /// Get the starting line number.
    pub fn line(&self) -> usize {
        self.start.line
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start.line == self.end.line {
            write!(f, "line {}", self.start.line)
        } else {
            write!(f, "lines {}-{}", self.start.line, self.end.line)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_advance() {
        let mut pos = Position::new();
        assert_eq!(pos.line, 1);
        assert_eq!(pos.column, 1);
        assert_eq!(pos.offset, 0);

        pos.advance('a');
        assert_eq!(pos.line, 1);
        assert_eq!(pos.column, 2);
        assert_eq!(pos.offset, 1);

        pos.advance('\n');
        assert_eq!(pos.line, 2);
        assert_eq!(pos.column, 1);
        assert_eq!(pos.offset, 2);

        pos.advance('b');
        assert_eq!(pos.line, 2);
        assert_eq!(pos.column, 2);
        assert_eq!(pos.offset, 3);
    }

    #[test]
    fn test_position_display() {
        let pos = Position {
            line: 5,
            column: 10,
            offset: 50,
        };
        assert_eq!(format!("{}", pos), "5:10");
    }

    #[test]
    fn test_span_merge() {
        let span1 = Span {
            start: Position {
                line: 1,
                column: 1,
                offset: 0,
            },
            end: Position {
                line: 1,
                column: 5,
                offset: 4,
            },
        };
        let span2 = Span {
            start: Position {
                line: 1,
                column: 10,
                offset: 9,
            },
            end: Position {
                line: 2,
                column: 3,
                offset: 15,
            },
        };
        let merged = span1.merge(span2);
        assert_eq!(merged.start.offset, 0);
        assert_eq!(merged.end.offset, 15);
    }

    #[test]
    fn test_span_display() {
        let single_line = Span {
            start: Position {
                line: 3,
                column: 1,
                offset: 0,
            },
            end: Position {
                line: 3,
                column: 10,
                offset: 9,
            },
        };
        assert_eq!(format!("{}", single_line), "line 3");

        let multi_line = Span {
            start: Position {
                line: 1,
                column: 1,
                offset: 0,
            },
            end: Position {
                line: 5,
                column: 1,
                offset: 50,
            },
        };
        assert_eq!(format!("{}", multi_line), "lines 1-5");
    }
}
