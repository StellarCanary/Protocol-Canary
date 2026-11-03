import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { verifyReleaseAsset } from "../scripts/verify-release-asset.mjs";

async function createFixture(t, { assetName, assetContents, manifest }) {
  const directory = await mkdtemp(path.join(os.tmpdir(), "canary-release-asset-"));
  t.after(() => rm(directory, { recursive: true, force: true }));

  const archivePath = path.join(directory, assetName);
  const manifestPath = path.join(directory, "SHA256SUMS");
  await writeFile(archivePath, assetContents);
  await writeFile(manifestPath, manifest);
  return { archivePath, manifestPath };
}

function sha256(contents) {
  return createHash("sha256").update(contents).digest("hex");
}

test("verifies only the exact requested asset entry", async (t) => {
  const assetName = "stellar-canary-x86_64-unknown-linux-gnu.tar.gz";
  const assetContents = "verified release archive";
  const manifest = [
    `${"0".repeat(64)}  stellar-canary-aarch64-apple-darwin.tar.gz`,
    `${sha256(assetContents)}  ${assetName}`,
  ].join("\n");
  const fixture = await createFixture(t, { assetName, assetContents, manifest });

  assert.deepEqual(
    await verifyReleaseAsset(fixture.archivePath, fixture.manifestPath),
    { assetName, sha256: sha256(assetContents) },
  );
});

test("rejects Linux-only manifests when macOS or Windows assets are requested", async (t) => {
  const linuxAsset = "stellar-canary-x86_64-unknown-linux-gnu.tar.gz";
  const linuxContents = "linux release archive";
  const manifest = `${sha256(linuxContents)}  ${linuxAsset}\n`;

  for (const requestedAsset of [
    "stellar-canary-aarch64-apple-darwin.tar.gz",
    "stellar-canary-x86_64-pc-windows-msvc.zip",
  ]) {
    await t.test(requestedAsset, async (subtest) => {
      const fixture = await createFixture(subtest, {
        assetName: requestedAsset,
        assetContents: "different platform archive",
        manifest,
      });

      await assert.rejects(
        verifyReleaseAsset(fixture.archivePath, fixture.manifestPath),
        /expected exactly one SHA256SUMS entry/,
      );
    });
  }
});

test("rejects an archive whose bytes do not match the exact manifest entry", async (t) => {
  const assetName = "stellar-canary-x86_64-unknown-linux-gnu.tar.gz";
  const fixture = await createFixture(t, {
    assetName,
    assetContents: "corrupted or truncated archive",
    manifest: `${sha256("published archive bytes")}  ${assetName}\n`,
  });

  await assert.rejects(
    verifyReleaseAsset(fixture.archivePath, fixture.manifestPath),
    /SHA-256 mismatch/,
  );
});

test("rejects missing and duplicate exact entries", async (t) => {
  const assetName = "stellar-canary-x86_64-unknown-linux-gnu.tar.gz";
  const assetContents = "release archive";
  const digest = sha256(assetContents);

  for (const [label, manifest, expectedError] of [
    ["missing", `${"0".repeat(64)}  another-asset.tar.gz\n`, /found 0/],
    ["duplicate", `${digest}  ${assetName}\n${digest}  ${assetName}\n`, /found 2/],
  ]) {
    await t.test(label, async (subtest) => {
      const fixture = await createFixture(subtest, { assetName, assetContents, manifest });
      await assert.rejects(
        verifyReleaseAsset(fixture.archivePath, fixture.manifestPath),
        expectedError,
      );
    });
  }
});

test("rejects malformed or path-bearing manifest entries", async (t) => {
  const assetName = "stellar-canary-x86_64-unknown-linux-gnu.tar.gz";
  for (const manifest of [
    "not a checksum entry\n",
    `${"0".repeat(64)}  ../${assetName}\n`,
  ]) {
    const fixture = await createFixture(t, { assetName, assetContents: "archive", manifest });
    await assert.rejects(
      verifyReleaseAsset(fixture.archivePath, fixture.manifestPath),
      /malformed SHA256SUMS entry|not an asset basename/,
    );
  }
});
