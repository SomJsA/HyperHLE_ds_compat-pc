# R4 buildable source

Use tag `20261112-r4-source` for source checkout. This corrects four Windows-frozen files whose actual regular-file contents were previously marked as Git symlinks. The file blobs are unchanged. Runtime implementation and exact build-input hashes remain recorded in BUILD-INPUTS.json. Release packages retain their verified hashes.

```
git clone --branch 20261112-r4-source --recurse-submodules https://github.com/SomJsA/HyperHLE_ds_compat-pc.git
```

PC uses the public SomJsA/dynarmic mirror at the exact already-built commit. Android retains its original upstream Dynarmic pin. See the project build documentation for the normal platform build steps.
