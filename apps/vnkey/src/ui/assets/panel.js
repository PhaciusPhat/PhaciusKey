(function () {
  "use strict";

  var send = function (o) {
    if (window.ipc) window.ipc.postMessage(JSON.stringify(o));
  };
  var $ = function (id) { return document.getElementById(id); };
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };

  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape") send({ cmd: "close_window" });
  });

  $("enabled").addEventListener("change", function () {
    send({ cmd: "toggle_vietnamese" });
  });

  ["method", "placement"].forEach(function (key) {
    $(key).addEventListener("click", function (e) {
      var v = e.target && e.target.dataset && e.target.dataset.v;
      if (v) send({ cmd: "set", key: key, value: v });
    });
  });

  $("update").addEventListener("click", function () { send({ cmd: "check_updates" }); });
  $("settings").addEventListener("click", function () { send({ cmd: "open_settings" }); });
  $("quit").addEventListener("click", function () { send({ cmd: "quit" }); });

  // The window is sized to the page, so every change of content has to be
  // reported: the warning and the update row both come and go.
  var reported = 0;
  function reportHeight() {
    var height = Math.ceil($("surface").getBoundingClientRect().height);
    if (!height || height === reported) return;
    reported = height;
    send({ cmd: "panel_height", height: height });
  }
  if (window.ResizeObserver) new ResizeObserver(reportHeight).observe($("surface"));

  var UPDATE_LABEL = {
    idle: "Check for updates…",
    checking: "Checking for updates…",
    installing: "Installing…",
  };

  function caps(parts) {
    return parts.map(function (p) {
      return "<kbd>" + (p === " " ? "Space" : p
        .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")) + "</kbd>";
    }).join("");
  }

  window.__setState = function (s) {
    document.documentElement.dataset.platform = s.platform;
    $("version").textContent = "v" + s.version;
    $("secure").hidden = !s.secure_input;

    $("enabled").checked = !!s.enabled;
    $("shortcut").innerHTML = caps(s.shortcut_parts || []);

    $("where").textContent = s.excluded_summary;
    $("excluded").hidden = !s.excluded_here;
    $("excluded").textContent = "⚠ " + (s.current_app || "This app") + " is one of them";

    each($("method").children, function (b) { b.classList.toggle("on", b.dataset.v === s.method); });
    each($("placement").children, function (b) { b.classList.toggle("on", b.dataset.v === s.placement); });

    var update = $("update");
    update.textContent = s.update_state === "available"
      ? "⬇ Update to v" + (s.update_version || "") + " now"
      : UPDATE_LABEL[s.update_state] || "Check for updates…";
    update.disabled = s.update_state === "checking" || s.update_state === "installing";
    update.classList.toggle("accent", s.update_state === "available");

    reportHeight();
  };

  send({ cmd: "init" });
}());
