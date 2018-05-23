use slog::Level;

pub(crate) fn level_to_severity(level: Level) -> u8 {
    match level {
        Level::Critical => 2,
        Level::Error => 3,
        Level::Warning => 4,
        Level::Info => 6,
        Level::Debug | Level::Trace => 7,
    }
}
