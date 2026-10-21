/* Page behaviour for the static report viewer (CF-08-A, CF-08-E).
 *
 * Report text is only ever placed into the page with `textContent`. The
 * file is read locally and never sent anywhere; the page's CSP forbids
 * network requests. The only thing stored is the theme preference.
 *
 * Everything reachable by touch is reachable by keyboard: the surface
 * tabs follow the WAI-ARIA tabs pattern (roving tabindex, arrow keys,
 * Home and End), the findings list is plain list content, the report
 * source opens in a native modal <dialog> (which traps Tab focus and
 * returns it to the trigger when closed), and `<details>`+`<summary>`
 * rows toggle with Enter or Space natively.
 */
(function () {
  "use strict";

  var V = self.CanaryViewer;
  var THEME_KEY = V.STORAGE_PREFIX + "theme";

  /* Tab order follows the surface names used elsewhere (E-085): All first,
   * then the closed surface set of CF-01 in its documented order. */
  var TAB_KEYS = ["all", "rpc", "xdr", "soroban"];
  /* Cap for the raw-JSON preview so a 10 MiB report cannot freeze a dialog. */
  var SOURCE_PREVIEW_CHARS = 1000000;

  var currentReport = null;
  var currentSurface = "all";
  var currentQuery = "";

  function el(id) {
    return document.getElementById(id);
  }

  function setText(id, text) {
    el(id).textContent = text;
  }

  function clearChildren(node) {
    while (node.firstChild) node.removeChild(node.firstChild);
  }

  function fact(list, label, value) {
    var term = document.createElement("dt");
    term.textContent = label;
    var detail = document.createElement("dd");
    detail.textContent = value;
    list.appendChild(term);
    list.appendChild(detail);
  }

  function span(cls, text) {
    var node = document.createElement("span");
    node.setAttribute("class", cls);
    node.textContent = text;
    return node;
  }

  /* The protocol the run observed, wherever the report carries it: the
   * schema nests it under `network`, but a top-level `observedProtocol` is
   * accepted too. `null` means "not observed", which is not a mismatch. */
  function observedProtocolOf(report) {
    var network = report.network;
    if (network && typeof network.observedProtocol === "number") {
      return network.observedProtocol;
    }
    if (typeof report.observedProtocol === "number") {
      return report.observedProtocol;
    }
    return null;
  }

  /* A mismatch between the target and the observed protocol is the one
   * finding that invalidates the whole report, so it gets its own alert
   * banner at the top rather than a line in the warnings list. Reports that
   * match, or that never observed a protocol (offline run, transport error),
   * leave the banner hidden — no banner, no undefined, no throw. */
  function showMismatchBanner(report) {
    var banner = el("protocol-mismatch");
    var observed = observedProtocolOf(report);
    var target = report.targetProtocol;
    if (observed === null || typeof target !== "number" || observed === target) {
      banner.hidden = true;
      banner.textContent = "";
      return;
    }
    banner.textContent =
      "Protocol Mismatch Warning: Target protocol (" + target +
      ") differs from network observed protocol (" + observed + ").";
    banner.hidden = false;
  }

  /* --- filter state helpers --- */

  function surfaceAllowed(surface) {
    return currentSurface === "all" || surface === currentSurface;
  }

  function idOfResult(result) {
    return result.fixtureId !== undefined ? result.fixtureId : result.testId;
  }

  /* Search covers "by fixture ID or description": the identity string
   * (fixture id, else test id) and the human summary. Details are only the
   * expanded body and are not searched. Matching is a case-insensitive
   * substring over both. */
  function searchHidden(summary, identity) {
    if (currentQuery === "") return false;
    var haystack = (identity + "\u0000" + summary).toLowerCase();
    return haystack.indexOf(currentQuery) === -1;
  }

  function showWarnings(list, warnings) {
    clearChildren(list);
    list.hidden = warnings.length === 0;
    warnings.forEach(function (message) {
      var item = document.createElement("li");
      item.textContent = message;
      list.appendChild(item);
    });
  }

  function showInvalid(reason) {
    currentReport = null;
    el("protocol-mismatch").hidden = true;
    el("protocol-mismatch").textContent = "";
    el("result").hidden = false;
    el("result").setAttribute("data-state", "invalid");
    setText("heading", V.heading(null, null));
    setText("status-line", "");
    setText("invalid-reason", reason);
    el("invalid-reason").hidden = false;
    el("facts").hidden = true;
    el("warnings").hidden = true;
    el("findings").hidden = true;
    el("skipped-section").hidden = true;
    el("search-status").textContent = "";
    el("findings-empty").hidden = true;
  }

  function findingRow(result) {
    var row = document.createElement("li");
    row.setAttribute("class", "finding");

    var status = span("status", result.status);
    status.setAttribute("data-status", result.status);
    row.appendChild(status);
    row.appendChild(span("meta", result.surface));
    row.appendChild(span("identity", idOfResult(result)));
    row.appendChild(span("summary", result.summary));

    if (typeof result.details === "string") {
      var more = document.createElement("details");
      var toggle = document.createElement("summary");
      toggle.textContent = "Details";
      toggle.setAttribute("aria-label", "Details of " + idOfResult(result));
      var body = document.createElement("p");
      body.textContent = result.details;
      more.appendChild(toggle);
      more.appendChild(body);
      row.appendChild(more);
    }
    return row;
  }

  function skippedRow(skip) {
    var row = document.createElement("li");
    row.setAttribute("class", "finding skipped");
    row.appendChild(span("status", "skipped"));
    row.appendChild(span("meta", skip.surface));
    row.appendChild(span("identity", skip.fixtureId));
    var reason = skip.reason;
    if (typeof skip.code === "string" && skip.code !== "") {
      reason += " (" + skip.code + ")";
    }
    row.appendChild(span("summary", reason));
    return row;
  }

  /* The live line above the list: what is being shown and why rows are
   * missing. Screen readers pick this up because it sits in the polite
   * live region; sighted users get the same numbers. */
  function setSearchStatus(matched, skippedMatched) {
    var skipped = currentReport && Array.isArray(currentReport.skipped)
      ? currentReport.skipped.length : 0;
    var text;
    if (currentQuery !== "") {
      text = matched === 0
        ? "No results match \"" + currentQuery + "\"."
        : matched + " of " + currentReport.results.length + " results match \"" + currentQuery + "\".";
    } else if (matched === 0) {
      text = "No results in this view.";
    } else {
      text = "Showing all " + matched + " results.";
    }
    if (skipped > 0) {
      text += " " + skippedMatched + " of " + skipped + " skipped fixtures shown.";
    }
    setText("search-status", text);
  }

  function renderFindings() {
    if (!currentReport) return;

    var list = el("findings-list");
    var skippedList = el("skipped-list");
    clearChildren(list);
    clearChildren(skippedList);

    var matched = 0;
    var skippedMatched = 0;

    currentReport.results.forEach(function (result) {
      if (!surfaceAllowed(result.surface)) return;
      if (searchHidden(result.summary, idOfResult(result))) return;
      matched++;
      list.appendChild(findingRow(result));
    });

    var skipped = Array.isArray(currentReport.skipped) ? currentReport.skipped : [];
    skipped.forEach(function (skip) {
      if (!surfaceAllowed(skip.surface)) return;
      if (searchHidden(skip.reason, skip.surface + "/" + skip.fixtureId)) return;
      skippedMatched++;
      skippedList.appendChild(skippedRow(skip));
    });

    el("findings-empty").hidden = matched !== 0;
    if (matched === 0) {
      el("findings-empty").textContent = currentQuery !== ""
        ? "No results match \"" + currentQuery + "\"."
        : "No results in this view.";
    }
    setSearchStatus(matched, skippedMatched);
  }

  /* Roving tabindex: exactly one tab is selected, and that tab is the only
   * one in the page's Tab order; the rest are reached with the arrow keys. */
  function setSelectedTab(key, moveFocus) {
    currentSurface = key;
    TAB_KEYS.forEach(function (k) {
      var tab = el("tab-" + k);
      if (!tab) return;
      var selected = k === key;
      tab.setAttribute("aria-selected", selected ? "true" : "false");
      tab.setAttribute("tabindex", selected ? "0" : "-1");
    });
    el("panel-findings").setAttribute("aria-labelledby", "tab-" + key);
    if (moveFocus) {
      var active = el("tab-" + key);
      if (active) active.focus();
    }
    renderFindings();
  }

  function onTabKeydown(event) {
    var index = TAB_KEYS.indexOf(currentSurface);
    var next;
    if (event.key === "ArrowRight") {
      next = TAB_KEYS[(index + 1) % TAB_KEYS.length];
    } else if (event.key === "ArrowLeft") {
      next = TAB_KEYS[(index - 1 + TAB_KEYS.length) % TAB_KEYS.length];
    } else if (event.key === "Home") {
      next = TAB_KEYS[0];
    } else if (event.key === "End") {
      next = TAB_KEYS[TAB_KEYS.length - 1];
    } else {
      return;
    }
    event.preventDefault();
    setSelectedTab(next, true);
  }

  /* The raw file, as received: the preview is inert text in a focusable
   * scroll region inside a native modal dialog. showDialog traps focus;
   * Escape and the Close button close it, and the browser returns focus
   * to the trigger. */
  function setSourcePreview(text) {
    var truncated = text.length > SOURCE_PREVIEW_CHARS;
    el("report-source-code").textContent = truncated ? text.slice(0, SOURCE_PREVIEW_CHARS) : text;
    el("report-source-note").textContent = truncated
      ? "Preview limited to the first " + SOURCE_PREVIEW_CHARS + " characters of the file."
      : "The file content is shown exactly as received. Nothing runs and nothing is uploaded.";
    el("open-source").hidden = false;
  }

  function resetView() {
    currentReport = null;
    currentQuery = "";
    el("search").value = "";
    setSelectedTab("all", false);
    el("search-status").textContent = "";
    el("findings-empty").hidden = true;
  }

  function showReport(report) {
    var counts = V.deriveCounts(report);
    var warnings = [];
    var mismatched = V.countMismatches(report, counts);
    if (mismatched.length > 0) {
      warnings.push(
        "The report's stated counts disagree with its results for: " +
          mismatched.join(", ") + ". The counts shown are derived from the results."
      );
    }
    var network = report.network || null;

    el("result").hidden = false;
    el("result").setAttribute("data-state", report.status);
    /* After the result section is visible, so the alert region is announced. */
    showMismatchBanner(report);
    setText("heading", V.heading(report, counts));
    el("invalid-reason").hidden = true;
    setText("status-line", "Report status: " + report.status);

    var list = el("facts");
    clearChildren(list);
    list.hidden = false;
    fact(list, "Project", report.project.name + " (" + report.project.type + ")");
    fact(list, "Target protocol", String(report.targetProtocol));
    if (network && typeof network.name === "string") {
      fact(list, "Network", network.name +
        (typeof network.observedProtocol === "number" ? ", observed protocol " + network.observedProtocol : ""));
    }
    fact(list, "Tool version", report.toolVersion);
    fact(list, "Executed", String(counts.total));
    fact(list, "Passed", String(counts.passed));
    fact(list, "Warnings", String(counts.warnings));
    fact(list, "Failed", String(counts.failed));
    fact(list, "Errors", String(counts.errors));
    fact(list, "Skipped", String(counts.skipped));
    fact(list, "Verification", "no verification information");

    showWarnings(el("warnings"), warnings);

    currentReport = report;
    el("findings").hidden = false;
    el("skipped-section").hidden = !Array.isArray(report.skipped) || report.skipped.length === 0;
    renderFindings();
  }

  function loadFile(file) {
    resetView();
    el("open-source").hidden = true;
    var problem = V.checkSize(file.size);
    if (problem) {
      /* The limit is enforced before the file is ever read. */
      showInvalid(problem);
      return;
    }
    file.text().then(function (text) {
      setSourcePreview(text);
      var parsed = V.parseReport(text);
      if (parsed.ok) showReport(parsed.report);
      else showInvalid(parsed.reason);
    }, function () {
      showInvalid("the file could not be read");
    });
  }

  // ---------------------------------------------------------------
  // The two-report comparison view (CF-08 section 2.4)
  // ---------------------------------------------------------------

  /* Each side holds its validated report or its refusal reason. Loading a
   * side re-runs the comparison only when both sides hold validated
   * reports; refusal is never replaced by a comparison that would label an
   * incomparable pair as compared. */
  var sides = {
    baseline: { report: null, reason: null },
    current: { report: null, reason: null },
  };

  function describeSide(name, slot) {
    if (slot.report === null && slot.reason === null) return name + ": no file loaded yet.";
    if (slot.report === null) return name + ": " + slot.reason;
    return (
      name +
      ": loaded. Target protocol " + slot.report.targetProtocol +
      ", status " + slot.report.status + "."
    );
  }

  function hideComparisonPanels() {
    el("compare-ineligible").hidden = true;
    el("compare-ineligible").textContent = "";
    el("compare-verdict").hidden = true;
    setText("compare-verdict", "");
    setText("compare-summary", "");
    el("compare-table").hidden = true;
    clearChildren(el("compare-body"));
    el("compare-result").hidden = true;
    el("compare-result").removeAttribute("data-verdict");
  }

  function showIneligible(reason) {
    hideComparisonPanels();
    el("compare-result").hidden = false;
    var banner = el("compare-ineligible");
    banner.textContent = "These reports are not compared: " + reason;
    banner.hidden = false;
  }

  function showComparison(comparison) {
    el("compare-ineligible").hidden = true;
    el("compare-ineligible").textContent = "";
    el("compare-result").hidden = false;
    el("compare-result").setAttribute("data-verdict", comparison.verdict);

    var verdict = el("compare-verdict");
    verdict.textContent = V.verdictHeading(comparison.verdict);
    verdict.hidden = false;

    var summary = comparison.summary;
    var notes = [
      summary.regressed + " regressed",
      summary.resolved + " resolved",
      summary.added + " added",
      summary.removed + " removed",
      summary.unchanged + " unchanged",
      "out of " + comparison.entries.length + " checks",
      summary.missing + " lost coverage",
    ].join(", ") + ".";
    if (comparison.emptyBaseline) {
      notes += " The baseline ran no checks.";
    }
    if (comparison.emptyCurrent) {
      notes += " The current run ran no checks.";
    }
    if (comparison.observedProtocolChanged) {
      notes += " The network observed protocol changed between runs.";
    }
    if (comparison.fixturePackChanged === true) {
      notes += " The fixture pack changed between runs.";
    } else if (comparison.fixturePackChanged === null) {
      notes += " The fixture pack could not be compared between runs.";
    }
    setText("compare-summary", notes);

    var body = el("compare-body");
    clearChildren(body);
    comparison.entries.forEach(function (entry) {
      var row = document.createElement("tr");

      var surface = document.createElement("td");
      surface.textContent = entry.surface;
      row.appendChild(surface);

      var check = document.createElement("td");
      check.textContent = entry.id;
      row.appendChild(check);

      var before = document.createElement("td");
      before.textContent = entry.from;
      row.appendChild(before);

      var after = document.createElement("td");
      after.textContent = entry.to;
      row.appendChild(after);

      var outcome = document.createElement("td");
      outcome.setAttribute("data-classification", entry.classification);
      outcome.textContent = entry.regression
        ? entry.classification + " — counts as a regression"
        : entry.classification;
      row.appendChild(outcome);

      body.appendChild(row);
    });
    el("compare-table").hidden = false;
  }

  function refreshComparison() {
    var baseline = sides.baseline;
    var current = sides.current;
    // An unloaded or refused side keeps the old comparison visible but
    // never invents one: the state line says what is missing.
    setText(
      "compare-status",
      [describeSide("Baseline", baseline), describeSide("Current", current)].join(" ")
    );
    el("compare-status").hidden = false;
    if (baseline.report === null || current.report === null) {
      hideComparisonPanels();
      return;
    }
    var outcome = V.compareReports(baseline.report, current.report);
    if (outcome.ok) {
      showComparison(outcome.comparison);
    } else {
      showIneligible(outcome.reason);
    }
  }

  function loadSide(slotName, file) {
    var slot = sides[slotName];
    var problem = V.checkSize(file.size);
    if (problem) {
      slot.report = null;
      slot.reason = problem;
      refreshComparison();
      return;
    }
    file.text().then(function (text) {
      var parsed = V.parseReport(text);
      if (parsed.ok) {
        slot.report = parsed.report;
        slot.reason = null;
      } else {
        slot.report = null;
        slot.reason = "the report was refused: " + parsed.reason;
      }
      refreshComparison();
    }, function () {
      slot.report = null;
      slot.reason = "the file could not be read";
      refreshComparison();
    });
  }

  /* Shared drag-and-drop activation for one drop zone plus its input. */
  function attachFilePicker(drop, input, onFile) {
    input.addEventListener("change", function () {
      if (input.files && input.files[0]) onFile(input.files[0]);
    });
    ["dragenter", "dragover"].forEach(function (name) {
      drop.addEventListener(name, function (event) {
        event.preventDefault();
        drop.setAttribute("data-active", "true");
      });
    });
    ["dragleave", "drop"].forEach(function (name) {
      drop.addEventListener(name, function (event) {
        event.preventDefault();
        drop.removeAttribute("data-active");
      });
    });
    drop.addEventListener("drop", function (event) {
      var files = event.dataTransfer && event.dataTransfer.files;
      if (files && files[0]) onFile(files[0]);
    });
  }

  /* --- theme (E-088) --- */

  function prefersDark() {
    return Boolean(self.matchMedia &&
      self.matchMedia("(prefers-color-scheme: dark)").matches);
  }

  function applyTheme(theme) {
    var root = document.documentElement;
    var dark;
    if (theme === "light" || theme === "dark") {
      root.setAttribute("data-theme", theme);
      dark = theme === "dark";
    } else {
      root.removeAttribute("data-theme");
      dark = prefersDark();
    }
    el("theme").setAttribute("aria-pressed", dark ? "true" : "false");
  }

  function storedTheme() {
    try {
      return self.localStorage.getItem(THEME_KEY);
    } catch (error) {
      return null;
    }
  }

  function storeTheme(theme) {
    try {
      self.localStorage.setItem(THEME_KEY, theme);
    } catch (error) {
      /* Storage can be unavailable (private browsing, sandboxing); the
       * preference simply is not kept. */
    }
  }

  function toggleTheme() {
    var root = document.documentElement;
    var dark = root.getAttribute("data-theme") === "dark" ||
      (!root.hasAttribute("data-theme") && prefersDark());
    var next = dark ? "light" : "dark";
    applyTheme(next);
    storeTheme(next);
  }

  function init() {
    applyTheme(storedTheme());
    setSelectedTab("all", false);
    el("theme").addEventListener("click", toggleTheme);

    TAB_KEYS.forEach(function (key) {
      var tab = el("tab-" + key);
      if (tab) {
        tab.addEventListener("click", function () {
          setSelectedTab(key, false);
        });
      }
    });
    el("tabs").addEventListener("keydown", onTabKeydown);

    el("search").addEventListener("input", function () {
      currentQuery = el("search").value.toLowerCase();
      renderFindings();
    });

    var dialog = el("report-source");
    el("open-source").addEventListener("click", function () {
      dialog.showModal();
    });
    el("close-source").addEventListener("click", function () {
      dialog.close();
    });
    dialog.addEventListener("click", function (event) {
      /* A click on the backdrop, not on the dialog's own content. */
      if (event.target === dialog) dialog.close();
    });

    var input = el("file");
    input.addEventListener("change", function () {
      if (input.files && input.files[0]) loadFile(input.files[0]);
    });

    var drop = el("drop");
    ["dragenter", "dragover"].forEach(function (name) {
      drop.addEventListener(name, function (event) {
        event.preventDefault();
        drop.setAttribute("data-active", "true");
      });
    });
    ["dragleave", "drop"].forEach(function (name) {
      drop.addEventListener(name, function (event) {
        event.preventDefault();
        drop.removeAttribute("data-active");
      });
    });
    drop.addEventListener("drop", function (event) {
      var files = event.dataTransfer && event.dataTransfer.files;
      if (files && files[0]) loadFile(files[0]);
    });

    attachFilePicker(el("baseline-drop"), el("baseline-file"), function (file) {
      loadSide("baseline", file);
    });
    attachFilePicker(el("current-drop"), el("current-file"), function (file) {
      loadSide("current", file);
    });
  }

  init();
})();
