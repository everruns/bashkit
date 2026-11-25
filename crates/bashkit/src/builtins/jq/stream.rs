//! `--stream` parses tokens into path events without building an input tree.
//! Ancestor keys are shared jaq strings, never copied into an event list.
//! THREAT[TM-DOS-110]: cursor, path and event bodies are admitted by the jq
//! meter before allocation; each traversal step checks shared work/deadline/
//! cancellation limits, including when the filter emits nothing.

use super::convert::{JqJson, MAX_JQ_JSON_DEPTH, jq_into_val};
use super::jaq_json::{Val, meter, metered};
use super::{Context, input};
use crate::error::{Error, Result};

pub(super) fn check(bytes: usize) -> Result<()> {
    meter::check(bytes).map_err(|e| Error::Execution(e.to_string()))
}

/// Admit capacity growth before reserve, charging it while the buffer lives.
pub(super) fn push_metered<T>(values: &mut meter::Metered<Vec<T>>, value: T) -> Result<()> {
    if values.len() == values.capacity() {
        let additional = values.capacity().max(1);
        check(additional.saturating_mul(std::mem::size_of::<T>()))?;
        values.reserve_exact(additional);
        values.resync();
    }
    values.push(value);
    Ok(())
}

fn array(values: impl ExactSizeIterator<Item = Val>) -> Result<Val> {
    check(values.len().saturating_mul(std::mem::size_of::<Val>()))?;
    let mut body = Vec::with_capacity(values.len());
    body.extend(values);
    Ok(Val::Arr(metered(body)))
}

#[derive(Clone, Copy)]
enum Kind {
    Array,
    Object,
}

#[derive(Clone, Copy)]
enum State {
    First,
    Next,
    After,
}

struct Frame {
    kind: Kind,
    state: State,
    path_len: usize,
    index: usize,
}

/// Token-driven path events: neither a top-level tree nor an event list is
/// retained. Only the open containers, shared path keys and current leaf live
/// across `next` calls. Syntax, depth and resource checks happen on demand.
pub(super) struct Parser<'a, 'shell> {
    text: &'a str,
    offset: usize,
    event_end: usize,
    stack: meter::Metered<Vec<Frame>>,
    path: meter::Metered<Vec<Val>>,
    lines: input::Lines,
    ctx: &'a Context<'shell>,
    done: bool,
}

type ReadResult<T> = std::result::Result<T, input::NormalizeError>;

impl<'a, 'shell> Parser<'a, 'shell> {
    pub(super) fn new(text: &'a str, ctx: &'a Context<'shell>) -> Self {
        Self {
            text,
            offset: 0,
            event_end: 0,
            stack: meter::Metered::default(),
            path: meter::Metered::default(),
            lines: input::Lines::default(),
            ctx,
            done: false,
        }
    }

    fn invalid(&self, message: &str) -> input::NormalizeError {
        input::NormalizeError::InvalidJson(format!("jq: invalid JSON: {message}"))
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.offset).copied()
    }

    fn advance(&mut self) -> ReadResult<()> {
        self.offset += 1;
        if self.offset.is_multiple_of(1024) {
            self.ctx.consume_budget_work(1024)?;
            meter::tick();
        }
        Ok(())
    }

    fn whitespace(&mut self) -> ReadResult<()> {
        while self.peek().is_some_and(input::is_json_whitespace) {
            self.advance()?;
        }
        Ok(())
    }

    fn scalar(&mut self) -> ReadResult<Val> {
        let start = self.offset;
        if self.peek() == Some(b'"') {
            self.advance()?;
            let mut escaped = false;
            loop {
                let Some(byte) = self.peek() else {
                    return Err(self.invalid("unterminated string at end of input"));
                };
                self.advance()?;
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    break;
                }
            }
        } else {
            while self.peek().is_some_and(|b| {
                !input::is_json_whitespace(b)
                    && !matches!(b, b'[' | b'{' | b'"' | b']' | b'}' | b',' | b':')
            }) {
                self.advance()?;
            }
        }
        if self.offset == start {
            return Err(self.invalid("expected a value"));
        }
        let token = &self.text[start..self.offset];
        // Admit the leaf/key before serde allocates its temporary decoded form.
        check(token.len())?;
        let value = super::parse_jq_json_value(self.ctx, token)?
            .map_err(input::NormalizeError::InvalidJson)?;
        Ok(jq_into_val(value))
    }

    fn next_event(&mut self) -> ReadResult<Option<Val>> {
        loop {
            self.ctx.consume_budget_work(1)?;
            meter::tick();
            self.whitespace()?;
            if let Some(frame) = self.stack.last() {
                let (kind, state, path_len, index) =
                    (frame.kind, frame.state, frame.path_len, frame.index);
                let closing = match kind {
                    Kind::Array => b']',
                    Kind::Object => b'}',
                };
                match state {
                    State::After => match self.peek() {
                        Some(b',') => {
                            self.advance()?;
                            self.stack
                                .last_mut()
                                .expect("container inspected above")
                                .state = State::Next;
                            continue;
                        }
                        Some(b) if b == closing => {
                            self.advance()?;
                            self.stack.pop();
                            self.path.truncate(path_len + 1);
                            let path = array(self.path.iter().cloned())?;
                            self.event_end = self.offset;
                            return Ok(Some(array([path].into_iter())?));
                        }
                        _ => return Err(self.invalid("expected a comma or container end")),
                    },
                    State::First if self.peek() == Some(closing) => {
                        self.advance()?;
                        self.stack.pop();
                        self.path.truncate(path_len);
                        let empty = match kind {
                            Kind::Array => JqJson::Array(Vec::new()),
                            Kind::Object => JqJson::Object(Vec::new()),
                        };
                        let path = array(self.path.iter().cloned())?;
                        self.event_end = self.offset;
                        return Ok(Some(array([path, jq_into_val(empty)].into_iter())?));
                    }
                    State::First | State::Next => {
                        self.path.truncate(path_len);
                        let key = match kind {
                            Kind::Array => Val::from(index),
                            Kind::Object => {
                                if self.peek() != Some(b'"') {
                                    return Err(self.invalid("expected an object key"));
                                }
                                let key = self.scalar()?;
                                self.whitespace()?;
                                if self.peek() != Some(b':') {
                                    return Err(self.invalid("expected a colon"));
                                }
                                self.advance()?;
                                self.whitespace()?;
                                key
                            }
                        };
                        push_metered(&mut self.path, key)?;
                        let frame = self.stack.last_mut().expect("container inspected above");
                        frame.state = State::After;
                        frame.index += 1;
                    }
                }
            } else {
                self.path.clear();
                if self.peek().is_none() {
                    return Ok(None);
                }
            }
            if self.stack.len() > MAX_JQ_JSON_DEPTH {
                return Err(input::NormalizeError::InvalidJson(format!(
                    "jq: JSON nesting too deep ({} levels, max {MAX_JQ_JSON_DEPTH})",
                    self.stack.len()
                )));
            }
            match self.peek() {
                Some(b'[' | b'{') => {
                    let kind = if self.peek() == Some(b'[') {
                        Kind::Array
                    } else {
                        Kind::Object
                    };
                    self.advance()?;
                    push_metered(
                        &mut self.stack,
                        Frame {
                            kind,
                            state: State::First,
                            path_len: self.path.len(),
                            index: 0,
                        },
                    )?;
                }
                Some(_) => {
                    let leaf = self.scalar()?;
                    // jq commits a scalar inside a container only once its
                    // following separator is valid (`[1` emits no event).
                    if let Some(frame) = self.stack.last() {
                        let closing = match frame.kind {
                            Kind::Array => b']',
                            Kind::Object => b'}',
                        };
                        self.whitespace()?;
                        if !self.peek().is_some_and(|b| b == b',' || b == closing) {
                            return Err(self.invalid("expected a comma or container end"));
                        }
                    }
                    let path = array(self.path.iter().cloned())?;
                    // A scalar was committed by looking at its separator;
                    // include that character in jq's line-read metadata.
                    self.event_end = self.offset + usize::from(!self.stack.is_empty());
                    return Ok(Some(array([path, leaf].into_iter())?));
                }
                None => return Err(self.invalid("unexpected end of input")),
            }
        }
    }
}

impl Iterator for Parser<'_, '_> {
    type Item = ReadResult<(Val, usize)>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.next_event() {
            Ok(Some(value)) => match self.lines.at(self.text, self.event_end, self.ctx) {
                Ok(line) => Some(Ok((value, line))),
                Err(error) => {
                    self.done = true;
                    Some(Err(error.into()))
                }
            },
            Ok(None) => {
                self.done = true;
                None
            }
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
        }
    }
}
