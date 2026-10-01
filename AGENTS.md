# NWC wallet plugin

Reuse the mesh-llm wallet.v1 contract; keep the backend in a reusable library and the binary thin. No host ledger or budget policy here. Never expose payment operations as MCP tools by default.

Use just for build/test/check/package/clean. Run the complete package test suite and Clippy with warnings denied. Tests must not provision wallets, move funds, contact live wallet services, or access OS credential stores. Live validation requires explicit authorization and isolated state. Preserve uncertainty after payment submission; never blindly retry a send.

The plugin stores nothing in the host-supplied wallet directory. Never accept the connection URI inline, log it, or include it in an error: it is the wallet's spending credential. Do not push main or publish releases without human approval. Draft commits use the configured implementing identity; preserve source license/attribution. Do not fabricate co-authors or DCO certification.
