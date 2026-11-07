# Load test

The integration tests prove correctness inside one process with a 16-connection
pool. This k6 test checks the same properties from the outside: the release
image, the docker compose stack, real HTTP, many clients at once.

```bash
make loadtest                    # smoke profile: ~30 s, also runs in CI on every push
PROFILE=load make loadtest       # heavier: 200 transfers/s for 2 minutes
```

The HTML report lands in `loadtest/results/report.html` (k6 web dashboard
export) and the end-of-test numbers in `loadtest/results/summary.json`. In CI
both are attached to the run as the `load-test-report` artifact.

Against another deployment (k6 installed locally, or the container):

```bash
BASE_URL=https://transfers.example.com P95_MS=800 k6 run loadtest/transfers.js
```

Each run creates its own accounts (ids derived from the start time), so it is
safe to repeat against the same database.

## Scenarios

| Scenario | Traffic | What it targets |
|---|---|---|
| `random_transfers` | constant arrival rate (smoke 20/s, load 200/s), random pairs and amounts | throughput, insufficient-funds paths, mixed directions |
| `opposite_directions` | closed loop, A→B and B→A on the same two accounts | the deadlock case the locking order exists for |
| `idempotent_retries` | 2/s, each transfer immediately resent with the same `Idempotency-Key` | a retry returns the original `transaction_id` with `Idempotent-Replayed: true` |
| `balance_reads` | a quarter of the transfer rate | reads under write contention |

## Pass criteria (k6 thresholds, the run fails if any is missed)

| Threshold | Meaning |
|---|---|
| `server_errors count==0` | no 5xx: no deadlock, lock failure or crash reached a client |
| `invariant_violations count==0` | after the run, the sum of all balances equals the opening total and no balance is negative |
| `checks rate>0.999` | every transfer is `201` or `422 insufficient_funds`; replays match |
| `http_req_failed rate<0.001` | anything other than 200/201/422 counts as a failure |
| `http_req_duration{name:transfer} p(95)<P95_MS, p(99)<2×P95_MS` | latency budget (default 250 ms, raise it across the internet) |

The conservation check is exact: every amount has two decimals, so balances
are summed as integer cents, never as floating point.

## Reading the results

- `invariant_violations` above zero is a correctness bug, never a capacity
  problem: stop and investigate (`debug` skill), do not raise a limit.
- `server_errors` above zero with `temporarily_unavailable` bodies means the
  service shed load as designed (pool or lock wait exhausted). That is a
  capacity signal: compare `DB_MAX_CONNECTIONS`, `max_connections` and the
  arrival rate before changing anything.
- Latency at high rates is dominated by the hot pair in
  `opposite_directions`: transfers on one account are serialised by its row
  lock (README, *Limitations*).
