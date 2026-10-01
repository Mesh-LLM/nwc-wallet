Standalone Nostr Wallet Connect (NIP-47) wallet provider for mesh-llm's
`wallet.v1` capability. Install with:

```sh
mesh-llm plugins install benthecarman/nwc-wallet
```

Then pass the connection URI file in the plugin's `[[plugin]]` stanza:
`args = ["--uri-file", "/path/to/nwc-uri"]`. Use a mesh-llm host with payments
enabled. Installation does not authorize spending; mesh-llm keeps policy,
budgets and the ledger.

Pays through NWC-321 `pay` with a fee limit when the wallet offers it, and core
`pay_invoice` otherwise. Core NIP-47 cannot cap routing fees, so a payment can
cost more than the authorized headroom; the fee still counts against the daily
budget. Amount-less invoices need NWC-321 `receive`.

Native archives and SHA-256 sidecars cover macOS Apple Silicon/Intel, Linux
ARM64/x86_64 and Windows ARM64/x86_64. Artifacts are not notarized or
Authenticode-signed. Build and test coverage is not a claim of live settlement
qualification against every NWC wallet.
