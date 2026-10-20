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
   * Unknown extra fields are ignored. */
  function validate(value) {
    if (!isObject(value)) return fail("the report must be a JSON object");
    if (value.schemaVersion !== 1) {
      return fail("schemaVersion must be 1 (found " + describe(value.schemaVersion) + ")");
    }
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

  var api = {
    MAX_BYTES: MAX_BYTES,
    STORAGE_PREFIX: STORAGE_PREFIX,
    validate: validate,
    parseReport: parseReport,
    deriveCounts: deriveCounts,
    countMismatches: countMismatches,
    heading: heading,
    checkSize: checkSize
  };

  if (typeof module !== "undefined" && module.exports) module.exports = api;
  root.CanaryViewer = api;
})(typeof self !== "undefined" ? self : this);
