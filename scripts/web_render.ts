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

// Escape JSON for embedding in script tag (prevent </script> injection)
const safeJson = jsonText.replaceAll("</script>", "<\\/script>");

const html = template
  .replaceAll("{{REVIEW_DATA_JSON}}", safeJson)
  .replaceAll("{{BRANCH}}", String(data.branch || ""))
  .replaceAll("{{COMMIT}}", String(data.commit || ""));

writeFileSync(outPath, html, "utf8");
console.log(`Rendered: ${outPath}`);
