//! Experimental Bashkit-owned container boundary.
//!
//! Decision: reuse jaq's language and native functions, but give them only
//! read access to container bodies. Every attachment and capacity change goes
//! through this API. No DerefMut, entry handle, or mutable child escapes it.
//! Keep the low-level meter shared with strings, streaming and cancellation.
//! THREAT[TM-DOS-027,TM-DOS-110]: admit depth before attachment, memory before
//! reserve/COW, and invalidate depth plus reconcile charges in the same method.

use super::jaq_json::meter::{self, Footprint, Metered};
use super::jaq_json::{Error, Map, Rc, Val, ValueDepth};
use std::ops::Deref;

/// A read-only body with checked, type-specific mutation operations.
#[derive(Debug, PartialEq, Eq, Hash)]
pub(super) struct Container<T: Footprint> {
    body: Metered<T>,
}

impl<T: Footprint + ValueDepth + Default> Container<T> {
    pub(super) fn admit(mut body: Metered<T>) -> Self {
        if meter::check_depth(body.depth(ValueDepth::value_depth)).is_err() {
            body = Metered::default(); // panic-abort: discard a rejected tree
        }
        Self { body }
    }
}

impl<T: Footprint> Container<T> {
    pub(super) fn depth(&self, compute: impl FnOnce(&T) -> usize) -> usize {
        self.body.depth(compute)
    }
}

impl<T: Footprint + Clone + ValueDepth + Default> Clone for Container<T> {
    fn clone(&self) -> Self {
        // Rc::make_mut invokes this before copying a shared body.
        if !meter::require(self.body.footprint()) {
            return Self::default(); // panic-abort: do not copy rejected storage
        }
        Self::admit(self.body.clone())
    }
}

impl<T: Footprint + Default + ValueDepth> Default for Container<T> {
    fn default() -> Self {
        Self::admit(Metered::default())
    }
}

impl<T: Footprint> Deref for Container<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.body
    }
}

impl<T: Footprint + Default + IntoIterator> IntoIterator for Container<T> {
    type Item = T::Item;
    type IntoIter = meter::OwnedIter<T::IntoIter>;

    fn into_iter(self) -> Self::IntoIter {
        self.body.into_charged_iter()
    }
}

impl<'a, T: Footprint> IntoIterator for &'a Container<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.deref().into_iter()
    }
}

fn child(value: &Val) -> Result<(), Error> {
    meter::tick();
    meter::check_depth(1 + value.value_depth())
}

impl Container<Vec<Val>> {
    fn reserve(&mut self, len: usize) -> Result<(), Error> {
        if len > self.body.capacity() {
            let additional = len - self.body.capacity();
            meter::check(additional.saturating_mul(size_of::<Val>()))?;
            let additional_len = len - self.body.len();
            self.body.reserve_exact(additional_len);
            self.body.resync();
        }
        Ok(())
    }

    pub(super) fn push(&mut self, value: Val) -> Result<(), Error> {
        child(&value)?;
        if self.len() == self.capacity() {
            self.reserve(self.capacity().max(1).saturating_mul(2))?;
        }
        self.body.push(value);
        Ok(())
    }

    pub(super) fn set(&mut self, index: usize, value: Val) -> Result<(), Error> {
        child(&value)?;
        self.reserve(index.saturating_add(1))?;
        if index >= self.len() {
            self.body.resize(index + 1, Val::Null);
        }
        self.body[index] = value;
        Ok(())
    }

    pub(super) fn take(&mut self, index: usize) -> Val {
        std::mem::take(&mut self.body[index])
    }

    pub(super) fn remove(&mut self, index: usize) {
        self.body.remove(index);
    }

    pub(super) fn splice(&mut self, skip: usize, take: usize, values: &Self) -> Result<(), Error> {
        for value in values {
            child(value)?;
        }
        self.reserve(self.len().saturating_sub(take).saturating_add(values.len()))?;
        // Splice an exact-size iterator; no uncharged temporary Vec of clones.
        self.body.splice(skip..skip + take, values.iter().cloned());
        self.body.resync();
        Ok(())
    }

    pub(super) fn append(&mut self, values: &Self) -> Result<(), Error> {
        self.splice(self.len(), 0, values)
    }

    pub(super) fn subtract(&mut self, values: &Self) -> Result<(), Error> {
        // Keep the lookup index metered, with O(m log m + n log m) work.
        // Sorting references in place needs no temporary value bodies.
        meter::check(values.len().saturating_mul(size_of::<&Val>()))?;
        let mut sorted = Metered::new(values.iter().collect::<Vec<_>>());
        sorted.sort_unstable_by(|left, right| {
            meter::tick();
            left.cmp(right)
        });
        self.body.retain(|value| {
            meter::tick();
            sorted.binary_search(&value).is_err()
        });
        Ok(())
    }
}

impl Container<Map> {
    pub(super) fn insert(&mut self, key: Val, value: Val) -> Result<(), Error> {
        child(&key)?;
        child(&value)?;
        if !self.body.contains_key(&key) && self.len() == self.capacity() {
            // IndexMap chooses geometric capacities for its entry/index tables.
            // Admit a conservative full replacement while the old body lives.
            // Include IndexMap's minimum table allocation even at len == 0.
            let capacity = self.len().saturating_add(1).next_power_of_two().max(4);
            let bytes = capacity
                .saturating_mul(2)
                .saturating_mul(size_of::<(Val, Val)>() + 2 * size_of::<usize>());
            meter::check(bytes)?;
            self.body.reserve(1);
            self.body.resync();
        }
        self.body.insert(key, value);
        Ok(())
    }

    pub(super) fn take(&mut self, key: &Val) -> Option<Val> {
        self.body.get_mut(key).map(std::mem::take)
    }

    pub(super) fn remove(&mut self, key: &Val) {
        self.body.swap_remove(key);
    }

    pub(super) fn append(&mut self, values: &Self) -> Result<(), Error> {
        for (key, value) in values {
            self.insert(key.clone(), value.clone())?;
        }
        Ok(())
    }
}

pub(super) fn array(iter: impl IntoIterator<Item = Val>) -> Container<Vec<Val>> {
    let mut array = Container::<Vec<Val>>::default();
    for value in iter {
        if array.push(value).is_err() {
            meter::require(usize::MAX);
            break; // panic-abort: never continue consuming an unbounded iterator
        }
    }
    array
}

pub(super) fn object(iter: impl IntoIterator<Item = (Val, Val)>) -> Result<Val, Error> {
    let mut object = Container::<Map>::default();
    for (key, value) in iter {
        if !matches!(key, Val::TStr(_) | Val::BStr(_)) {
            // Keep jq's existing object-key diagnostic.
            return super::jaq_json::invalid_object_key(&key);
        }
        object.insert(key, value)?;
    }
    Ok(Val::Obj(Rc::new(object)))
}

/// Charged scratch for object ordering/hashing; references cannot mutate values.
pub(super) fn entries(object: &Container<Map>) -> Metered<Vec<(&Val, &Val)>> {
    if !meter::require(object.len().saturating_mul(size_of::<(&Val, &Val)>())) {
        return Metered::default();
    }
    Metered::new(object.iter().collect())
}

pub(super) fn merge(left: &mut Rc<Container<Map>>, right: Rc<Container<Map>>) -> Result<(), Error> {
    let left = Rc::make_mut(left);
    for (key, value) in super::jaq_json::rc_unwrap_or_clone(right) {
        let value = match (left.take(&key), value) {
            (Some(Val::Obj(mut left)), Val::Obj(right)) => {
                merge(&mut left, right)?;
                Val::Obj(left)
            }
            (_, value) => value,
        };
        left.insert(key, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
