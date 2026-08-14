(function () {
  "use strict";

  var send = function (o) {
    if (window.ipc) window.ipc.postMessage(JSON.stringify(o));
  };
  var $ = function (id) { return document.getElementById(id); };
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }

  // ---- Window chrome ----
  // The window is borderless, so moving it is ours to implement: the titlebar
  // hands the drag to the window manager, which then owns the gesture.
  $("titlebar").addEventListener("mousedown", function (e) {
    if (e.target.closest("button")) return;
    send({ cmd: "drag_window" });
  });
  $("close").addEventListener("click", function () { send({ cmd: "close_window" }); });
  $("quit").addEventListener("click", function () { send({ cmd: "quit" }); });
  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape" && !recording) send({ cmd: "close_window" });
    if (e.key === "w" && (e.metaKey || e.ctrlKey)) send({ cmd: "close_window" });
  });

  // ---- Tabs ----
  var tabs = $("nav").querySelectorAll("button[data-tab]");
  each(tabs, function (btn) {
    btn.addEventListener("click", function () {
      each(tabs, function (b) {
        var on = b === btn;
        b.setAttribute("aria-selected", on ? "true" : "false");
        $("tab-" + b.dataset.tab).hidden = !on;
      });
      $("pane").scrollTop = 0;
    });
  });

  // ---- Switches ----
  // Driven off the markup rather than a list kept in step with it by hand, so
  // a switch cannot be added or renamed without its binding following.
  function toggles() {
    return document.querySelectorAll("input[type=checkbox][data-key]");
  }
  each(toggles(), function (el) {
    el.addEventListener("change", function () {
      send({ cmd: "set", key: this.dataset.key, value: this.checked });
    });
  });

  ["method", "placement"].forEach(function (key) {
    $(key).addEventListener("click", function (e) {
      var v = e.target && e.target.dataset && e.target.dataset.v;
      if (v) send({ cmd: "set", key: key, value: v });
    });
  });

  // ---- Shortcut recorder ----
  // The page never invents a shortcut string: it reports the raw event fields
  // and Rust canonicalises them, so what is stored is always something the
  // keyboard hook can match.
  var shortcut = $("shortcut");
  var shortcutHint = $("shortcut-hint");
  var recording = false;
  var lastParts = [];
  var lastHint = shortcutHint.textContent;
  var MODS = ["⌃", "⌥", "⇧", "⌘"]; // control, option, shift, command
  var ASK = "Hold modifiers, then press a key — or just let go. Esc cancels.";
  var TOO_MANY = "That’s too many — three keys at most.";
  var NEED_TWO = "Use two modifiers, or add a key.";
  var peak = 0;
  var peakMods = [];
  var usedKey = false;

  function heldMods(e) {
    var held = [e.ctrlKey, e.altKey, e.shiftKey, e.metaKey];
    return MODS.filter(function (_, i) { return held[i]; });
  }
  function heldCount(e) { return heldMods(e).length; }
  function setHint(text) { shortcutHint.textContent = text; }

  function renderCaps(el, parts, pending) {
    el.innerHTML = parts.map(function (p) {
      return "<kbd>" + esc(p) + "</kbd>";
    }).join("") + (pending ? '<kbd class="pending">?</kbd>' : "");
  }

  function startRecording() {
    if (recording) return stopRecording();
    recording = true;
    peak = 0;
    peakMods = [];
    usedKey = false;
    shortcut.classList.add("rec-on");
    shortcut.focus();
    renderCaps(shortcut, [], true);
    setHint(ASK);
    // Hold the current shortcut back, or every attempt would toggle typing.
    send({ cmd: "shortcut_record", on: true });
  }

  function stopRecording() {
    if (!recording) return;
    recording = false;
    shortcut.classList.remove("rec-on");
    renderCaps(shortcut, lastParts, false);
    setHint(lastHint);
    send({ cmd: "shortcut_record", on: false });
  }

  shortcut.addEventListener("click", startRecording);

  // Clicking anywhere else disarms. WebKit does not reliably focus a <button>
  // on click, so a blur handler alone would never fire here.
  document.addEventListener("mousedown", function (e) {
    if (recording && e.target !== shortcut) stopRecording();
  }, true);

  // Listening on the document, in the capture phase, for the same reason: the
  // button may never hold focus, and the keys must not reach the page either.
  document.addEventListener("keydown", function (e) {
    if (!recording) {
      // Keyboard users need a way in that is not a mouse click.
      if (document.activeElement === shortcut && (e.key === "Enter" || e.key === " ")) {
        e.preventDefault();
        startRecording();
      }
      return;
    }
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") return stopRecording();

    // A modifier on its own is not reported yet — the release handler below
    // reports the peak set once the whole gesture is known.
    if (["Control", "Alt", "Shift", "Meta"].indexOf(e.key) !== -1) {
      var mods = heldMods(e);
      if (mods.length >= peak) { peak = mods.length; peakMods = mods; }
      renderCaps(shortcut, mods, true);
      setHint(mods.length > 3 ? TOO_MANY : ASK);
      return;
    }

    var usable = /^Key[A-Z]$/.test(e.code) || /^Digit[0-9]$/.test(e.code) || e.code === "Space";
    if (!usable) {
      usedKey = true;
      setHint("That key can’t be used — pick a letter, a digit or space.");
      return;
    }
    if (heldCount(e) === 0) {
      usedKey = true;
      setHint("Add a modifier — a bare key would fire while you type.");
      return;
    }
    if (heldCount(e) > 2) {
      usedKey = true;
      setHint(TOO_MANY);
      return;
    }

    usedKey = true;
    send({
      cmd: "shortcut_capture",
      code: e.code,
      ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, meta: e.metaKey,
    });
    stopRecording();
  }, true);

  // Key releases must not leak to the page either while the recorder is armed.
  document.addEventListener("keyup", function (e) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();

    var stillHeld = e.ctrlKey || e.altKey || e.shiftKey || e.metaKey;
    if (stillHeld || usedKey) return;

    if (peak < 2) return setHint(NEED_TWO);
    if (peak > 3) return setHint(TOO_MANY);

    send({
      cmd: "shortcut_capture",
      code: null,
      ctrl: peakMods.indexOf("⌃") !== -1,
      alt: peakMods.indexOf("⌥") !== -1,
      shift: peakMods.indexOf("⇧") !== -1,
      meta: peakMods.indexOf("⌘") !== -1,
    });
    stopRecording();
  }, true);

  // ---- List editors ----
  function adder(inputId, buttonId, build) {
    var input = $(inputId);
    var add = function () {
      var name = input.value.trim();
      if (!name) return;
      input.value = "";
      send(build(name));
    };
    $(buttonId).addEventListener("click", add);
    input.addEventListener("keydown", function (e) { if (e.key === "Enter") add(); });
  }

  adder("addname", "addbtn", function (n) { return { cmd: "exclude", name: n, on: true }; });
  adder("slowname", "slowbtn", function (n) { return { cmd: "slow_app", name: n, on: true }; });
  adder("acname", "acbtn", function (n) { return { cmd: "autocomplete_app", name: n, on: true }; });

  var mtrig = $("mtrig"), mexp = $("mexp");
  var addMacro = function () {
    var trigger = mtrig.value.trim(), expansion = mexp.value;
    if (!trigger || !expansion) return;
    mtrig.value = ""; mexp.value = "";
    send({ cmd: "macro_set", trigger: trigger, expansion: expansion });
  };
  $("maddbtn").addEventListener("click", addMacro);
  mexp.addEventListener("keydown", function (e) { if (e.key === "Enter") addMacro(); });

  function renderList(box, names, title, remove) {
    if (!names.length) {
      box.innerHTML = '<div class="empty">Nothing listed yet.</div>';
      return;
    }
    box.innerHTML = names.map(function (n) {
      return '<div class="row"><div class="lbl"><b>' + esc(n.label) + (n.tag || "") + "</b>" +
        (n.sub ? "<small>" + esc(n.sub) + "</small>" : "") + "</div>" +
        '<button class="rm" title="' + title + '" data-name="' + esc(n.key) + '">✕</button></div>';
    }).join("");
    each(box.querySelectorAll("button.rm"), function (el) {
      el.addEventListener("click", function () { send(remove(this.dataset.name)); });
    });
  }

  // ---- Macro import / export ----
  // Export is written by the app; import goes through a file input so the
  // picker is the system's, then hands the *text* over — the page never parses
  // or validates a macro file itself.
  $("mexport").addEventListener("click", function () { send({ cmd: "macros_export" }); });
  var mfile = $("mfile");
  $("mimport").addEventListener("click", function () { mfile.click(); });
  mfile.addEventListener("change", function () {
    var file = this.files && this.files[0];
    if (!file) return;
    var reader = new FileReader();
    reader.onload = function () { send({ cmd: "macros_import", text: String(reader.result) }); };
    reader.onerror = function () { showToast("Could not read that file."); };
    reader.readAsText(file);
    // Clear it, so picking the same file twice in a row still fires `change`.
    this.value = "";
  });

  $("open_config").addEventListener("click", function () { send({ cmd: "open_config" }); });
  $("report").addEventListener("click", function () { send({ cmd: "report_issue" }); });
  $("forget").addEventListener("click", function () { send({ cmd: "forget_apps" }); });
  $("update-btn").addEventListener("click", function () { send({ cmd: "check_updates" }); });

  var toast = $("toast");
  var toastTimer = null;
  function showToast(text) {
    toast.textContent = text;
    toast.classList.add("on");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { toast.classList.remove("on"); }, 5000);
  }

  // Rebuilt only when the list actually changes, so the datalist popover isn't
  // torn down mid-search by the frequent state pushes.
  var suggestKey = null;
  function renderSuggestions(names) {
    var key = names.join("\n");
    if (key === suggestKey) return;
    suggestKey = key;
    $("applist").innerHTML = names.map(function (n) {
      return '<option value="' + esc(n) + '">';
    }).join("");
  }

  var UPDATE_LABEL = {
    checking: "Checking for updates…",
    installing: "Installing…",
    available: "Update now",
  };

  window.__setState = function (s) {
    document.documentElement.dataset.platform = s.platform;
    $("version").textContent = "v" + s.version;
    renderSuggestions(s.suggestions || []);

    each(toggles(), function (el) { el.checked = !!s[el.dataset.key]; });
    ["method", "placement"].forEach(function (key) {
      each($(key).children, function (b) { b.classList.toggle("on", b.dataset.v === s[key]); });
    });

    // Four switches that do nothing under VNI are worse than four missing ones.
    $("telex-group").hidden = s.method !== "telex";
    $("dock-row").hidden = s.platform !== "macos";

    if (s.notice) showToast(s.notice);

    lastParts = s.shortcut_parts || [s.toggle_shortcut];
    if (!recording) renderCaps(shortcut, lastParts, false);
    shortcut.classList.toggle("bad", !s.shortcut_valid);
    shortcut.title = s.toggle_shortcut;

    $("update-status").textContent = s.update_detail;
    $("update-btn").textContent = UPDATE_LABEL[s.update_state] || "Check for updates…";
    $("update-btn").disabled = s.update_state === "checking" || s.update_state === "installing";
    $("update-btn").classList.toggle("accent", s.update_state === "available");

    renderList($("apps"), (s.excluded_apps || []).map(function (n) {
      return { key: n, label: n, tag: n === s.current_app ? '<span class="tag">active</span>' : "" };
    }), "Remove from the list", function (n) {
      return { cmd: "exclude", name: n, on: false };
    });

    renderList($("macros"), (s.macros || []).map(function (m) {
      return { key: m.trigger, label: m.trigger, sub: m.expansion };
    }), "Remove this macro", function (n) {
      return { cmd: "macro_remove", trigger: n };
    });

    renderList($("slowapps"), (s.slow_apps || []).map(function (n) {
      return { key: n, label: n };
    }), "Back to normal speed", function (n) {
      return { cmd: "slow_app", name: n, on: false };
    });

    renderList($("acapps"), (s.autocomplete_fix_apps || []).map(function (n) {
      return { key: n, label: n };
    }), "Back to normal injection", function (n) {
      return { cmd: "autocomplete_app", name: n, on: false };
    });
  };

  send({ cmd: "init" });
})();
