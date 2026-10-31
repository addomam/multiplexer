------------------------------ MODULE ReopenSettlement ------------------------------
\* One saved conversation record in reopen_conversation, mark_reopen, and
\* refresh_observed_peer. CheckPending stands for the launch_pending refusal.
EXTENDS Integers

CONSTANT CheckPending

VARIABLES pending, recorded_alive, unrecorded, phase

vars == <<pending, recorded_alive, unrecorded, phase>>

Init ==
  /\ pending = FALSE
  /\ recorded_alive = FALSE
  /\ unrecorded = 0
  /\ phase = "down"

Start ==
  /\ phase = "down"
  /\ phase' = "deciding"
  /\ UNCHANGED <<pending, recorded_alive, unrecorded>>

RefusePending ==
  /\ phase = "deciding"
  /\ pending
  /\ CheckPending
  /\ phase' = "down"
  /\ UNCHANGED <<pending, recorded_alive, unrecorded>>

RefuseAlivePeer ==
  /\ phase = "deciding"
  /\ ~pending
  /\ recorded_alive
  /\ phase' = "down"
  /\ UNCHANGED <<pending, recorded_alive, unrecorded>>

MarkPending ==
  /\ phase = "deciding"
  /\ ~recorded_alive
  /\ (~CheckPending \/ ~pending)
  /\ pending' = TRUE
  /\ phase' = "marked"
  /\ UNCHANGED <<recorded_alive, unrecorded>>

Spawn ==
  /\ phase = "marked"
  /\ unrecorded < 2
  /\ unrecorded' = unrecorded + 1
  /\ phase' = "spawned"
  /\ UNCHANGED <<pending, recorded_alive>>

Settle ==
  /\ phase = "spawned"
  /\ unrecorded > 0
  /\ pending' = FALSE
  /\ recorded_alive' = TRUE
  /\ unrecorded' = unrecorded - 1
  /\ phase' = "down"

Rollback ==
  /\ phase = "marked"
  /\ pending' = FALSE
  /\ phase' = "down"
  /\ UNCHANGED <<recorded_alive, unrecorded>>

Crash ==
  /\ phase \in {"deciding", "marked", "spawned"}
  /\ phase' = "down"
  /\ UNCHANGED <<pending, recorded_alive, unrecorded>>

RecordedPeerExits ==
  /\ phase = "down"
  /\ recorded_alive
  /\ recorded_alive' = FALSE
  /\ UNCHANGED <<pending, unrecorded, phase>>

UnrecordedExits ==
  /\ phase = "down"
  /\ pending
  /\ ~recorded_alive
  /\ unrecorded > 0
  /\ unrecorded' = unrecorded - 1
  /\ UNCHANGED <<pending, recorded_alive, phase>>

Next ==
  \/ Start
  \/ RefusePending
  \/ RefuseAlivePeer
  \/ MarkPending
  \/ Spawn
  \/ Settle
  \/ Rollback
  \/ Crash
  \/ RecordedPeerExits
  \/ UnrecordedExits

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ pending \in BOOLEAN
  /\ recorded_alive \in BOOLEAN
  /\ unrecorded \in 0..2
  /\ phase \in {"down", "deciding", "marked", "spawned"}

NoDuplicate == unrecorded + (IF recorded_alive THEN 1 ELSE 0) <= 1

PendingMeansUnsettled == pending => ~recorded_alive

=============================================================================
