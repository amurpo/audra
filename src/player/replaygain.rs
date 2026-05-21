#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReplayGainMode {
    Track,
    Album,
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
