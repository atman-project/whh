# whh — Next Step: Causal Delivery + Partial Replay

> Status: **next step (not part of the current implementation).** The spec being implemented now does not change. Work on this starts after the current implementation is stable.
> Background: design discussion on 2026-10-13 (started from Automerge's DAG structure)

---

## 1. Problem

Today, whh puts ops in `(hlc, site)` order. When a late op lands in front of others, whh **rebuilds the whole table**.

Two different problems are mixed here.

1. **A correctness problem.** An op that comes later causally can arrive first. Then it runs in the wrong context.
2. **A performance problem.** Every time a truly concurrent op lands in the middle, the table is rebuilt from the start.

### Example of the correctness problem

- Device A: op X = `INSERT INTO trades (id=1, qty=10)`
- Device B: after receiving X, op Y = `UPDATE trades SET qty=12 WHERE id=1`

If Y arrives at device C before X, Y runs when the row does not exist yet. **It updates 0 rows and ends quietly.** There is no error. When X arrives later, the rewind makes the result correct in the end. But in the meantime, C's state is wrong.

---

## 2. Why we cannot remove rewinding the way Automerge does

Automerge can apply a late op directly because of **the commutativity of its ops, not because of the DAG.** Every position is named by a unique ID, and the merge rules give the same result in any order.

whh ops are arbitrary SQL. `balance = balance - 100` and `balance = 0` give different results when their order changes. Adding a DAG does not change this.

To remove rewinding completely, ops would have to be limited to "row-level upsert + per-column LWW". That is crrdb. We moved to an oplog to get the freedom of arbitrary SQL and schema changes. So **we accept rewinding for truly concurrent ops as an intended cost.**

The goal is not to remove rewinding. The goal is to **guarantee correctness and reduce how much is rewound.**

---

## 3. Solution A — causal delivery (correctness)

**When an op arrives and an op it depends on has not arrived yet, do not run it. Put it in a waiting queue.** When the earlier op arrives, apply it first, and then apply the waiting ops.

Effect: an op that comes later causally but arrives first is never run in the wrong context. Rewinding is needed only for **truly concurrent ops that were made without seeing each other.**

### Expressing dependencies — a version vector is enough (first stage)

If every site's ops have gap-free sequence numbers (seq), it is enough to attach to each op "the last seq of each site seen when the op was made" (a version vector).

```
op Y.deps = { A: 5, B: 3 }
→ the receiver applies Y only after it has received A ≥ 5 and B ≥ 3
```

whh already has version vectors, so the extra cost is small.

### Hash DAG — when there is a trust boundary (second stage)

If each op includes the hashes of earlier ops as its dependencies, we also get **integrity**. Tampering with an op, or making two different ops with the same seq (equivocation), is detected at once.

- Sync between my own devices → a version vector is enough
- A space shared with a friend, where there is a trust boundary → consider a hash DAG

### Things to handle in the implementation

1. **Request missing ops instead of waiting (pull).** When an op enters the waiting queue, we know what is missing. Ask the peer for that range directly.
2. **The waiting queue must have a limit.** When it goes over a size or time limit, warn, or fall back to a full resync with that peer.
3. A waiting op does not count as "received" yet. The version vector and the horizon H are computed **from applied ops only.**

---

## 4. Solution B — partial replay (performance)

This reduces the cost when a truly concurrent op lands in the middle.

### B-1. Checkpoints + partial replay

Save snapshots (checkpoints) at regular intervals. When a late op arrives, **go back only to the checkpoint just before that op**, and run everything after it again. Do not start from the beginning.

The horizon H is the upper limit of how far we rewind. Everything at or below H is final and nothing can land there, so checkpoints are needed only after H. This extends the existing `final.db` snapshot structure.

### B-2. Skip rewinding with an overlap check

If no op after the late op touched the tables that the late op touches, **apply it directly without rewinding.** The current table-level rebuild is a rough version of this idea.

Going down to the row level would reduce the cost more. But with arbitrary SQL, it is hard to know exactly which rows an op touches. **We use the table level as the practical baseline.**

---

## 5. How the two solutions relate

| | Solution A (causal delivery) | Solution B (partial replay) |
|---|---|---|
| Solves | correctness | performance |
| Target | ops that are causally later but arrive first | truly concurrent ops |
| Method | wait in a queue | start from the nearest checkpoint, only when ops overlap |

**The two must be used together to be effective.** A alone does not reduce rewinding much. B alone leaves wrong intermediate states.

---

## 6. Order of work (proposal)

1. Solution A — causal delivery based on version vectors + requesting missing ops + a limit on the waiting queue
2. Solution B-2 — table-level overlap check (a generalization of the current rebuild logic)
3. Solution B-1 — checkpoints after H
4. (When a trust boundary appears) hash DAG

For sync between a few personal devices, late ops usually reach back only a short way. So steps 1–2 alone will likely keep the real cost small enough. Start step 3 when the rebuild cost actually shows up during dogfooding.
