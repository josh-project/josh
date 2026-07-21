// Privacy-friendly analytics by Plausible.
// Injects the Plausible script at runtime so it appears in every mdBook page.
(function () {
  var s = document.createElement("script");
  s.async = true;
  s.defer = true;
  s.setAttribute("data-domain", "josh-project.dev");
  s.src = "https://plausible.internal.josh-project.dev/js/script.js";
  document.head.appendChild(s);
})();
