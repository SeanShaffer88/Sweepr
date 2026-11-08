# Sweepr Audit

Evidence labels: **Verified** (run and observed) · **Partially verified** · **Unverified**.

| Area | Label | Notes |
|---|---|---|
| Model, error codes, id validator | Verified (Linux unit tests) | |
| Parsers (WinGet, Choco, Scoop) | **Unverified** against real output | Only synthetic fixtures (`*.synthetic.txt`). Run `tools/capture-fixtures.ps1` on Windows, add `*.real.txt`, parameterize the tests. WinGet header is localized: non-English Windows Unverified. |
