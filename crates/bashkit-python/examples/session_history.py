"""Session persistence with bashkit snapshot history.

Stores one snapshot per conversation message, then rewinds and branches from
any point — the pattern an agent product needs for "edit an earlier message"
and "explore an alternative from here".

The naive version of this is one full snapshot per message, which re-encodes
the whole workspace every turn. Here each commit stores only what changed:
unchanged files cost a 32-byte hash reference, and a branch shares everything
with its parent.

Storage is a plain SQLite table, because bashkit never touches your database.
`commit()` hands back objects; `checkout()` takes them. Swap SQLite for
Postgres, S3, or a dict — nothing else changes.

Run:
    maturin develop -m crates/bashkit-python/Cargo.toml
    python crates/bashkit-python/examples/session_history.py
"""

import sqlite3

from bashkit import Bash, SnapshotGraph


class ObjectStore:
    """Content-addressed object store on SQLite.

    Objects are immutable and keyed by their own hash, so `INSERT OR IGNORE`
    is the whole write path — two sessions committing identical content
    converge on one row rather than conflicting.
    """

    def __init__(self, db: sqlite3.Connection) -> None:
        self.db = db
        self.db.execute("CREATE TABLE IF NOT EXISTS objects (id TEXT PRIMARY KEY, blob BLOB)")
        self.db.execute(
            "CREATE TABLE IF NOT EXISTS messages ("
            "  seq INTEGER PRIMARY KEY AUTOINCREMENT,"
            "  text TEXT NOT NULL,"
            "  commit_id TEXT NOT NULL)"
        )

    def put_all(self, objects: dict[str, bytes]) -> None:
        self.db.executemany(
            "INSERT OR IGNORE INTO objects (id, blob) VALUES (?, ?)",
            objects.items(),
        )
        self.db.commit()

    def known_ids(self) -> list[str]:
        """Ids already stored, passed to `have=` so commits stay incremental."""
        return [row[0] for row in self.db.execute("SELECT id FROM objects")]

    def load_all(self) -> dict[str, bytes]:
        return {row[0]: row[1] for row in self.db.execute("SELECT id, blob FROM objects")}

    def load(self, ids: list[str]) -> dict[str, bytes]:
        """Fetch specific objects — the lazy path, used with plan_checkout."""
        marks = ",".join("?" * len(ids))
        rows = self.db.execute(f"SELECT id, blob FROM objects WHERE id IN ({marks})", ids)
        return {row[0]: row[1] for row in rows}

    def total_bytes(self) -> int:
        return self.db.execute("SELECT COALESCE(SUM(LENGTH(blob)), 0) FROM objects").fetchone()[0]

    def record_message(self, text: str, commit_id: str) -> int:
        cur = self.db.execute("INSERT INTO messages (text, commit_id) VALUES (?, ?)", (text, commit_id))
        self.db.commit()
        return cur.lastrowid

    def commit_for_message(self, seq: int) -> str:
        return self.db.execute("SELECT commit_id FROM messages WHERE seq = ?", (seq,)).fetchone()[0]


def run_turn(bash: Bash, store: ObjectStore, text: str, script: str, parent: str | None) -> str:
    """Execute one conversational turn and commit the resulting state."""
    bash.execute_sync(script)

    packed = bash.commit(
        parents=[parent] if parent else None,
        # Without `have`, every commit would be self-contained and re-store the
        # entire workspace. With it, only new content is emitted.
        have=store.known_ids(),
        meta={"message": text},
    )
    store.put_all(packed.objects)
    seq = store.record_message(text, packed.id)

    print(f"  turn {seq}: {packed.object_count:>2} new objects, {packed.stored_bytes:>6} bytes  [{packed.id[:12]}]")
    return packed.id


def main() -> None:
    store = ObjectStore(sqlite3.connect(":memory:"))
    bash = Bash(username="agent", max_commands=100_000)

    print("Conversation:")
    parent = None
    turns = [
        ("set up the project", "mkdir -p /project/src && echo 'print(1)' > /project/src/main.py"),
        ("add a README", "echo '# Project' > /project/README.md"),
        ("add tests", "mkdir -p /project/tests && echo 'def test(): pass' > /project/tests/t.py"),
        ("fix the entrypoint", "echo 'print(42)' > /project/src/main.py"),
    ]
    for text, script in turns:
        parent = run_turn(bash, store, text, script, parent)

    tip = parent
    print(f"\nWhole history: {store.total_bytes()} bytes for {len(turns)} snapshots")

    # ---- Rewind -----------------------------------------------------------
    # Truncating a conversation is just checking out an earlier commit. No
    # replay, no copy — and the later commits stay intact in the store.
    print("\nRewind to turn 2:")
    at_turn_2 = store.commit_for_message(2)
    rewound = Bash(username="agent")
    rewound.checkout(at_turn_2, store.load_all())
    print("  files:", rewound.execute_sync("find /project -type f | sort").stdout.split())
    print("  main.py:", rewound.execute_sync("cat /project/src/main.py").stdout.strip())

    # ---- Branch -----------------------------------------------------------
    # A fork is a commit whose parent is not the tip. Both branches share every
    # unchanged object with their common ancestor.
    print("\nBranch from turn 2:")
    branch = Bash(username="agent")
    branch.checkout(at_turn_2, store.load_all())
    before = store.total_bytes()
    branch_tip = run_turn(
        branch, store, "try a different approach", "echo 'print(99)' > /project/src/main.py", at_turn_2
    )
    print(f"  branch cost {store.total_bytes() - before} bytes on top of the shared history")

    # ---- Inspect ----------------------------------------------------------
    objects = store.load_all()

    print("\nWhat changed on the branch:")
    diff = SnapshotGraph.diff(at_turn_2, branch_tip, objects)
    print(f"  added:    {diff.files_added}")
    print(f"  modified: {diff.files_modified}")
    print(f"  removed:  {diff.files_removed}")

    print("\nAncestry of the main branch tip (newest first):")
    for commit_id in SnapshotGraph.ancestry(tip, objects):
        message = SnapshotGraph.meta(commit_id, objects).get("message", "?")
        print(f"  {commit_id[:12]}  {message}")

    # ---- Lazy checkout ----------------------------------------------------
    # For a large workspace you may not want every object in memory. Ask what
    # is missing, fetch only that, and repeat until the plan comes back empty.
    print("\nLazy checkout (fetching only what each round needs):")
    local: dict[str, bytes] = {}
    rounds = 0
    while True:
        needed = SnapshotGraph.plan_checkout(branch_tip, local)
        if not needed:
            break
        rounds += 1
        print(f"  round {rounds}: fetching {len(needed)} objects")
        local.update(store.load(needed))

    lazy = Bash(username="agent")
    lazy.checkout(branch_tip, local)
    print("  main.py:", lazy.execute_sync("cat /project/src/main.py").stdout.strip())

    # ---- Garbage collection ----------------------------------------------
    # `reachable` reports what a commit needs, so you can drop the rest when a
    # branch is deleted. Bashkit reports reachability; retention is your call.
    live = set(SnapshotGraph.reachable(tip, objects)) | set(SnapshotGraph.reachable(branch_tip, objects))
    print(f"\nGC: {len(live)} of {len(objects)} objects reachable from the two tips")


if __name__ == "__main__":
    main()
