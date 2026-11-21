// L1-M1: the read API a dashboard reads. 70% the board (/v1/markets: the in-memory view
// with freshness judged per read), 20% one market's history (a bounded PostgreSQL read,
// limit 50), 10% the plain HTML list (/markets), offered at a constant arrival rate (an
// open model: what k6 could not send is counted as dropped). The worker is fetching from
// a loopback copy of the recorded DefiLlama body throughout, so reads run against writes.
//
// Env: BASE_URL, POOL (a stored pool id), MODE (smoke | full), RATE (requests/s); TOTAL
// for plumbing probes only (scripts/load.sh keeps those out of perf/results).
import http from 'k6/http';
import { check } from 'k6';
import { Counter, Rate } from 'k6/metrics';

const MODE = __ENV.MODE || 'smoke';
const RATE = parseInt(__ENV.RATE || '1000', 10);
const TOTAL = __ENV.TOTAL ? parseInt(__ENV.TOTAL, 10) : MODE === 'full' ? 1000000 : 100000;
const BASE = __ENV.BASE_URL;
const BOARD = `${BASE}/v1/markets`;
const HISTORY = `${BASE}/v1/markets/${__ENV.POOL}/history?limit=50`;
const PAGE = `${BASE}/markets`;

const shed = new Rate('shed'); // 503 overloaded: load shedding, counted apart
const unexpected = new Counter('unexpected_status');

export const options = {
  discardResponseBodies: true,
  summaryTrendStats: ['min', 'med', 'avg', 'p(90)', 'p(95)', 'p(99)', 'max'],
  scenarios: {
    markets: {
      executor: 'constant-arrival-rate',
      rate: RATE,
      timeUnit: '1s',
      duration: `${Math.ceil(TOTAL / RATE)}s`,
      preAllocatedVUs: 100,
      maxVUs: 1000,
    },
  },
  // Written before the first run: END_GOAL L1's proposed read bound (p99 <= 50 ms, zero 5xx).
  thresholds: {
    http_req_failed: ['rate<0.001'],
    'http_req_duration{kind:board}': ['p(99)<50'],
    'http_req_duration{kind:history}': ['p(99)<50'],
    'http_req_duration{kind:page}': ['p(99)<50'],
    shed: ['rate<0.001'],
    unexpected_status: ['count<1'],
    dropped_iterations: ['count<1'],
  },
};

export default function () {
  const roll = Math.random();
  const [url, kind] = roll < 0.7 ? [BOARD, 'board'] : roll < 0.9 ? [HISTORY, 'history'] : [PAGE, 'page'];
  const res = http.get(url, { tags: { kind } });
  shed.add(res.status === 503);
  if (!check(res, { 'status 200': (r) => r.status === 200 })) {
    if (res.status !== 503) unexpected.add(1);
  }
}
