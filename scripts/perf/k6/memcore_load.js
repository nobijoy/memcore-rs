// memcore API load test foundation (k6).
//
// Usage:
//   MEMCORE_BASE_URL=http://localhost:8080 \
//   MEMCORE_API_KEY=... \
//   MEMCORE_ORG_ID=org_perf \
//   MEMCORE_TEST_PROFILE=smoke \
//   k6 run scripts/perf/k6/memcore_load.js
//
// Profiles: smoke (default), baseline, stress
// Never logs API keys. Uses synthetic content and unique per-VU user IDs.
// Does not call forget-user, backup/restore, or import/export.

import http from "k6/http";
import { check, group, sleep } from "k6";
import { Rate, Trend } from "k6/metrics";
import { textSummary } from "https://jslib.k6.io/k6-summary/0.0.4/index.js";

const BASE_URL = (__ENV.MEMCORE_BASE_URL || "http://localhost:8080").replace(/\/$/, "");
const API_KEY = __ENV.MEMCORE_API_KEY || "";
const ORG_ID = __ENV.MEMCORE_ORG_ID || "org_perf";
const PROFILE = (__ENV.MEMCORE_TEST_PROFILE || "smoke").toLowerCase();
const RUN_ID = __ENV.MEMCORE_PERF_RUN_ID || `${Date.now()}`;

const AUTH_ENABLED = API_KEY.length > 0;

const SAFE_CONTENTS = [
  "User prefers concise technical summaries.",
  "User is testing memcore API latency.",
  "User likes green tea during focused work.",
];

const errorRate = new Rate("memcore_errors");
const healthLatency = new Trend("memcore_health_duration", true);
const memoryCreateLatency = new Trend("memcore_memory_create_duration", true);
const searchLatency = new Trend("memcore_search_duration", true);
const contextLatency = new Trend("memcore_context_duration", true);

function profileOptions() {
  if (PROFILE === "baseline") {
    return {
      stages: [
        { duration: "1m", target: 10 },
        { duration: "3m", target: 10 },
        { duration: "1m", target: 0 },
      ],
      thresholds: {
        http_req_failed: ["rate<0.05"],
        http_req_duration: ["p(95)<2000"],
        memcore_errors: ["rate<0.05"],
      },
    };
  }
  if (PROFILE === "stress") {
    return {
      stages: [
        { duration: "2m", target: 25 },
        { duration: "5m", target: 25 },
        { duration: "2m", target: 0 },
      ],
      thresholds: {
        // Stress is informational — fail only on catastrophic error rates.
        http_req_failed: ["rate<0.25"],
        memcore_errors: ["rate<0.25"],
      },
    };
  }
  // smoke (default)
  return {
    vus: 1,
    duration: "30s",
    thresholds: {
      http_req_failed: ["rate<0.01"],
      http_req_duration: ["p(95)<1000"],
      memcore_errors: ["rate<0.01"],
    },
  };
}

export const options = profileOptions();

function authHeaders() {
  return {
    Authorization: `Bearer ${API_KEY}`,
    "X-Organization-ID": ORG_ID,
    "Content-Type": "application/json",
  };
}

function recordStatus(res, ok) {
  errorRate.add(!ok);
  return ok;
}

function userId() {
  return `perf-user-${RUN_ID}-${__VU}`;
}

function pickContent() {
  return SAFE_CONTENTS[Math.floor(Math.random() * SAFE_CONTENTS.length)];
}

function parseJson(res) {
  try {
    return res.json();
  } catch (_) {
    return null;
  }
}

export function setup() {
  // Do not print API key. Confirm target only.
  console.log(`memcore load: profile=${PROFILE} base=${BASE_URL} auth=${AUTH_ENABLED} org=${ORG_ID}`);
  if (PROFILE === "stress" && __ENV.MEMCORE_ALLOW_STRESS_TEST !== "true") {
    throw new Error("stress profile requires MEMCORE_ALLOW_STRESS_TEST=true");
  }
  return { startedAt: new Date().toISOString() };
}

export default function () {
  group("operational", () => {
    let res = http.get(`${BASE_URL}/health`);
    healthLatency.add(res.timings.duration);
    recordStatus(
      res,
      check(res, {
        "health status 2xx": (r) => r.status >= 200 && r.status < 300,
      })
    );

    res = http.get(`${BASE_URL}/ready`);
    recordStatus(
      res,
      check(res, {
        "ready status 2xx": (r) => r.status >= 200 && r.status < 300,
      })
    );

    res = http.get(`${BASE_URL}/api/v1/version`);
    recordStatus(
      res,
      check(res, {
        "version status 2xx": (r) => r.status >= 200 && r.status < 300,
      })
    );
  });

  if (!AUTH_ENABLED) {
    sleep(PROFILE === "smoke" ? 1 : 0.3);
    return;
  }

  const uid = userId();
  const content = pickContent();
  let memoryId = null;

  group("authenticated_memory_flow", () => {
    const createBody = JSON.stringify({
      user_id: uid,
      messages: [{ role: "user", content }],
      metadata: { source: "perf_load", run_id: RUN_ID },
    });

    let res = http.post(`${BASE_URL}/api/v1/memories`, createBody, {
      headers: authHeaders(),
      tags: { endpoint: "memories_create" },
    });
    memoryCreateLatency.add(res.timings.duration);
    const createOk = check(res, {
      "create status 2xx": (r) => r.status >= 200 && r.status < 300,
    });
    recordStatus(res, createOk);

    if (createOk) {
      const body = parseJson(res);
      if (body && body.memories && body.memories[0] && body.memories[0].id) {
        memoryId = body.memories[0].id;
      }
    }

    res = http.get(`${BASE_URL}/api/v1/users/${uid}/memories`, {
      headers: authHeaders(),
      tags: { endpoint: "memories_list" },
    });
    recordStatus(
      res,
      check(res, {
        "list status 2xx": (r) => r.status >= 200 && r.status < 300,
      })
    );

    const searchBody = JSON.stringify({
      user_id: uid,
      query: content,
    });
    res = http.post(`${BASE_URL}/api/v1/memories/search`, searchBody, {
      headers: authHeaders(),
      tags: { endpoint: "memories_search" },
    });
    searchLatency.add(res.timings.duration);
    recordStatus(
      res,
      check(res, {
        "search status 2xx": (r) => r.status >= 200 && r.status < 300,
      })
    );

    const contextBody = JSON.stringify({
      user_id: uid,
      query: content,
      max_memories: 5,
    });
    res = http.post(`${BASE_URL}/api/v1/context`, contextBody, {
      headers: authHeaders(),
      tags: { endpoint: "context" },
    });
    contextLatency.add(res.timings.duration);
    recordStatus(
      res,
      check(res, {
        "context status 2xx": (r) => r.status >= 200 && r.status < 300,
      })
    );

    if (memoryId) {
      res = http.del(`${BASE_URL}/api/v1/users/${uid}/memories/${memoryId}`, null, {
        headers: authHeaders(),
        tags: { endpoint: "memories_delete" },
      });
      recordStatus(
        res,
        check(res, {
          "delete status 2xx": (r) => r.status >= 200 && r.status < 300,
        })
      );
    }
  });

  sleep(PROFILE === "smoke" ? 1 : 0.2);
}

export function handleSummary(data) {
  const summary = {
    profile: PROFILE,
    base_url: BASE_URL,
    auth_enabled: AUTH_ENABLED,
    org_id: ORG_ID,
    run_id: RUN_ID,
    // Intentionally omit API key / Authorization material.
    metrics: {
      http_reqs: data.metrics.http_reqs,
      http_req_duration: data.metrics.http_req_duration,
      http_req_failed: data.metrics.http_req_failed,
      memcore_errors: data.metrics.memcore_errors,
      memcore_health_duration: data.metrics.memcore_health_duration,
      memcore_memory_create_duration: data.metrics.memcore_memory_create_duration,
      memcore_search_duration: data.metrics.memcore_search_duration,
      memcore_context_duration: data.metrics.memcore_context_duration,
    },
  };

  return {
    stdout: textSummary(data, { indent: " ", enableColors: true }),
    "reports/perf/last-summary.json": JSON.stringify(summary, null, 2),
  };
}
