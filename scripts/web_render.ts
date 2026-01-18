#!/usr/bin/env bun
/**
 * Web render script for quickdiff.
 * Takes a template, JSON data, and outputs rendered HTML.
 *
 * Usage: bun run scripts/web_render.ts <template> <json> <output>
 */

import { readFileSync, writeFileSync } from "node:fs";

const [templatePath, jsonPath, outPath] = process.argv.slice(2);

if (!templatePath || !jsonPath || !outPath) {
  console.error(
    "Usage: bun run web_render.ts <template.html> <data.json> <output.html>"
  );
  process.exit(1);
}

const template = readFileSync(templatePath, "utf8");
const jsonText = readFileSync(jsonPath, "utf8");
const data = JSON.parse(jsonText);

// Base64 encode the JSON to avoid escaping issues
const b64 = Buffer.from(jsonText).toString("base64");

const html = template
  .replaceAll("{{REVIEW_DATA_B64}}", b64)
  .replaceAll("{{BRANCH}}", String(data.branch || ""))
  .replaceAll("{{COMMIT}}", String(data.commit || ""));

writeFileSync(outPath, html, "utf8");
console.log(`Rendered: ${outPath}`);
