# Changelog
## Unreleased
- Track scope active status
- Estimate and print variance
- Use [`tabular`](https://docs.rs/tabular/latest/tabular) for aligned printing
- Also print self percentage, number of calls, and last duration

## Version 0.2.5 (2021-01-01)
- Drop dependency on `floating-duration`, no longer needed
- Use [instant](https://github.com/sebcrozet/instant) crate for portability to WASM targets

## Version 0.2.4 (2020-01-20)
- Fix a bug that caused the frequency of some scopes to be slightly overestimated when printing ([#1](https://github.com/leod/coarse-prof/pull/1))

## Version 0.2.3 (2019-12-06)
- Print percentage of total time for root nodes
- Add `coarse_prof::enter()` for conveniently entering a scope manually

## Version 0.2.2 (2019-12-03)
- Update readme 

## Version 0.2.1 (2019-12-03)
- Fix readme 

## Version 0.2.0 (2019-12-03)
- Rename `print` to `write`
- Return `std::io::Result` instead of calling `unwrap`

## Version 0.1.0 (2019-12-03)
- Initial version
