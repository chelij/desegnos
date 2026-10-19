# Export browser checks

These checks use installed Chromium/ChromeDriver and Firefox. They download no browsers or libraries, use disposable profiles on a guarded X11 display numbered 90 or higher, and block outside web traffic through a rejecting local proxy. Local static hosting binds only to loopback.

Create the real controlled export through the existing compiler and safe writer:

```sh
mkdir -p test-results/export-browser-fixture
node scripts/cargo.mjs run --offline -p desegni-storage --example export_browser_fixture -- test-results/export-browser-fixture
```

The example prints JSON containing the newly written website `path`. Pass that path to the browser matrix:

```sh
~/.local/bin/xvfb-run -d node scripts/export-browser-test.mjs --site /absolute/new/export/path --output test-results/export-browser-matrix
```

The matrix opens the same two-page export in Chromium and Firefox from disk, a local static server root, and a `/portfolio/` host subdirectory. It checks actual image decoding, stylesheet/background resources, a real JavaScript button, isolated page colors, nested navigation with query/fragment and return navigation. Twelve screenshots, versions, exact source hashes, local request paths, blocked background requests and cleanup results are retained. The exported files must remain byte-identical. The fixture uses valid 1×1 and 2×2 PNG candidates for its actual `1x`/`2x` responsive image declarations.

`tests/export/chromium.mjs` additionally exposes `inspectChromiumPage({url, outputDirectory, label, expression, waitForExpression})` for the native acceptance workflow. It returns the actual browser expression value, browser version, screenshot path, blocked requests and profile cleanup confirmation. The native workflow supplies its own generated-page DOM assertions; this helper never substitutes an application preview or mock DOM. `expression: 'document.documentElement.outerHTML'` returns the loaded document as a string in `result.value`.

The dedicated two-page fixture proves portable compilation and browser behavior. It does not claim live AI quality, configured external services or the complete future publication/React export gates.

Protocol references: [ChromeDriver capabilities](https://developer.chrome.com/docs/chromedriver/capabilities), [Firefox direct BiDi connection](https://developer.mozilla.org/en-US/docs/Web/WebDriver/How_to/Create_BiDi_connection), [WebDriver BiDi navigation and script execution](https://www.w3.org/TR/webdriver-bidi/).
