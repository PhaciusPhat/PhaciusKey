/* PhaciusKey site — Telex composing demo + scroll reveal. No dependencies. */
(function () {
  "use strict";
  document.documentElement.classList.add("js");

  var reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ------------------------------------------------------------------
     Hero demo: each phrase is the exact keystroke sequence a user types
     in Telex, paired with the buffer PhaciusKey composes after that key.
     ------------------------------------------------------------------ */
  var PHRASES = [
    [
      ["T", "T"], ["i", "Ti"], ["e", "Tie"], ["e", "Tiê"], ["n", "Tiên"],
      ["g", "Tiêng"], ["s", "Tiếng"], ["␣", "Tiếng "], ["V", "Tiếng V"],
      ["i", "Tiếng Vi"], ["e", "Tiếng Vie"], ["e", "Tiếng Viê"],
      ["j", "Tiếng Việ"], ["t", "Tiếng Việt"]
    ],
    [
      ["X", "X"], ["i", "Xi"], ["n", "Xin"], ["␣", "Xin "], ["c", "Xin c"],
      ["h", "Xin ch"], ["a", "Xin cha"], ["o", "Xin chao"], ["f", "Xin chào"]
    ],
    [
      ["c", "c"], ["a", "ca"], ["f", "cà"], ["␣", "cà "], ["p", "cà p"],
      ["h", "cà ph"], ["e", "cà phe"], ["e", "cà phê"], ["␣", "cà phê "],
      ["s", "cà phê s"], ["u", "cà phê su"], ["w", "cà phê sư"],
      ["a", "cà phê sưa"], ["x", "cà phê sữa"], ["␣", "cà phê sữa "],
      ["d", "cà phê sữa d"], ["d", "cà phê sữa đ"], ["a", "cà phê sữa đa"],
      ["s", "cà phê sữa đá"]
    ]
  ];

  var buffer = document.getElementById("demo-text");
  var keys = document.getElementById("demo-keys");

  if (buffer && keys) {
    if (reduced) {
      buffer.textContent = "Tiếng Việt";
    } else {
      var phrase = 0, step = 0, timer;

      var renderKeys = function (frames, upto) {
        keys.innerHTML = "";
        var start = Math.max(0, upto - 7);
        for (var i = start; i <= upto; i++) {
          var k = document.createElement("kbd");
          k.textContent = frames[i][0];
          keys.appendChild(k);
        }
      };

      var tick = function () {
        var frames = PHRASES[phrase];
        buffer.textContent = frames[step][1];
        renderKeys(frames, step);
        step++;
        if (step >= frames.length) {
          step = 0;
          phrase = (phrase + 1) % PHRASES.length;
          timer = setTimeout(function () {
            buffer.textContent = "";
            keys.innerHTML = "";
            timer = setTimeout(tick, 500);
          }, 2200);
        } else {
          timer = setTimeout(tick, 130 + Math.random() * 90);
        }
      };
      tick();
    }
  }

  /* ---- Scroll reveal ---- */
  if (!reduced && "IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
      });
    }, { rootMargin: "0px 0px -8% 0px" });
    document.querySelectorAll(".reveal").forEach(function (el) { io.observe(el); });
  } else {
    document.querySelectorAll(".reveal").forEach(function (el) { el.classList.add("in"); });
  }
})();
