# whh — Specification

> *What's happened, happened.*

- Status: draft 1, ready for implementation (Week 0)
- Date: 2026-10-04
- Scope: the `whh` crate only. `daddb` (operations = SQL statements) is a separate spec.

---

## 1. What whh is

whh is a replication engine for local-first, peer-to-peer apps.

Each device keeps a full copy of the data. Devices can write while offline. When two devices meet, each sends what the other is missing. Once all devices hold the same set of writes, they all have the same state.

whh does this with an **operation log**. Every write is recorded as an entry in an append-only log. The state is a deterministic function of the log: start from empty, and apply every entry in one fixed total order. Merging two replicas means taking the union of their logs. A union cannot fail, and it does not care about order or duplicates. So replicas always converge.

whh does not know what an operation means. To whh, an operation is an opaque byte string. The meaning comes from a **machine** that the app provides. The first machine will be `daddb`, where each operation is one SQL statement. whh itself knows nothing about SQL.

**About the name.** It comes from the film *Tenet*. In whh, entries are facts. Once an entry is in the log, it is never changed or removed. What can change is the state computed from the facts, when a fact from the past arrives late. The past does not change; we only learn about it later.

---

## 2. Goals and non-goals

### Goals

- **Convergence.** Two replicas that hold the same set of entries have identical state. This holds no matter in what order, how late, or how many times the entries arrived.
- **Any deterministic machine works.** Operations do not need to commute.
- **No offline write is lost or refused because it arrives late.**
- **No coordinator, no primary device, no consensus.**
- **Small and easy to verify.**

### Non-goals (excluded on purpose)

- Finalization (a point after which history can no longer change).
- Log compaction, snapshots, garbage collection.
- Membership. Adding or removing devices is not a whh concept.
- Authentication, authorization, encryption, transport. These belong to other layers (§13).
- Character-level merging of text for real-time co-editing.
- Replaying only part of the state (for example, one table). This may come later (§15).

---

## 3. Model

### 3.1 Space

A **space** is the unit of replication. One space has one log and one state. Sync always transfers a whole space. There is no partial sync. A device can hold many spaces, and they are independent of each other.

**Why.** The unit of sync is also the unit of sharing. Anyone who receives a space receives its whole log. So data that should be shared with a different group of people must live in a different space. The expected usage is many small spaces, not one big one.

### 3.2 Site

A **site** is one replica of one space on one device. Each site has a `SiteId`: 16 random bytes, created when the replica is created. A site writes entries only under its own `SiteId`.

Two replicas must never share a `SiteId`. If you copy a replica's files to another device and open them there, both copies have the same `SiteId`. This breaks convergence (§11). A new device must create a new replica with a new `SiteId`, and fill it through normal sync.

### 3.3 Entry, key, and order

```
entry = (hlc, site, op)
key   = (hlc, site)
```

- `hlc`: a `u64` hybrid logical clock value (§4).
- `site`: the `SiteId` of the writer.
- `op`: opaque bytes. Only the machine interprets them.

The key is unique within a space. Keys are totally ordered:

1. Compare `hlc` as an unsigned integer.
2. If equal, compare `site` as bytes (lexicographic).

This order is the same on every device. It never depends on when an entry arrived.

### 3.4 State

```
state(log) = fold(apply, empty, entries of log sorted by key)
```

This is the whole definition. Every other rule in this document exists to compute this function cheaply while entries keep arriving out of order.

**Why an operation log, not a state-based CRDT.**

1. Any operation can be replicated, including operations that change structure. For daddb these are `CREATE TABLE` and `ALTER TABLE`. Structure cannot be merged as a state-based CRDT, because there must be exactly one structure, not several in parallel. In a log, a structure change is just one more entry.
2. The state stays plain. The machine stores only its own data, with no per-field clocks and no tombstones.
3. Merging is set union. It cannot fail and needs no conflict rules.
4. It needs much less code.

**Why this is still a CRDT.** The replicated object is the set of entries. Union is commutative, associative, and idempotent, so the set is a grow-only set CRDT. The state is a pure function of that set. This pattern is known as a *pure operation-based CRDT* (Baquero, Almeida, Shoker). Automerge and Yjs follow the same idea internally.

**Why a total order instead of commutative operations.** Classic operation-based CRDTs need operations that commute. SQL statements do not commute. A fixed total order removes this need: every replica applies the same entries in the same order, so any deterministic machine converges. The cost is that a late entry may force the entries after it to be applied again (§8.4).

**Conflicts are resolved by the order.** When two entries touch the same thing, the entry with the larger key wins, because it is applied later. For daddb, two `UPDATE`s of the same column resolve as last-writer-wins by HLC. whh needs no other conflict rule.

---

## 4. Hybrid logical clock (HLC)

### 4.1 Format

64 bits:

- upper 48 bits: physical time, in milliseconds since the Unix epoch
- lower 16 bits: a counter

```
pack(ms, counter) = (ms << 16) | counter
physical(hlc)     = hlc >> 16
```

48 bits of milliseconds are enough until the year 10889.

### 4.2 Operations

Each site keeps one value, `last`.

```
now():
    last = max(pack(wall_ms(), 0), last + 1)
    return last

observe(remote_hlc):
    last = max(last, remote_hlc)
```

When the counter overflows, `last + 1` carries into the physical part. This is fine. HLC values are only compared as integers.

The wall clock must be injectable (a `Clock` trait), so tests can move time forward and backward.

### 4.3 Persistence

`last` must never go backward, also across restarts. whh does not need to store it separately. On open:

```
last = max hlc in the log   (0 if the log is empty)
```

This is safe for two reasons. First, `observe()` is only called for entries that are appended to the log. Second, every value from `now()` is either used by an entry that is appended, or it is thrown away together with a rejected write (§8.2). A thrown-away value was never used by any entry, so forgetting it after a restart is harmless.

**Why an HLC.**

- **A wall clock alone** is not monotonic, has ties, and can be skewed. With skew, an entry written after reading another entry can sort before it.
- **A Lamport clock alone** follows causality but has nothing to do with real time. A device that wrote many entries gets large counters, and its old writes beat newer writes from other devices. Users expect "the most recent edit wins" in real time.
- **An HLC gives both.** If a site writes entry `b` after it observed entry `a`, then `b > a`. And the value stays close to wall time, so in normal cases the order matches what a person expects.

---

## 5. Version vector

The **version vector** `vv` maps each `SiteId` to the highest `hlc` held from that site. It is derived from the log: for each site, take the maximum `hlc` among its entries. It may be cached.

**What the version vector is for.** It has one job: deciding what to send during sync. A peer sends its version vector in `Hello`. The other side then sends, for each site, the entries above that site's number. The peer does not need to talk to every site directly. Entries of a site it never meets arrive through other peers.

**Version vector vs. replay.** These solve different problems. The version vector makes sure no entry is missed. Replay makes sure the received entries are applied in the right order. An entry can be new for its own site (above `vv[site]`, so it is sent) and old for the whole log (below `W`, so it triggers a replay). Both are true at the same time.

### 5.1 The prefix property

> For every site `s`, the entries of `s` held by any replica form a prefix of `s`'s own sequence: all of `s`'s entries up to some `hlc`, with no gaps.

This holds because:

1. A site's own entries have strictly increasing `hlc` (§4).
2. Senders send entries in ascending key order, so each site's entries are sent in ascending order (§9).
3. Receivers ingest in order. For each site, they stop at the first entry they cannot take (§8.3, §12).

Because of the prefix property, `vv[s]` alone says exactly which entries of `s` a replica has: all entries with `hlc <= vv[s]`, and none above.

**Why not one cursor.** A natural idea is "send me everything after key X". It fails, because an entry's `hlc` is the time it was written, not the time it arrived. Example:

1. A and B synced. Both hold everything up to `hlc` 50.
2. C was offline and wrote an entry at `hlc` 30.
3. C syncs with B. B now holds entry 30.
4. A asks B for "everything after 50". A never receives entry 30.

A per-site vector has no such hole. A holds nothing from C, so `vv_A[C] = 0`, and B sends all of C's entries.

The root cause is that whh has no finality. Across the whole log, no point in history is closed: a late entry can always land before the newest entry a replica holds. So "everything after X" is never safe. Within one site, however, history is closed. A site never writes an entry below its own previous `hlc`, so once a replica holds a site's entries up to `vv[s]`, no new entry of that site can appear below `vv[s]`. The version vector is one cursor per site, and it works because each site's own history is closed.

**Why entries are forwarded.** A replica sends the entries of every site it holds, not only its own. So A learns about C's entries through B, even if A and C never meet. Devices do not need to connect to each other directly.

---

## 6. Storage and atomicity

Persistent data per replica:

| Data     | Content                                                                 | Kind     |
|----------|-------------------------------------------------------------------------|----------|
| `meta`   | `space_id`, `site_id`, `watermark` (a key, or none), `dirty` (bool)    | stored   |
| `log`    | all entries; primary key `(hlc, site)`; an index on `(site, hlc)` is recommended for §9 | stored |
| `failed` | set of `(key, reason)` for entries the machine rejected                 | derived, rebuilt by replay |
| `state`  | owned by the machine                                                    | derived  |

`vv` and the HLC value `last` are derived from the log. They may be cached.

**Atomicity requirement.** The log, `meta`, `failed`, and the machine state must be changed in **one transaction**. Each step in §8 either commits fully or not at all. For daddb this comes for free, because all of them live in the same SQLite file.

**Why.** A local write applies an operation to the state and appends it to the log. If a crash could keep one without the other, this replica's state would contain an operation that no other replica will ever receive. The replicas would then diverge without anyone noticing.

---

## 7. The machine contract

```
apply(state, op) -> Applied | Rejected(reason) | Fatal(error)
reset(state)     -> make the state empty
```

Rules:

1. **Deterministic.** The result and the new state depend only on the current state and `op`. They must not depend on the wall clock, randomness, local settings, which site runs the operation, or anything else outside `(state, op)`.
2. **Atomic per operation.** After `Rejected`, the state is exactly as it was before the call.
3. **`Rejected` and `Fatal` are different things.**
   - `Rejected` means the operation is invalid for the current state. Examples: the table does not exist, or the primary key already exists. This outcome is deterministic. Every replica gets the same result at the same position in the log.
   - `Fatal` means the environment failed. Examples: an I/O error, a full disk, out of memory. This outcome is not deterministic. On `Fatal`, whh aborts the whole transaction and returns the error. Nothing is recorded. The step can be retried later.

**Why the difference matters.** If a full disk were recorded as `Rejected`, this replica would compute a different state than other replicas for the same log. Only deterministic outcomes may become part of history.

**Machine versions.** If the machine's behavior changes between software versions, replicas on different versions may compute different states from the same log. Handling this is an open item (§16).

---

## 8. Algorithm

### 8.1 Watermark and dirty flag

The **watermark** `W` is the key of the last entry applied to the state.

> **Invariant.** When `dirty` is false, the state equals the fold of all log entries with key `<= W`, in key order.

Entries with key `> W` may exist in the log without being applied yet. They are pending, and the next settle (§8.4) applies them.

`dirty = true` means the log holds at least one entry with key `< W` that the state does not reflect. The state is then stale, and only a full replay fixes it.

While `dirty` is true, the state is stale but **consistent**: it is the exact state of an earlier set of entries. Reads are safe. They may only be missing the late entries.

**Why a watermark.** It splits new arrivals into two cases:

- A **new** entry (key `> W`) extends history at the end. It is applied on top of the current state.
- A **late** entry (key `< W`) belongs in the middle of history. The state has already passed that point, so the state must be rebuilt.

Without `W`, every arrival would need a full replay.

### 8.2 Local write

```
write(op):
    begin transaction
    settle()                          // §8.4: the state must be current
    hlc = clock.now()
    key = (hlc, my_site)              // always > W, since hlc > every observed hlc
    match machine.apply(op):
        Applied     -> log.append(key, op); W = key; commit; return Ok(key)
        Rejected(r) -> rollback; return Err(Rejected(r))   // nothing is logged
        Fatal(e)    -> rollback; return Err(Fatal(e))
```

**Why settle first.** The writer expects the operation to act on the latest known state. On a stale state, the operation could succeed locally and then fail in the next replay, or the other way around.

**Why a rejected local operation is not logged.** The author gets the error at once and can fix it. Logging it would only add noise to every replica's log.

Note: an operation that was applied locally can still be rejected later, when a late entry lands before it (§8.5). This is the only way a write can fail after the fact.

### 8.3 Ingest

Input: a batch of entries from a peer, sorted ascending by key. Ingest only writes the log. It never touches the state.

```
ingest(batch):
    begin transaction
    blocked = {}                           // sites skipped for the rest of this batch
    for e in batch:
        if e.site in blocked: continue
        if size(e.op) > max_op_bytes or not admit(e):         // §12
            blocked.add(e.site); report refused(e.site); continue
        if e.hlc <= vv[e.site]:
            if log.get(e.key) == Some(e.op): continue          // duplicate, ignore
            return Violation(e.site)                            // §11; rollback
        if physical(e.hlc) > wall_ms() + max_future_skew:      // §12
            blocked.add(e.site); continue                       // deferred, not dropped
        log.append(e)
        vv[e.site] = e.hlc
        clock.observe(e.hlc)
        if W is set and e.key < W:
            dirty = true
    commit
```

Each chunk of a sync session can be ingested in its own transaction. The prefix property holds after every commit.

**Why skip the rest of a site's entries after one is skipped.** Taking a later entry of the same site would create a gap and break the prefix property (§5.1). The skipped entries are not lost. The sender will send them again in a later session.

### 8.4 Settle

```
settle():
    if dirty:
        replay()
    else:
        for e in log where e.key > W, ascending:
            apply_one(e)
            W = e.key

apply_one(e):
    match machine.apply(e.op):
        Applied     -> ok
        Rejected(r) -> failed.insert(e.key, r)
        Fatal(err)  -> abort the transaction, return err

replay():
    machine.reset()
    failed.clear()
    for e in log, ascending:
        apply_one(e)
    W = key of the last entry (none if the log is empty)
    dirty = false
```

Replay runs in one transaction. Readers never see a half-built state. (With SQLite in WAL mode, readers keep seeing the old state until the commit.)

**When settle runs.** Before every local write (§8.2), at the end of every sync session, and whenever the app asks for it (for example, before a read that must be current). The app or the sync driver decides the schedule. The recommended rule is **at most once per sync session**, not once per chunk.

**Why replay the whole space.**

1. It is simple. There is one `W` and one `dirty` flag per space.
2. An operation may touch any part of the state. For daddb this means joins, `INSERT ... SELECT`, and transactions over several tables are all allowed. Replaying only one part would require knowing which parts each operation touches. Replaying everything needs no such knowledge.
3. It is fast enough at the expected scale. Teller writes about 100 entries per day. Two years is about 73,000 entries, about 15 MB. This should replay in well under a second. This must be measured (§15).

**Why replay from empty, not from a snapshot.** A late entry can land anywhere in history, including before a snapshot. That snapshot would then be wrong and would have to be rebuilt. Keeping snapshots correct costs more code than it saves time at this scale.

### 8.5 Report

Every `settle` returns a report, so the app can tell the user what changed:

- `replayed`: whether a full replay ran
- `applied`: keys applied for the first time
- `newly_failed`: keys that are rejected now but were not rejected before (this includes entries rejected on their first apply)
- `recovered`: keys that were rejected before and are applied now

`newly_failed` and `recovered` come from comparing the old `failed` set with the new one.

**A rejection is not permanent.** The `failed` set is rebuilt from scratch on every replay. An entry rejected at one position may succeed once a late entry lands before it. Example: an `UPDATE` of a column that does not exist yet is rejected. Later, the `ALTER TABLE` that adds the column arrives, and it has a smaller key. On replay, the `UPDATE` succeeds.

**Why failures are reported, not hidden.** Example: two sites insert rows with the same primary key. The entry with the smaller key wins. After replay, the other insert is rejected, and the row its author saw disappears. whh cannot prevent this; it is a key design problem for the app (§13). But whh must make it visible. An entry in `newly_failed` whose site is the local site means: "one of your own writes was undone by history." The app should tell the user.

---

## 9. Sync protocol

whh does not care about the transport. It defines two messages:

```
Hello   { protocol: u16, space_id: SpaceId, vv: [(SiteId, u64)] }
Entries { entries: [Entry], last: bool }
```

A session:

1. Both sides send `Hello`.
2. If `protocol` or `space_id` differ, close the session.
3. Each side computes what the peer is missing. For each site `s` in its own `vv`, it selects the entries of `s` with `hlc > peer_vv[s]` (use 0 if the peer does not list `s`). It sends them sorted by key, in chunks of at most `max_entries_per_message`. The final chunk has `last = true`.
4. Each side ingests every chunk it receives (§8.3).
5. When it has sent its own last chunk and ingested the peer's last chunk, it runs `settle` (§8.4).

Properties:

- **Stateless.** Nothing about a session is stored. To resume, start a new session with a new `Hello`.
- **Safe to interrupt** at any point. Every committed chunk keeps the prefix property.
- **Idempotent.** Running a session twice changes nothing the second time.
- **One round is enough.** After one complete session (with no deferred entries and no new writes during it), both sides hold the same set of entries.

Sending in global key order is stricter than needed. Only ascending order per site is required. Global key order also puts late entries first in a batch, which is convenient for §8.3.

The wire encoding is the implementation's choice (for example, `postcard` with `serde`). It must include the `protocol` number.

---

## 10. Why replicas converge

1. **Same log.** Merging is set union. After all replicas have exchanged entries, all logs hold the same set.
2. **Same order.** Keys are unique and totally ordered (§3.3), so every replica walks the same sequence.
3. **Same results.** The machine is deterministic and starts from empty (§7). So every replica gets the same results at every position, the same final state, and the same `failed` set.
4. **Direct apply equals replay.** When `dirty` is false, applying the entries above `W` in order extends the fold exactly. When a late entry arrives, `dirty` forces a replay. So after every settle, `state = state(log)`.

This argument needs one assumption: no two different entries share a key. §11 covers it.

---

## 11. Integrity

The only way to break convergence is **two different operations with the same key**. Two replicas could each keep a different one, and the set union would never fix it.

This can only happen through bugs:

- two replicas share a `SiteId` (for example, copied files, §3.2), or
- a site's HLC went backward (for example, a lost `last`, §4.3).

Both are prevented by design. They are still checked, because the damage is silent and permanent.

**Guard.** During ingest (§8.3), an entry with `hlc <= vv[site]` must already be in the log with identical `op` bytes. If the key is missing, or the bytes differ, ingest returns `Violation(site)` and rolls back the batch.

On a `Violation`, stop ingesting from that peer and report the site to the app. Do not try to repair anything automatically.

---

## 12. Clock skew, admission, and limits

### 12.1 Entries from the future

`max_future_skew` (default: 10 minutes) limits how far ahead of the local wall clock an entry's physical time may be. An entry beyond this limit is **deferred**: it is not ingested now, and the rest of that site's entries in the batch are skipped (§8.3). The sender will send them again in a later session. They are accepted once the local clock catches up.

**Why not accept it.** `observe()` would move the local clock far into the future. Every later local write would carry that future time and win every conflict. One device with a broken clock, or one malicious peer, could damage every replica permanently.

**Why not drop it.** Dropping would leave a permanent gap for that site. Deferring keeps the prefix property and loses nothing.

**Cost.** A device whose clock runs ahead by more than the limit sees its entries delayed by the difference. This is accepted.

### 12.2 Admission hook

The layer above may install an `admit(entry) -> bool` hook, for example to check a signature or a write permission. A refused entry is handled like a deferred one: that site is skipped for the rest of the batch, and the refusal is reported.

Two consequences:

- A refusal blocks all later entries from the same site, because of the prefix property. A misbehaving site therefore stops being accepted at all.
- `admit` should depend only on the entry and on configuration that every replica shares. If replicas use different rules, their logs may never become equal. (They still never diverge for the same log; they just never get the same log.)

### 12.3 Limits

- `max_op_bytes` (default: 1 MiB)
- `max_entries_per_message` (default: 1,000)

---

## 13. Responsibilities of other layers

whh guarantees that replicas converge. It says nothing about which writes are good.

- **Machine (daddb).** Determinism. Reject non-deterministic operations at write time (for daddb: `random()`, `datetime('now')`, `AUTOINCREMENT`, and similar). The app supplies ids and timestamps as values. Atomicity per operation.
- **App: key design.** Primary keys must not collide across sites. Use ids that each site generates on its own: a UUID, or `SiteId` plus a local counter. If two sites insert the same key, one wins and the other is rejected, possibly after the fact (§8.5). Finalization would not fix this either: two inserts after a finalization point collide in the same way. daddb should refuse table definitions with collision-prone keys as a guardrail.
- **Permission layer.** Who may write what in a shared space, and signatures. whh accepts any well-formed entry as valid history, including a malicious one, such as a backdated `DROP TABLE`. Such entries must be refused before ingest (through `admit`, §12.2), or the space must not be shared with untrusted peers.
- **Transport (for example, iroh).** Connections, peer discovery, encryption in transit.

---

## 14. Accepted costs and risks

1. **The log grows forever.** About 15 MB per two years at Teller's rate. Accepted. To be measured.
2. **Late entries change the visible state.** This is correct behavior, for example when an offline device catches up. Changes are reported (§8.5).
3. **A late entry causes a full replay.** The cost grows with the log size.
4. **A misbehaving peer can cause repeated replays.** It can create backdated entries (each only needs to be above its own previous `hlc`), so each session triggers a replay. This is bounded by running settle at most once per session, and prevented by the permission layer. It is a performance problem, not a convergence problem.
5. **No entry is ever refused for being old.** Without finalization, there is no point in history that is closed.

---

## 15. Possible later work (not now)

- **Partial replay** (for example, per table), if measurements show full replay is too slow. This would require the machine to declare which parts each operation touches.
- **Checkpoints.** Cache the state at some key, and throw the cache away when a late entry lands before it.
- **Fast bootstrap.** Give a new device a copy of the current state plus the log (with a new `SiteId`), instead of replaying from empty.
- **Measurement.** Replay time for 10⁴, 10⁵, and 10⁶ entries, with a toy machine and with daddb.

---

## 16. Open items

- How machines version their behavior, and what happens when devices in one space run different versions.
- The exact shape of the `admit` hook, and how blocked sites are reported.
- Default values for the limits in §12.

---

## 17. API sketch (not binding)

This is a starting point. Change it if a better shape appears during implementation.

```rust
pub struct SiteId(pub [u8; 16]);
pub struct SpaceId(pub [u8; 16]);
pub struct Hlc(pub u64);

#[derive(PartialEq, Eq, PartialOrd, Ord)]   // order: hlc, then site
pub struct Key { pub hlc: Hlc, pub site: SiteId }

pub struct Entry { pub key: Key, pub op: Vec<u8> }

pub enum Outcome { Applied, Rejected(String) }

/// Everything whh needs, inside one transaction (§6).
pub trait Transaction {
    type Error;                                         // always fatal
    // log and meta
    fn append(&mut self, e: &Entry) -> Result<(), Self::Error>;
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, Self::Error>;
    fn scan_after(&self, after: Option<&Key>) -> Result<Vec<Entry>, Self::Error>; // may become an iterator
    fn scan_site_after(&self, site: &SiteId, hlc: Hlc) -> Result<Vec<Entry>, Self::Error>;
    fn version_vector(&self) -> Result<Vec<(SiteId, Hlc)>, Self::Error>;
    fn meta(&self) -> Result<Meta, Self::Error>;
    fn set_meta(&mut self, m: &Meta) -> Result<(), Self::Error>;
    fn failed(&self) -> Result<Vec<(Key, String)>, Self::Error>;
    fn set_failed(&mut self, key: &Key, reason: &str) -> Result<(), Self::Error>;
    fn clear_failed(&mut self) -> Result<(), Self::Error>;
    // machine
    fn apply(&mut self, op: &[u8]) -> Result<Outcome, Self::Error>;
    fn reset_state(&mut self) -> Result<(), Self::Error>;
    fn commit(self) -> Result<(), Self::Error>;
}

pub trait Backend {
    type Tx<'a>: Transaction where Self: 'a;
    fn begin(&mut self) -> Result<Self::Tx<'_>, <Self::Tx<'_> as Transaction>::Error>;
}

pub trait Clock { fn wall_ms(&self) -> u64; }

pub struct Replica<B: Backend, C: Clock> { /* ... */ }

impl<B: Backend, C: Clock> Replica<B, C> {
    pub fn create(backend: B, clock: C, space: SpaceId, config: Config) -> Result<Self, Error>;
    pub fn open(backend: B, clock: C, config: Config) -> Result<Self, Error>;

    pub fn write(&mut self, op: Vec<u8>) -> Result<Key, WriteError>; // Rejected | Fatal
    pub fn settle(&mut self) -> Result<Report, Error>;
    pub fn is_dirty(&self) -> bool;
    pub fn failed(&self) -> Result<Vec<(Key, String)>, Error>;

    // sync, transport-agnostic
    pub fn hello(&self) -> Result<Hello, Error>;
    pub fn missing_for(&self, peer: &Hello) -> Result<Vec<Entries>, Error>; // chunked
    pub fn ingest(&mut self, chunk: &Entries) -> Result<IngestReport, Error>; // Violation is an error
}
```

whh ships two things for testing:

- `MemBackend`: an in-memory backend that implements the transaction rules.
- `KvMachine`: a small deterministic machine (see §19).

The SQLite backend and the SQL machine belong to daddb, not to whh.

---

## 18. Invariants

Each of these must be checked by tests.

- **I1. Key uniqueness.** No log holds two different operations with the same key.
- **I2. Prefix.** For every site, the entries a replica holds form a prefix of that site's sequence.
- **I3. Version vector.** `vv[s]` equals the maximum `hlc` of site `s` in the log.
- **I4. Clock.** `last >= ` the maximum `hlc` in the log, and every `now()` returns a value greater than every earlier one.
- **I5. State.** After any settle, `dirty` is false, and the state and `failed` set equal those produced by a replay from empty.
- **I6. Convergence.** Two replicas with equal logs have equal state and equal `failed` sets after settle.
- **I7. Local writes.** The key of every local write is greater than `W` at the time of the write.

---

## 19. Test plan (Week 0)

### 19.1 Test machine

`KvMachine`, a key-value map with these operations:

- `Insert(k, v)`: rejected if `k` exists
- `Put(k, v)`: always applied (overwrite)
- `Delete(k)`: rejected if `k` does not exist
- A fault switch (not an operation): when set, the next `apply` returns `Fatal`. Used only in fault-injection tests.

### 19.2 Unit tests

- HLC: `now()` is strictly increasing when the wall clock stands still, jumps forward, or jumps backward. Counter overflow carries into the physical part.
- Key order: `hlc` first, then `site` bytes.

### 19.3 Scenario tests

1. **Late entry and last-writer-wins.** A writes `Put(k, 1)` at t=200. B writes `Put(k, 2)` at t=100 while offline. After sync, both show `k = 1`, and A reports `replayed`.
2. **Rejected after the fact.** A writes `Insert(k, a)` at t=200 and it is applied. B writes `Insert(k, b)` at t=100 while offline. After sync, both show `k = b`. A's insert appears in A's `newly_failed`.
3. **Rejected between two entries of another site.** B writes `Insert(k, x)` at t=100 and `Delete(k)` at t=300. A writes `Insert(k, y)` at t=200 while offline. After sync, the order is B's insert (applied), A's insert (rejected, the key exists), B's delete (applied). Both replicas show `k` absent. A's insert appears in A's `newly_failed`.
4. **Recovered.** Build the log by ingest only: ingest `Delete(k)` from site S1 at t=300 → settle → it is in `failed`. Then ingest `Insert(k, x)` from site S2 at t=100 → settle → replay; `Delete(k)` is in `recovered`, and `k` is absent.
5. **The single-cursor hole (§5.1).** A and B synced up to t=50. C, whose test clock is behind, writes at t=30 and syncs with B. A syncs with B and receives C's entry.
6. **Duplicates and reordering.** Deliver the same chunks twice and in different session orders. The final state is the same.
7. **Violation.** Ingest an entry with an existing key and different `op`. Ingest returns `Violation`, and the log is unchanged.
8. **Future skew.** An entry 1 hour ahead is deferred, together with that site's later entries. After the test clock advances, a new session accepts them.
9. **Fatal during replay.** Set the fault switch so that an `apply` in the middle of a replay returns `Fatal`. The replay aborts. Nothing is committed. After clearing the switch, settle produces the correct state.
10. **Rejected local write.** It returns `Rejected`, and the log, `W`, and the state are unchanged.

### 19.4 Property test (proptest)

- 2 to 5 sites, each with its own test clock.
- A random sequence of actions:
  - `write(site, op)` with random `KvMachine` operations over a small key space (so conflicts are common)
  - `sync(a, b)`, sometimes cut off after a random number of chunks
  - move a site's clock forward or backward
- At the end, fully sync all pairs until nothing changes.
- Check I1 to I7. Check that all states and `failed` sets are equal, and equal to a replay from empty of the shared log.

### 19.5 Week 0 deliverable

The `whh` crate with `Replica`, `MemBackend`, `KvMachine`, and all tests above passing. daddb starts after this.
