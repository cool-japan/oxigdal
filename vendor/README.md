# vendor/

This directory holds interim local patches of third-party crates that the
workspace pulls in transitively. Each subdirectory pins exactly one crate via
`[patch.crates-io]` in the workspace `Cargo.toml`. See the per-subdirectory
`README.md` for the rationale and removal plan for each patch.
