# Build script — converts each spec MD into a styled HTML page that uses
# the shared parser.js renderer. Run from any directory.
#
# Usage: pwsh -File Docs/build.ps1
# (Or `pwsh Docs\build.ps1` from the repo root.)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$docsDir  = $PSScriptRoot

# (filename, title, sidebar-section) tuples.
# Order matters: this is the canonical reading order.
$docs = @(
  @{ md='JUX-LANG-V1.md';                       title='Language V1 — Architecture & Design';    section='Foundation' },
  @{ md='JUX-GRAMMAR-ADDENDUM.md';              title='Formal Grammar';                          section='Core' },
  @{ md='JUX-SEMANTICS-ADDENDUM.md';            title='Execution Semantics';                     section='Core' },
  @{ md='JUX-OPERATORS-ADDENDUM.md';            title='Operators (Final)';                       section='Core' },
  @{ md='JUX-LAYOUT-ABI-ADDENDUM.md';           title='Layout / ABI / unsafe';                   section='Core' },
  @{ md='JUX-EXCEPTIONS-ADDENDUM.md';           title='Exceptions & Result Lowering';            section='Core' },
  @{ md='JUX-MISSING-DEFS-ADDENDUM.md';         title='Missing Definitions';                     section='Core' },
  @{ md='JUX-OBSERVABLE-PROPERTIES-ADDENDUM.md'; title='Observable Properties';                  section='Core' },
  @{ md='JUX-ANNOTATIONS-ADDENDUM.md';          title='Annotation System';                       section='Core' },
  @{ md='JUX-ENTRY-POINTS-ADDENDUM.md';         title='Entry Points';                            section='Core' },
  @{ md='JUX-COMPILER-PIPELINE-ADDENDUM.md';    title='Compiler Pipeline';                       section='Compiler' },
  @{ md='JUX-TYPE-SYSTEM-ADDENDUM.md';          title='Type System Algorithms';                  section='Compiler' },
  @{ md='JUX-CLASS-REPRESENTATION-ADDENDUM.md'; title='Class Representation';                    section='Compiler' },
  @{ md='JUX-CORE-LIB-ADDENDUM.md';             title='Minimal Core Library';                    section='Compiler' },
  @{ md='JUX-BUILD-SYSTEM-ADDENDUM.md';         title='Build System';                            section='Compiler' },
  @{ md='JUX-RUNTIME-ABI-ADDENDUM.md';          title='Runtime / Mangling / Coherence';          section='Compiler' },
  @{ md='JUX-DIAGNOSTICS-ADDENDUM.md';          title='Diagnostic Codes';                        section='Compiler' },
  @{ md='JUX-CODEGEN-FIXES.md';                 title='Codegen Fixes';                           section='Compiler' },
  @{ md='JUX-BINDGEN-ADDENDUM.md';              title='Bindgen & Interface Stubs';               section='Tooling' },
  @{ md='JUX-LSP-SERVER-ADDENDUM.md';           title='Language Server (juxc-lsp)';              section='Tooling' },
  @{ md='JUX-EDITOR-TOOLING-ADDENDUM.md';       title='Editor & IDE Tooling';                    section='Tooling' },
  @{ md='JUX-INTELLIJ-PLUGIN-ADDENDUM.md';      title='IntelliJ Platform Plugin';                section='Tooling' },
  @{ md='ERRATA.md';                            title='ERRATA — Spec Reconciliations';           section='Process' },
  @{ md='JUX-INHERITANCE-BORROW-ADDENDUM.md';   title='Inheritance × Borrow Inference';          section='Process' },
  @{ md='JUX-ASYNC-ADDENDUM-v2.md';             title='Async / Await (v2)';                      section='Process' },
  @{ md='JUX-GAPS-ROADMAP.md';                  title='Gaps Roadmap (historical)';               section='Process' }
)

# Sidebar HTML (same on every page)
function Get-SidebarHtml {
  $sb = New-Object System.Text.StringBuilder
  [void]$sb.AppendLine('<aside class="sidebar">')
  [void]$sb.AppendLine('  <div class="sidebar-top">')
  [void]$sb.AppendLine('    <a class="sidebar-brand" href="index.html">')
  [void]$sb.AppendLine('      <span class="sidebar-brand-mark">J</span>')
  [void]$sb.AppendLine('      <span>Jux</span>')
  [void]$sb.AppendLine('    </a>')
  [void]$sb.AppendLine('    <button class="theme-toggle" id="theme-toggle" type="button" aria-label="Toggle color theme">☾</button>')
  [void]$sb.AppendLine('  </div>')
  [void]$sb.AppendLine('  <div class="sidebar-section">')
  [void]$sb.AppendLine('    <div class="sidebar-section-title">Start Here</div>')
  [void]$sb.AppendLine('    <a href="index.html">Overview</a>')
  [void]$sb.AppendLine('  </div>')

  $sections = @('Foundation', 'Core', 'Compiler', 'Tooling', 'Process')
  $sectionTitles = @{
    'Foundation' = 'Foundation'
    'Core'       = 'Core Specifications'
    'Compiler'   = 'Compiler'
    'Tooling'    = 'Tooling &amp; IDE'
    'Process'    = 'Process &amp; History'
  }

  foreach ($sec in $sections) {
    [void]$sb.AppendLine('  <div class="sidebar-section">')
    [void]$sb.AppendLine("    <div class=`"sidebar-section-title`">$($sectionTitles[$sec])</div>")
    foreach ($d in $docs) {
      if ($d.section -eq $sec) {
        $href = ($d.md -replace '\.md$', '.html')
        [void]$sb.AppendLine("    <a href=`"$href`">$($d.title)</a>")
      }
    }
    [void]$sb.AppendLine('  </div>')
  }

  [void]$sb.AppendLine('  <div class="sidebar-footer">')
  [void]$sb.AppendLine('    Generated from spec markdown.<br>')
  [void]$sb.AppendLine('    Self-contained, offline-ready.')
  [void]$sb.AppendLine('  </div>')
  [void]$sb.AppendLine('</aside>')
  return $sb.ToString()
}

$sidebarHtml = Get-SidebarHtml

foreach ($d in $docs) {
  $mdPath = Join-Path $repoRoot $d.md
  if (-not (Test-Path $mdPath)) {
    Write-Warning "Skipping missing file: $($d.md)"
    continue
  }
  $mdContent = [System.IO.File]::ReadAllText($mdPath)

  # Embed safely inside <script type="text/markdown"> — escape only </script>
  $escaped = $mdContent -replace '</script>', '<\/script>'

  $htmlName = $d.md -replace '\.md$', '.html'
  $outPath  = Join-Path $docsDir $htmlName

  $page = @"
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>$($d.title) — Jux</title>
<link rel="stylesheet" href="styles.css">
<script>
/* Apply the persisted theme before first paint to avoid a flash. */
try { var t = localStorage.getItem('jux-docs-theme');
      if (t) document.documentElement.setAttribute('data-theme', t); } catch (e) {}
</script>
</head>
<body>
<div class="layout">
$sidebarHtml
  <main class="main">
    <div class="content">
      <div class="breadcrumb"><a href="index.html">← Jux Specification</a></div>
      <div id="rendered"></div>
    </div>
    <nav class="toc-rail" id="toc" aria-label="On this page"></nav>
  </main>
</div>

<script id="md-content" type="text/markdown">
$escaped
</script>
<script src="parser.js"></script>
</body>
</html>
"@

  [System.IO.File]::WriteAllText($outPath, $page, [System.Text.UTF8Encoding]::new($false))
  Write-Host "Built: $htmlName" -ForegroundColor Green
}

Write-Host ""
Write-Host "Done. Open Docs/index.html in a browser." -ForegroundColor Cyan
