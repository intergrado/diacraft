// Each download link already goes to its file in the release these pages were made for, so a click starts the download with no script at all.
// Upgrade the links to the files of a newer release when the repository has one.
(async () => {
  try {
    const links = [...document.querySelectorAll("a[data-download][data-asset]")];
    const first = links.find(link => link.dataset.asset);
    const canonical = document.querySelector('link[rel="canonical"]');
    if (!first || !canonical || new URL(canonical.href).hostname.endsWith(".invalid")) return;

    // The repository and the release of the page's own links: .../<owner>/<name>/releases/download/<tag>/<file>
    const repo = /^https:\/\/github\.com\/([a-z\d-]+)\/([\w.-]+)\/releases\/download\/([^/?#]+)\/[^/?#]+$/i.exec(first.href);
    if (!repo || /^public-owner$/i.test(repo[1]) || /^public-repo$/i.test(repo[2])) return;
    const path = `${repo[1]}/${repo[2]}`;
    const own = parse(decodeURIComponent(repo[3]));
    if (!own) return;

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
    // The latest release the API names is never a draft, so while a release waits to be published it names an older one: only a newer release replaces the page's own links.
    const latest = parse(tag);
    if (!latest || !newer(latest, own)) return;

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
    // A timeout, offline browser or malformed response leaves the links as the page made them.
  }

  // A tag as numbers and an optional suffix: v1.2.3 or v1.2.3-rc.1.
  function parse(tag) {
    const match = /^v?(\d{1,9})\.(\d{1,9})\.(\d{1,9})(?:-([0-9A-Za-z.-]+))?$/.exec(tag);
    return match ? { numbers: match.slice(1, 4).map(Number), pre: match[4] || "" } : null;
  }

  // Whether release `a` is newer than release `b`: a larger number, or the same numbers without the suffix that `b` has.
  function newer(a, b) {
    for (let i = 0; i < 3; i++) {
      if (a.numbers[i] !== b.numbers[i]) return a.numbers[i] > b.numbers[i];
    }
    return !a.pre && !!b.pre;
  }
})();
