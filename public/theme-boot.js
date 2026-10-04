// Runs before first paint so the window never flashes the wrong theme. The saved choice
// lives in Fino's settings; this cache only covers the moment before they load.
(function () {
  var choice = "system";
  try {
    choice = localStorage.getItem("fino.appearance") || "system";
  } catch (e) {}
  var light = choice === "light" || (choice === "system" && matchMedia("(prefers-color-scheme: light)").matches);
  document.documentElement.dataset.theme = light ? "light" : "dark";
})();
