# whh — Specification

> *What's happened, happened.*

- Status: draft 2, ready for implementation (Week 0)
- Date: 2026-10-09
- Scope: the `whh` crate only. `daddb` (operations = SQL statements) is a separate spec.
- Changes from draft 1: compaction is added (§9). Finality is an app decision, not a whh feature. whh only provides the minimal tools the app needs to compact a space.

---

## 1. What whh is

whh is a replication engine for local-first, peer-to-peer apps.

Each device keeps a full copy of the data. Devices can write while offline. When two devices meet, each sends what the other is missing. Once all devices hold the same set of writes, they all have the same state.

whh does this with an **operation log**. Every write is recorded as an entry in an append-only log. The state is a deterministic function of the log: start from empty, and apply every entry in one fixed total order. Merging two replicas means taking the union of their logs. A union cannot fail, and it does not care about order or duplicates. So replicas always converge.

> All replicas that have the same operations reach the same state, as long as every operation is deterministic.

whh does not know what an operation means. To whh, an operation is an opaque byte string. The meaning comes from a **machine** that the app provides. The first machine will be `daddb`, where each operation is one SQL statement. whh itself knows nothing about SQL.

whh never decides on its own that part of history is final. If the app knows that an old part of the log will not change anymore, it can tell whh to **compact** the log up to that point (§9).

**About the name.** It comes from the film *Tenet*. In whh, entries are facts. Once an entry is in the log, it is never changed. What can change is the state computed from the facts, when a fact from the past arrives late. The past does not change; we only learn about it later.

---

## 2. Goals and non-goals

### Goals

- **Convergence.** Two replicas that hold the same set of entries have identical state. This holds no matter in what order, how late, or how many times the entries arrived.
- **Any deterministic machine works.** Operations do not need to commute.
- **No offline write is lost or refused because it arrives late.** The only exception is an entry at or below a base that the app chose (§9).
- **No coordinator, no primary device, no consensus** inside whh.
- **Small and easy to verify.**

### Non-goals (excluded on purpose)

- Finality decided by whh. When history is final is an app decision (§9.1). whh does not run consensus, track membership, or check whether the app's decision is right.
- Verifying snapshots. A replica trusts the base it receives, just as it trusts that other replicas apply the log correctly.
- Membership. Adding or removing devices is not a whh concept.
- Authentication, authorization, encryption, transport. These belong to other layers (§14).
- Character-level merging of text for real-time co-editing.
- Replaying only part of the state (for example, one table). This may come later (§16).

---

## 3. Model

### 3.1 Space

A **space** is the unit of replication. One space has one log and one state. Sync always transfers a whole space. There is no partial sync. A device can hold many spaces, and they are independent of each other.

**Why.** The unit of sync is also the unit of sharing. Anyone who receives a space receives its whole log. So data that should be shared with a different group of people must live in a different space. The expected usage is many small spaces, not one big one.

### 3.2 Site

A **site** is one replica of one space on one device. Each site has a `SiteId`: 16 random bytes, created when the replica is created. A site writes entries only under its own `SiteId`.

Two replicas must never share a `SiteId`. If you copy a replica's files to another device and open them there, both copies have the same `SiteId`. This breaks convergence (§12). A new device must create a new replica with a new `SiteId`, and fill it through normal sync.

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
state(log) = fold(apply, base_state, entries of log sorted by key)
```

`base_state` is the empty state, unless the space was compacted. In that case it is the state saved in the base (§9.2), and the log holds only entries above the base.

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
last = max(max hlc in the log, hlc of the base key)   (0 if both are missing)
```

The base key is included because compaction deletes entries, and the deleted entries may have held the highest `hlc`.

This is safe for two reasons. First, `observe()` is only called for entries that are appended to the log and for bases that are adopted. Second, every value from `now()` is either used by an entry that is appended, or it is thrown away together with a rejected write (§8.2). A thrown-away value was never used by any entry, so forgetting it after a restart is harmless.

**Why an HLC.**

- **A wall clock alone** is not monotonic, has ties, and can be skewed. With skew, an entry written after reading another entry can sort before it.
- **A Lamport clock alone** follows causality but has nothing to do with real time. A device that wrote many entries gets large counters, and its old writes beat newer writes from other devices. Users expect "the most recent edit wins" in real time.
- **An HLC gives both.** If a site writes entry `b` after it observed entry `a`, then `b > a`. And the value stays close to wall time, so in normal cases the order matches what a person expects.

---

## 5. Version vector

The **version vector** `vv` maps each `SiteId` to the highest `hlc` held from that site. It is derived from the log: for each site, take the maximum `hlc` among its entries. A site that is not listed has the value 0. It may be cached.

**What the version vector is for.** It has one job: deciding what to send during sync. A peer sends its version vector in `Hello`. The other side then sends, for each site, the entries above that site's number. The peer does not need to talk to every site directly. Entries of a site it never meets arrive through other peers.

**Version vector vs. replay.** These solve different problems. The version vector makes sure no entry is missed. Replay makes sure the received entries are applied in the right order. An entry can be new for its own site (above `vv[site]`, so it is sent) and old for the whole log (below `W`, so it triggers a replay). Both are true at the same time.

**After compaction.** Compaction deletes entries, so `vv[s]` may drop, even to 0. This is fine and needs no extra storage. `Hello` also carries the base key, and the sender never sends entries at or below the peer's base (§10).

### 5.1 The prefix property

> For every site `s`, the entries of `s` held by any replica above its base form a prefix of the rest of `s`'s own sequence: all of `s`'s entries above the base, up to some `hlc`, with no gaps.

This holds because:

1. A site's own entries have strictly increasing `hlc` (§4).
2. Senders send entries in ascending key order, so each site's entries are sent in ascending order (§10).
3. Receivers ingest in order. For each site, they stop at the first entry they cannot take (§8.3, §13).
4. Compaction only removes entries at or below the base. What is left above the base has no gaps.

Because of the prefix property, `vv[s]` alone says exactly which entries of `s` above the base a replica has: all entries with `hlc <= vv[s]`, and none above.

**Why not one cursor.** A natural idea is "send me everything after key X". It fails, because an entry's `hlc` is the time it was written, not the time it arrived. Example:

1. A and B synced. Both hold everything up to `hlc` 50.
2. C was offline and wrote an entry at `hlc` 30.
3. C syncs with B. B now holds entry 30.
4. A asks B for "everything after 50". A never receives entry 30.

A per-site vector has no such hole. A holds nothing from C, so `vv_A[C] = 0`, and B sends all of C's entries.

The root cause is that whh itself never closes any point in history: a late entry can always land before the newest entry a replica holds. So "everything after X" is never safe. (A base, §9, is closed only because the app decided so, not because whh knows it.) Within one site, however, history is closed. A site never writes an entry below its own previous `hlc`, so once a replica holds a site's entries up to `vv[s]`, no new entry of that site can appear below `vv[s]`. The version vector is one cursor per site, and it works because each site's own history is closed.

**Why entries are forwarded.** A replica sends the entries of every site it holds, not only its own. So A learns about C's entries through B, even if A and C never meet. Devices do not need to connect to each other directly.

---

## 6. Storage and atomicity

Persistent data per replica:

| Data     | Content                                                                 | Kind     |
|----------|-------------------------------------------------------------------------|----------|
| `meta`   | `space_id`, `site_id`, `watermark` (a key, or none), `dirty` (bool)    | stored   |
| `base`   | none, or `(key, by, state)`: the compaction point, the site that made it, and the machine state at that point (§9.2) | stored |
| `log`    | all entries above the base; primary key `(hlc, site)`; an index on `(site, hlc)` is recommended for §10 | stored |
| `failed` | set of `(key, reason)` for entries the machine rejected                 | derived, rebuilt by replay |
| `state`  | owned by the machine                                                    | derived  |

`vv` and the HLC value `last` are derived from the log and the base. They may be cached.

**Atomicity requirement.** The log, `meta`, `base`, `failed`, and the machine state must be changed in **one transaction**. Each step in §8 and §9 either commits fully or not at all. For daddb this comes for free, because all of them live in the same SQLite file.

**Why.** A local write applies an operation to the state and appends it to the log. If a crash could keep one without the other, this replica's state would contain an operation that no other replica will ever receive. The replicas would then diverge without anyone noticing.

---

## 7. The machine contract

```
apply(state, op) -> Applied | Rejected(reason) | Fatal(error)
reset(state)     -> make the state empty
export(state)    -> bytes          // only needed for compaction (§9)
import(bytes)    -> state          // only needed for compaction (§9)
```

Rules:

1. **Deterministic.** The result and the new state depend only on the current state and `op`. They must not depend on the wall clock, randomness, local settings, which site runs the operation, or anything else outside `(state, op)`.
2. **Atomic per operation.** After `Rejected`, the state is exactly as it was before the call.
3. **`Rejected` and `Fatal` are different things.**
   - `Rejected` means the operation is invalid for the current state. Examples: the table does not exist, or the primary key already exists. This outcome is deterministic. Every replica gets the same result at the same position in the log.
   - `Fatal` means the environment failed. Examples: an I/O error, a full disk, out of memory. This outcome is not deterministic. On `Fatal`, whh aborts the whole transaction and returns the error. Nothing is recorded. The step can be retried later.
4. **Export and import are exact.** `import(export(s))` must behave exactly like `s` for every later `apply`. For daddb, the bytes are a copy of the SQLite database.

**Why the difference matters.** If a full disk were recorded as `Rejected`, this replica would compute a different state than other replicas for the same log. Only deterministic outcomes may become part of history.

**Machine versions.** If the machine's behavior changes between software versions, replicas on different versions may compute different states from the same log. The same is true for the format of exported state. Handling this is an open item (§17).

---

## 8. Algorithm

### 8.1 Watermark and dirty flag

The **watermark** `W` is the key of the last entry applied to the state. After compaction, `W` is never below the base key.

> **Invariant.** When `dirty` is false, the state equals the fold of all log entries with key `<= W`, in key order, starting from `base_state`.

Entries with key `> W` may exist in the log without being applied yet. They are pending, and the next settle (§8.4) applies them.

`dirty = true` means the log holds at least one entry with key `< W` that the state does not reflect, or the base was replaced (§9.4). The state is then stale, and only a full replay fixes it.

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

Note: an operation that was applied locally can still be rejected later, when a late entry lands before it (§8.5). This is the only way a write can fail after the fact. Separately, a write can be lost if the app compacts past it before this replica has synced it (§9.5).

### 8.3 Ingest

Input: a batch of entries from a peer, sorted ascending by key. Ingest only writes the log. It never touches the state.

```
ingest(batch):
    begin transaction
    blocked = {}                           // sites skipped for the rest of this batch
    for e in batch:
        if base is set and e.key <= base.key: continue          // §9.3: ignored
        if e.site in blocked: continue
        if size(e.op) > max_op_bytes or not admit(e):           // §13
            blocked.add(e.site); report refused(e.site); continue
        if e.hlc <= vv[e.site]:
            if log.get(e.key) == Some(e.op): continue            // duplicate, ignore
            return Violation(e.site)                              // §12; rollback
        if physical(e.hlc) > wall_ms() + max_future_skew:        // §13
            blocked.add(e.site); continue                         // deferred, not dropped
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
    if base is set: machine.import(base.state)
    else:           machine.reset()
    failed.clear()
    for e in log, ascending:
        apply_one(e)
    W = key of the last entry, or base.key if the log is empty (none if neither exists)
    dirty = false
```

Replay runs in one transaction. Readers never see a half-built state. (With SQLite in WAL mode, readers keep seeing the old state until the commit.)

**When settle runs.** Before every local write (§8.2), at the end of every sync session, and whenever the app asks for it (for example, before a read that must be current). The app or the sync driver decides the schedule. The recommended rule is **at most once per sync session**, not once per chunk.

**Why replay the whole space.**

1. It is simple. There is one `W` and one `dirty` flag per space.
2. An operation may touch any part of the state. For daddb this means joins, `INSERT ... SELECT`, and transactions over several tables are all allowed. Replaying only one part would require knowing which parts each operation touches. Replaying everything needs no such knowledge.
3. It is fast enough at the expected scale. Teller writes about 100 entries per day. Two years is about 73,000 entries, about 15 MB. This should replay in well under a second. This must be measured (§16). Compaction (§9) also shortens replay, because replay starts from the base.

**Why replay starts only from the base, and not from other snapshots.** A late entry can land anywhere above the base. A snapshot taken above the base would then be wrong and would have to be rebuilt. Keeping such snapshots correct costs more code than it saves time at this scale. The base is different: entries at or below it are ignored (§9.3), so nothing can make it wrong.

### 8.5 Report

Every `settle` returns a report, so the app can tell the user what changed:

- `replayed`: whether a full replay ran
- `applied`: keys applied for the first time
- `newly_failed`: keys that are rejected now but were not rejected before (this includes entries rejected on their first apply)
- `recovered`: keys that were rejected before and are applied now

`newly_failed` and `recovered` come from comparing the old `failed` set with the new one.

**A rejection is not permanent.** The `failed` set is rebuilt from scratch on every replay. An entry rejected at one position may succeed once a late entry lands before it. Example: an `UPDATE` of a column that does not exist yet is rejected. Later, the `ALTER TABLE` that adds the column arrives, and it has a smaller key. On replay, the `UPDATE` succeeds.

**Why failures are reported, not hidden.** Example: two sites insert rows with the same primary key. The entry with the smaller key wins. After replay, the other insert is rejected, and the row its author saw disappears. whh cannot prevent this; it is a key design problem for the app (§14). But whh must make it visible. An entry in `newly_failed` whose site is the local site means: "one of your own writes was undone by history." The app should tell the user.

---

## 9. Compaction (app-level finality)

### 9.1 Who decides: the app

**Finality is an app decision. whh never decides it, and never checks it.**

The app decides two things:

1. **Who may compact a space.** Exactly one administrator per space. In practice this is one site, for example the user's main device. Only the administrator calls `compact`.
2. **When, and up to which key.** The administrator picks a key `K` such that it is sure of this:

   > Every entry with key `<= K` that exists on any device is already held by the administrator.

   For example, the administrator compacts right after all devices have synced. A safer choice is to pick `K` well in the past, for example 30 days ago. Then only a device that has not synced for 30 days can be affected.

whh provides only the tools in §9.2 to §9.4. It does not know the devices of a space, does not run any agreement, and does not verify anything.

**Why the app, not whh.** Deciding finality inside whh needs either agreement among all devices (one rarely used device then blocks everyone) or rules for concurrent decisions by several devices. Both add a lot of complexity. The app knows things whh cannot know: which devices exist, who is in charge, and how careful to be. With a single trusted administrator, the whh side becomes very small.

### 9.2 The base and `compact`

A **base** is `(key, by, state)`:

- `key`: the compaction point `K`
- `by`: the `SiteId` of the administrator that created it
- `state`: the exported machine state after applying every entry with key `<= K` (§7)

A replica has at most one base. A newer base (larger key) replaces an older one.

```
compact(K):
    begin transaction
    settle()
    if base is set and K <= base.key: rollback; return Err(NotNewer)
    if W is none or K > W:              rollback; return Err(NotApplied)
    // rebuild the state up to K, export it, then continue to the end
    if base is set: machine.import(base.state) else machine.reset()
    failed.clear()
    for e in log where e.key <= K, ascending: apply_one(e)
    snapshot = machine.export()
    for e in log where e.key > K, ascending: apply_one(e)
    base = (K, my_site, snapshot)
    delete every log entry with key <= K
    commit
```

After `compact`, the state is unchanged: it was already `base_state` plus the entries above `K`.

### 9.3 Entries at or below the base

Ingest ignores every entry with key `<= base.key` (§8.3). Such an entry is not stored, not applied, and causes no replay.

**Why this is safe.** The administrator promised that every such entry is already part of the base. So ignoring it loses nothing, if the promise holds.

**Why this also helps.** A misbehaving peer can no longer cause replays with backdated entries below the base (§15).

### 9.4 Spreading the base

The base spreads through normal sync (§10). A replica that receives a newer base adopts it:

```
adopt(b):
    begin transaction
    if physical(b.key.hlc) > wall_ms() + max_future_skew: rollback; defer   // §13
    if base is set:
        if b.key < base.key: rollback; return                    // older; ignore
        if b.key == base.key:
            if b.by != base.by: rollback; return Err(ConflictingBase)
            rollback; return                                      // same base; ignore
    base = b
    delete every log entry with key <= b.key
    clock.observe(b.key.hlc)
    dirty = true                                                  // replay from the new base
    commit
```

Any replica can forward a base, not only the administrator. The base is the same everywhere, because only the administrator creates bases.

**`ConflictingBase`.** Two different bases with the same key can only come from two administrators. This breaks the rule in §9.1, and the replicas would diverge without notice. whh reports it instead of choosing one. It is a cheap guard, not a full check: bases with different keys from two administrators are not detected.

### 9.5 If the app is wrong

If the administrator compacts while some device still holds an entry with key `<= K` that the administrator did not have, that entry is **lost**:

- It is not in the base.
- Every replica ignores it once it has the base (§9.3).
- The device that held it deletes it when it adopts the base (§9.4), and it cannot tell that the entry was not included.

Replicas still converge, because all of them end with the same base and the same entries above it. Only the lost write is gone. This is the accepted cost of keeping finality simple (§15).

---

## 10. Sync protocol

whh does not care about the transport. It defines three messages:

```
Hello    { protocol: u16, space_id: SpaceId, base: Option<Key>, vv: [(SiteId, u64)] }
Base     { key: Key, by: SiteId, state: bytes }
Entries  { entries: [Entry], last: bool }
```

A session:

1. Both sides send `Hello`.
2. If `protocol` or `space_id` differ, close the session.
3. If this side's base is newer than the peer's (the peer has none, or a smaller key), it sends `Base` first.
4. Each side computes what the peer is missing. For each site `s` in its own `vv`, it selects the entries of `s` with `hlc > peer_vv[s]` (use 0 if the peer does not list `s`) and with key above the peer's base. It sends them sorted by key, in chunks of at most `max_entries_per_message`. The final chunk has `last = true`.
5. Each side adopts a received `Base` (§9.4) and ingests every chunk it receives (§8.3).
6. When it has sent its own last chunk and ingested the peer's last chunk, it runs `settle` (§8.4).

Properties:

- **Stateless.** Nothing about a session is stored. To resume, start a new session with a new `Hello`.
- **Safe to interrupt** at any point. Every committed step keeps the prefix property.
- **Idempotent.** Running a session twice changes nothing the second time.
- **One round is enough.** After one complete session (with no deferred entries and no new writes during it), both sides hold the same base and the same set of entries above it.
- **A new device needs no full history.** It receives the base and the entries above it.

Sending in global key order is stricter than needed. Only ascending order per site is required. Global key order also puts late entries first in a batch, which is convenient for §8.3.

The `Base` message can be large, because it holds the whole state. The transport may stream it.

The wire encoding is the implementation's choice (for example, `postcard` with `serde`). It must include the `protocol` number.

---

## 11. Why replicas converge

1. **Same base.** Only the administrator creates bases, and a newer base always replaces an older one. After all replicas have synced, all hold the same base.
2. **Same log above the base.** Merging is set union. After all replicas have exchanged entries, all logs hold the same set above the base.
3. **Same order.** Keys are unique and totally ordered (§3.3), so every replica walks the same sequence.
4. **Same results.** The machine is deterministic and starts from the same `base_state` (§7). So every replica gets the same results at every position, the same final state, and the same `failed` set.
5. **Direct apply equals replay.** When `dirty` is false, applying the entries above `W` in order extends the fold exactly. When a late entry arrives or the base changes, `dirty` forces a replay. So after every settle, `state = state(log)`.

This argument needs two assumptions: no two different entries share a key (§12), and only one administrator creates bases (§9.1).

---

## 12. Integrity

The only way to break convergence is **two different operations with the same key** (or two different bases with the same key, §9.4). Two replicas could each keep a different one, and the set union would never fix it.

This can only happen through bugs:

- two replicas share a `SiteId` (for example, copied files, §3.2), or
- a site's HLC went backward (for example, a lost `last`, §4.3).

Both are prevented by design. They are still checked, because the damage is silent and permanent.

**Guard.** During ingest (§8.3), an entry with `hlc <= vv[site]` must already be in the log with identical `op` bytes. If the key is missing, or the bytes differ, ingest returns `Violation(site)` and rolls back the batch. Entries at or below the base cannot be checked, because they are deleted. They are ignored instead (§9.3).

On a `Violation`, stop ingesting from that peer and report the site to the app. Do not try to repair anything automatically.

---

## 13. Clock skew, admission, and limits

### 13.1 Entries from the future

`max_future_skew` (default: 10 minutes) limits how far ahead of the local wall clock an entry's physical time may be. An entry beyond this limit is **deferred**: it is not ingested now, and the rest of that site's entries in the batch are skipped (§8.3). The sender will send them again in a later session. They are accepted once the local clock catches up. The same rule applies to the key of a received base (§9.4).

**Why not accept it.** `observe()` would move the local clock far into the future. Every later local write would carry that future time and win every conflict. One device with a broken clock, or one malicious peer, could damage every replica permanently.

**Why not drop it.** Dropping would leave a permanent gap for that site. Deferring keeps the prefix property and loses nothing.

**Cost.** A device whose clock runs ahead by more than the limit sees its entries delayed by the difference. This is accepted.

### 13.2 Admission hook

The layer above may install an `admit(entry) -> bool` hook, for example to check a signature or a write permission. A refused entry is handled like a deferred one: that site is skipped for the rest of the batch, and the refusal is reported.

The hook is also called for a received base, with its `by` and `key`. The app can use this to accept bases only from its administrator (§9.1).

Two consequences:

- A refusal blocks all later entries from the same site, because of the prefix property. A misbehaving site therefore stops being accepted at all.
- `admit` should depend only on the entry and on configuration that every replica shares. If replicas use different rules, their logs may never become equal. (They still never diverge for the same log; they just never get the same log.)

### 13.3 Limits

- `max_op_bytes` (default: 1 MiB)
- `max_entries_per_message` (default: 1,000)

---

## 14. Responsibilities of other layers

whh guarantees that replicas converge. It says nothing about which writes are good.

- **Machine (daddb).** Determinism. Reject non-deterministic operations at write time (for daddb: `random()`, `datetime('now')`, `AUTOINCREMENT`, and similar). The app supplies ids and timestamps as values. Atomicity per operation. Exact export and import of state, if compaction is used.
- **App: key design.** Primary keys must not collide across sites. Use ids that each site generates on its own: a UUID, or `SiteId` plus a local counter. If two sites insert the same key, one wins and the other is rejected, possibly after the fact (§8.5). Compaction does not fix this: two inserts above the base collide in the same way. daddb should refuse table definitions with collision-prone keys as a guardrail.
- **App: finality.** Who the administrator is, when to compact, and up to which key (§9.1). Whether it is safe to compact is the app's judgment.
- **Permission layer.** Who may write what in a shared space, who may compact it, and signatures. whh accepts any well-formed entry as valid history, including a malicious one, such as a backdated `DROP TABLE`. Such entries must be refused before ingest (through `admit`, §13.2), or the space must not be shared with untrusted peers.
- **Transport (for example, iroh).** Connections, peer discovery, encryption in transit.

---

## 15. Accepted costs and risks

1. **The log grows until the app compacts it.** Without compaction, about 15 MB per two years at Teller's rate. Accepted. To be measured.
2. **Late entries change the visible state.** This is correct behavior, for example when an offline device catches up. Changes are reported (§8.5).
3. **A late entry causes a full replay.** Replay starts from the base, or from empty if there is no base. The cost grows with the log size above the base.
4. **A misbehaving peer can cause repeated replays.** It can create backdated entries (each only needs to be above its own previous `hlc`), so each session triggers a replay. This is bounded by running settle at most once per session, prevented by the permission layer, and limited by compaction (entries at or below the base are ignored). It is a performance problem, not a convergence problem.
5. **whh never refuses an entry for being old.** Only entries at or below the base are ignored, and only because the app compacted.
6. **A wrong compaction loses writes silently.** If the app compacts past an entry that the administrator did not hold, that write is lost on every replica, and no device is told (§9.5).

---

## 16. Possible later work (not now)

- **Partial replay** (for example, per table), if measurements show full replay is too slow. This would require the machine to declare which parts each operation touches.
- **Checkpoints above the base.** Cache the state at some key above the base, and throw the cache away when a late entry lands before it.
- **Detecting lost writes.** Attach the administrator's version vector to the base, so a device can tell which of its entries were not included (§9.5). Left out for now on purpose.
- **Measurement.** Replay time for 10⁴, 10⁵, and 10⁶ entries, with a toy machine and with daddb.

---

## 17. Open items

- How machines version their behavior and their exported state, and what happens when devices in one space run different versions.
- The exact shape of the `admit` hook, and how blocked sites and refused bases are reported.
- Default values for the limits in §13.
- How the app learns that it is safe to compact (for example, every known device reports its `vv` to the administrator).

---

## 18. API sketch (not binding)

This is a starting point. Change it if a better shape appears during implementation.

```rust
pub struct SiteId(pub [u8; 16]);
pub struct SpaceId(pub [u8; 16]);
pub struct Hlc(pub u64);

#[derive(PartialEq, Eq, PartialOrd, Ord)]   // order: hlc, then site
pub struct Key { pub hlc: Hlc, pub site: SiteId }

pub struct Entry { pub key: Key, pub op: Vec<u8> }

pub struct Base { pub key: Key, pub by: SiteId, pub state: Vec<u8> }

pub enum Outcome { Applied, Rejected(String) }

/// Everything whh needs, inside one transaction (§6).
pub trait Transaction {
    type Error;                                         // always fatal
    // log and meta
    fn append(&mut self, e: &Entry) -> Result<(), Self::Error>;
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, Self::Error>;
    fn scan_after(&self, after: Option<&Key>) -> Result<Vec<Entry>, Self::Error>; // may become an iterator
    fn scan_site_after(&self, site: &SiteId, hlc: Hlc) -> Result<Vec<Entry>, Self::Error>;
    fn delete_through(&mut self, key: &Key) -> Result<(), Self::Error>;          // compaction
    fn version_vector(&self) -> Result<Vec<(SiteId, Hlc)>, Self::Error>;
    fn meta(&self) -> Result<Meta, Self::Error>;
    fn set_meta(&mut self, m: &Meta) -> Result<(), Self::Error>;
    fn base(&self) -> Result<Option<Base>, Self::Error>;
    fn set_base(&mut self, b: &Base) -> Result<(), Self::Error>;
    fn failed(&self) -> Result<Vec<(Key, String)>, Self::Error>;
    fn set_failed(&mut self, key: &Key, reason: &str) -> Result<(), Self::Error>;
    fn clear_failed(&mut self) -> Result<(), Self::Error>;
    // machine
    fn apply(&mut self, op: &[u8]) -> Result<Outcome, Self::Error>;
    fn reset_state(&mut self) -> Result<(), Self::Error>;
    fn export_state(&self) -> Result<Vec<u8>, Self::Error>;                      // compaction
    fn import_state(&mut self, state: &[u8]) -> Result<(), Self::Error>;         // compaction
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

    pub fn write(&mut self, op: Vec<u8>) -> Result<Key, WriteError>;       // Rejected | Fatal
    pub fn settle(&mut self) -> Result<Report, Error>;
    pub fn is_dirty(&self) -> bool;
    pub fn failed(&self) -> Result<Vec<(Key, String)>, Error>;

    // compaction (§9): the app decides when; only its administrator calls this
    pub fn compact(&mut self, upto: Key) -> Result<(), CompactError>;      // NotNewer | NotApplied | Fatal
    pub fn base_key(&self) -> Option<Key>;

    // sync, transport-agnostic
    pub fn hello(&self) -> Result<Hello, Error>;
    pub fn base_for(&self, peer: &Hello) -> Result<Option<Base>, Error>;   // Some if ours is newer
    pub fn missing_for(&self, peer: &Hello) -> Result<Vec<Entries>, Error>; // chunked
    pub fn adopt(&mut self, base: &Base) -> Result<(), AdoptError>;        // ConflictingBase | Deferred | Fatal
    pub fn ingest(&mut self, chunk: &Entries) -> Result<IngestReport, Error>; // Violation is an error
}
```

whh ships two things for testing:

- `MemBackend`: an in-memory backend that implements the transaction rules.
- `KvMachine`: a small deterministic machine with export and import (see §20).

The SQLite backend and the SQL machine belong to daddb, not to whh.

---

## 19. Invariants

Each of these must be checked by tests.

- **I1. Key uniqueness.** No log holds two different operations with the same key.
- **I2. Prefix.** For every site, the entries a replica holds above its base form a prefix of the rest of that site's sequence.
- **I3. Version vector.** `vv[s]` equals the maximum `hlc` of site `s` in the log.
- **I4. Clock.** `last >=` the maximum `hlc` in the log and the `hlc` of the base key, and every `now()` returns a value greater than every earlier one.
- **I5. State.** After any settle, `dirty` is false, and the state and `failed` set equal those produced by a replay from the base (or from empty if there is no base).
- **I6. Convergence.** Two replicas with the same base and equal logs have equal state and equal `failed` sets after settle.
- **I7. Local writes.** The key of every local write is greater than `W` at the time of the write.
- **I8. Base.** The base key never decreases. No log entry has a key at or below the base key.

---

## 20. Test plan

### 20.1 Test machine

`KvMachine`, a key-value map with these operations:

- `Insert(k, v)`: rejected if `k` exists
- `Put(k, v)`: always applied (overwrite)
- `Delete(k)`: rejected if `k` does not exist
- A fault switch (not an operation): when set, the next `apply` returns `Fatal`. Used only in fault-injection tests.

It supports `export` and `import` (for example, by serializing the map).

### 20.2 Unit tests

- HLC: `now()` is strictly increasing when the wall clock stands still, jumps forward, or jumps backward. Counter overflow carries into the physical part.
- Key order: `hlc` first, then `site` bytes.

### 20.3 Scenario tests (core)

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

### 20.4 Scenario tests (compaction)

1. **Compact and spread.** A, B, and C are synced. A compacts at `K`. A syncs with B: B adopts the base, deletes its entries `<= K`, and has the same state as A. B syncs with C: C gets the base from B.
2. **New device.** D is new. It syncs with A and receives the base and only the entries above it. D's state equals A's.
3. **Entry below the base.** After case 1, a peer sends an entry with key `<= K`. It is ignored, no replay runs, and the state does not change.
4. **Late entry above the base.** After compaction, an entry with key between `K` and `W` arrives. Settle replays from the base, and the result equals a replay of the full original log.
5. **Wrong compaction.** C writes at t=30 while offline. A compacts at t=100 without C's entry. After sync, C's write is gone on every replica, and all replicas have the same state.
6. **Invalid compact.** `compact` with `K <= base.key` returns `NotNewer`. `compact` with `K > W` returns `NotApplied`. Nothing changes.
7. **Older base.** A replica with base `K2` receives base `K1 < K2`. It is ignored.
8. **Conflicting base.** Two different sites create bases with the same key. Adopting the second one returns `ConflictingBase`, and nothing changes.
9. **Restart after compaction.** All entries are compacted, and the replica restarts. `last` is restored from the base key, so the next local write gets a larger key.

### 20.5 Property test (proptest)

- 2 to 5 sites, each with its own test clock.
- A random sequence of actions:
  - `write(site, op)` with random `KvMachine` operations over a small key space (so conflicts are common)
  - `sync(a, b)`, sometimes cut off after a random number of chunks
  - move a site's clock forward or backward
  - `compact(site 0, K)`: only site 0 is the administrator, and it compacts only at a key that every site has already synced past (so the app's promise holds)
- At the end, fully sync all pairs until nothing changes.
- Check I1 to I8. Check that all states and `failed` sets are equal.
- Also run the same action sequence without the `compact` actions. The final state must be the same. This checks that a correct compaction loses nothing.

### 20.6 Deliverables

- **Week 0.** The `whh` crate with `Replica`, `MemBackend`, `KvMachine`, §8 without the base, and the tests in §20.2, §20.3, and §20.5 without `compact`.
- **Phase 2.** Compaction (§9), the `Base` message, and the remaining tests.

daddb starts after Week 0.
