/* Page behaviour for the static report viewer (CF-08-A).
 *
 * Report text is only ever placed into the page with `textContent`. The
 * file is read locally and never sent anywhere; the page's CSP forbids
 * network requests. The only thing stored is the theme preference.
 */
(function () {
  "use strict";

  var V = self.CanaryViewer;
  var THEME_KEY = V.STORAGE_PREFIX + "theme";

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

  function showInvalid(reason) {
    el("result").hidden = false;
    el("result").setAttribute("data-state", "invalid");
    setText("heading", V.heading(null, null));
    setText("status-line", "");
    setText("invalid-reason", reason);
    el("invalid-reason").hidden = false;
    el("facts").hidden = true;
    el("warnings").hidden = true;
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
    if (network && typeof network.observedProtocol === "number" && network.observedProtocol !== report.targetProtocol) {
      warnings.push(
        "The network reported protocol " + network.observedProtocol +
          ", but this report targets protocol " + report.targetProtocol + "."
      );
    }

    el("result").hidden = false;
    el("result").setAttribute("data-state", report.status);
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

    var box = el("warnings");
    clearChildren(box);
    box.hidden = warnings.length === 0;
    warnings.forEach(function (message) {
      var item = document.createElement("li");
      item.textContent = message;
      box.appendChild(item);
    });
  }

  function loadFile(file) {
    var problem = V.checkSize(file.size);
    if (problem) {
      showInvalid(problem);
      return;
    }
    file.text().then(function (text) {
      var parsed = V.parseReport(text);
      if (parsed.ok) showReport(parsed.report);
      else showInvalid(parsed.reason);
    }, function () {
      showInvalid("the file could not be read");
    });
  }

  function applyTheme(theme) {
    if (theme === "light" || theme === "dark") {
      document.documentElement.setAttribute("data-theme", theme);
    } else {
      document.documentElement.removeAttribute("data-theme");
    }
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
      /* Storage can be unavailable; the preference simply is not kept. */
    }
  }

  function init() {
    applyTheme(storedTheme());
    el("theme").addEventListener("click", function () {
      var root = document.documentElement;
      var dark = root.getAttribute("data-theme") === "dark" ||
        (!root.hasAttribute("data-theme") &&
          self.matchMedia && self.matchMedia("(prefers-color-scheme: dark)").matches);
      var next = dark ? "light" : "dark";
      applyTheme(next);
      storeTheme(next);
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
  }

  init();
})();
