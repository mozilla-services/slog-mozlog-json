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

pub(crate) fn level_to_gcp_severity(level: Level) -> u16 {
    match level {
        // EMERGENCY => 800,
        // ALERT => 700,
        Level::Critical => 600,
        Level::Error => 500,
        Level::Warning => 400,
        // NOTICE => 300,
        Level::Info => 200,
        Level::Debug | Level::Trace => 100,
        // DEFAULT => 0
    }
}
