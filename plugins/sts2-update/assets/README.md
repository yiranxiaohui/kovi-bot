# Simplified Chinese terminology snapshot

`official_terms.zhs.json` contains English-to-Chinese entity names extracted from
the Simplified Chinese localization shipped with *Slay the Spire 2*. The snapshot
is generated through the unofficial [Spire Codex](https://spire-codex.com/) API;
Spire Codex is the transport and extraction layer, not the author of the localized
strings.

Refresh the snapshot after game localization updates:

```sh
python3 plugins/sts2-update/scripts/update_official_terms.py
```

When refreshing the snapshot, also bump `TRANSLATION_CACHE_NAMESPACE` in
`src/lib.rs` so persisted translations are regenerated with the new terminology.
