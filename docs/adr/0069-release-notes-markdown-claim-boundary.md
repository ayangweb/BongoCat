# ADR-0069: Release notes render through gpui-kit TextView behind a claim-based refusal layer

- Status: accepted
- Date: 2026-09-26
- Depends on: ADR-0020, ADR-0054, ADR-0068
- Amends: none

## Context

`bongocat-ui` rendered the update window's release notes with a hand-written
intermediate representation and a hand-written element tree. The changelog arrives in
the release manifest, so it is untrusted input and the only text the product renders
that it did not author. That made the module a security boundary, and it had grown to
carry its own Markdown parser, its own inline layout, and roughly seven hundred lines
of tests describing what a manifest is allowed to become.

The layout half was reimplementing what the product already depends on.
`gpui-kit` (ADR-0020) ships `TextView`, which parses CommonMark plus GFM, runs a real
inline layout, and is the same renderer every other part of the settings UI uses.

The obstacle was that `TextView` cannot be handed untrusted Markdown unchanged.
Checked against the pinned `gpui-kit` revision `500852f`:

- `TextView` renders `![alt](url)` by handing `url` to GPUI's resource loader, which
  fetches it, and `ImageNode::source` only exempts `data:` URLs.
- A Markdown HTML node is passed to `gpui-kit`'s HTML parser and laid out, and that
  parser builds `ImageNode`s — so `<img src="https://…">` is a second route to the
  same request.
- There is no image-policy or link-scheme hook, and the depth of the parsed tree is
  not bounded anywhere.

So the decision was never "delegate the rendering" but "delegate the rendering and own
the policy". The question this ADR records is how that policy is expressed.

## Decision

`crates/bongocat-ui/src/update_markdown.rs` keeps three rules and no renderer. It
hands `TextView` the notes, plus a `MarkdownExtensions` registry built from four
plugins:

- `NoRemoteImage` claims every `Node::Image` and `Node::ImageReference` and shows the
  alt text. An image with empty alt text is still claimed, because declining it would
  hand the URL back.
- `LiteralHtml` claims `Node::Html` and shows the source. It is registered twice — once
  as an inline plugin and once as a block plugin — because `gpui-kit` has a separate
  dispatcher for each and a block-level `<div>…</div>` never reaches the inline one.
- `RefusedLink` claims a `Node::Link` whose target fails the HTTPS test and shows the
  link's label as plain text, so a refused target is not underlined and accent-coloured.
  Accepted links are declined to `gpui-kit`, whose click handler still routes through
  the platform opener, which re-checks the scheme.

Each rule is a plain function over the parsed node (`image_alt_text`,
`literal_html_text`, `refused_link_text`) with the plugin as a thin shell, so each
decision is unit-testable without a window.

The input bounds are applied to the raw text, before anything parses it:

- the byte cap stays at 32 KiB;
- a container-marker cap replaces the old block-depth cap. The old cap counted nesting
  while parsing, which the delegated parser no longer exposes. The new cap bounds the
  number of block containers in the source, which bounds tree depth because every
  level of CommonMark nesting is opened by at least one container marker. `TextView`
  walks the tree with one Rust call per level, so tree depth is renderer stack depth
  and this is a sound bound rather than an estimate of apparent depth.

A probe plugin registered after the four production plugins asserts the mechanism
itself: `MarkdownExtensions` resolves plugins in registration order and stops at the
first claim, so a node the refusals claim is never offered to the probe. The test also
reads the rendered text back, because a plugin that claimed more than it was meant to
would leave the changelog blank while still passing any test that only asks whether
something rendered.

## Consequences

- The module drops from a parser plus renderer to a policy layer; `pulldown-cmark` is
  no longer a direct dependency of `bongocat-ui`, since `TextView` owns parsing.
- GFM tables and task lists now render, because `TextView` enables them. The update
  window's changelog is styled by `TextViewStyle::from_theme`, so it follows the theme
  instead of a local token set.
- The update window's link elements no longer carry product-owned element ids;
  `gpui-kit` owns link rendering. The window-level tests now cover what only the window
  can show (the release reaches the window, the notes hold their height budget, the
  phase's actions stay reachable) and the document-level tests cover what each node
  becomes.
- A link *reference* resolves to its target inside `gpui-kit`, so `RefusedLink` cannot
  judge it and a reference link with a non-HTTPS target is presented as a link. It
  still cannot open: the click goes through the HTTPS-only opener. Closing that gap
  needs an upstream hook.
- **This depends on `gpui-kit` resolving plugins before its own handlers.** That is what
  makes the refusals refusals rather than suggestions. A `gpui-kit` revision that
  consulted plugins afterwards would reopen the fetch, which is why the probe test
  asserts the ordering instead of trusting it. The `gpui-kit` revision stays pinned per
  ADR-0020, and switching back to crates.io is a change that has to re-run this test.

## Verification

- `the_refusals_claim_their_nodes_before_the_built_in_renderer` drives a real
  `TextView` in a test window over a document containing an inline image, a reference
  image, a raw HTML block, an accepted link and a refused link. It asserts the probe
  never saw an image, reference image or HTML node, and that the rendered text contains
  every alt text, both link labels and the literal HTML — and none of the URLs the
  manifest named.
- `nesting_deeper_than_the_stack_allows_is_cut` and
  `a_long_list_is_not_mistaken_for_nesting` pin the marker cap from both directions: a
  document written to exhaust the stack is cut, and a list long enough to outgrow the
  notes area is not mistaken for nesting.
- `a_changelog_of_every_refused_shape_still_reaches_the_window` drives the real update
  window with the same document and asserts the notes stay inside their height budget,
  the window stays inside its ceiling, and the phase's install action is still
  reachable.
- The remaining module tests cover the three refusal decisions and the two bounds
  directly, against constructed `mdast` nodes.
