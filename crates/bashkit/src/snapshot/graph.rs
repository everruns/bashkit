// Decision: bashkit never calls into host storage. `commit` hands back objects
// and `checkout` takes them, so persistence stays async, language-specific, and
// entirely the host's business. A store trait would have to cross PyO3 and
// wasm-bindgen as an async callback — the hardest part of the design to build,
// bind, and test, in exchange for nothing the pull model cannot express.
//
// Decision: `plan_checkout` returns only what is missing *and currently
// reachable*. Chunk IDs live inside file manifests, so a caller that has no
// objects yet cannot learn them in one round. Callers loop until the plan comes
// back empty; the graph is four levels deep, so that terminates in <= 4 rounds.

//! Walking, planning, and diffing the snapshot object graph.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use super::capabilities::CapabilityFingerprint;
use super::chunker;
use super::container;
use super::objects::{
    self, CommitId, CommitObject, Encoded, FileContent, KIND_CAPS, KIND_CHUNK, KIND_COMMIT,
    KIND_FILE, KIND_SHELL, KIND_TREE, ObjectId, TreeEntry, TreeEntryKind,
};
use super::{SNAPSHOT_VERSION, Snapshot, SnapshotOptions};
use crate::FsLimits;
use crate::interpreter::{ShellState, ShellStateOptions};

/// Hard ceiling on commits walked by [`SnapshotGraph::ancestry`], so a cyclic
/// or adversarial parent chain cannot spin forever (TM-SNAP-005).
const MAX_ANCESTRY: usize = 1_000_000;

/// Absolute ceiling on checkout materialization, including unlimited backends.
const MAX_CHECKOUT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Running total of file bytes materialized during one checkout.
struct CheckoutBudget {
    remaining: u64,
    max_file_size: u64,
}

impl CheckoutBudget {
    fn new(limits: &FsLimits) -> Self {
        Self {
            remaining: limits.max_total_bytes.min(MAX_CHECKOUT_BYTES),
            max_file_size: limits.max_file_size.min(MAX_CHECKOUT_BYTES),
        }
    }

    fn spend_file(&mut self, bytes: u64) -> crate::Result<()> {
        if bytes > self.max_file_size {
            return Err(crate::Error::Internal(format!(
                "snapshot file size {bytes} exceeds the {} byte filesystem limit",
                self.max_file_size
            )));
        }
        self.remaining = self.remaining.checked_sub(bytes).ok_or_else(|| {
            crate::Error::Internal(
                "snapshot checkout exceeds the filesystem materialization limit".to_string(),
            )
        })?;
        Ok(())
    }
}

/// Read access to a content-addressed object store.
///
/// Implemented for the standard maps, so a host can pass whatever it already
/// uses to hold fetched objects.
pub trait ObjectSource {
    /// Fetch the storage blob for `id`, or `None` if this source lacks it.
    fn get_object(&self, id: &ObjectId) -> Option<&[u8]>;
}

impl ObjectSource for HashMap<ObjectId, Vec<u8>> {
    fn get_object(&self, id: &ObjectId) -> Option<&[u8]> {
        self.get(id).map(Vec::as_slice)
    }
}

impl ObjectSource for BTreeMap<ObjectId, Vec<u8>> {
    fn get_object(&self, id: &ObjectId) -> Option<&[u8]> {
        self.get(id).map(Vec::as_slice)
    }
}

impl<T: ObjectSource + ?Sized> ObjectSource for &T {
    fn get_object(&self, id: &ObjectId) -> Option<&[u8]> {
        (**self).get_object(id)
    }
}

/// How a commit is built: its place in the history, and what to leave out.
#[derive(Debug, Clone, Default)]
pub struct CommitOptions {
    pub(crate) parents: Vec<CommitId>,
    pub(crate) meta: BTreeMap<String, String>,
    pub(crate) snapshot: SnapshotOptions,
    pub(crate) have: HashSet<ObjectId>,
}

impl CommitOptions {
    /// A root commit that captures everything and assumes an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Descend from `parent`. Call more than once for a merge commit; pass a
    /// commit that is not the branch tip to fork.
    pub fn parent(mut self, parent: CommitId) -> Self {
        self.parents.push(parent);
        self
    }

    /// Attach opaque host metadata (message id, timestamp, author). Bashkit
    /// stores it verbatim and never interprets it — it has no clock on wasm
    /// targets and no concept of a session.
    pub fn meta(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.meta.insert(key.into(), value.into());
        self
    }

    /// Object IDs the store already holds, so they are not emitted again.
    ///
    /// This is what makes a commit incremental: pass `store.keys()`.
    pub fn have<'a, I: IntoIterator<Item = &'a ObjectId>>(mut self, ids: I) -> Self {
        self.have.extend(ids.into_iter().copied());
        self
    }

    /// Capture shell state only, skipping the filesystem.
    pub fn exclude_filesystem(mut self, exclude: bool) -> Self {
        self.snapshot.exclude_filesystem = exclude;
        self
    }

    /// Skip shell functions.
    pub fn exclude_functions(mut self, exclude: bool) -> Self {
        self.snapshot.exclude_functions = exclude;
        self
    }
}

/// A commit plus the objects the store does not already have.
#[derive(Debug, Clone)]
pub struct PackedCommit {
    pub(crate) id: CommitId,
    pub(crate) objects: BTreeMap<ObjectId, Vec<u8>>,
    pub(crate) self_contained: bool,
}

impl PackedCommit {
    /// The commit's content address — the value a host stores per message.
    pub fn id(&self) -> CommitId {
        self.id
    }

    /// Objects to persist, borrowed.
    pub fn objects(&self) -> impl Iterator<Item = (ObjectId, &[u8])> {
        self.objects.iter().map(|(id, blob)| (*id, blob.as_slice()))
    }

    /// Objects to persist, owned.
    pub fn into_objects(self) -> impl Iterator<Item = (ObjectId, Vec<u8>)> {
        self.objects.into_iter()
    }

    /// Number of new objects.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Total encoded size of the new objects, in bytes.
    ///
    /// This is what an incremental commit actually costs the store.
    pub fn stored_bytes(&self) -> usize {
        self.objects.values().map(Vec::len).sum()
    }

    /// Whether this commit carries every object needed to restore it.
    ///
    /// False once [`CommitOptions::have`] has excluded anything.
    pub fn is_self_contained(&self) -> bool {
        self.self_contained
    }

    /// Serialize into one integrity-protected blob.
    ///
    /// # Errors
    ///
    /// Fails when the commit is not self-contained — packing an incremental
    /// commit would silently produce bytes that cannot be restored.
    pub fn to_bytes(&self) -> crate::Result<Vec<u8>> {
        self.pack(None)
    }

    /// Like [`to_bytes`](Self::to_bytes) but authenticated with a secret key.
    pub fn to_bytes_keyed(&self, key: &[u8]) -> crate::Result<Vec<u8>> {
        self.pack(Some(key))
    }

    fn pack(&self, key: Option<&[u8]>) -> crate::Result<Vec<u8>> {
        if !self.self_contained {
            return Err(crate::Error::Internal(
                "cannot pack an incremental commit: it omits objects the store already holds"
                    .to_string(),
            ));
        }
        let body = container::encode(self.id, &self.objects);
        Ok(super::seal(&body, key))
    }
}

/// What changed between two commits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotDiff {
    /// Paths present in the second commit only.
    pub files_added: Vec<String>,
    /// Paths in both, with different content, mode, or type.
    pub files_modified: Vec<String>,
    /// Paths present in the first commit only.
    pub files_removed: Vec<String>,
    /// Whether shell state (variables, env, cwd, functions, …) differs.
    pub shell_changed: bool,
}

impl SnapshotDiff {
    /// True when nothing changed.
    pub fn is_empty(&self) -> bool {
        self.files_added.is_empty()
            && self.files_modified.is_empty()
            && self.files_removed.is_empty()
            && !self.shell_changed
    }
}

/// Read-only operations over a snapshot object graph.
///
/// Bashkit does not own the store, so every method takes the objects it needs.
/// A method only walks as far as the supplied [`ObjectSource`] reaches.
pub struct SnapshotGraph;

impl SnapshotGraph {
    /// Decode a commit object.
    pub fn read_commit(root: CommitId, source: &impl ObjectSource) -> crate::Result<CommitObject> {
        let payload = load(root, KIND_COMMIT, source)?;
        objects::decode_commit(&payload)
    }

    /// Commits `root` descends from.
    pub fn parents(root: CommitId, source: &impl ObjectSource) -> crate::Result<Vec<CommitId>> {
        Ok(Self::read_commit(root, source)?.parents)
    }

    /// Host metadata attached at commit time.
    pub fn meta(
        root: CommitId,
        source: &impl ObjectSource,
    ) -> crate::Result<BTreeMap<String, String>> {
        Ok(Self::read_commit(root, source)?.meta)
    }

    /// Capability fingerprint of the instance that produced `root`.
    pub fn capabilities(
        root: CommitId,
        source: &impl ObjectSource,
    ) -> crate::Result<CapabilityFingerprint> {
        let commit = Self::read_commit(root, source)?;
        let payload = load(commit.caps, KIND_CAPS, source)?;
        objects::from_canonical_json(&payload)
    }

    /// Walk ancestry from `root`, newest first, stopping at `limit` commits or
    /// at the first ancestor the source does not contain.
    ///
    /// Cycles are impossible in a well-formed graph (a commit's ID covers its
    /// parents) but a hostile store can serve one anyway, so already-visited
    /// commits are skipped rather than trusted.
    pub fn ancestry(
        root: CommitId,
        source: &impl ObjectSource,
        limit: usize,
    ) -> crate::Result<Vec<CommitId>> {
        let limit = limit.min(MAX_ANCESTRY);
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([root]);
        let mut out = Vec::new();

        while let Some(id) = queue.pop_front() {
            if out.len() >= limit || !seen.insert(id) {
                continue;
            }
            // A pruned ancestor and a corrupt one are different answers.
            // Absence stops the walk; anything present that fails to verify or
            // decode is an integrity failure and must not be reported as
            // history that simply ended here.
            if source.get_object(&id).is_none() {
                continue;
            }
            let commit = Self::read_commit(id, source)?;
            out.push(id);
            queue.extend(commit.parents);
        }

        Ok(out)
    }

    /// Object IDs needed to check out `root` that `source` does not have.
    ///
    /// Call repeatedly — fetching one wave reveals the next — until it returns
    /// an empty vector.
    pub fn plan_checkout(
        root: CommitId,
        source: &impl ObjectSource,
    ) -> crate::Result<Vec<ObjectId>> {
        let mut missing = BTreeSet::new();
        walk(root, source, &mut missing, &mut BTreeSet::new())?;
        Ok(missing.into_iter().collect())
    }

    /// Every object `root` reaches, for a host implementing garbage collection.
    ///
    /// # Errors
    ///
    /// Fails if any reachable object is missing from `source`; use
    /// [`plan_checkout`](Self::plan_checkout) when partial results are fine.
    pub fn reachable(root: CommitId, source: &impl ObjectSource) -> crate::Result<Vec<ObjectId>> {
        let mut missing = BTreeSet::new();
        let mut present = BTreeSet::new();
        walk(root, source, &mut missing, &mut present)?;
        if let Some(id) = missing.iter().next() {
            return Err(crate::Error::Internal(format!(
                "snapshot object {id} is missing from the store"
            )));
        }
        Ok(present.into_iter().collect())
    }

    /// Compare two commits.
    pub fn diff(
        a: CommitId,
        b: CommitId,
        source: &impl ObjectSource,
    ) -> crate::Result<SnapshotDiff> {
        let commit_a = Self::read_commit(a, source)?;
        let commit_b = Self::read_commit(b, source)?;

        let tree_a = read_tree(commit_a.tree, source)?;
        let tree_b = read_tree(commit_b.tree, source)?;

        let map_a: BTreeMap<&str, &TreeEntry> =
            tree_a.iter().map(|e| (e.path.as_str(), e)).collect();
        let map_b: BTreeMap<&str, &TreeEntry> =
            tree_b.iter().map(|e| (e.path.as_str(), e)).collect();

        let mut diff = SnapshotDiff {
            shell_changed: commit_a.shell != commit_b.shell,
            ..Default::default()
        };

        for (path, entry) in &map_b {
            match map_a.get(path) {
                None => diff.files_added.push((*path).to_string()),
                // Content pointers are content addresses, so comparing them is
                // comparing content — no need to fetch chunks to detect a change.
                Some(before) if before.mode != entry.mode || before.kind != entry.kind => {
                    diff.files_modified.push((*path).to_string());
                }
                Some(_) => {}
            }
        }
        for path in map_a.keys() {
            if !map_b.contains_key(path) {
                diff.files_removed.push((*path).to_string());
            }
        }

        Ok(diff)
    }

    /// Rebuild the state a commit describes, without touching any instance.
    pub(crate) fn materialize(
        root: CommitId,
        source: &impl ObjectSource,
        limits: &FsLimits,
    ) -> crate::Result<(Snapshot, CapabilityFingerprint)> {
        let commit = Self::read_commit(root, source)?;

        let caps_payload = load(commit.caps, KIND_CAPS, source)?;
        let caps: CapabilityFingerprint = objects::from_canonical_json(&caps_payload)?;

        let shell_payload = load(commit.shell, KIND_SHELL, source)?;
        let shell: ShellState = objects::from_canonical_json(&shell_payload)?;

        let vfs = match commit.tree {
            None => None,
            Some(tree_id) => {
                let entries = objects::decode_tree(&load(tree_id, KIND_TREE, source)?)?;
                // One budget for the whole tree: many small files must not add
                // up to more than a single huge one is allowed to be.
                let mut budget = CheckoutBudget::new(limits);
                Some(objects::tree_to_vfs(&entries, |file_id| {
                    resolve_file(file_id, source, &mut budget)
                })?)
            }
        };

        Ok((
            Snapshot {
                version: SNAPSHOT_VERSION,
                shell,
                vfs,
                session_commands: commit.session_commands,
                session_exec_calls: commit.session_exec_calls,
            },
            caps,
        ))
    }
}

/// Load and verify a single object, checking that it is the kind expected.
fn load(id: ObjectId, kind: u8, source: &impl ObjectSource) -> crate::Result<Vec<u8>> {
    let blob = source
        .get_object(&id)
        .ok_or_else(|| crate::Error::Internal(format!("snapshot object {id} not found")))?;
    let raw = container::from_storage_blob(blob)?;
    let object = Encoded::from_storage(id, &raw)?;
    object.expect_kind(kind)?;
    Ok(object.payload)
}

fn read_tree(tree: Option<ObjectId>, source: &impl ObjectSource) -> crate::Result<Vec<TreeEntry>> {
    match tree {
        None => Ok(Vec::new()),
        Some(id) => objects::decode_tree(&load(id, KIND_TREE, source)?),
    }
}

/// Reassemble one file's content from its manifest and chunks.
fn resolve_file(
    file_id: ObjectId,
    source: &impl ObjectSource,
    budget: &mut CheckoutBudget,
) -> crate::Result<Vec<u8>> {
    match objects::decode_file(&load(file_id, KIND_FILE, source)?)? {
        FileContent::Inline(bytes) => {
            budget.spend_file(bytes.len() as u64)?;
            Ok(bytes)
        }
        FileContent::Chunked { size, chunks } => {
            // Charge the *declared* size to the budget before allocating, so a
            // manifest cannot commit us to work the budget would refuse.
            budget.spend_file(size)?;
            let mut out =
                Vec::with_capacity(usize::try_from(size).unwrap_or_default().min(1 << 24));

            for chunk_id in chunks {
                let chunk = load(chunk_id, KIND_CHUNK, source)?;
                // Format constant: the chunker never emits a larger chunk, so a
                // bigger one means a hostile or corrupt manifest.
                if chunk.len() > chunker::MAX_CHUNK {
                    return Err(crate::Error::Internal(format!(
                        "snapshot chunk {chunk_id} is {} bytes, above the {} byte maximum",
                        chunk.len(),
                        chunker::MAX_CHUNK
                    )));
                }
                // Stop at the declared size rather than after the fact. A
                // manifest repeating one chunk id would otherwise grow `out`
                // without bound before the equality check below could fire.
                if out.len() as u64 + chunk.len() as u64 > size {
                    return Err(crate::Error::Internal(format!(
                        "snapshot file {file_id} has chunks exceeding its declared {size} bytes"
                    )));
                }
                out.extend_from_slice(&chunk);
            }

            if out.len() as u64 != size {
                return Err(crate::Error::Internal(format!(
                    "snapshot file {file_id} reassembled to {} bytes but declares {size}",
                    out.len()
                )));
            }
            Ok(out)
        }
    }
}

/// Breadth-first walk of the graph, recording what is present and what is not.
fn walk(
    root: CommitId,
    source: &impl ObjectSource,
    missing: &mut BTreeSet<ObjectId>,
    present: &mut BTreeSet<ObjectId>,
) -> crate::Result<()> {
    // (id, expected kind). Kinds are known from context, so a store cannot
    // smuggle a chunk in where a tree belongs.
    let mut queue: VecDeque<(ObjectId, u8)> = VecDeque::from([(root, KIND_COMMIT)]);
    let mut seen: HashSet<ObjectId> = HashSet::new();

    while let Some((id, kind)) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        let Some(blob) = source.get_object(&id) else {
            missing.insert(id);
            continue;
        };
        let raw = container::from_storage_blob(blob)?;
        let object = Encoded::from_storage(id, &raw)?;
        object.expect_kind(kind)?;
        present.insert(id);

        match kind {
            KIND_COMMIT => {
                let commit = objects::decode_commit(&object.payload)?;
                queue.push_back((commit.shell, KIND_SHELL));
                queue.push_back((commit.caps, KIND_CAPS));
                if let Some(tree) = commit.tree {
                    queue.push_back((tree, KIND_TREE));
                }
                // Parents are history, not content: a checkout of this commit
                // does not need them, so the walk deliberately stops here.
            }
            KIND_TREE => {
                for entry in objects::decode_tree(&object.payload)? {
                    if let TreeEntryKind::File(file_id) = entry.kind {
                        queue.push_back((file_id, KIND_FILE));
                    }
                }
            }
            KIND_FILE => {
                if let FileContent::Chunked { chunks, .. } = objects::decode_file(&object.payload)?
                {
                    for chunk in chunks {
                        queue.push_back((chunk, KIND_CHUNK));
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

impl crate::Bash {
    /// Capture state as a content-addressed commit.
    ///
    /// The returned [`PackedCommit`] holds the objects to persist and the
    /// [`CommitId`] to remember. Pass the store's existing IDs via
    /// [`CommitOptions::have`] to keep consecutive commits incremental — an
    /// unchanged file costs one hash reference, not a copy.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, CommitOptions};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::new();
    /// bash.exec("echo hello > /greeting.txt").await?;
    ///
    /// let commit = bash.commit(CommitOptions::new())?;
    /// println!("{} objects, {} bytes", commit.object_count(), commit.stored_bytes());
    /// # Ok(())
    /// # }
    /// ```
    pub fn commit(&self, options: CommitOptions) -> crate::Result<PackedCommit> {
        let shell = self
            .interpreter
            .shell_state_with_options(ShellStateOptions {
                include_functions: !options.snapshot.exclude_functions,
            });
        let vfs = if options.snapshot.exclude_filesystem {
            None
        } else {
            self.fs().vfs_snapshot()
        };

        let mut emitted: Vec<Encoded> = Vec::new();

        let tree_id = match &vfs {
            None => None,
            Some(vfs) => {
                let (tree, content) = objects::build_tree(vfs)?;
                let id = tree.id;
                emitted.push(tree);
                emitted.extend(content);
                Some(id)
            }
        };

        let shell_obj = Encoded::new(KIND_SHELL, objects::canonical_json(&shell)?);
        let caps_obj = Encoded::new(
            KIND_CAPS,
            objects::canonical_json(&CapabilityFingerprint::capture(self))?,
        );
        let shell_id = shell_obj.id;
        let caps_id = caps_obj.id;
        emitted.push(shell_obj);
        emitted.push(caps_obj);

        let counters = self.interpreter.counters();
        let commit_obj = objects::encode_commit(&CommitObject {
            parents: options.parents.clone(),
            tree: tree_id,
            shell: shell_id,
            caps: caps_id,
            session_commands: counters.session_commands,
            session_exec_calls: counters.session_exec_calls,
            meta: options.meta.clone(),
        })?;
        let commit_id = commit_obj.id;
        emitted.push(commit_obj);

        let mut store = BTreeMap::new();
        for object in emitted {
            // The commit itself is new by construction; everything else is
            // skipped when the host already holds it.
            if object.id != commit_id && options.have.contains(&object.id) {
                continue;
            }
            store.entry(object.id).or_insert_with(|| {
                container::storage_blob_from_parts(object.kind, &object.payload)
            });
        }

        Ok(PackedCommit {
            id: commit_id,
            objects: store,
            self_contained: options.have.is_empty(),
        })
    }

    /// Restore the state a commit describes, pulling objects from `source`.
    ///
    /// This is how forks and rewinds work: check out any commit, including one
    /// that is not the branch tip. The instance's own configuration (limits,
    /// builtins, filesystem backend) is preserved.
    ///
    /// # Errors
    ///
    /// Fails if an object is missing or fails hash verification, or if the
    /// capability fingerprint violates `policy`. Nothing is mutated on failure.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, CheckoutPolicy, CommitOptions, ObjectId};
    /// use std::collections::HashMap;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut store: HashMap<ObjectId, Vec<u8>> = HashMap::new();
    /// let mut bash = Bash::new();
    /// bash.exec("echo saved > /state.txt").await?;
    ///
    /// let commit = bash.commit(CommitOptions::new())?;
    /// let id = commit.id();
    /// store.extend(commit.into_objects());
    ///
    /// let mut restored = Bash::new();
    /// restored.checkout(id, &store, CheckoutPolicy::Strict)?;
    /// assert_eq!(restored.exec("cat /state.txt").await?.stdout, "saved\n");
    /// # Ok(())
    /// # }
    /// ```
    pub fn checkout(
        &mut self,
        root: CommitId,
        source: &impl ObjectSource,
        policy: super::CheckoutPolicy,
    ) -> crate::Result<()> {
        let (snapshot, caps) = SnapshotGraph::materialize(root, source, &self.fs.limits())?;
        self.apply_checked(&snapshot, &caps, policy)
    }

    /// Shared tail of every restore path: capability gate, state-evidence gate,
    /// then the one restore routine both formats funnel into.
    pub(crate) fn apply_checked(
        &mut self,
        snapshot: &Snapshot,
        caps: &CapabilityFingerprint,
        policy: super::CheckoutPolicy,
    ) -> crate::Result<()> {
        let live = CapabilityFingerprint::capture(self);
        policy.check(&caps.compare(&live))?;
        if let Some(vfs) = &snapshot.vfs {
            super::capabilities::check_state_evidence(vfs)?;
        }
        self.restore_snapshot_inner(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::objects::{encode_file, encode_tree};

    /// Build a store entry for an object, exactly as `commit` would.
    fn store_object(store: &mut HashMap<ObjectId, Vec<u8>>, object: &Encoded) -> ObjectId {
        store.insert(
            object.id,
            container::storage_blob_from_parts(object.kind, &object.payload),
        );
        object.id
    }

    /// A file manifest that names one real chunk many times over.
    ///
    /// The manifest is tiny and hashes correctly, so hash verification cannot
    /// catch it — this is precisely the input the materialization budget and
    /// declared-size check exist for (TM-SNAP-004).
    fn chunk_bomb(
        store: &mut HashMap<ObjectId, Vec<u8>>,
        repeats: usize,
        declared_size: u64,
    ) -> ObjectId {
        let chunk = Encoded::new(KIND_CHUNK, vec![b'A'; chunker::MAX_CHUNK]);
        let chunk_id = store_object(store, &chunk);
        let manifest = encode_file(&FileContent::Chunked {
            size: declared_size,
            chunks: vec![chunk_id; repeats],
        });
        store_object(store, &manifest)
    }

    #[test]
    fn a_repeated_chunk_cannot_grow_a_file_past_its_declared_size() {
        let mut store = HashMap::new();
        // 64 chunks of 64 KiB = 4 MiB of real content, but the manifest claims
        // the file is only 1 KiB. Assembly must stop at the declared size.
        let file_id = chunk_bomb(&mut store, 64, 1024);

        let mut budget = CheckoutBudget::new(&FsLimits::unlimited());
        let err = resolve_file(file_id, &store, &mut budget).unwrap_err();
        assert!(
            err.to_string().contains("declared"),
            "expected a declared-size rejection, got: {err}"
        );
    }

    #[test]
    fn an_absurd_declared_size_is_refused_before_allocating() {
        let mut store = HashMap::new();
        // The manifest is a few hundred bytes; the claim is terabytes. The
        // budget has to reject this on the declared size alone, because
        // allocating first is the failure mode.
        let file_id = chunk_bomb(&mut store, 4, u64::MAX / 2);

        let mut budget = CheckoutBudget::new(&FsLimits::unlimited());
        let err = resolve_file(file_id, &store, &mut budget).unwrap_err();
        assert!(
            err.to_string().contains("filesystem limit"),
            "expected a budget rejection, got: {err}"
        );
    }

    #[test]
    fn configured_file_limit_is_refused_before_materializing_chunks() {
        let mut store = HashMap::new();
        let file_id = chunk_bomb(&mut store, 2, (chunker::MAX_CHUNK * 2) as u64);
        let limits = crate::FsLimits::new().max_file_size(chunker::MAX_CHUNK as u64);

        let mut budget = CheckoutBudget::new(&limits);
        let err = resolve_file(file_id, &store, &mut budget).unwrap_err();
        assert!(
            err.to_string().contains("file size"),
            "expected a configured file-size rejection, got: {err}"
        );
    }

    #[test]
    fn the_budget_is_shared_across_every_file_in_a_tree() {
        // Many individually-legal files must not add up past the ceiling.
        let mut budget = CheckoutBudget::new(&FsLimits::unlimited());
        assert!(budget.spend_file(MAX_CHECKOUT_BYTES / 2).is_ok());
        assert!(budget.spend_file(MAX_CHECKOUT_BYTES / 2).is_ok());
        assert!(
            budget.spend_file(1).is_err(),
            "the budget must be cumulative, not per-file"
        );
    }

    #[test]
    fn an_oversized_chunk_is_rejected() {
        // The chunker never emits one, so a larger chunk means the manifest is
        // hostile or corrupt even when every hash checks out.
        let mut store = HashMap::new();
        let chunk = Encoded::new(KIND_CHUNK, vec![b'B'; chunker::MAX_CHUNK + 1]);
        let chunk_id = store_object(&mut store, &chunk);
        let manifest = encode_file(&FileContent::Chunked {
            size: chunker::MAX_CHUNK as u64 + 1,
            chunks: vec![chunk_id],
        });
        let file_id = store_object(&mut store, &manifest);

        let mut budget = CheckoutBudget::new(&FsLimits::unlimited());
        let err = resolve_file(file_id, &store, &mut budget).unwrap_err();
        assert!(
            err.to_string().contains("maximum"),
            "expected an oversized-chunk rejection, got: {err}"
        );
    }

    #[test]
    fn a_well_formed_chunked_file_still_resolves() {
        // The guards must not reject legitimate multi-chunk content.
        let mut store = HashMap::new();
        let a = Encoded::new(KIND_CHUNK, vec![b'x'; 1000]);
        let b = Encoded::new(KIND_CHUNK, vec![b'y'; 500]);
        let a_id = store_object(&mut store, &a);
        let b_id = store_object(&mut store, &b);
        let manifest = encode_file(&FileContent::Chunked {
            size: 1500,
            chunks: vec![a_id, b_id],
        });
        let file_id = store_object(&mut store, &manifest);

        let mut budget = CheckoutBudget::new(&FsLimits::unlimited());
        let out = resolve_file(file_id, &store, &mut budget).unwrap();
        assert_eq!(out.len(), 1500);
        assert_eq!(&out[..1000], &vec![b'x'; 1000][..]);
        assert_eq!(&out[1000..], &vec![b'y'; 500][..]);
    }

    #[test]
    fn a_tree_of_many_bomb_files_is_bounded_in_aggregate() {
        // Each file is individually under the ceiling; together they are not.
        let mut store = HashMap::new();
        let chunk = Encoded::new(KIND_CHUNK, vec![b'C'; chunker::MAX_CHUNK]);
        let chunk_id = store_object(&mut store, &chunk);

        let mut entries = Vec::new();
        for i in 0..64 {
            let manifest = encode_file(&FileContent::Chunked {
                size: MAX_CHECKOUT_BYTES / 8,
                chunks: vec![chunk_id],
            });
            let file_id = store_object(&mut store, &manifest);
            entries.push(TreeEntry {
                path: format!("/f{i}"),
                mode: 0o644,
                kind: TreeEntryKind::File(file_id),
            });
        }
        let tree = encode_tree(&entries);
        let entries = objects::decode_tree(&tree.payload).unwrap();

        let mut budget = CheckoutBudget::new(&FsLimits::unlimited());
        let result = objects::tree_to_vfs(&entries, |file_id| {
            resolve_file(file_id, &store, &mut budget)
        });
        assert!(
            result.is_err(),
            "a tree of oversized files must exhaust the shared budget"
        );
    }
}
