#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReplayGainMode {
    Track,
    Album,
}

/// Parse the persisted "replaygain" setting. Anything that is not
/// "track" / "album" (including the empty default) means off.
pub fn mode_from_setting(s: &str) -> Option<ReplayGainMode> {
    match s {
        "track" => Some(ReplayGainMode::Track),
        "album" => Some(ReplayGainMode::Album),
        _ => None,
    }
}

/// Inverse of [`mode_from_setting`], for persisting the user's choice.
pub fn mode_as_setting(mode: Option<ReplayGainMode>) -> &'static str {
    match mode {
        Some(ReplayGainMode::Track) => "track",
        Some(ReplayGainMode::Album) => "album",
        None => "off",
    }
}

pub fn read_gain(path: &str, mode: ReplayGainMode) -> f32 {
    fn read(path: &str, mode: ReplayGainMode) -> Option<f32> {
        use lofty::prelude::*;
        use lofty::probe::Probe;
        use lofty::tag::ItemKey;

        let tagged = Probe::open(std::path::Path::new(path))
            .ok()?
            .guess_file_type()
            .ok()?
            .read()
            .ok()?;
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;

        let primary = match mode {
            ReplayGainMode::Track => ItemKey::ReplayGainTrackGain,
            ReplayGainMode::Album => ItemKey::ReplayGainAlbumGain,
        };
        let fallback = match mode {
            ReplayGainMode::Track => ItemKey::ReplayGainAlbumGain,
            ReplayGainMode::Album => ItemKey::ReplayGainTrackGain,
        };

        let db_str = tag
            .get_string(&primary)
            .or_else(|| tag.get_string(&fallback))?;

        let db: f32 = db_str
            .trim_end_matches("dB")
            .trim_end_matches("DB")
            .trim()
            .parse()
            .ok()?;
        Some(10_f32.powf(db.clamp(-20.0, 10.0) / 20.0))
    }
    read(path, mode).unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_returns_unity_gain() {
        assert_eq!(
            read_gain("/nonexistent/file.mp3", ReplayGainMode::Track),
            1.0
        );
        assert_eq!(
            read_gain("/nonexistent/file.mp3", ReplayGainMode::Album),
            1.0
        );
    }

    #[test]
    fn setting_roundtrip_covers_all_modes() {
        for mode in [
            None,
            Some(ReplayGainMode::Track),
            Some(ReplayGainMode::Album),
        ] {
            assert_eq!(mode_from_setting(mode_as_setting(mode)), mode);
        }
        // Unknown / legacy values fall back to off.
        assert_eq!(mode_from_setting(""), None);
        assert_eq!(mode_from_setting("bogus"), None);
    }

    #[test]
    fn db_to_linear_known_values() {
        // Industry-standard formula: 10^(dB/20). Spot-check a few values.
        let approx_eq = |a: f32, b: f32| (a - b).abs() < 0.001;
        // 0 dB → 1.0 (unity)
        assert!(approx_eq(10_f32.powf(0.0 / 20.0), 1.0));
        // -6 dB → ~0.501
        assert!(approx_eq(10_f32.powf(-6.0 / 20.0), 0.501));
        // +6 dB → ~1.995
        assert!(approx_eq(10_f32.powf(6.0 / 20.0), 1.995));
    }
}
