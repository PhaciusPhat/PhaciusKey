(function () {
  "use strict";

  var send = function (o) {
    if (window.ipc) window.ipc.postMessage(JSON.stringify(o));
  };
  var $ = function (id) { return document.getElementById(id); };
  var close = function () { send({ cmd: "close_window" }); };

  $("done").addEventListener("click", close);

  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape") return close();
    if (e.key === "Enter" && !$("action").hidden) $("action").click();
  });

  var reported = 0;
  function reportHeight() {
    var height = Math.ceil($("surface").getBoundingClientRect().height);
    if (!height || height === reported) return;
    reported = height;
    send({ cmd: "panel_height", height: height });
  }
  if (window.ResizeObserver) new ResizeObserver(reportHeight).observe($("surface"));

  window.__setNotice = function (n) {
    $("title").textContent = n.title;
    $("body").textContent = n.body;
    $("warn").hidden = !n.warn;
    $("warn").textContent = n.warn || "";

    var action = $("action");
    action.hidden = !n.action;
    action.textContent = n.action ? n.action.label : "";
    action.onclick = n.action
      ? function () { send({ cmd: n.action.cmd }); close(); }
      : null;

    reportHeight();
  };
}());
