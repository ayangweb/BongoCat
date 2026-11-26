# Release public keys

These files are checked-in build inputs, not runtime configuration:

- `app.pub` authenticates application update payloads.
- `plugin.pub` authenticates the plugin catalog.

The two trust roots are intentionally separate. Both files are embedded with
`include_str!`, so they must contain exactly one base64-encoded Minisign public-key box
and no trailing whitespace.

Public keys are not secrets. Private keys and their passwords belong only in GitHub
Actions secrets. Generate an application key pair with:

```sh
just keygen ~/.bongocat/release.key
```

Replace `app.pub` with the generated public file after verifying the key ID.
Rotating either key requires shipping an application that trusts both the old and new
key before releases are signed with the new private key.
