# Native Update Manifest v1

`manifest.schema.json` defines the signed payload consumed by `bongocat-update`. The transport signs
the exact manifest bytes with Ed25519 and carries the stable key ID plus the raw 64-byte detached
signature separately. The verifier authenticates those bytes before parsing them.

The manifest channel is the immutable Development or Production build environment. Each artifact
uses one supported Windows/macOS target and its matching architecture, an HTTPS URL, an exact byte
length, and a lowercase SHA-256 digest. `release_sequence` is monotonic within an environment and is
the anti-rollback ordering value; semantic versions remain the user-facing release identity.

The files under `fixtures/` cover schema acceptance and rejection. They do not contain real release
URLs, packages, signatures, public keys, or distribution authorization.
