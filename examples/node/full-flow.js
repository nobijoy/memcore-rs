#!/usr/bin/env node
/**
 * Create → search → context → delete using the example MemcoreClient.
 * Never prints MEMCORE_API_KEY.
 */

import { MemcoreApiError, MemcoreClient } from "./memcore-client.js";

function requireEnv(name) {
  const value = (process.env[name] || "").trim();
  if (!value) {
    console.error(`error: missing required environment variable: ${name}`);
    process.exit(1);
  }
  return value;
}

async function main() {
  const baseUrl = requireEnv("MEMCORE_BASE_URL");
  const apiKey = requireEnv("MEMCORE_API_KEY");
  const orgId = requireEnv("MEMCORE_ORG_ID");
  const userId = (process.env.MEMCORE_USER_ID || "user_demo").trim() || "user_demo";

  const client = new MemcoreClient({ baseUrl, apiKey, orgId });

  try {
    const created = await client.createMemory({
      userId,
      text: "User prefers concise technical summaries.",
    });
    const memories = created.memories || [];
    const memoryId = memories[0]?.id;
    console.log(`create status=${created.status} memories=${memories.length}`);

    const search = await client.searchMemories({
      userId,
      query: "technical summaries",
      limit: 5,
    });
    console.log(`search results=${(search.results || []).length}`);

    const context = await client.buildContext({
      userId,
      query: "How should replies be written?",
      maxTokens: 1000,
    });
    console.log(
      `context status=${context.status} chars=${(context.context || "").length}`,
    );

    if (memoryId) {
      const deleted = await client.deleteMemory({ userId, memoryId });
      console.log(`delete status=${deleted.status}`);
    } else {
      console.error("warning: no memory id returned; skipping delete");
    }
  } catch (error) {
    if (error instanceof MemcoreApiError) {
      console.error(String(error));
      process.exit(1);
    }
    throw error;
  }
}

main();
