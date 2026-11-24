// The page already links to the latest release. Upgrade each platform's links to the named download when the release has it.
(async () => {
  try {
    const links = [...document.querySelectorAll("a[data-platform][data-asset]")];
    const first = links.find(link => link.dataset.asset);
    const canonical = document.querySelector('link[rel="canonical"]');
    if (!first || !canonical || new URL(canonical.href).hostname.endsWith(".invalid")) return;

    const repo = /^https:\/\/github\.com\/([a-z\d-]+)\/([\w.-]+)\/releases\/latest$/i.exec(first.href);
    if (!repo || /^public-owner$/i.test(repo[1]) || /^public-repo$/i.test(repo[2])) return;
    const path = `${repo[1]}/${repo[2]}`;
    const response = await fetch(`https://api.github.com/repos/${path}/releases/latest`, {
      credentials: "omit",
      redirect: "error",
      referrerPolicy: "no-referrer",
      signal: AbortSignal.timeout(3500)
    });
    if (!response.ok) return;
    const release = await response.json();
    const tag = release.tag_name;
    if (typeof tag !== "string" || !tag || !Array.isArray(release.assets)) return;
    const version = tag.replace(/^v(?=\d)/, "");
    for (const link of links) {
      if (!link.dataset.asset) continue;
      const expected = link.dataset.asset.replace(/\{(tag|version)\}/g, (_, key) => key === "tag" ? tag : version);
      const asset = release.assets.find(item => item?.name === expected);
      if (!asset) continue;
      const url = new URL(asset.browser_download_url);
      if (url.origin !== "https://github.com" || url.username || url.password || url.search || url.hash ||
          decodeURIComponent(url.pathname) !== `/${path}/releases/download/${tag}/${expected}`) continue;
      link.href = url.href;
    }
    for (const label of document.querySelectorAll("[data-release-version]")) label.textContent = tag;
  } catch {
    // A timeout, offline browser or malformed response leaves the static release links in place.
  }
})();
