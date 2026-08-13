# Open US Law ingestion

The MVP uses [Vaquill AI's Open US Law](https://github.com/Vaquill-AI/open-us-law)
as a discovery corpus. It fetches the published manifest, selects an exact
jurisdiction/type file, and verifies declared size, SHA-256, and Parquet magic
bytes before parsing. The snapshot is pinned instead of following `latest`.

Corpus provenance and legal authority are different questions. Every candidate
obligation still needs a verified controlling-source URL, locator, content hash,
and human review before it can enter an approved policy pack. Known-noisy
jurisdictions are quarantined; data-only or unverified snapshots remain
ineligible for automatic publication.

The [Vaquill Word add-in](https://github.com/Vaquill-AI/ms-word-addin) is useful
product research for source review and drafting workflows, but it is not a
runtime dependency or a legal-data authority for Featherlane.

Attribution must be retained with exports. The console and API expose the corpus
snapshot and license metadata separately from policy approval state.
