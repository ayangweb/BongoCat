# 0084 — A plugin asks the host to play a sound, and never opens the device

**Status:** accepted

A plugin that wants to make a noise cannot make one. It asks the host, and the host decides
whether to. The reason is not politeness about layering: two processes with the output
device open is a thing the operating system arbitrates badly, and users hear the result as
a stutter.

## Context

A sound in BongoCat belongs to a motion. The audio device plays the clip a model attaches
to a motion, and the only way to make it play is to play that motion — which is what the
first typing-sound plugin did, and it worked, and it left the user with one voice: the
model's.

Asking for a sound of the user's own is a reasonable thing to want — a click is a click, and
a model that carries no clip is a silent model. But the obvious implementation is wrong.
A plugin is a separate process, so a plugin that opened the audio device itself would be a
second writer on it: the model window's own sound and the plugin's would interleave in ways
neither asked for, on a device whose arbitration is not ours to fix. The SDK cannot make
this a convention either, because a convention is not a boundary.

## The decision

`ModelRequest` gains `PlaySound { path, volume }`, and the audio service is the only thing
that ever opens the device.

The path is bounded in the protocol and checked by the host, because the host is the only
side that knows what it will open. Three things are checked, and each is a fact about the
machine rather than about the plugin:

- **It is a file.** A directory, a device, or a path that does not exist is refused, so a
  typo is a refusal the plugin can show rather than an error inside a decoder on somebody
  else's thread.
- **It is a size this product will decode.** Four megabytes, checked before the file is
  opened. A keystroke sound is short by definition; a bound is what makes "a plugin asks
  for a sound" a cheap thing for the product to allow.
- **It is a path, not a URL.** A string containing `://` is refused, because the audio
  device reads a file and a plugin that could make the product *fetch* something would be a
  capability the protocol has no business granting.

What is **not** checked is anything about taste. The product does not decide which formats
a user's own sound file is in beyond what its decoder can already read, and it does not
rewrite the path: the user's file is the user's file, and a plugin that plays the wrong one
is a plugin the user turns off.

### The subscription still applies

A sound request is refused for a plugin that did not ask for model reactions, with the
protocol's own `NotSubscribed` code rather than a generic one — because unlike "the file is
not there", a plugin *can* do something structural about this: ask for the feed.

Every other refusal answers `HostCannot`, and the specific reason is counted and logged on
the plugin's own log rather than the product's. "The file you chose is not there" and "the
product will not open that" have the same remedy, which is to stop asking, so a fifth
refusal code would tell a plugin author to distinguish two cases that lead to the same
place. It is the plugin's log rather than the product's because the product's is a closed
vocabulary of event codes and this would need a code per reason.

### The volume is clamped, not refused

It is a multiplier, so the useful range is nothing to one. A plugin that asked for `1.5` —
or for an infinity — wanted "as loud as possible" more than it wanted to be told no.
`NaN` is the one value clamping cannot read, and it becomes silence, which is the safe
reading of a request that carries no meaning at all.

## What this costs

- A plugin's sound is serialized behind the model's, because there is exactly one voice. A
  keystroke sound during a model's own motion is cut short rather than layered. That is the
  cost of one device and one writer, and it is the same behaviour a single model sound
  already had.
- An audio file is named by path, not uploaded. A plugin has no notion of an upload, and a
  desktop user already has the file on the machine; the setting is a path with the
  protocol's own text bound, and a value too long to be a path is refused by the host
  before the plugin sees it.
- The format list is a convenience, not the decision. A file with no extension is not
  refused for that alone, because the decoder knows more than a list of extensions does.

## Consequences

- `bongocat-plugin-protocol` gains the request, the path bound, and a `sanitized` arm that
  clamps the volume.
- `bongocat-plugin` gains the sound check, its refusals and a counter of its own — a sound
  is not a model reaction, and a diagnostic that conflated them could not answer the
  question a user actually asks of it: "is the typing-sound plugin working?"
- `bongocat-plugin-sdk` gains `Host::play_sound` and a public `Host::request`, so a plugin
  that has a request in hand rather than a name to type can send it and still get its
  answer back on the same counter.
- The worker is handed the product's audio client at start. A build with no audio service
  answers with the unavailable client, so the worker needs no knowledge of whether there is
  one.
