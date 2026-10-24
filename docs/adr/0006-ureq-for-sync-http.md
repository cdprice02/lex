# ureq for synchronous HTTP

Downloads are cold-path and run once per language, so `lex-data` fetches synchronously with ureq and parallelises shards with scoped threads. `reqwest::blocking` was rejected because it still embeds a tokio runtime, which is the dependency stack the sync rewrite existed to remove.
