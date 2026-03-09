# Emulation Result (2026-03-09)

## Fixture Checks
- Script: `bash scripts/check-fixtures.sh`
- Status: passed
- Outputs:
  - `fixture_ok name=case_a_emu emulate_cycles=448971`
  - `fixture_ok name=case_b_emu emulate_cycles=1570460`
  - `fixture_ok name=case_c_proof proof_and_mask_checked`
  - `fixtures: all checks passed`

## Commit Tier Perf Matrix
- Script: `bash scripts/perf-matrix.sh commit`
- Status: passed (`perf caps passed`)
- CSV: `artifacts/perf_matrix_commit.csv`

| tier | label | width | height | pixels | cycles | emu_ms | accessed_addrs |
|---|---|---:|---:|---:|---:|---:|---:|
| commit | series_256x256 | 256 | 256 | 65536 | 5518251 | 499.31 | 53153 |
| commit | series_512x512 | 512 | 512 | 262144 | 20964267 | 1884.55 | 151457 |
| commit | series_1024x1024 | 1024 | 1024 | 1048576 | 82748332 | 7897.76 | 544673 |
