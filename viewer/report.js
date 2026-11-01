/* Report validation and presentation rules for the static viewer (CF-08-A).
 *
 * Pure functions with no DOM access, so they run unchanged in the browser
 * (as a classic script exposing `CanaryViewer`) and under Node for tests.
 * Nothing here performs I/O; the page decides what to do with the result.
 */
(function (root) {
  "use strict";

  var MAX_BYTES = 10 * 1024 * 1024;
  var STORAGE_PREFIX = "stellar-canary-viewer:";
  var STATUSES = ["pass", "warning", "fail", "error", "skipped"];
  var REPORT_STATUSES = ["pass", "warning", "fail", "error"];
  var SURFACES = ["xdr", "rpc", "soroban"];

  function isObject(value) {
    return value !== null && typeof value === "object" && !Array.isArray(value);
  }

  function isInteger(value) {
    return typeof value === "number" && isFinite(value) && Math.floor(value) === value;
  }

  function fail(reason) {
    return { ok: false, reason: reason };
  }

  function identity(surface, id) {
    return surface + "\u0000" + id;
  }

  function describe(value) {
    if (value === undefined) return "nothing";
    var text = typeof value === "string" ? JSON.stringify(value) : String(value);
    return text.length > 40 ? text.slice(0, 40) + "..." : text;
  }

  /* Validates parsed JSON against report schema version 1 (CF-01). Returns
   * `{ ok: true, report }` or `{ ok: false, reason }` with the first problem.
   * Unknown extra fields are ignored.
   *
   * Note: schemaVersion != 1 is not treated as a validation failure here;
   * the viewer handles it separately to show an upgrade guidance banner.
   */
  function validate(value) {
    if (!isObject(value)) return fail("the report must be a JSON object");
    // schemaVersion is checked by the viewer, not here - we accept any
    // value so the viewer can show appropriate guidance.
    if (typeof value.toolVersion !== "string") return fail("toolVersion must be a string");
    if (!isInteger(value.targetProtocol) || value.targetProtocol < 0) {
      return fail("targetProtocol must be a non-negative integer");
    }
    if (!isObject(value.project)) return fail("project must be an object");
    if (typeof value.project.name !== "string") return fail("project.name must be a string");
    if (typeof value.project.type !== "string") return fail("project.type must be a string");
    if (REPORT_STATUSES.indexOf(value.status) === -1) {
      return fail("status must be one of " + REPORT_STATUSES.join(", "));
    }
    if (!Array.isArray(value.results)) return fail("results must be an array");

    var seen = Object.create(null);
    for (var i = 0; i < value.results.length; i++) {
      var result = value.results[i];
      var where = "results[" + i + "]";
      if (!isObject(result)) return fail(where + " must be an object");
      if (typeof result.testId !== "string") return fail(where + ".testId must be a string");
      if (!isInteger(result.protocol) || result.protocol < 0) {
        return fail(where + ".protocol must be a non-negative integer");
      }
      if (SURFACES.indexOf(result.surface) === -1) {
        return fail(where + ".surface must be one of " + SURFACES.join(", "));
      }
      if (STATUSES.indexOf(result.status) === -1) {
        return fail(where + ".status must be one of " + STATUSES.join(", "));
      }
      if (typeof result.summary !== "string") return fail(where + ".summary must be a string");
      if (typeof result.durationMs !== "number" || !isFinite(result.durationMs) || result.durationMs < 0) {
        return fail(where + ".durationMs must be a non-negative number");
      }
      if (result.fixtureId !== undefined && typeof result.fixtureId !== "string") {
        return fail(where + ".fixtureId must be a string when present");
      }
      if (result.details !== undefined && result.details !== null && typeof result.details !== "string") {
        return fail(where + ".details must be a string when present");
      }
      var id = result.fixtureId !== undefined ? result.fixtureId : result.testId;
      var key = identity(result.surface, id);
      if (seen[key]) return fail(where + " repeats the result identity " + result.surface + "/" + id);
      seen[key] = true;
    }

    if (value.skipped !== undefined) {
      if (!Array.isArray(value.skipped)) return fail("skipped must be an array when present");
      for (var j = 0; j < value.skipped.length; j++) {
        var skip = value.skipped[j];
        var at = "skipped[" + j + "]";
        if (!isObject(skip)) return fail(at + " must be an object");
        if (typeof skip.fixtureId !== "string") return fail(at + ".fixtureId must be a string");
        if (SURFACES.indexOf(skip.surface) === -1) {
          return fail(at + ".surface must be one of " + SURFACES.join(", "));
        }
        if (typeof skip.reason !== "string") return fail(at + ".reason must be a string");
        var skipKey = identity(skip.surface, skip.fixtureId);
        if (seen[skipKey]) return fail(at + " repeats the result identity " + skip.surface + "/" + skip.fixtureId);
        seen[skipKey] = true;
      }
    }

    if (value.counts !== undefined && !isObject(value.counts)) {
      return fail("counts must be an object when present");
    }
    if (value.network !== undefined && value.network !== null) {
      if (!isObject(value.network)) return fail("network must be an object when present");
      var observed = value.network.observedProtocol;
      if (observed !== undefined && observed !== null && (!isInteger(observed) || observed < 0)) {
        return fail("network.observedProtocol must be a non-negative integer when present");
      }
    }
    return { ok: true, report: value };
  }

  /* Counts derived from `results` and `skipped`, as CF-01 defines them. */
  function deriveCounts(report) {
    var counts = { total: report.results.length, passed: 0, failed: 0, warnings: 0, errors: 0, skipped: 0 };
    for (var i = 0; i < report.results.length; i++) {
      switch (report.results[i].status) {
        case "pass": counts.passed++; break;
        case "fail": counts.failed++; break;
        case "warning": counts.warnings++; break;
        case "error": counts.errors++; break;
        default: break;
      }
    }
    counts.skipped = Array.isArray(report.skipped) ? report.skipped.length : 0;
    return counts;
  }

  /* Names of the stated `counts` entries that disagree with `results`. The
   * derived counts are always the ones displayed. */
  function countMismatches(report, derived) {
    var out = [];
    if (!isObject(report.counts)) return out;
    var keys = Object.keys(derived);
    for (var i = 0; i < keys.length; i++) {
      var stated = report.counts[keys[i]];
      if (stated !== undefined && stated !== derived[keys[i]]) out.push(keys[i]);
    }
    return out;
  }

  /* The fixed heading table of CF-08 section 2.3. `report` is a validated
   * report, or null for invalid input. The viewer displays the report's
   * `status`; it never decides pass or fail from `results`. */
  function heading(report, counts) {
    if (report === null) return "Invalid report";
    switch (report.status) {
      case "pass":
        return counts.total > 0 ? "All executed checks passed" : "No checks ran";
      case "warning": return "Passed with warnings";
      case "fail": return "Compatibility failures";
      default: return "Could not complete";
    }
  }

  /* `size` is `File.size`: checked before the file is read. */
  function checkSize(size) {
    if (size > MAX_BYTES) {
      return "the file is " + size + " bytes, over the " + MAX_BYTES + "-byte (10 MiB) limit";
    }
    return null;
  }

  /* Parses report text and validates it. */
  function parseReport(text) {
    var value;
    try {
      value = JSON.parse(text);
    } catch (error) {
      return fail("the file is not valid JSON");
    }
    return validate(value);
  }

  // ---------------------------------------------------------------
  // Comparison (CF-05, viewer role per CF-08 section 2.4)
  // ---------------------------------------------------------------
  //
  // These functions reimplement the transition table of CF-05 section 4
  // for display only, exactly as `crates/canary-report/src/diff.rs`
  // `classify` does. A hidden divergence is prevented by the shared
  // golden transition table in the viewer's tests (CF-05-F).

  var DIFF_VERSION = 1;

  /* One side of an incomparable pair, for messages. */
  var SIDE_LABELS = { baseline: "the baseline report", current: "the current report" };

  function isGood(state) {
    return state === "pass" || state === "warning";
  }

  function isFailing(state) {
    return state === "fail" || state === "error";
  }

  /* Classifies one identity's transition and says whether it counts as a
   * regression. `warningsAreFailures` is the policy the *current* run was
   * evaluated under: with it, `pass` to `warning` is a regression.
   * Mirrors `diff.rs::classify`; every row has a golden test. */
  function classifyTransition(from, to, warningsAreFailures) {
    if (from === to) return { classification: "unchanged", regression: false };
    if (from === "pass" && to === "warning") {
      return { classification: "degraded", regression: warningsAreFailures };
    }
    if (from === "warning" && to === "pass") {
      return { classification: "improved", regression: false };
    }
    if (isGood(from) && isFailing(to)) {
      return { classification: "regressed", regression: true };
    }
    if (from === "fail" && to === "error") {
      return { classification: "degraded", regression: true };
    }
    if (from === "error" && to === "fail") {
      return { classification: "improved", regression: false };
    }
    if (isFailing(from) && isGood(to)) {
      return { classification: "resolved", regression: false };
    }
    if ((from === "skipped" && isGood(to)) || (from === "absent" && to === "pass")) {
      return { classification: "added", regression: false };
    }
    if ((from === "absent" || from === "skipped") && isFailing(to)) {
      return { classification: "added-failing", regression: true };
    }
    if (from === "absent" && to === "warning") {
      return { classification: "added-failing", regression: true };
    }
    if (isGood(from) && to === "absent") {
      return { classification: "removed", regression: false };
    }
    if (isFailing(from) && to === "absent") {
      return { classification: "removed-failing", regression: false };
    }
    if (isGood(from) && to === "skipped") {
      return { classification: "skipped", regression: false };
    }
    if (isFailing(from) && to === "skipped") {
      return { classification: "skipped-failing", regression: true };
    }
    if ((from === "skipped" && to === "absent") || (from === "absent" && to === "skipped")) {
      return { classification: "unchanged-skip", regression: false };
    }
    return { classification: "unchanged", regression: false };
  }

  /* UTF-8 byte order comparison of two strings (CF-05 section 5 wants
   * `(surface, id)` sorted in UTF-8 byte order). `TextEncoder` exists in
   * every browser that matters and in Node; the fallback covers the rest
   * by encoding each character to UTF-8 by hand. */
  var textEncoder = typeof TextEncoder !== "undefined" ? new TextEncoder() : null;

  function utf8BytesOf(text) {
    if (textEncoder) return textEncoder.encode(text);
    var bytes = [];
    for (var i = 0; i < text.length; i++) {
      var encoded = unescape(encodeURIComponent(text.charAt(i)));
      for (var k = 0; k < encoded.length; k++) bytes.push(encoded.charCodeAt(k) & 0xff);
    }
    return bytes;
  }

  function utf8Less(first, second) {
    var a = utf8BytesOf(first);
    var b = utf8BytesOf(second);
    for (var i = 0; i < a.length && i < b.length; i++) {
      if (a[i] !== b[i]) return a[i] < b[i];
    }
    return a.length !== b.length && a.length < b.length;
  }

  /* Identifies one side: `(surface, id)` -> state, where `id` is
   * `fixtureId` or `testId` when `fixtureId` is absent and skipped entries
   * use `fixtureId` (CF-05 section 3). Duplicate identities are refused
   * with the side named. Mirrors `diff.rs::index`. */
  function stateIndex(report, side) {
    var index = Object.create(null);
    var insert = function (surface, id, state, source) {
      var key = surface + "\u0000" + id;
      if (index[key] !== undefined) {
        return fail(
          SIDE_LABELS[side] + " repeats the result identity " + surface + "/" + id
        );
      }
      index[key] = { state: state, source: source };
      return null;
    };
    var results = Array.isArray(report.results) ? report.results : [];
    for (var i = 0; i < results.length; i++) {
      var entry = results[i];
      var id = entry.fixtureId !== undefined ? entry.fixtureId : entry.testId;
      var problem = insert(entry.surface, id, entry.status, entry.source);
      if (problem !== null) return fail(problem.reason);
    }
    var skips = Array.isArray(report.skipped) ? report.skipped : [];
    for (var j = 0; j < skips.length; j++) {
      var skipEntry = skips[j];
      var skipProblem = insert(skipEntry.surface, skipEntry.fixtureId, "skipped", undefined);
      if (skipProblem !== null) return fail(skipProblem.reason);
    }
    return { ok: true, index: index };
  }

  /* The observed protocol of one report, or `null` when it did not
   * observe one. */
  function observedProtocol(report) {
    if (isObject(report.network) && typeof report.network.observedProtocol === "number") {
      return report.network.observedProtocol;
    }
    return null;
  }

  /* Compares two *validated* CF-01 reports (issue #461).
   *
   * Returns `{ ok: true, comparison }` or `{ ok: false, reason }`, exactly
   * the shapes `parseReport` makes. The comparison mirrors
   * `crates/canary-report/src/diff.rs::compare` for `diffVersion` 1:
   * eligibility first (different `targetProtocol` is refused; each side's
   * schema must be 1), then one entry per identity sorted by
   * `(surface, id)` in UTF-8 byte order, the missing set, the summary
   * counts, and the first-match-wins verdict.
   *
   * Options: `warningsAreFailures` (mirror of `CompareOptions`), defaulting
   * to false when not given.
   */
  function compareReports(baseline, current, options) {
    var warningPolicy = options === true || (options && options.warningsAreFailures === true);

    if (!isObject(baseline) || baseline.schemaVersion !== 1) {
      return fail("the baseline report must be a validated CF-01 report (schemaVersion 1)");
    }
    if (!isObject(current) || current.schemaVersion !== 1) {
      return fail("the current report must be a validated CF-01 report (schemaVersion 1)");
    }
    if (baseline.targetProtocol !== current.targetProtocol) {
      return fail(
        "these reports cannot be compared: the baseline is about target protocol " +
          baseline.targetProtocol +
          " and the current report is about target protocol " +
          current.targetProtocol +
          ". Results about different protocols are not comparable."
      );
    }

    var before = stateIndex(baseline, "baseline");
    if (!before.ok) return before;
    var after = stateIndex(current, "current");
    if (!after.ok) return after;
    before = before.index;
    after = after.index;

    var emptyBaseline = Array.isArray(baseline.results) && baseline.results.length === 0;
    var emptyCurrent = Array.isArray(current.results) && current.results.length === 0;

    // One sorted key list over the union of both sides.
    var keys = Object.keys(before).concat(Object.keys(after).filter(function (key) {
      return before[key] === undefined;
    }));
    keys.sort(function (a, b) {
      var pa = a.split("\u0000");
      var pb = b.split("\u0000");
      var surface = utf8Less(pa[0], pb[0]) ? -1 : (utf8Less(pb[0], pa[0]) ? 1 : 0);
      if (surface !== 0) return surface;
      return utf8Less(pa[1], pb[1]) ? -1 : (utf8Less(pb[1], pa[1]) ? 1 : 0);
    });

    var entries = [];
    var missing = [];
    for (var i = 0; i < keys.length; i++) {
      var parts = keys[i].split("\u0000");
      var surface = parts[0];
      var id = parts[1];
      var from = before[keys[i]] || { state: "absent", source: undefined };
      var to = after[keys[i]] || { state: "absent", source: undefined };
      var classified = classifyTransition(from.state, to.state, warningPolicy);
      var regression = classified.regression;
      // An empty side cannot establish a regression (CF-05 section 2).
      if (emptyBaseline || emptyCurrent) regression = false;
      if (isGood(from.state) && (to.state === "absent" || to.state === "skipped")) {
        missing.push({ surface: surface, id: id });
      }
      var cached =
        from.source === "cache" || to.source === "cache";
      entries.push({
        surface: surface,
        id: id,
        from: from.state,
        to: to.state,
        classification: classified.classification,
        regression: regression,
        // `null`, not `false`, when this identity never touched the cache.
        sourceChanged: cached ? from.source !== to.source : null,
      });
    }

    var summary = {
      regressed: 0,
      resolved: 0,
      added: 0,
      removed: 0,
      unchanged: 0,
      missing: missing.length,
    };
    for (var e = 0; e < entries.length; e++) {
      if (entries[e].regression) summary.regressed++;
      switch (entries[e].classification) {
        case "resolved": summary.resolved++; break;
        case "added":
        case "added-failing": summary.added++; break;
        case "removed":
        case "removed-failing": summary.removed++; break;
        case "unchanged":
        case "unchanged-skip": summary.unchanged++; break;
        default: break;
      }
    }

    var verdict = "no-change";
    if (entries.some(function (entry) { return entry.regression; })) {
      verdict = "regression";
    } else if (missing.length > 0) {
      verdict = "coverage-lost";
    } else if (
      entries.some(function (entry) {
        return entry.classification === "resolved" ||
          entry.classification === "improved" ||
          entry.classification === "added";
      })
    ) {
      verdict = "improved";
    }

    var baselineProtocol = observedProtocol(baseline);
    var currentProtocol = observedProtocol(current);
    var packOf = function (report) {
      return isObject(report.fixturePack) &&
        typeof report.fixturePack.digest === "string"
        ? report.fixturePack.digest
        : null;
    };
    var baselinePack = packOf(baseline);
    var currentPack = packOf(current);
    var fixturePackChanged;
    if (baselinePack === null || currentPack === null) {
      fixturePackChanged = null; // unknown, not false (CF-05 section 2)
    } else {
      fixturePackChanged = baselinePack !== currentPack;
    }

    return {
      ok: true,
      comparison: {
        diffVersion: DIFF_VERSION,
        comparable: true,
        baseline: {
          toolVersion: baseline.toolVersion,
          targetProtocol: baseline.targetProtocol,
          status: baseline.status,
        },
        current: {
          toolVersion: current.toolVersion,
          targetProtocol: current.targetProtocol,
          status: current.status,
        },
        fixturePackChanged: fixturePackChanged,
        observedProtocolChanged:
          baselineProtocol !== null && currentProtocol !== null
            ? baselineProtocol !== currentProtocol
            : false,
        emptyBaseline: emptyBaseline,
        emptyCurrent: emptyCurrent,
        entries: entries,
        missing: missing,
        summary: summary,
        verdict: verdict,
      },
    };
  }

  /* The fixed verdict headings of the comparison view. Incomparable or
   * refused comparisons never reach one (they get the refusal alert). */
  function verdictHeading(verdict) {
    switch (verdict) {
      case "regression": return "Regression";
      case "coverage-lost": return "Coverage lost";
      case "improved": return "Improved";
      case "no-change": return "No change";
      case "incomparable": return "Not comparable";
      default: return "Comparison";
    }
  }

  // ---------------------------------------------------------------
  // Fixture provenance for the detail modal (E-086, issue #462)
  // ---------------------------------------------------------------

  /* CA-02 bumps `inventoryVersion` on a breaking change to the shape;
   * unknown extra fields are ignored (CA-02 section 5). */
  var INVENTORY_VERSION = 1;

  /* One string field of an object, or `null`. An empty string carries no
   * information, so it reads the same as absent. */
  function stringField(value, key) {
    if (!isObject(value)) return null;
    var item = value[key];
    return typeof item === "string" && item !== "" ? item : null;
  }

  function integerField(value, key) {
    if (!isObject(value)) return null;
    return isInteger(value[key]) && value[key] >= 0 ? value[key] : null;
  }

  /* CF-01 reserves `results[].verification` as a copy of the fixture's
   * CF-04 record and says it is never synthesized; a value that is not an
   * object is therefore no record, not a record of unknown shape. Absence
   * is the unknown state (CF-04 section 3), which CF-08 section 2.3 makes
   * the viewer show rather than read as verified. */
  function verificationRecord(result) {
    return isObject(result) && isObject(result.verification) ? result.verification : null;
  }

  /* The verifier as one readable name: `stellar-canary 0.2.0`. A record
   * with neither half gives what it carries. */
  function verifierLabel(record) {
    if (!isObject(record)) return null;
    var name = stringField(record.verifier, "name");
    var version = stringField(record.verifier, "version");
    if (name !== null && version !== null) return name + " " + version;
    return name !== null ? name : version;
  }

  /* One sentence naming what is known about a fixture's verification. The
   * wording is fixed so a reader can never take a stale, mismatched or
   * missing record for a pass: `matched` is the only state that reads as
   * verified, and a mismatched record says it is not trusted (CF-04). */
  function verificationState(record) {
    if (record === null) {
      return "No verification information: this report carries no verification record for this fixture.";
    }
    var method = stringField(record, "method");
    var how = method === null ? "a record with no stated method" : "a " + method + " record";
    var outcome = stringField(record, "outcome");
    if (outcome === "matched") {
      return "Verified by " + (method === null ? "an unstated method" : method) + ".";
    }
    if (outcome === null) return "Unverified: " + how + ", with no stated outcome.";
    return "Unverified: " + how + " with outcome " + outcome + ".";
  }

  /* The report-level line in the facts list. It counts what the report
   * actually carries, so a run whose fixtures were never verified keeps
   * the "no verification information" wording instead of an empty count. */
  function verificationSummary(report) {
    var results = isObject(report) && Array.isArray(report.results) ? report.results : [];
    var withRecord = 0;
    for (var i = 0; i < results.length; i++) {
      if (verificationRecord(results[i]) !== null) withRecord++;
    }
    if (withRecord === 0) return "no verification information";
    return withRecord + " of " + results.length + " results carry a verification record";
  }

  /* CF-01 reserves `fixturePack` as
   * `{ digest: "sha256:<hex>", registryVersion, source, revision }`. The
   * viewer shows the digest string the report carries (CF-08 section 5:
   * "shows if present") and computes nothing. */
  function packDigest(report) {
    return isObject(report) ? stringField(report.fixturePack, "digest") : null;
  }

  /* The inventory is a second local file, never fetched: it is the only
   * place a fixture's category, description and required capabilities
   * exist (CA-02 section 3), and a report carries none of them. Validation
   * is deliberately permissive about fields the viewer does not show and
   * strict about the ones it keys on. */
  function parseInventory(text) {
    var value;
    try {
      value = JSON.parse(text);
    } catch (error) {
      return fail("the inventory is not valid JSON");
    }
    if (!isObject(value)) return fail("the inventory must be a JSON object");
    if (value.inventoryVersion !== INVENTORY_VERSION) {
      return fail(
        "inventoryVersion must be " + INVENTORY_VERSION + " (found " + describe(value.inventoryVersion) + ")"
      );
    }
    if (!Array.isArray(value.fixtures)) return fail("fixtures must be an array");
    var byId = Object.create(null);
    for (var i = 0; i < value.fixtures.length; i++) {
      var fixture = value.fixtures[i];
      var where = "fixtures[" + i + "]";
      if (!isObject(fixture)) return fail(where + " must be an object");
      if (typeof fixture.id !== "string") return fail(where + ".id must be a string");
      if (byId[fixture.id] !== undefined) {
        return fail(where + " repeats the fixture id " + fixture.id);
      }
      byId[fixture.id] = fixture;
    }
    return {
      ok: true,
      inventory: {
        document: value,
        byId: byId,
        /* CA-02 lists one protocol's fixtures, so the document is only
         * evidence about a report that targets the same protocol. */
        protocol: integerField(value, "protocol"),
      },
    };
  }

  /* CA-02 ids are unique across the directory, so the join is by id; the
   * surface has to agree as well, because an entry describing a different
   * surface is not evidence about this result. A result known only by its
   * `testId` has no fixture to look up. */
  function inventoryFixture(inventory, fixtureId, surface) {
    if (!isObject(inventory) || typeof fixtureId !== "string") return null;
    var entry = inventory.byId[fixtureId];
    if (entry === undefined) return null;
    var entrySurface = stringField(entry, "surface");
    if (entrySurface !== null && typeof surface === "string" && entrySurface !== surface) {
      return null;
    }
    return entry;
  }

  /* CA-02's `requiredCapabilities` is an array of strings and `[]` means
   * "none required", which is a different answer from "the inventory does
   * not say" (`null`). */
  function capabilityList(fixture) {
    if (!isObject(fixture) || !Array.isArray(fixture.requiredCapabilities)) return null;
    var names = [];
    for (var i = 0; i < fixture.requiredCapabilities.length; i++) {
      if (typeof fixture.requiredCapabilities[i] === "string") {
        names.push(fixture.requiredCapabilities[i]);
      }
    }
    return names;
  }

  /* One sentence naming why an inventory-sourced value is missing. The
   * modal always shows the row, so the reader learns which of the four
   * situations they are in instead of finding a blank cell. */
  function inventoryAbsence(inventory, fixture, protocol) {
    if (!isObject(inventory)) return "no fixture inventory loaded";
    if (inventory.protocol !== null && protocol !== null && inventory.protocol !== protocol) {
      return "the loaded inventory lists protocol " + inventory.protocol + ", not " + protocol;
    }
    if (fixture === null) return "no inventory entry for this fixture";
    return "not in this inventory";
  }

  /* Everything the detail modal can show about one fixture, joined from
   * the three contracts that each hold a piece of it: CF-01's copy of the
   * CF-04 record, CF-01's reserved pack digest, and the CA-02 inventory.
   * `entry` is one `results[]` item or one `skipped[]` item.
   *
   * Every field is `null` when its source does not carry it. The modal
   * then states the absence (issue #462 negative case: an unverified
   * fixture must be visibly unverified, not blank), so no value is ever
   * invented and the row set does not depend on what a report happens to
   * carry. */
  function fixtureDetail(report, entry, inventory) {
    var record = verificationRecord(entry);
    var id = isObject(entry) && typeof entry.fixtureId === "string" ? entry.fixtureId : null;
    var surface = isObject(entry) ? entry.surface : null;
    var fixture = id === null ? null : inventoryFixture(inventory, id, surface);
    var target = isObject(report) && isInteger(report.targetProtocol) ? report.targetProtocol : null;

    var digest = packDigest(report);
    if (digest === null && isObject(inventory)) {
      digest = stringField(inventory.document, "packDigest");
    }
    var sha256 = stringField(record, "fixtureSha256");
    if (sha256 === null) sha256 = stringField(fixture, "sha256");

    var recordNetwork = isObject(record) ? record.network : null;
    return {
      identity: id === null ? stringField(entry, "testId") : id,
      surface: stringField(entry, "surface"),
      hasRecord: record !== null,
      state: verificationState(record),
      method: stringField(record, "method"),
      outcome: stringField(record, "outcome"),
      checkedAt: stringField(record, "checkedAt"),
      verifier: verifierLabel(record),
      endpointHost: stringField(record, "endpointHost"),
      evidenceRef: stringField(record, "evidenceRef"),
      evidenceProtocol: integerField(recordNetwork, "observedProtocol"),
      sha256: sha256,
      category: stringField(fixture, "category"),
      description: stringField(fixture, "description"),
      capabilities: capabilityList(fixture),
      packDigest: digest,
      targetProtocol: target,
      observedProtocol: isObject(report) ? observedProtocol(report) : null,
      inventoryLoaded: isObject(inventory),
      inventoryMatched: fixture !== null,
      inventoryAbsence: inventoryAbsence(inventory, fixture, target),
    };
  }

  var api = {
    MAX_BYTES: MAX_BYTES,
    STORAGE_PREFIX: STORAGE_PREFIX,
    INVENTORY_VERSION: INVENTORY_VERSION,
    validate: validate,
    parseReport: parseReport,
    deriveCounts: deriveCounts,
    countMismatches: countMismatches,
    heading: heading,
    checkSize: checkSize,
    DIFF_VERSION: DIFF_VERSION,
    classifyTransition: classifyTransition,
    stateIndex: stateIndex,
    compareReports: compareReports,
    verdictHeading: verdictHeading,
    verificationRecord: verificationRecord,
    verificationState: verificationState,
    verificationSummary: verificationSummary,
    packDigest: packDigest,
    parseInventory: parseInventory,
    inventoryFixture: inventoryFixture,
    fixtureDetail: fixtureDetail,
  };

  if (typeof module !== "undefined" && module.exports) module.exports = api;
  root.CanaryViewer = api;
})(typeof self !== "undefined" ? self : this);
