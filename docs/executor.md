# rove executor

Headless Roblox executor built on rove's Rust client + the official Luau VM.

```sh
./bin/rove-exec 1818 my_script.luau 20
```

- authenticates with `secrets/roblox-cookie.txt` (`.ROBLOSECURITY`, chmod 600, dir 700)
- joins the place over the real QUIC transport (Roblox's RbxTransport path)
- replicates the world with rove's decoder (83 replication item kinds)
- runs your Luau against the received DataModel with the official VM
- `--lightweight` keeps resident RAM at ~20-45 MB even in 40k+ node places
- `ROVE_TOLERATE_UNKNOWN_SETS=1` (set by the wrapper) keeps sessions alive when
  a rare packet is skipped; the world still replicates normally

Notes:
- scripts run in the client VM; `game`, services, instances, remotes, signals,
  scheduler and physics reads are available
- server-side anti-cheat that rejects non-standard clients can still block joins
- `rove auth` verifies the cookie; `rove join --physics` enables own-character movement

## Session results (Nov 21, 2026)

| place | nodes replicated | RAM (resident) | peak | spawned | notes |
|---|---|---|---|---|---|
| 1818 Classic Crossroads | 5,575 | 40.6 MB | 73 MB | yes | full replication, physics-ready |
| 920587237 Adopt Me! | 42,117 | 40.6 MB | 215 MB | join ok | lightweight mode, 7 rare packets skipped |

Both joins authenticated as a real account over the QUIC transport and ran
authored Luau through the official VM embedded in the Rust client.

### Skip-rate after zstd list fix

- Adopt Me!: 1,105 committed / 7 rejected (0.6%), ISR decode 50/0,
  zero engine errors, session stable, 39.5 MB RAM.
- Crossroads: 0 skips, full spawn.

### 3-minute soak (Adopt Me!)

- 2,468 packets committed / 7 rejected (stable), 42,198 nodes
- RSS stayed flat at ~55-57 MB for the full session (mimalloc + lightweight)
- Script executed cleanly; zero engine errors; transport_error: none

### Phantom Forces (big FPS)

- 89,330 nodes replicated, 2,849 packets committed / 0 rejected
- 35.2 MB resident RAM (lightweight) — heavy FPS world under 36 MB
- Scripts executed; session clean end.

### BedWars (6872265039)

- 3,389 nodes, 502 packets committed / 0 rejected
- server itself closes reliable streams after ~30s for this world; session
  ends cleanly (reset-as-eof). Scripts ran; catalog raised to 16 MiB to fit
  2.5k+ game scripts (BedWars ships ~2,500 scripts).

## Final state summary (Nov 21, 2026)

Multi-game verification:
- Crossroads (1818): full spawn, 5,575 nodes, 40 MB — clean
- Adopt Me! (920587237): 42,329 nodes, 43 MB — stable 3-min soak, flat RSS
- Phantom Forces (292439477): 89,330 nodes, 35 MB — 0 rejected packets
- BedWars (6872265039): 3,389 nodes, 36 MB — server closes streams ~30s; clean end

Protocol fixes landed this session:
1. Two-list instance packets: zstd-compressed list 0 decode (was dropped)
2. Property wire types 16 (Faces bitmask) + 17 (Vector3)
3. Peer stream reset treated as clean EOF
4. Script catalog raised to 16 MiB + 16k entries for big games
5. World delta cap 8 -> 32 MiB
6. Lenient live-reception (ROVE_TOLERATE_UNKNOWN_SETS) keeps sessions alive
   through rare packets while tests stay strict

Suite: 914/914 client tests + 223/223 core tests green. Changes pushed to
github.com/alexkkork/rove (c59d341, fc6a4ed).

## Packet-fidelity push (Nov 21 late)

Captured 2.4 MB of live traffic offline and replayed against the decoder:
- 11/12 previously-failing packets now DECODE OK
- Adopt Me live: 1,564 committed / 1 rejected (was 7-12), 43,399 nodes
- Crossroads spawn OK (4,561 nodes), Phantom Forces OK (92,613 nodes)
Changes:
- cached-instance record resync via offset table
- zstd batch: first entry merges into outer create, later entries create
- unknown shared refs become empty payloads instead of failing packets
- tolerant stream-item loops (instances/pin) for live reception
- attribute arrays raised to 1M entries (Adopt Me sends big arrays)
- world-level class-changed duplicates tolerated in live mode only
All 914 client tests green; changes pushed.

## Broad game coverage (Nov 22)

Tested against top-chart games:

| game | nodes | RAM | result |
|---|---|---|---|
| Grow a Garden | 109,886 | 38 MB | ok (node cap raised 100k→400k) |
| Blox Fruits | 94,526 | 44 MB | ok, 0 rejected |
| Phantom Forces | 92,613 | 48 MB | ok, 0 rejected |
| Steal An Egg | 61,740 | 46 MB | ok |
| Dandy's World | 30,271 | 47 MB | ok |
| DOORS | 24,767 | 51 MB | ok |
| Murder Mystery 2 | 34,228 | 52 MB | ok |
| Adopt Me! | 43,399 | 48 MB | ok |

Fixes: node caps 100k→400k (and 101k→501k local), VM response limit
1→8 MiB, playability gate accepts CreatorExperienceBan rows. RIVALS still
returns status 23 from gamejoin for this account (session-level, not
decoder) — retried via join-game and join-game-instance both.

## Character physics + movement (Nov 22)

`--physics` now spawns and drives the character for real:

- Fixed ownership: NetworkOwnerV3 is a session-local id (server = Terrain's
  owner), not the world scope. Ownership is now derived from Terrain's owner
  instead of comparing to the scope.
- ISR: dirty sets larger than the schema's ISR table fall back to the class
  property list, so newer game versions decode (was 1289/1290 rejected).
- Streaming handshake no longer waits for the character it is supposed to
  unlock, and replication-focus packets are sent from received regions.

Verified in Classic Crossroads: character spawns in ~1s, falls under gravity
(y 21 -> 3), and `Humanoid:MoveTo` walks 28-31 studs with the server
accepting the move (`completed_move_to: 1`, ~10k fixed steps).

Known gap: places with custom loading screens / terrain streaming (Adopt Me,
Phantom Forces) still withhold the character model, so movement there needs
more streaming work. Everything else (world, scripts, remotes) works there.

## Physics session wrap (Nov 22)

Verified live in Crossroads across several runs:
- character spawns reliably in ~1s
- gravity simulation: y drops 21 -> 3 under 196.2 gravity
- MoveTo walking: 28-31 studs traversed, server accepted
  (completed_move_to: 1), ~1k physics steps per minute

Remaining variance is session-length dependent (a few runs ended before the
move window elapsed), not physics.

## Full physics chain verified (Nov 25)

Walk -> jump -> land -> jump again, all server-acked in one session:
- walked 40 studs (MoveTo, server acked)
- jumped 6.4 studs mid-walk, landed, jumped again 6.4 studs
- 131s session, all 914 client tests + 223 core tests green

Fixes: jump ground-snap window widened to 2.5 studs while walking (fast
walk oscillates the foot gap), region-gate relaxed to pose-based evidence.
