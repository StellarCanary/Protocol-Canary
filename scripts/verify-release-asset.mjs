import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { basename } from "node:path";
import { pathToFileURL } from "node:url";

export async function verifyReleaseAsset(archivePath, manifestPath) {
  const assetName = basename(archivePath);
  if (!assetName || assetName === "." || assetName === "..") {
    throw new Error("an archive file path is required");
  }

  const [archive, manifest] = await Promise.all([
    readFile(archivePath),
    readFile(manifestPath, "utf8"),
  ]);

  const entries = [];
  for (const [index, line] of manifest.split(/\r?\n/).entries()) {
    if (line === "") continue;
    const match = /^([0-9a-fA-F]{64})[ \t]+\*?(.+)$/.exec(line);
    if (!match) {
      throw new Error(`malformed SHA256SUMS entry on line ${index + 1}`);
    }

    const [, digest, name] = match;
    if (name.includes("/") || name.includes("\\")) {
      throw new Error(`SHA256SUMS entry on line ${index + 1} is not an asset basename`);
    }
    if (name === assetName) entries.push(digest.toLowerCase());
  }

  if (entries.length !== 1) {
    throw new Error(
      `expected exactly one SHA256SUMS entry for ${assetName}, found ${entries.length}`,
    );
  }

  const expected = entries[0];
  const actual = createHash("sha256").update(archive).digest("hex");
  if (actual !== expected) {
    throw new Error(`SHA-256 mismatch for ${assetName}`);
  }

  return { assetName, sha256: actual };
}

async function main() {
  const [, , archivePath, manifestPath] = process.argv;
  if (!archivePath || !manifestPath) {
    throw new Error("usage: node verify-release-asset.mjs <archive> <SHA256SUMS>");
  }

  const result = await verifyReleaseAsset(archivePath, manifestPath);
  console.log(`Verified ${result.assetName} (SHA-256 ${result.sha256})`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(`Release asset verification failed: ${error.message}`);
    process.exitCode = 1;
  });
}
