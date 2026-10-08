//! `--stream` traverses the depth-bounded input tree incrementally. Ancestor
//! keys are shared jaq strings, never copied into a retained list of events.
//! THREAT[TM-DOS-110]: cursor, path and event bodies are admitted by the jq
//! meter before allocation; each traversal step checks shared work/deadline/
//! cancellation limits, including when the filter emits nothing.

use super::Context;
use super::convert::{JqJson, jq_to_val};
use super::jaq_json::{Rc, Val, meter};
use crate::error::{Error, Result};

fn check(bytes: usize) -> Result<()> {
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
    Ok(Val::Arr(Rc::new(meter::Metered::new(body))))
}

enum Children<'a> {
    Array(std::iter::Enumerate<std::slice::Iter<'a, JqJson>>),
    Object(std::slice::Iter<'a, (String, JqJson)>),
}

impl<'a> Children<'a> {
    fn next(&mut self) -> Result<Option<(Val, &'a JqJson)>> {
        match self {
            Self::Array(iter) => Ok(iter.next().map(|(i, v)| (Val::from(i), v))),
            Self::Object(iter) => iter
                .next()
                .map(|(k, v)| {
                    check(k.len())?;
                    Ok((Val::from(k.clone()), v))
                })
                .transpose(),
        }
    }
}

struct Frame<'a> {
    children: Children<'a>,
    path_len: usize,
}

pub(super) struct StreamEvents<'a, 'shell> {
    pending: Option<&'a JqJson>,
    stack: meter::Metered<Vec<Frame<'a>>>,
    path: meter::Metered<Vec<Val>>,
    ctx: &'a Context<'shell>,
    done: bool,
}

impl<'a, 'shell> StreamEvents<'a, 'shell> {
    pub(super) fn new(value: &'a JqJson, ctx: &'a Context<'shell>) -> Self {
        Self {
            pending: Some(value),
            stack: meter::Metered::default(),
            path: meter::Metered::default(),
            ctx,
            done: false,
        }
    }

    fn next_event(&mut self) -> Result<Option<Val>> {
        loop {
            self.ctx.consume_budget_work(1)?;
            meter::tick();
            if let Some(value) = self.pending.take() {
                let children = match value {
                    JqJson::Array(a) if !a.is_empty() => {
                        Some(Children::Array(a.iter().enumerate()))
                    }
                    JqJson::Object(o) if !o.is_empty() => Some(Children::Object(o.iter())),
                    _ => None,
                };
                if let Some(children) = children {
                    push_metered(
                        &mut self.stack,
                        Frame {
                            children,
                            path_len: self.path.len(),
                        },
                    )?;
                    continue;
                }
                if let JqJson::String(s) | JqJson::Number(s) = value {
                    check(s.len())?;
                }
                let leaf = jq_to_val(value);
                let path = array(self.path.iter().cloned())?;
                return array([path, leaf].into_iter()).map(Some);
            }
            let Some(frame) = self.stack.last_mut() else {
                return Ok(None);
            };
            if let Some((key, value)) = frame.children.next()? {
                self.path.truncate(frame.path_len);
                push_metered(&mut self.path, key)?;
                self.pending = Some(value);
            } else {
                // A closing event keeps the last child's key, not its descendants.
                self.path.truncate(frame.path_len + 1);
                self.stack.pop();
                let path = array(self.path.iter().cloned())?;
                return array([path].into_iter()).map(Some);
            }
        }
    }
}

impl Iterator for StreamEvents<'_, '_> {
    type Item = Result<Val>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.next_event() {
            Ok(Some(value)) => Some(Ok(value)),
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
