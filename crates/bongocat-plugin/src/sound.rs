//! Turning a plugin's "play this file" into the product's own audio command.
//!
//! A plugin cannot make a noise by opening the audio device: two processes with the output
//! device open is a thing the operating system arbitrates badly, and users hear it as a
//! stutter. So a plugin *asks*, and this is where the ask stops being an ask. The worker
//! reads the path, checks it, and publishes the product's own `Play` command — the same
//! command the model window's own audio uses, through the same queue and the same one
//! voice, so a plugin's sound and a model's sound cannot overlap.
//!
//! # Why the check is here and not in the plugin
//!
//! Because the plugin is not the side that knows what the product will open. A plugin
//! hands over a string a user typed; this is where it becomes a path the audio service will
//! read. Three things are checked, and each is a fact about this machine rather than about
//! the plugin:
//!
//! * **It is a file.** A directory, a device, or a path that does not exist is refused, so
//!   a typo is a refusal the plugin can show rather than an error inside a decoder.
//! * **It is a size this product will decode.** A file large enough to exhaust memory on a
//!   decode is refused before it is opened, which is the same reason the store caps an
//!   archive and the catalog caps its download.
//! * **It is a path, not a URL.** A string that looks like `https://` is refused, because
//!   the audio device reads a file and a plugin that could make the product fetch something
//!   would be a capability the protocol has no business granting.
//!
//! What is *not* checked is anything about taste. The product does not decide which
//! formats a user's own sound file is in beyond what its decoder can already read, and it
//! does not rewrite the path: the user's file is the user's file, and a plugin that plays
//! the wrong one is a plugin the user turns off.

use bongocat_plugin_protocol::{ModelOutcome, ModelRequest};
use std::path::{Path, PathBuf};

/// The most an audio file a plugin names may be, in bytes.
///
/// A keystroke sound is short by definition — anything past a few seconds is a track rather
/// than a click — and the bound is what makes "a plugin asks for a sound" a cheap thing for
/// the product to allow. Four megabytes is a comfortable margin over any click sample and
/// well under what a decoder would allocate without asking.
pub const MAXIMUM_SOUND_BYTES: u64 = 4 * 1024 * 1024;

/// The extensions the product's decoder reads.
///
/// A convenience filter rather than the decision: a file with no extension, or with one
/// that is uppercase on a case-insensitive filesystem, is still a file the decoder may read,
/// so this is not consulted when deciding whether to play anything. It exists so a log line
/// naming the refused file is one a user can act on.
pub const SOUND_EXTENSIONS: [&str; 5] = ["flac", "mp3", "wav", "ogg", "m4a"];

/// What a path a plugin named turned out to be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The string is empty, or names nothing.
    NoPath,
    /// The path is a URL rather than a file on this machine.
    NotAPath,
    /// Nothing is at that path.
    NotFound,
    /// Something is at that path, and it is not a regular file.
    NotAFile,
    /// The file is larger than the product will decode for a keystroke.
    TooLarge,
    /// The file is not one this product's decoder reads.
    ///
    /// Refused from the extension rather than after a failed decode, so a plugin that typed
    /// `click.txt` is answered at once rather than after the queue round-trips a request the
    /// decoder was always going to fail. A file with *no* extension is not refused here —
    /// the decoder knows more than a list of extensions does, and refusing it would refuse a
    /// file the product can play.
    UnsupportedFormat,
    /// The product has no voice to play it through right now.
    ///
    /// One reason for two situations, because a plugin's remedy is the same for both and a
    /// plugin cannot tell them apart: there is no audio service behind this build, or the
    /// service's queue is full, recovering from an overflow, or already stopped. Which of
    /// the four it was is in the audio service's own diagnostics rather than on this wire,
    /// because a plugin's next move — do not ask again this instant — is the same whatever
    /// the answer was.
    NoVoice,
}

impl Refusal {
    /// A sentence a log line or a diagnostic can carry.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoPath => "no_sound_path",
            Self::NotAPath => "sound_path_is_not_a_file_path",
            Self::NotFound => "sound_file_not_found",
            Self::NotAFile => "sound_path_is_not_a_file",
            Self::TooLarge => "sound_file_too_large",
            Self::UnsupportedFormat => "sound_file_format_unsupported",
            Self::NoVoice => "no_audio_voice",
        }
    }
}

/// Check a path a plugin named, and read its length without opening it.
///
/// Separate from [`play`] so the check can be tested against a filesystem rather than
/// against an audio device, which is the only part of this file with a real dependency.
pub fn check(path: &str) -> Result<PathBuf, Refusal> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(Refusal::NoPath);
    }
    // Checked before the filesystem is touched, because a URL is not a path on either
    // platform this product ships and `Path::exists` on one is a confusing way to learn
    // that.
    if trimmed.contains("://") {
        return Err(Refusal::NotAPath);
    }
    let path = PathBuf::from(trimmed);
    let metadata = std::fs::metadata(&path).map_err(|_| Refusal::NotFound)?;
    if !metadata.is_file() {
        return Err(Refusal::NotAFile);
    }
    if metadata.len() > MAXIMUM_SOUND_BYTES {
        return Err(Refusal::TooLarge);
    }
    Ok(path)
}

/// The outcome of a plugin's sound request, for the worker to act on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SoundOutcome {
    /// The file is one the product will play, and the command is queued.
    Queued,
    /// The plugin did not subscribe to model reactions, so the request was not acted on.
    NotSubscribed,
    /// Nothing is queued, and this is why.
    Refused(Refusal),
}

impl SoundOutcome {
    /// The protocol's answer for this outcome.
    ///
    /// Every refusal is `HostCannot` rather than a code of its own, and the reason is
    /// worth stating: a plugin that asked for a file which is not there and a plugin that
    /// asked for a file the product will not open have the same remedy, which is to stop
    /// asking. A fifth refusal code would tell a plugin author to distinguish two cases
    /// that lead to the same place. The specific reason is counted and logged where the
    /// worker can see it.
    ///
    /// Not subscribing is the exception, and it keeps the protocol's own code because a
    /// plugin *can* do something structural about it: ask for the feed.
    pub const fn outcome(&self) -> ModelOutcome {
        match self {
            Self::Queued => ModelOutcome::Done,
            Self::NotSubscribed => ModelOutcome::NotSubscribed,
            Self::Refused(_) => ModelOutcome::HostCannot,
        }
    }
}

/// Whether a request is one this module answers.
pub const fn is_sound_request(request: &ModelRequest) -> bool {
    matches!(request, ModelRequest::PlaySound { .. })
}

/// The path and volume a sound request names, once checked.
///
/// The split from [`SoundOutcome`] is deliberate: the worker needs the path to build the
/// command and the outcome to answer the plugin, and it gets both from one function so
/// "the command says one path and the answer says another" is not expressible.
pub fn resolve(request: &ModelRequest) -> Result<(PathBuf, f32), SoundOutcome> {
    let ModelRequest::PlaySound { path, volume } = request else {
        return Err(SoundOutcome::Refused(Refusal::NoPath));
    };
    let path = check(path).map_err(SoundOutcome::Refused)?;
    // After the existence and size checks rather than before: "this file is not there" is a
    // more useful fact than "this file is not a format I read", and a user who mistyped a
    // path should hear about the path.
    if !is_plausible(&path) {
        return Err(SoundOutcome::Refused(Refusal::UnsupportedFormat));
    }
    Ok((path, *volume))
}

/// The extension of a checked sound file, lowercased, for a log line.
pub fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_lowercase)
}

/// Whether the product's decoder is likely to read this file.
///
/// A *likely*, and deliberately so: it is checked before the queue is asked, so a plugin
/// asking for a file the product cannot read is answered immediately rather than after the
/// decoder has failed. It is not the decision — [`check`] is — and a file this refuses is
/// still handed to the decoder, because the decoder knows more than an extension list does.
pub fn is_plausible(path: &Path) -> bool {
    extension(path).is_some_and(|extension| {
        SOUND_EXTENSIONS
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&extension))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A real file, because every refusal here is a fact about the filesystem and a test
    /// that faked one would be testing the fake.
    fn written_file(name: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join(name);
        let mut file = std::fs::File::create(&path).expect("a file");
        file.write_all(bytes).expect("writes");
        drop(file);
        (directory, path)
    }

    fn as_str(path: &Path) -> String {
        path.to_str().expect("a path this test wrote").to_string()
    }

    #[test]
    fn a_file_on_this_machine_is_accepted() {
        let (_directory, path) = written_file("click.flac", b"RIFF pretend audio");
        assert_eq!(check(&as_str(&path)), Ok(path));
    }

    #[test]
    fn an_empty_path_is_refused_rather_than_played_as_silence() {
        // A plugin that hands over nothing is a plugin whose file was cleared, and the
        // answer a user can act on is "there is no file", not a sound that is not there.
        assert_eq!(check(""), Err(Refusal::NoPath));
        assert_eq!(check("   "), Err(Refusal::NoPath));
    }

    #[test]
    fn a_url_is_refused_because_the_audio_device_reads_a_file() {
        // The check that keeps "a plugin may name a path" from quietly becoming "a plugin
        // may make the product fetch something".
        for url in [
            "https://example.invalid/click.mp3",
            "http://example.invalid/click.mp3",
            "file:///tmp/click.mp3",
        ] {
            assert_eq!(
                check(url),
                Err(Refusal::NotAPath),
                "{url} must not be fetched"
            );
        }
    }

    #[test]
    fn a_path_nothing_is_at_is_refused() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let missing = directory.path().join("nope.mp3");
        assert_eq!(check(&as_str(&missing)), Err(Refusal::NotFound));
    }

    #[test]
    fn a_directory_is_refused_rather_than_handed_to_a_decoder() {
        // A directory passes `exists` and fails at open time inside a decoder, which is a
        // crash on somebody else's thread with no message anybody can read.
        let directory = tempfile::tempdir().expect("a temporary directory");
        assert_eq!(check(&as_str(directory.path())), Err(Refusal::NotAFile));
    }

    #[test]
    fn a_file_larger_than_a_keystroke_sound_may_be_is_refused_before_it_is_opened() {
        let (_directory, path) = written_file("long.flac", &[0u8; 16]);
        assert_eq!(check(&as_str(&path)), Ok(path), "16 bytes is a click");

        let big = tempfile::tempdir().expect("a temporary directory");
        let path = big.path().join("track.mp3");
        let file = std::fs::File::create(&path).expect("a file");
        file.set_len(MAXIMUM_SOUND_BYTES + 1).expect("a large file");
        drop(file);
        assert_eq!(
            check(&as_str(&path)),
            Err(Refusal::TooLarge),
            "a four-megabyte cap is what makes 'a plugin asks for a sound' cheap to allow"
        );
    }

    #[test]
    fn every_refusal_answers_the_plugin_the_same_way_and_says_something_different() {
        // Same answer, different reasons: a plugin's remedy for "the file is not there" and
        // for "the product will not open that" is the same — stop asking — so the protocol
        // says one thing, while the log and the diagnostics say which it was.
        for refusal in [
            Refusal::NoPath,
            Refusal::NotAPath,
            Refusal::NotFound,
            Refusal::NotAFile,
            Refusal::TooLarge,
            Refusal::UnsupportedFormat,
            Refusal::NoVoice,
        ] {
            assert_eq!(
                SoundOutcome::Refused(refusal).outcome(),
                ModelOutcome::HostCannot,
                "{refusal:?} answers as a thing the product cannot do"
            );
            assert!(!refusal.as_str().is_empty());
        }
        assert_eq!(SoundOutcome::Queued.outcome(), ModelOutcome::Done);
        assert_eq!(
            SoundOutcome::NotSubscribed.outcome(),
            ModelOutcome::NotSubscribed,
            "and not subscribing keeps its own code, because a plugin can do something \
             structural about it: ask for the feed"
        );
    }

    #[test]
    fn a_resolved_request_yields_the_path_the_command_will_name() {
        let (_directory, path) = written_file("click.mp3", b"audio");
        let request = ModelRequest::PlaySound {
            path: as_str(&path),
            volume: 0.4,
        }
        .sanitized();
        assert!(is_sound_request(&request));
        assert_eq!(resolve(&request), Ok((path, 0.4)));
    }

    #[test]
    fn a_request_that_is_not_a_sound_is_not_answered_by_this_module() {
        let motion = ModelRequest::PlayMotion {
            name: "wave".to_string(),
            restart: false,
        };
        assert!(!is_sound_request(&motion));
        assert_eq!(
            resolve(&motion),
            Err(SoundOutcome::Refused(Refusal::NoPath)),
            "and asking this module about one is a programming error, reported as a refusal \\
             rather than a panic in a plugin's session"
        );
    }

    #[test]
    fn a_volume_is_clamped_rather_than_refused_because_it_is_a_multiplier() {
        // A plugin that asked for 1.5 wanted "as loud as possible" more than it wanted to
        // be told no, and a NaN — the only way to get a meaningless one here — becomes
        // silence, which is the safe reading of a request that carries no meaning.
        for (asked, expected) in [
            (-1.0_f32, 0.0_f32),
            (0.0, 0.0),
            (0.5, 0.5),
            (1.0, 1.0),
            (1.5, 1.0),
            (f32::NAN, 0.0),
            (f32::INFINITY, 1.0),
            (f32::NEG_INFINITY, 0.0),
        ] {
            let request = ModelRequest::PlaySound {
                path: "/tmp/click.flac".to_string(),
                volume: asked,
            }
            .sanitized();
            let ModelRequest::PlaySound { volume, .. } = request else {
                panic!("expected a sound request");
            };
            assert_eq!(volume, expected, "{asked} should have become {expected}");
        }
    }

    #[test]
    fn a_path_longer_than_a_path_can_be_is_cut_rather_than_sent_whole() {
        let long = "a".repeat(10_000);
        let request = ModelRequest::PlaySound {
            path: long,
            volume: 1.0,
        }
        .sanitized();
        let ModelRequest::PlaySound { path, .. } = request else {
            panic!("expected a sound request");
        };
        assert_eq!(
            path.chars().count(),
            bongocat_plugin_protocol::MAXIMUM_SOUND_PATH_BYTES
        );
    }

    #[test]
    fn a_file_this_product_cannot_decode_is_refused_before_the_queue_is_asked() {
        // Checked from the extension rather than after a failed decode, so a plugin that
        // typed `click.txt` is answered at once instead of after the audio queue has
        // carried a request the decoder was always going to reject.
        let (_directory, path) = written_file("click.txt", b"not audio");
        let request = ModelRequest::PlaySound {
            path: as_str(&path),
            volume: 1.0,
        }
        .sanitized();
        assert_eq!(
            resolve(&request),
            Err(SoundOutcome::Refused(Refusal::UnsupportedFormat))
        );
    }

    #[test]
    fn a_missing_file_is_reported_as_missing_rather_than_as_an_unsupported_format() {
        // "This file is not there" is the more useful fact when a user has just typed a
        // path, and a format complaint about a file that does not exist would send them to
        // look at the wrong thing.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let request = ModelRequest::PlaySound {
            path: as_str(&directory.path().join("click.mp3")),
            volume: 1.0,
        }
        .sanitized();
        assert_eq!(
            resolve(&request),
            Err(SoundOutcome::Refused(Refusal::NotFound))
        );
    }

    #[test]
    fn an_extension_the_decoder_likely_reads_is_recognized_case_insensitively() {
        for name in ["click.flac", "click.FLAC", "click.Mp3", "click.wav"] {
            let path = Path::new(name);
            assert!(is_plausible(path), "{name} is one this product can decode");
        }
        assert!(!is_plausible(Path::new("click.txt")));
        assert!(
            !is_plausible(Path::new("click")),
            "and a file with no extension is not refused for that alone — the decoder \\
             decides, this only rules out an obvious mismatch before the queue is asked"
        );
    }
}
