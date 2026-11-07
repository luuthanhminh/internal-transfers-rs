// HTTP load test for the transfer service (k6).
//
// What it proves at the HTTP level, under real concurrency:
//   - no 5xx: no deadlock, lock failure or crash reaches a client;
//   - the total of all balances is unchanged after the run (no money created
//     or lost), and no balance is negative;
//   - idempotent replays move money once;
//   - latency stays inside the thresholds below.
//
// Every run creates its own accounts (ids derived from the start time), so it
// can run repeatedly against the same database, including a live deployment.
//
// Usage (see loadtest/README.md):
//   make loadtest                          # smoke profile against docker compose
//   PROFILE=load make loadtest             # heavier profile
//   BASE_URL=https://example.com k6 run loadtest/transfers.js

import http from 'k6/http';
import { check, fail } from 'k6';
import { Counter } from 'k6/metrics';

const BASE_URL = (__ENV.BASE_URL || 'http://localhost:8080').replace(/\/$/, '');
const PROFILE = __ENV.PROFILE || 'smoke';
const ACCOUNTS = Number(__ENV.ACCOUNTS || 20);
const INITIAL_BALANCE = '1000.00';
// Latency budget for transfers; raise it when testing across the internet.
const P95_MS = Number(__ENV.P95_MS || 250);

// Balances are compared exactly: every amount has two decimals, so values are
// handled as integer cents (well inside Number's exact integer range).
const toCents = (decimal) => {
  const [whole, fraction = ''] = decimal.split('.');
  if (fraction.length > 2) fail(`unexpected precision in ${decimal}`);
  return Number(whole) * 100 + Number(fraction.padEnd(2, '0'));
};

const profiles = {
  // Short and gentle: runs in CI on every push.
  smoke: { rate: 20, duration: '30s', hotVus: 4 },
  // A few minutes of sustained traffic on a laptop or a small VM.
  load: { rate: 200, duration: '2m', hotVus: 16 },
};
const profile = profiles[PROFILE] || fail(`unknown PROFILE ${PROFILE}`);

const serverErrors = new Counter('server_errors');
const invariantViolations = new Counter('invariant_violations');

// 201 and 422 (insufficient funds) are both correct answers for a transfer.
http.setResponseCallback(http.expectedStatuses(200, 201, 422));

export const options = {
  scenarios: {
    // Random transfers between all accounts, both directions.
    random_transfers: {
      executor: 'constant-arrival-rate',
      exec: 'randomTransfer',
      rate: profile.rate,
      timeUnit: '1s',
      duration: profile.duration,
      preAllocatedVUs: 50,
      maxVUs: 200,
    },
    // The deadlock-prone case: A->B and B->A on the same two accounts at once.
    opposite_directions: {
      executor: 'constant-vus',
      exec: 'oppositeDirections',
      vus: profile.hotVus,
      duration: profile.duration,
    },
    // A client retrying with the same Idempotency-Key must not pay twice.
    idempotent_retries: {
      executor: 'constant-arrival-rate',
      exec: 'idempotentRetry',
      rate: 2,
      timeUnit: '1s',
      duration: profile.duration,
      preAllocatedVUs: 5,
    },
    balance_reads: {
      executor: 'constant-arrival-rate',
      exec: 'readBalance',
      rate: Math.max(5, profile.rate / 4),
      timeUnit: '1s',
      duration: profile.duration,
      preAllocatedVUs: 10,
    },
  },
  thresholds: {
    server_errors: ['count==0'],
    invariant_violations: ['count==0'],
    checks: ['rate>0.999'],
    http_req_failed: ['rate<0.001'],
    'http_req_duration{name:transfer}': [`p(95)<${P95_MS}`, `p(99)<${P95_MS * 2}`],
    'http_req_duration{name:balance}': [`p(95)<${P95_MS}`],
  },
};

const JSON_HEADERS = { 'Content-Type': 'application/json' };

function createAccount(id) {
  const res = http.post(
    `${BASE_URL}/accounts`,
    JSON.stringify({ account_id: id, initial_balance: INITIAL_BALANCE }),
    { headers: JSON_HEADERS, tags: { name: 'create_account' } },
  );
  if (res.status !== 201) fail(`create account ${id}: ${res.status} ${res.body}`);
}

function balanceCents(id) {
  const res = http.get(`${BASE_URL}/accounts/${id}`, { tags: { name: 'balance' } });
  if (res.status !== 200) fail(`balance of ${id}: ${res.status} ${res.body}`);
  return toCents(res.json('balance'));
}

function transfer(from, to, amount, key) {
  const headers = key ? { ...JSON_HEADERS, 'Idempotency-Key': key } : JSON_HEADERS;
  const res = http.post(
    `${BASE_URL}/transactions`,
    JSON.stringify({ source_account_id: from, destination_account_id: to, amount }),
    { headers, tags: { name: 'transfer' } },
  );
  if (res.status >= 500) serverErrors.add(1);
  check(res, {
    'transfer: 201, or 422 insufficient_funds': (r) =>
      r.status === 201 ||
      (r.status === 422 && r.json('error.code') === 'insufficient_funds'),
  });
  return res;
}

const randomInt = (n) => Math.floor(Math.random() * n);
const randomAmount = () => `${1 + randomInt(80)}.${String(randomInt(100)).padStart(2, '0')}`;

export function setup() {
  const health = http.get(`${BASE_URL}/ready`);
  if (health.status !== 200) fail(`service not ready at ${BASE_URL}: ${health.status}`);

  // Unique ids per run: seconds since epoch x 1000 + index.
  const firstId = Math.floor(Date.now() / 1000) * 1000;
  const ids = Array.from({ length: ACCOUNTS }, (_, i) => firstId + i);
  ids.forEach(createAccount);
  return { ids, expectedTotal: ACCOUNTS * toCents(INITIAL_BALANCE) };
}

export function randomTransfer({ ids }) {
  const from = randomInt(ids.length);
  const to = (from + 1 + randomInt(ids.length - 1)) % ids.length;
  transfer(ids[from], ids[to], randomAmount());
}

export function oppositeDirections({ ids }) {
  const [a, b] = __ITER % 2 === 0 ? [ids[0], ids[1]] : [ids[1], ids[0]];
  transfer(a, b, '1.00');
}

export function idempotentRetry({ ids }) {
  const key = `loadtest-${ids[0]}-${__VU}-${__ITER}`;
  const [from, to] = [ids[2 + randomInt(ids.length - 2)], ids[0]];
  const first = transfer(from, to, '0.01', key);
  if (first.status !== 201) return;
  const retry = transfer(from, to, '0.01', key);
  check(retry, {
    'retry: same transaction_id, marked as replay': (r) =>
      r.status === 201 &&
      r.headers['Idempotent-Replayed'] === 'true' &&
      r.json('transaction_id') === first.json('transaction_id'),
  });
}

export function readBalance({ ids }) {
  http.get(`${BASE_URL}/accounts/${ids[randomInt(ids.length)]}`, { tags: { name: 'balance' } });
}

// After all traffic has stopped: money is conserved and nothing went negative.
export function teardown({ ids, expectedTotal }) {
  const balances = ids.map(balanceCents);
  const total = balances.reduce((sum, cents) => sum + cents, 0);
  const negative = balances.filter((cents) => cents < 0).length;
  console.log(`accounts=${ids.length} total=${total / 100} expected=${expectedTotal / 100} negative=${negative}`);
  if (total !== expectedTotal) invariantViolations.add(1, { invariant: 'total_unchanged' });
  if (negative > 0) invariantViolations.add(negative, { invariant: 'no_negative_balance' });
  if (total !== expectedTotal || negative > 0) {
    fail(`invariant broken: total ${total / 100} (expected ${expectedTotal / 100}), ${negative} negative balances`);
  }
}
